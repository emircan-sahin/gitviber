//! Add co-author: the repository's recent authors and co-authors to search, or one typed as
//! "Name <email>". Read as it opens, gone when it closes.

use crate::model::text::{author_name, is_co_author};
use gitviber_core::api;
use gitviber_ui::ActiveAppTheme as _;
use gitviber_ui::memory::Live;
use gitviber_ui::widgets::{Ends, list_step};
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{Icon, Sizable as _, v_flex};
use gpui_kit::{
    App, AppContext as _, Context, Entity, FocusHandle, Focusable, InteractiveElement as _,
    IntoElement, KeyDownEvent, ParentElement as _, Render, StatefulInteractiveElement as _,
    Styled as _, Subscription, Task, Window, div, prelude::FluentBuilder as _, px,
};
use std::rc::Rc;

const SHOWN: usize = 8;

pub type OnAdd = Rc<dyn Fn(String, &mut Window, &mut App)>;

pub struct CoAuthorPicker {
    query: Entity<InputState>,
    /// None until read.
    authors: Option<Vec<String>>,
    taken: Vec<String>,
    index: Option<usize>,
    on_add: OnAdd,
    _subscription: Subscription,
    _load: Task<()>,
    _live: Live,
}

impl CoAuthorPicker {
    pub fn new(
        taken: Vec<String>,
        on_add: OnAdd,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let query = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Search recent authors, or Name <email>")
        });
        let subscription = cx.subscribe(&query, |this, _, e: &InputEvent, cx| {
            if matches!(e, InputEvent::Change) {
                this.index = Some(0);
                cx.notify();
            }
        });
        let core = crate::core(cx);
        let load = cx.spawn(async move |this, cx| {
            let authors = cx
                .background_spawn(async move { api::changes::recent_authors(&core) })
                .await
                .unwrap_or_default();
            this.update(cx, |this, cx| {
                this.authors = Some(authors);
                cx.notify();
            })
            .ok();
        });
        CoAuthorPicker {
            query,
            authors: None,
            taken,
            index: Some(0),
            on_add,
            _subscription: subscription,
            _load: load,
            _live: Live::new("CoAuthorPicker"),
        }
    }

    /// What it offers: the text typed, when it's a whole co-author, then the authors it finds.
    fn options(&self, cx: &App) -> (Option<String>, Vec<String>) {
        let q = self.query.read(cx).value().trim().to_string();
        let needle = q.to_lowercase();
        let found: Vec<String> = self
            .authors
            .iter()
            .flatten()
            .filter(|a| !self.taken.contains(a) && a.to_lowercase().contains(&needle))
            .take(SHOWN)
            .cloned()
            .collect();
        let typed =
            (is_co_author(&q) && !self.taken.contains(&q) && !found.contains(&q)).then_some(q);
        (typed, found)
    }

    fn rows(&self, cx: &App) -> Vec<String> {
        let (typed, found) = self.options(cx);
        typed.into_iter().chain(found).collect()
    }

    fn add(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(a) = self.rows(cx).get(ix).cloned() {
            (self.on_add)(a, window, cx);
        }
    }

    fn on_key(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let m = e.keystroke.modifiers;
        if m.platform || m.control || m.alt || m.shift {
            return;
        }
        let n = self.rows(cx).len();
        let typed = !self.query.read(cx).value().is_empty();
        if let Some(to) = list_step(&e.keystroke, self.index, n, Ends::Stop, typed) {
            self.index = Some(to);
            cx.stop_propagation();
            cx.notify();
            return;
        }
        match e.keystroke.key.as_str() {
            "enter" => {
                if let Some(i) = self.index {
                    self.add(i, window, cx);
                }
            }
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }
}

impl Focusable for CoAuthorPicker {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.query.focus_handle(cx)
    }
}

impl Render for CoAuthorPicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.app_theme().clone();
        let (typed, found) = self.options(cx);
        let typed_one = typed.clone();
        let rows: Vec<String> = typed.into_iter().chain(found).collect();
        let empty = match (&self.authors, self.query.read(cx).value().trim().is_empty()) {
            (None, _) => "Loading…",
            (Some(_), false) => "No match. Type it as Name <email@example.com>.",
            (Some(_), true) => "No recent authors yet. Type Name <email@example.com>.",
        };
        v_flex()
            .w(px(320.))
            .p_1()
            .gap_1()
            .on_key_down(cx.listener(Self::on_key))
            .child(Input::new(&self.query).small())
            .child(
                v_flex()
                    .id("co-author-rows")
                    .max_h(px(224.))
                    .overflow_y_scroll()
                    .children(rows.iter().enumerate().map(|(i, a)| {
                        let hot = self.index == Some(i);
                        let name = author_name(a).to_string();
                        let email = a[name.len().min(a.len())..].trim().to_string();
                        div()
                            .id(("co-author", i))
                            .flex()
                            .items_center()
                            .gap_2()
                            .h(px(28.))
                            .px_2()
                            .rounded(px(3.))
                            .text_size(px(12.))
                            .cursor_pointer()
                            .when(hot, |d| d.bg(t.primary).text_color(t.primary_foreground))
                            .when(typed_one.as_deref() == Some(a.as_str()), |d| {
                                d.child(Icon::new(Lucide::Plus).xsmall())
                            })
                            .child(div().min_w_0().truncate().child(name))
                            .child(
                                div()
                                    .ml_auto()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(11.))
                                    .opacity(0.7)
                                    .child(email),
                            )
                            .on_mouse_move(cx.listener(move |this, _, _, cx| {
                                if this.index != Some(i) {
                                    this.index = Some(i);
                                    cx.notify();
                                }
                            }))
                            .on_click(
                                cx.listener(move |this, _, window, cx| this.add(i, window, cx)),
                            )
                    }))
                    .when(rows.is_empty(), |el| {
                        el.child(
                            div()
                                .px_2()
                                .py_2()
                                .text_size(px(11.5))
                                .text_color(t.subtle)
                                .child(empty),
                        )
                    }),
            )
    }
}
