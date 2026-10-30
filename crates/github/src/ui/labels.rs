//! Picks any number of the labels one or two repositories define, with a search box: for a
//! list's label filter (it keeps items carrying all of them) and for an issue's own labels.

use std::rc::Rc;
use std::time::Duration;

use gitviber_ui::memory::Live;
use gitviber_ui::widgets::{Ends, list_step, on_list_keys};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{Icon, Sizable as _, WindowExt as _};
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    KeyDownEvent, ParentElement as _, Render, ScrollHandle, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Window, div,
    prelude::FluentBuilder as _, px, rems,
};

use crate::cache::Cache;
use crate::model::{LabelItem, Target};
use crate::ui::common::{error_text, label_dot, theme};

const AGE: Duration = Duration::from_secs(600);

pub type OnLabels = Rc<dyn Fn(Vec<LabelItem>, &mut Window, &mut App)>;

fn key(target: &Target) -> String {
    format!("issues:labels:{}", target.as_deref().unwrap_or("origin"))
}

pub struct LabelPicker {
    repos: Vec<Target>,
    selected: Vec<LabelItem>,
    /// Selected as of opening: they go first, without reordering under the pointer.
    first: Vec<LabelItem>,
    query: Entity<InputState>,
    /// The row ↑/↓ are on, which ↵ toggles.
    index: usize,
    scroll: ScrollHandle,
    hint: Option<SharedString>,
    on_change: OnLabels,
    stamp: u64,
    _subs: Vec<Subscription>,
    _live: Live,
}

impl LabelPicker {
    /// Opens it in a dialog; `on_change` hears each pick, `on_close` the labels as they're left.
    pub fn open(
        repos: Vec<Target>,
        selected: Vec<LabelItem>,
        hint: Option<SharedString>,
        on_change: OnLabels,
        on_close: Option<OnLabels>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let picker = cx.new(|cx| LabelPicker::new(repos, selected, hint, on_change, window, cx));
        let focus = picker.read(cx).query.clone();
        let first = std::cell::Cell::new(true);
        let closing = picker.downgrade();
        window.open_dialog(cx, move |dialog, window, cx| {
            if first.replace(false) {
                focus.update(cx, |q, cx| q.focus(window, cx));
            }
            let closing = closing.clone();
            let on_close = on_close.clone();
            dialog
                .title("Labels")
                .w(px(340.))
                .child(picker.clone())
                .on_close(move |_, window, cx| {
                    if let (Some(picker), Some(f)) = (closing.upgrade(), on_close.as_ref()) {
                        let picked = picker.read(cx).selected.clone();
                        f(picked, window, cx);
                    }
                })
        });
    }

    fn new(
        repos: Vec<Target>,
        selected: Vec<LabelItem>,
        hint: Option<SharedString>,
        on_change: OnLabels,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let query = cx.new(|cx| InputState::new(window, cx).placeholder("Filter labels…"));
        let mut subs = vec![cx.subscribe_in(
            &query,
            window,
            |this: &mut Self, _, ev: &InputEvent, window, cx| match ev {
                InputEvent::Change => {
                    this.index = 0;
                    cx.notify()
                }
                InputEvent::PressEnter { .. } => {
                    let shown = this.shown(cx);
                    if let Some(l) = shown.get(this.index.min(shown.len().saturating_sub(1))) {
                        this.toggle(l.clone(), window, cx);
                    }
                }
                _ => {}
            },
        )];
        subs.push(cx.observe_global::<Cache>(|this: &mut Self, cx| {
            let s = this.stamp_now(cx);
            if s != this.stamp {
                this.stamp = s;
                cx.notify();
            }
        }));
        for target in &repos {
            let target = target.clone();
            Cache::load(cx, &key(&target), AGE, move |core| {
                gitviber_core::api::github::issue_labels(core, target.clone())
                    .map(|ls| ls.iter().map(LabelItem::from).collect::<Vec<_>>())
            });
        }
        LabelPicker {
            repos,
            first: selected.clone(),
            selected,
            query,
            index: 0,
            scroll: ScrollHandle::new(),
            hint,
            on_change,
            stamp: 0,
            _subs: subs,
            _live: Live::new("LabelPicker"),
        }
    }

    fn stamp_now(&self, cx: &App) -> u64 {
        let keys: Vec<String> = self.repos.iter().map(key).collect();
        Cache::stamp(cx, &keys.iter().map(String::as_str).collect::<Vec<_>>())
    }

    fn defined(&self, cx: &App) -> (Vec<LabelItem>, bool, Option<String>) {
        let mut all: Vec<LabelItem> = Vec::new();
        let mut loaded = true;
        let mut error = None;
        for t in &self.repos {
            let read = Cache::read::<Vec<LabelItem>>(cx, &key(t));
            match read.data {
                Some(ls) => {
                    for l in ls.iter() {
                        if !all.iter().any(|m| m.name == l.name) {
                            all.push(l.clone());
                        }
                    }
                }
                None => loaded = false,
            }
            error = error.or(read.error);
        }
        (all, loaded, error)
    }

