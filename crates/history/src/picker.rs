//! Picking a point of history in a dialog: a branch, a remote branch, a tag, or a commit by its
//! id typed or pasted. Type to narrow, ↑/↓ and Enter or a click to pick.

use std::rc::Rc;

use gitviber_core::api;
use gitviber_refs::Branch;
use gitviber_refs::points::{ComparePoint, branch_point, commit_point, head_point, tag_point};
use gitviber_ui::ActiveAppTheme;
use gitviber_ui::memory::Live;
use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{Icon, Sizable as _, WindowExt as _};
use gpui_kit::{
    App, AppContext as _, Context, Entity, FocusHandle, Focusable as _, InteractiveElement as _,
    IntoElement, KeyDownEvent, ParentElement as _, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Task, Window, div,
    prelude::FluentBuilder as _, px, rems,
};

/// A repo can have thousands of tags: the rest wait for a longer search.
const SHOWN: usize = 80;

pub type OnPick = Rc<dyn Fn(ComparePoint, &mut Window, &mut App)>;

#[derive(Clone)]
struct Row {
    group: &'static str,
    point: ComparePoint,
    icon: IconName,
    note: Option<SharedString>,
    current: bool,
}

pub struct RefPicker {
    input: Entity<InputState>,
    branches: Rc<Vec<Branch>>,
    /// Detached: HEAD is offered as a point of its own.
    detached: bool,
    with_tags: bool,
    tags: Vec<String>,
    commit: Option<(String, ComparePoint, SharedString)>,
    picked: Option<String>,
    rows: Vec<Row>,
    index: usize,
    on_pick: OnPick,
    _tags: Option<Task<()>>,
    _lookup: Option<Task<()>>,
    _sub: Subscription,
    _live: Live,
}

fn sha_like(q: &str) -> bool {
    (4..=40).contains(&q.len()) && q.chars().all(|c| c.is_ascii_hexdigit())
}

impl RefPicker {
    fn query(&self, cx: &App) -> String {
        self.input.read(cx).value().trim().to_string()
    }

    fn rank(&mut self, cx: &mut Context<Self>) {
        let q = self.query(cx);
        let lower = q.to_lowercase();
        let m = |name: &str| name.to_lowercase().contains(&lower);
        let mut heads: Vec<&Branch> = self
            .branches
            .iter()
            .filter(|b| !b.name.ends_with("/HEAD") && m(&b.name))
            .collect();
        heads.sort_by_key(|b| (std::cmp::Reverse(b.current), std::cmp::Reverse(b.timestamp)));
        let mut rows = vec![];
        if self.detached && m("HEAD") {
            rows.push(Row {
                group: "Checked out",
                point: head_point(),
                icon: IconName::GitCommitHorizontal,
                note: Some("detached".into()),
                current: false,
            });
        }
        rows.extend(heads.iter().filter(|b| !b.remote).take(SHOWN).map(|b| Row {
            group: "Branches",
            point: branch_point(&b.name, false),
            icon: IconName::GitBranch,
            note: None,
            current: b.current,
        }));
        rows.extend(heads.iter().filter(|b| b.remote).take(SHOWN).map(|b| Row {
            group: "Remote branches",
            point: branch_point(&b.name, true),
            icon: IconName::Cloud,
            note: None,
            current: false,
        }));
        rows.extend(self.tags.iter().filter(|t| m(t)).take(SHOWN).map(|t| Row {
            group: "Tags",
            point: tag_point(t),
            icon: IconName::Tag,
            note: None,
            current: false,
        }));
        if let Some((asked, point, subject)) = &self.commit
            && *asked == q
        {
            rows.push(Row {
                group: "Commit",
                point: point.clone(),
                icon: IconName::GitCommitHorizontal,
                note: Some(subject.clone()),
                current: false,
            });
        }
        self.rows = rows;
        self.index = self.index.min(self.rows.len().saturating_sub(1));
        cx.notify();
    }

    fn changed(&mut self, cx: &mut Context<Self>) {
        self.index = 0;
        let q = self.query(cx);
        if self.with_tags && sha_like(&q) && self.commit.as_ref().is_none_or(|c| c.0 != q) {
            let core = gitviber_ui::core(cx);
            let asked = q.clone();
            let find = cx.background_spawn(async move { api::history::find_commit(&core, asked) });
            self._lookup = Some(cx.spawn(async move |this, cx| {
                let found = find.await.ok().flatten();
                let _ = this.update(cx, |this, cx| {
                    this.commit = found.map(|c| (q, commit_point(&c.sha), c.subject.into()));
                    this.rank(cx);
                });
            }));
        }
        self.rank(cx);
    }

    fn pick(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(row) = self.rows.get(i).cloned() else {
            return;
        };
        window.close_dialog(cx);
        (self.on_pick)(row.point, window, cx);
    }

