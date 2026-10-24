//! What the PRs and Issues panels share above their lists: Open / Closed / All with counts, the
//! chips that narrow it (whose work, drafts, an author), the labels picked, and a box that
//! filters the rows loaded.

use gitviber_ui::AppTheme;
use gitviber_ui::widgets::CountBadge;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, Div, Entity, FontWeight, InteractiveElement as _,
    IntoElement, ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _,
    Subscription, Window, div, prelude::FluentBuilder as _, px, rems,
};

use crate::logic::narrow::{Choice, Filter, ListKind, Narrow, Scope, compact_count, scopes_for};
use crate::model::{Counts, LabelItem};
use crate::store::Store;
use crate::ui::common::{label_dot, theme};

/// A list panel whose controls these are.
pub trait ListPanel: 'static + Sized {
    fn controls(&mut self) -> &mut ListControls;
    /// What narrows the list changed: back to its first page, and read again.
    fn narrowed(&mut self, window: &mut Window, cx: &mut Context<Self>);
}

pub struct ListControls {
    pub kind: ListKind,
    /// The repository's main worktree: what the chips' choice is kept under.
    pub main: String,
    pub filter: Filter,
    pub choice: Choice,
    pub labels: Vec<LabelItem>,
    pub author: Option<String>,
    pub find: Entity<InputState>,
    pub needle: String,
    _find: Subscription,
}

impl ListControls {
    pub fn new<P: ListPanel>(
        kind: ListKind,
        main: String,
        window: &mut Window,
        cx: &mut Context<P>,
    ) -> Self {
        let placeholder = if kind == ListKind::Pulls {
            "Filter loaded pull requests"
        } else {
            "Filter loaded issues"
        };
        let find = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
        let sub = cx.subscribe_in(
            &find,
            window,
            |this: &mut P, input, ev: &InputEvent, _, cx| {
                if matches!(ev, InputEvent::Change) {
                    this.controls().needle = input.read(cx).value().trim().to_lowercase();
                    cx.notify();
                }
            },
        );
        let choice = Store::get(cx).choice(&main, kind);
        ListControls {
            kind,
            main,
            filter: Filter::Open,
            choice,
            labels: vec![],
            author: None,
            find,
            needle: String::new(),
            _find: sub,
        }
    }

    /// The chips that name the account wait on it, but not for it: the saved one lists at once.
    /// Once it can't be read (`me_usable` false) everyone's are listed, and the choice stays.
    pub fn narrow(&self, me_usable: bool) -> Narrow {
        Narrow {
            scope: if me_usable { self.choice.scope } else { None },
            draft: self.choice.draft,
            labels: self.labels.iter().map(|l| l.name.clone()).collect(),
            author: self.author.clone(),
        }
    }

    pub fn set_choice(&mut self, choice: Choice, cx: &mut App) {
        // "Created by me" and an author both say who opened it: picking one lets the other go.
        if choice.scope == Some(Scope::Created) {
            self.author = None;
        }
        self.save(choice, cx);
    }

    fn save(&mut self, choice: Choice, cx: &mut App) {
        self.choice = choice;
        let (main, kind) = (self.main.clone(), self.kind);
        Store::update(cx, |s| s.set_choice(&main, kind, choice));
    }

    pub fn set_author(&mut self, login: Option<String>, cx: &mut App) {
        if login.is_some() && self.choice.scope == Some(Scope::Created) {
            self.save(
                Choice {
                    scope: None,
                    ..self.choice
                },
                cx,
            );
        }
        self.author = login;
    }

    pub fn add_label(&mut self, label: LabelItem) {
        if !self.labels.iter().any(|l| l.name == label.name) {
            self.labels.push(label);
        }
    }

    pub fn clear(&mut self, cx: &mut App) {
        self.save(Choice::default(), cx);
        self.author = None;
        self.labels.clear();
    }

    /// The find box's test, while it has text.
    pub fn matches(&self, fields: &[&str]) -> bool {
        self.needle.is_empty()
            || fields
                .iter()
                .any(|f| f.to_lowercase().contains(&self.needle))
    }
}