    fn shown(&self, cx: &App) -> Vec<LabelItem> {
        let (defined, _, _) = self.defined(cx);
        let q = self.query.read(cx).value().trim().to_lowercase();
        let is_first = |l: &LabelItem| self.first.iter().any(|m| m.name == l.name);
        // One renamed or deleted since it was picked still shows, so it can be taken off.
        let gone = self
            .first
            .iter()
            .filter(|l| !defined.iter().any(|m| m.name == l.name))
            .cloned();
        let mut all: Vec<LabelItem> = gone.chain(defined.iter().cloned()).collect();
        all.sort_by_key(|l| !is_first(l));
        all.retain(|l| {
            q.is_empty()
                || l.name.to_lowercase().contains(&q)
                || l.description.to_lowercase().contains(&q)
        });
        all
    }

    /// ↑/↓ in the search box walk the rows, round from either end; Home/End, PgUp/PgDn too.
    fn key_down(&mut self, ev: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let m = ev.keystroke.modifiers;
        let n = self.shown(cx).len();
        if m.alt || m.platform || m.control || m.shift || n == 0 {
            return;
        }
        let typed = !self.query.read(cx).value().is_empty();
        let Some(to) = list_step(&ev.keystroke, Some(self.index), n, Ends::Wrap, typed) else {
            return;
        };
        self.index = to;
        self.scroll.scroll_to_item(self.index);
        cx.stop_propagation();
        cx.notify();
    }

    fn toggle(&mut self, label: LabelItem, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected.iter().any(|m| m.name == label.name) {
            self.selected.retain(|m| m.name != label.name);
        } else {
            self.selected.push(label);
        }
        (self.on_change)(self.selected.clone(), window, cx);
        cx.notify();
    }
}

impl Render for LabelPicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let (_, loaded, error) = self.defined(cx);
        let shown = self.shown(cx);
        let q = !self.query.read(cx).value().trim().is_empty();
        let index = self.index.min(shown.len().saturating_sub(1));
        let rows: Vec<AnyElement> = shown
            .iter()
            .enumerate()
            .map(|(i, l)| {
                let on = self.selected.iter().any(|m| m.name == l.name);
                let label = l.clone();
                let hover = t.hover;
                div()
                    .id(("label", i))
                    .flex()
                    .items_start()
                    .gap(px(8.))
                    .px(px(8.))
                    .py(px(6.))
                    .rounded(px(3.))
                    .cursor_pointer()
                    .when(i == index, |d| d.bg(hover))
                    .on_mouse_move(cx.listener(move |this, _, _, cx| {
                        if this.index != i {
                            this.index = i;
                            cx.notify();
                        }
                    }))
                    .child(div().w(px(14.)).flex_none().when(on, |d| {
                        d.child(Icon::new(IconName::Check).xsmall().text_color(t.primary))
                    }))
                    .child(div().mt(px(4.)).child(label_dot(l, &t)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(div().truncate().text_size(rems(0.75)).child(l.name.clone()))
                            .when(!l.description.is_empty(), |d| {
                                d.child(
                                    div()
                                        .truncate()
                                        .text_size(rems(0.656))
                                        .text_color(t.subtle)
                                        .child(l.description.clone()),
                                )
                            }),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.toggle(label.clone(), window, cx)
                    }))
                    .into_any_element()
            })
            .collect();
        let error = error.filter(|_| !loaded);
        let empty = if error.is_some() {
            None
        } else if !loaded && rows.is_empty() {
            Some("Loading labels…")
        } else if rows.is_empty() {
            Some(if q {
                "No labels match"
            } else {
                "This repository has no labels"
            })
        } else {
            None
        };
        let has = !self.selected.is_empty();
        on_list_keys(div(), cx.listener(Self::key_down))
            .key_context("ListFilter")
            .flex()
            .flex_col()
            .gap(px(6.))
            .capture_key_down(cx.listener(Self::key_down))
            .child(Input::new(&self.query).small())
            .child(
                div()
                    .id("labels")
                    .max_h(px(320.))
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .children(rows)
                    .when_some(empty, |d, e| {
                        d.child(
                            div()
                                .py(px(12.))
                                .text_center()
                                .text_size(rems(0.75))
                                .text_color(t.subtle)
                                .child(e),
                        )
                    })
                    .when_some(error, |d, e| {
                        d.child(
                            div()
                                .py(px(12.))
                                .text_center()
                                .text_size(rems(0.75))
                                .child(error_text("labels-error", &e, t.muted_foreground, cx)),
                        )
                    }),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .text_size(rems(0.656))
                    .text_color(t.subtle)
                    .child(
                        div().flex_1().truncate().child(
                            self.hint
                                .clone()
                                .unwrap_or_else(|| "↑↓ navigate · ↵ select".into()),
                        ),
                    )
                    .when(has, |d| {
                        d.child(
                            Button::new("labels-none")
                                .ghost()
                                .xsmall()
                                .label("Clear")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.selected.clear();
                                    (this.on_change)(vec![], window, cx);
                                    cx.notify();
                                })),
                        )
                    }),
            )
    }
}