    fn key(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        match e.keystroke.key.as_str() {
            "down" => self.index = (self.index + 1).min(self.rows.len().saturating_sub(1)),
            "up" => self.index = self.index.saturating_sub(1),
            "enter" => return self.pick(self.index, window, cx),
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }

    fn focus(&self, cx: &App) -> FocusHandle {
        self.input.focus_handle(cx)
    }
}

impl Render for RefPicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.app_theme().clone();
        let mut last_group = "";
        let mut children = vec![];
        for (i, row) in self.rows.iter().enumerate() {
            if row.group != last_group {
                last_group = row.group;
                children.push(
                    div()
                        .px_1()
                        .pt_2()
                        .pb_1()
                        .text_size(rems(0.65625))
                        .font_weight(t.semibold())
                        .text_color(t.subtle)
                        .child(row.group.to_uppercase())
                        .into_any_element(),
                );
            }
            let hot = i == self.index;
            let picked = self.picked.as_deref() == Some(row.point.r#ref.as_str());
            children.push(
                div()
                    .id(("point", i))
                    .flex()
                    .h(px(28.))
                    .items_center()
                    .gap_2()
                    .px_2()
                    .rounded(px(4.))
                    .text_size(rems(0.75))
                    .cursor_pointer()
                    .when(hot, |d| d.bg(t.primary).text_color(t.primary_foreground))
                    .child(
                        Icon::new(if picked { IconName::Check } else { row.icon })
                            .small()
                            .when(!picked, |i| i.opacity(0.6)),
                    )
                    .child(
                        div()
                            .flex_none()
                            .max_w(px(220.))
                            .truncate()
                            .font_family(t.fonts.code.clone())
                            .child(row.point.label.clone()),
                    )
                    .when(row.current, |d| {
                        d.child(
                            div()
                                .ml_auto()
                                .flex_none()
                                .text_size(rems(0.6875))
                                .opacity(0.7)
                                .child("current"),
                        )
                    })
                    .when_some(row.note.clone(), |d, n| {
                        d.child(
                            div()
                                .min_w_0()
                                .flex_1()
                                .truncate()
                                .text_size(rems(0.6875))
                                .opacity(0.7)
                                .child(n),
                        )
                    })
                    .on_mouse_move(cx.listener(move |this, _, _, cx| {
                        if this.index != i {
                            this.index = i;
                            cx.notify();
                        }
                    }))
                    .on_click(cx.listener(move |this, _, window, cx| this.pick(i, window, cx)))
                    .into_any_element(),
            );
        }
        let empty = if sha_like(&self.query(cx)) {
            "No commit with that id."
        } else {
            "Nothing matches."
        };
        div()
            .flex()
            .flex_col()
            .gap_1()
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, window, cx| this.key(e, window, cx)))
            .child(Input::new(&self.input))
            .child(
                div()
                    .id("points")
                    .max_h(px(320.))
                    .overflow_y_scroll()
                    .children(children)
                    .when(self.rows.is_empty(), |d| {
                        d.child(
                            div()
                                .py_3()
                                .text_center()
                                .text_size(rems(0.75))
                                .text_color(t.subtle)
                                .child(empty),
                        )
                    }),
            )
            .child(
                div()
                    .text_size(rems(0.65625))
                    .text_color(t.subtle)
                    .child(if self.with_tags {
                        "↑↓ navigate · ↵ pick · paste a commit id to compare a commit"
                    } else {
                        "↑↓ navigate · ↵ pick"
                    }),
            )
    }
}

/// What the picker offers.
pub struct Offer {
    pub title: SharedString,
    pub branches: Rc<Vec<Branch>>,
    /// Detached: HEAD is a point of its own.
    pub detached: bool,
    /// Tags and commit ids too (Compare's sides); else branches only.
    pub with_tags: bool,
    /// The point picked now, checked.
    pub picked: Option<String>,
}

/// Opens the picker; `on_pick` gets the point picked.
pub fn open_ref_picker(offer: Offer, on_pick: OnPick, window: &mut Window, cx: &mut App) {
    let Offer {
        title,
        branches,
        detached,
        with_tags,
        picked,
    } = offer;
    let input = cx.new(|cx| {
        InputState::new(window, cx).placeholder(if with_tags {
            "Branch, tag or commit id…"
        } else {
            "Go to branch…"
        })
    });
    let picker = cx.new(|cx| {
        let sub = cx.subscribe(&input, |this: &mut RefPicker, _, e: &InputEvent, cx| {
            if let InputEvent::Change = e {
                this.changed(cx);
            }
        });
        // Asked on each open: a tag made in the terminal is there.
        let tags = with_tags.then(|| {
            let core = gitviber_ui::core(cx);
            let read = cx.background_spawn(async move { api::branches::tags(&core) });
            cx.spawn(async move |this, cx| {
                let tags = read.await.unwrap_or_default();
                let _ = this.update(cx, |this, cx| {
                    this.tags = tags;
                    this.rank(cx);
                });
            })
        });
        let mut p = RefPicker {
            input: input.clone(),
            branches,
            detached,
            with_tags,
            tags: vec![],
            commit: None,
            picked,
            rows: vec![],
            index: 0,
            on_pick,
            _tags: tags,
            _lookup: None,
            _sub: sub,
            _live: Live::new("RefPicker"),
        };
        p.rank(cx);
        p
    });
    let first = std::cell::Cell::new(true);
    window.open_dialog(cx, move |dialog, window, cx| {
        if first.replace(false) {
            picker.read(cx).focus(cx).focus(window, cx);
        }
        dialog
            .title(title.clone())
            .w(px(380.))
            .child(picker.clone())
    });
}