fn change<P: ListPanel>(
    cx: &mut Context<P>,
    f: impl Fn(&mut ListControls, &mut App) + 'static,
) -> impl Fn(&gpui_kit::ClickEvent, &mut Window, &mut App) + 'static {
    cx.listener(move |this: &mut P, _, window, cx| {
        f(this.controls(), cx);
        this.narrowed(window, cx);
        cx.notify();
    })
}

/// Open / Closed / All, each with its count when known (Closed is All less Open, so uncounted).
pub fn filter_tabs<P: ListPanel>(
    value: Filter,
    counts: Option<Counts>,
    cx: &mut Context<P>,
) -> Div {
    let t = theme(cx);
    let tabs: Vec<AnyElement> = Filter::ALL
        .into_iter()
        .map(|f| {
            let on = f == value;
            let count = counts.and_then(|c| match f {
                Filter::Open => Some(c.open),
                Filter::Closed => None,
                Filter::All => Some(c.open + c.closed),
            });
            let hover = t.foreground;
            div()
                .id(SharedString::from(format!("filter-{}", f.as_str())))
                .flex()
                .items_center()
                .gap(px(4.))
                .h(px(20.))
                .px(px(6.))
                .rounded(px(3.))
                .cursor_pointer()
                .text_size(rems(0.72))
                .font_weight(FontWeight::MEDIUM)
                .when(on, |d| d.bg(t.active).text_color(t.foreground))
                .when(!on, |d| {
                    d.text_color(t.subtle).hover(move |s| s.text_color(hover))
                })
                .child(f.label())
                .when_some(count, |d, n| {
                    d.child(CountBadge::new(compact_count(n)).active(on))
                })
                .on_click(change(cx, move |c, _| c.filter = f))
                .into_any_element()
        })
        .collect();
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(2.))
        .children(tabs)
}

fn chip(
    id: &'static str,
    label: impl Into<SharedString>,
    on: bool,
    off: Option<&'static str>,
    t: &AppTheme,
) -> gpui_kit::Stateful<Div> {
    let hover = t.hover;
    div()
        .id(id)
        .flex_none()
        .h(px(20.))
        .px(px(8.))
        .flex()
        .items_center()
        .rounded_full()
        .border_1()
        .text_size(rems(0.69))
        .when(on, |d| {
            d.border_color(t.primary.opacity(0.5))
                .bg(t.primary.opacity(0.15))
                .text_color(t.foreground)
        })
        .when(!on, |d| {
            d.border_color(t.border).text_color(t.muted_foreground)
        })
        .when(off.is_none() && !on, |d| {
            d.cursor_pointer().hover(move |s| s.bg(hover))
        })
        .when_some(off, |d, why| {
            d.opacity(0.5)
                .tooltip(move |window, cx| Tooltip::new(why).build(window, cx))
        })
        .child(label.into())
}

/// The chips under a list's header: All, then whose work it is, and for pull requests drafts or
/// those ready. One scope at a time; Draft and Ready toggle. `me_off`: why the ones that name the
/// account are off.
pub fn narrow_bar<P: ListPanel>(
    c: &ListControls,
    me_off: Option<&'static str>,
    cx: &mut Context<P>,
) -> Div {
    let t = theme(cx);
    let choice = c.choice;
    let mut chips: Vec<AnyElement> = vec![
        chip(
            "chip-all",
            "All",
            choice.scope.is_none() && c.author.is_none(),
            None,
            &t,
        )
        .on_click(change(cx, move |c, cx| {
            c.set_choice(
                Choice {
                    scope: None,
                    ..choice
                },
                cx,
            );
            c.author = None;
        }))
        .into_any_element(),
    ];
    for s in scopes_for(c.kind) {
        let id = s.id;
        let mut el = chip(s.key, s.label, choice.scope == Some(id), me_off, &t);
        if me_off.is_none() {
            el = el.on_click(change(cx, move |c, cx| {
                c.set_choice(
                    Choice {
                        scope: Some(id),
                        ..choice
                    },
                    cx,
                )
            }));
        }
        chips.push(el.into_any_element());
    }
    if let Some(author) = &c.author {
        chips.push(removable(
            "chip-author",
            div()
                .truncate()
                .child(format!("Author: {author}"))
                .into_any_element(),
            change(cx, |c, cx| c.set_author(None, cx)),
            &t,
        ));
    }
    if c.kind == ListKind::Pulls {
        chips.push(
            div()
                .mx(px(2.))
                .h(px(12.))
                .w(px(1.))
                .bg(t.border)
                .into_any_element(),
        );
        let draft = choice.draft;
        chips.push(
            chip("chip-draft", "Draft", draft == Some(true), None, &t)
                .on_click(change(cx, move |c, cx| {
                    c.set_choice(
                        Choice {
                            draft: if draft == Some(true) {
                                None
                            } else {
                                Some(true)
                            },
                            ..choice
                        },
                        cx,
                    )
                }))
                .into_any_element(),
        );
        chips.push(
            chip("chip-ready", "Ready", draft == Some(false), None, &t)
                .on_click(change(cx, move |c, cx| {
                    c.set_choice(
                        Choice {
                            draft: if draft == Some(false) {
                                None
                            } else {
                                Some(false)
                            },
                            ..choice
                        },
                        cx,
                    )
                }))
                .into_any_element(),
        );
    }
    div()
        .flex()
        .flex_none()
        .flex_wrap()
        .items_center()
        .gap(px(4.))
        .px(px(8.))
        .py(px(6.))
        .border_b_1()
        .border_color(t.border)
        .children(chips)
}

/// A filter the list is narrowed by, with × to take it off.
pub fn removable(
    id: &'static str,
    content: AnyElement,
    on_remove: impl Fn(&gpui_kit::ClickEvent, &mut Window, &mut App) + 'static,
    t: &AppTheme,
) -> AnyElement {
    let hover = t.hover;
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(4.))
        .max_w(px(190.))
        .pl(px(6.))
        .pr(px(2.))
        .rounded_full()
        .border_1()
        .border_color(t.border_strong)
        .bg(t.active)
        .text_size(rems(0.656))
        .child(content)
        .child(
            div()
                .id(id)
                .flex_none()
                .size(px(14.))
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .cursor_pointer()
                .hover(move |s| s.bg(hover))
                .child(Icon::new(IconName::X).xsmall().text_color(t.subtle))
                .on_click(on_remove),
        )
        .into_any_element()
}

/// The labels the list is filtered by, each to take off, under its header.
pub fn selected_labels<P: ListPanel>(labels: &[LabelItem], cx: &mut Context<P>) -> Option<Div> {
    if labels.is_empty() {
        return None;
    }
    let t = theme(cx);
    let mut row: Vec<AnyElement> = Vec::new();
    for (i, l) in labels.iter().enumerate() {
        let name = l.name.clone();
        let id: &'static str = [
            "label-0", "label-1", "label-2", "label-3", "label-4", "label-5", "label-6", "label-7",
        ]
        .get(i)
        .copied()
        .unwrap_or("label-n");
        row.push(removable(
            id,
            div()
                .flex()
                .items_center()
                .gap(px(4.))
                .child(label_dot(l, &t))
                .child(div().truncate().child(l.name.clone()))
                .into_any_element(),
            change(cx, move |c, _| c.labels.retain(|m| m.name != name)),
            &t,
        ));
    }
    if labels.len() > 1 {
        row.push(
            Button::new("labels-clear")
                .ghost()
                .xsmall()
                .label("Clear")
                .on_click(change(cx, |c, _| c.labels.clear()))
                .into_any_element(),
        );
    }
    Some(
        div()
            .flex()
            .flex_none()
            .flex_wrap()
            .items_center()
            .gap(px(4.))
            .px(px(8.))
            .py(px(6.))
            .border_b_1()
            .border_color(t.border)
            .children(row),
    )
}

/// The box that filters the loaded rows.
pub fn find_box(c: &ListControls, t: &AppTheme) -> Div {
    div()
        .flex_none()
        .px(px(8.))
        .py(px(4.))
        .border_b_1()
        .border_color(t.border)
        .child(Input::new(&c.find).xsmall().cleanable(true))
}
