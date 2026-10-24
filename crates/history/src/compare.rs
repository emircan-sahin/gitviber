//! Two points of history against each other, as a screen of its own: the commits one has that
//! the other lacks, the files between them, and merging. Either side is a branch, a remote
//! branch, a tag or a commit; from their merge base (what a pull request shows) or as they stand.

use std::rc::Rc;

use gitviber_core::{api, git};
use gitviber_refs::RepoState;
use gitviber_refs::names::plural;
use gitviber_refs::points::{ComparePoint, PointKind, github_compare_url, point_kind};
use gitviber_ui::memory::Live;
use gitviber_ui::workspace::{ComparePoint as UiPoint, Selection, TabIcon, TabItem};
use gitviber_ui::{ActiveAppTheme, Workspace};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::{Disableable as _, Icon, Selectable as _, Sizable as _};
use gpui_kit::{
    App, AppContext as _, Context, Entity, FocusHandle, FontWeight, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, SharedString, StatefulInteractiveElement as _,
    Styled as _, Subscription, Task, WeakEntity, Window, div, prelude::FluentBuilder as _, px,
    rems,
};

use crate::files::FileList;
use crate::list::{CommitList, ListOptions, Source};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    Ahead,
    Behind,
}

pub struct CompareView {
    repo: Option<Entity<RepoState>>,
    workspace: WeakEntity<Workspace>,
    base: ComparePoint,
    head: ComparePoint,
    merge_base: bool,
    comparison: Option<Result<Rc<git::Comparison>, SharedString>>,
    /// Ahead first, as it's what a pull request is made of; Behind when that's empty.
    side: Option<Side>,
    commits: Option<Entity<CommitList>>,
    files: Entity<FileList>,
    merge: Option<git::MergeCheck>,
    /// Where the branches stood when last read: a fetch or a commit moving them reads again.
    tips: String,
    focus: FocusHandle,
    _read: Task<()>,
    _check: Task<()>,
    _repo: Option<Subscription>,
    _live: Live,
}

impl CompareView {
    pub fn new(
        base: ComparePoint,
        head: ComparePoint,
        merge_base: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let weak = cx.weak_entity();
        let files = cx.new(|cx| {
            FileList::new(
                "No differences.",
                Rc::new(move |f, pin, window, cx| {
                    let _ = weak.update(cx, |this, cx| this.open_file(f, pin, window, cx));
                }),
                cx,
            )
        });
        // Built inside the workspace's own update: its project is looked up once that's over.
        cx.defer_in(window, |this, window, cx| this.attach(window, cx));
        CompareView {
            repo: None,
            workspace: WeakEntity::new_invalid(),
            base,
            head,
            merge_base,
            comparison: None,
            side: None,
            commits: None,
            files,
            merge: None,
            tips: String::new(),
            focus: cx.focus_handle(),
            _read: Task::ready(()),
            _check: Task::ready(()),
            _repo: None,
            _live: Live::new("CompareView"),
        }
    }

    fn attach(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ws) = Workspace::for_window(window, cx) else {
            return;
        };
        let Some(project) = ws.read(cx).project().cloned() else {
            return;
        };
        let repo = RepoState::get(&project, cx);
        self._repo = Some(cx.subscribe(
            &repo,
            |this: &mut Self, _, e: &gitviber_refs::RepoEvent, cx| {
                if let gitviber_refs::RepoEvent::Changed = e {
                    let tips = this.tips_now(cx);
                    if tips != this.tips {
                        this.read(cx);
                    }
                }
            },
        ));
        self.workspace = ws.downgrade();
        self.repo = Some(repo);
        self.read(cx);
    }

    fn tips_now(&self, cx: &App) -> String {
        let Some(repo) = &self.repo else {
            return String::new();
        };
        let r = repo.read(cx);
        let mut tips = r.head().unwrap_or_default().to_string();
        for b in r.branches.iter() {
            tips.push_str(&b.name);
            tips.push_str(&b.sha);
        }
        tips
    }

    /// What the tab shows, as the workspace saves it.
    pub fn selection(&self) -> Selection {
        let ui = |p: &ComparePoint| UiPoint {
            reference: p.r#ref.clone(),
            label: p.label.clone(),
        };
        Selection::Compare {
            base: ui(&self.base),
            head: ui(&self.head),
            merge_base: self.merge_base,
        }
    }

    /// Shows other sides; a side left out stays.
    pub fn set_points(
        &mut self,
        base: Option<ComparePoint>,
        head: Option<ComparePoint>,
        cx: &mut Context<Self>,
    ) {
        let (b, h) = (
            base.unwrap_or_else(|| self.base.clone()),
            head.unwrap_or_else(|| self.head.clone()),
        );
        if b == self.base && h == self.head {
            return;
        }
        self.base = b;
        self.head = h;
        self.side = None;
        self.read(cx);
    }

    /// Tells the workspace what's shown now, so a restored tab comes back to it.
    fn changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let sel = self.selection();
        if let Some(ws) = self.workspace.upgrade() {
            ws.update(cx, |ws, cx| ws.open(sel, false, window, cx));
        }
        self.read(cx);
    }

    fn read(&mut self, cx: &mut Context<Self>) {
        let core = gitviber_ui::core(cx);
        self.tips = self.tips_now(cx);
        let (base, head, mb) = (
            self.base.r#ref.clone(),
            self.head.r#ref.clone(),
            self.merge_base,
        );
        let read = cx.background_spawn(async move { api::history::compare(&core, base, head, mb) });
        self._read = cx.spawn(async move |this, cx| {
            let c = read.await;
            let _ = this.update(cx, |this, cx| {
                match c {
                    Ok(c) => {
                        let c = Rc::new(c);
                        let (b, h) = (&this.base.label, &this.head.label);
                        let empty = match (c.ahead > 0 || c.behind > 0, this.merge_base) {
                            (true, true) => {
                                format!("{h} changes no files since it parted from {b}.")
                            }
                            (true, false) => "Both sides hold the same files.".into(),
                            (false, _) => "No differences.".into(),
                        };
                        this.files.update(cx, |f, cx| {
                            f.set_empty(empty, cx);
                            f.set(Ok(c.files.clone()), cx)
                        });
                        this.comparison = Some(Ok(c));
                        this.list_side(cx);
                        this.check_merge(cx);
                    }
                    Err(e) => {
                        this.comparison = Some(Err(e.into()));
                        this.commits = None;
                    }
                }
                cx.notify();
            });
        });
        cx.notify();
    }

    fn side(&self) -> Side {
        let Some(Ok(c)) = &self.comparison else {
            return Side::Ahead;
        };
        self.side.unwrap_or(if c.ahead == 0 && c.behind > 0 {
            Side::Behind
        } else {
            Side::Ahead
        })
    }

    fn list_side(&mut self, cx: &mut Context<Self>) {
        let (Some(Ok(c)), Some(repo)) = (&self.comparison, &self.repo) else {
            return;
        };
        let (b, h) = (self.base.label.clone(), self.head.label.clone());
        let (source, empty) = match self.side() {
            Side::Ahead => (
                Source::Between {
                    base: c.base.clone(),
                    head: c.head.clone(),
                },
                format!("{h} has nothing {b} lacks."),
            ),
            Side::Behind => (
                Source::Between {
                    base: c.head.clone(),
                    head: c.base.clone(),
                },
                format!("{b} has nothing {h} lacks."),
            ),
        };
        let options = ListOptions {
            empty: empty.into(),
            ..Default::default()
        };
        match &self.commits {
            Some(list) => list.update(cx, |l, cx| l.set_source(source, options, cx)),
            None => {
                let (repo, ws) = (repo.clone(), self.workspace.clone());
                self.commits = Some(cx.new(|cx| CommitList::new(repo, ws, source, options, cx)));
            }
        }
    }

    /// What merging Compare into the branch checked out would bring, and whether it would conflict.
    fn check_merge(&mut self, cx: &mut Context<Self>) {
        self.merge = None;
        let (Some(Ok(c)), Some(repo)) = (&self.comparison, &self.repo) else {
            return;
        };
        let r = repo.read(cx);
        // Not into itself, nor while a merge or rebase waits on its own conflicts.
        let asks = r.branch().is_some()
            && r.head().is_some_and(|h| h != c.head)
            && r.status.as_ref().is_some_and(|s| s.operation.is_none());
        if !asks {
            return;
        }
        let core = gitviber_ui::core(cx);
        let with = self.head.r#ref.clone();
        let read = cx.background_spawn(async move { api::history::merge_check(&core, with) });
        self._check = cx.spawn(async move |this, cx| {
            let check = read.await.ok();
            let _ = this.update(cx, |this, cx| {
                this.merge = check;
                cx.notify();
            });
        });
    }

    fn open_file(
        &mut self,
        f: &git::FileChange,
        pin: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(Ok(c)) = &self.comparison else {
            return;
        };
        let range = crate::open::Range {
            base: c.from.clone(),
            head: c.head.clone(),
            label: self.label(),
        };
        crate::open::open_range_file(&self.workspace, range, f, !pin, window, cx);
    }

    /// `base...head` from the merge base, `base..head` as they stand.
    fn label(&self) -> String {
        format!(
            "{}{}{}",
            self.base.label,
            if self.merge_base { "..." } else { ".." },
            self.head.label
        )
    }

    fn pick(&mut self, base_side: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(repo) = &self.repo else { return };
        let r = repo.read(cx);
        let branches = r.branches.clone();
        let detached = r.branch().is_none();
        let picked = Some(if base_side {
            self.base.r#ref.clone()
        } else {
            self.head.r#ref.clone()
        });
        let weak = cx.weak_entity();
        let offer = crate::picker::Offer {
            title: (if base_side { "Base" } else { "Compare" }).into(),
            branches,
            detached,
            with_tags: true,
            picked,
        };
        crate::picker::open_ref_picker(
            offer,
            Rc::new(move |point, window, cx| {
                let _ = weak.update(cx, |this, cx| {
                    if base_side {
                        this.base = point;
                    } else {
                        this.head = point;
                    }
                    this.side = None;
                    this.changed(window, cx);
                });
            }),
            window,
            cx,
        );
    }

    fn render_point(
        &self,
        id: &'static str,
        side: &'static str,
        p: &ComparePoint,
        base_side: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let t = cx.app_theme().clone();
        let icon = match point_kind(&p.r#ref) {
            PointKind::Head | PointKind::Commit => IconName::GitCommitHorizontal,
            PointKind::Branch => IconName::GitBranch,
            PointKind::Remote => IconName::Cloud,
            PointKind::Tag => IconName::Tag,
        };
        div()
            .id(id)
            .flex()
            .h(px(28.))
            .max_w(px(320.))
            .min_w_0()
            .items_center()
            .gap(px(6.))
            .px_2()
            .rounded(px(6.))
            .border_1()
            .border_color(t.border_strong)
            .cursor_pointer()
            .hover(|s| s.bg(t.hover))
            .child(
                div()
                    .flex_none()
                    .text_size(rems(0.65625))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(t.subtle)
                    .child(side.to_uppercase()),
            )
            .child(Icon::new(icon).small().text_color(t.primary))
            .child(
                div()
                    .truncate()
                    .font_family(t.fonts.code.clone())
                    .text_size(rems(0.75))
                    .child(p.label.clone()),
            )
            .child(
                Icon::new(IconName::ChevronsUpDown)
                    .xsmall()
                    .text_color(t.subtle),
            )
            .on_click(cx.listener(move |this, _, window, cx| this.pick(base_side, window, cx)))
    }

    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.app_theme().clone();
        let mb = self.merge_base;
        let seg = |id: &'static str,
                   label: &'static str,
                   on: bool,
                   value: bool,
                   cx: &mut Context<Self>| {
            Button::new(id)
                .xsmall()
                .label(label)
                .when(on, |b| b.selected(true))
                .ghost()
                .on_click(cx.listener(move |this, _, window, cx| {
                    if this.merge_base != value {
                        this.merge_base = value;
                        this.changed(window, cx);
                    }
                }))
        };
        div()
            .flex()
            .flex_none()
            .flex_wrap()
            .min_h(px(36.))
            .items_center()
            .gap_2()
            .py_1()
            .pl_3()
            .pr_2()
            .border_b_1()
            .border_color(t.border)
            .child(self.render_point("base", "Base", &self.base, true, cx))
            .child(Button::new("swap").ghost().xsmall().icon(IconName::ArrowLeftRight).tooltip("Swap sides").on_click(cx.listener(|this, _, window, cx| {
                std::mem::swap(&mut this.base, &mut this.head);
                this.side = None;
                this.changed(window, cx);
            })))
            .child(self.render_point("head", "Compare", &self.head, false, cx))
            .child(
                div()
                    .ml_auto()
                    .flex()
                    .gap_px()
                    .p_px()
                    .rounded(px(6.))
                    .bg(t.elevated)
                    .child(seg("mb", "Merge base", mb, true, cx).tooltip("Only what Compare changed since the two parted, as a pull request shows it."))
                    .child(seg("direct", "Direct", !mb, false, cx).tooltip("Every difference between the two as they stand now, which also undoes what Base gained since they parted.")),
            )
    }

    fn render_summary(&self, c: &git::Comparison, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.app_theme().clone();
        let (add, del) = self.files.read(cx).counts();
        let web = self
            .repo
            .as_ref()
            .and_then(|r| r.read(cx).status.as_ref()?.web_url.clone().flatten());
        let url = github_compare_url(web.as_deref(), &self.base, &self.head, self.merge_base);
        let mono = t.fonts.code.clone();
        let standing = if c.ahead > 0 || c.behind > 0 {
            [
                (c.ahead > 0).then(|| format!("{} ahead", c.ahead)),
                (c.behind > 0).then(|| format!("{} behind", c.behind)),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" and ")
        } else {
            "up to date with".into()
        };
        let range = (c.from.clone(), c.head.clone(), self.label());
        div()
            .flex()
            .flex_none()
            .flex_wrap()
            .items_center()
            .gap_x_3()
            .gap_y_1p5()
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(t.border)
            .text_size(rems(0.75))
            .child(
                div()
                    .flex()
                    .min_w_0()
                    .gap_1()
                    .text_color(t.muted_foreground)
                    .child(div().font_family(mono.clone()).text_color(t.foreground).child(self.head.label.clone()))
                    .child("is")
                    .child(div().font_weight(FontWeight::SEMIBOLD).text_color(t.foreground).child(standing))
                    .child(div().font_family(mono).text_color(t.foreground).child(self.base.label.clone()))
                    .when(!c.files.is_empty(), |d| {
                        d.child(div().px_1().text_color(t.subtle).child("·"))
                            .child(plural(c.files.len(), "file"))
                            .child(div().text_color(t.added).child(format!("+{add}")))
                            .child(div().text_color(t.removed).child(format!("−{del}")))
                    }),
            )
            .child(
                div()
                    .ml_auto()
                    .flex()
                    .gap_1p5()
                    .when_some(url, |d, url| {
                        d.child(Button::new("on-github").ghost().small().icon(IconName::ExternalLink).label("Open on GitHub").on_click(move |_, _, cx| cx.open_url(&url)))
                    })
                    .child(Button::new("open-all").small().icon(IconName::Files).label("Open All").disabled(c.files.is_empty()).on_click(cx.listener(move |this, _, window, cx| {
                        let (base, head, label) = range.clone();
                        crate::open::open_range(&this.workspace, base, head, label, window, cx);
                    }))),
            )
            .when(c.unrelated, |d| {
                d.child(div().w_full().text_size(rems(0.71875)).text_color(t.modified).child("These have no commit in common, so what's listed is every difference between them."))
            })
            .children(self.render_merge(cx))
    }

    fn render_merge(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let check = self.merge.as_ref().filter(|m| m.incoming > 0)?;
        let repo = self.repo.clone()?;
        let into = repo.read(cx).branch()?.to_string();
        let busy = repo.read(cx).busy().is_some();
        let t = cx.app_theme().clone();
        let off = busy || check.unrelated;
        let name = self.head.r#ref.clone();
        let go = move |how: git::MergeKind| {
            let (repo, name) = (repo.clone(), name.clone());
            move |_: &gpui_kit::ClickEvent, window: &mut Window, cx: &mut App| {
                gitviber_refs::ops::merge(&repo, name.clone(), how, false, window, cx)
            }
        };
        let into2 = into.clone();
        let menu_go = go.clone();
        let text: SharedString = if check.unrelated {
            "No commit in common: git won't merge them.".into()
        } else if let Some(c) = check.conflicts.as_ref().filter(|c| !c.is_empty()) {
            let list = c.iter().take(3).cloned().collect::<Vec<_>>().join(", ");
            format!(
                "{} would conflict: {list}{}",
                plural(c.len(), "file"),
                if c.len() > 3 { ", …" } else { "" }
            )
            .into()
        } else {
            format!(
                "This merges {} from {} into {into}{}.",
                plural(check.incoming as usize, "commit"),
                self.head.label,
                if check.conflicts.is_some() {
                    ", with no conflicts"
                } else {
                    ""
                }
            )
            .into()
        };
        let warn = check.unrelated || check.conflicts.as_ref().is_some_and(|c| !c.is_empty());
        Some(
            div()
                .w_full()
                .flex()
                .flex_wrap()
                .items_center()
                .gap_2()
                .text_size(rems(0.71875))
                .child(
                    div()
                        .flex()
                        .child(
                            Button::new("merge")
                                .small()
                                .primary()
                                .icon(IconName::GitMerge)
                                .label(format!("Merge into {into}"))
                                .disabled(off)
                                .on_click(go(git::MergeKind::Ff)),
                        )
                        .child(
                            Button::new("merge-more")
                                .small()
                                .primary()
                                .icon(IconName::ChevronDown)
                                .disabled(off)
                                .dropdown_menu(move |m, _, _| {
                                    m.item(
                                        PopupMenuItem::new(format!("Merge into {into2}"))
                                            .on_click(menu_go(git::MergeKind::Ff)),
                                    )
                                    .item(
                                        PopupMenuItem::new(format!(
                                            "Merge into {into2} (No Fast-forward)"
                                        ))
                                        .on_click(menu_go(git::MergeKind::NoFf)),
                                    )
                                    .item(
                                        PopupMenuItem::new(format!(
                                            "Squash and Merge into {into2}"
                                        ))
                                        .on_click(menu_go(git::MergeKind::Squash)),
                                    )
                                }),
                        ),
                )
                .child(
                    div()
                        .min_w_0()
                        .text_color(if warn { t.modified } else { t.muted_foreground })
                        .child(text),
                ),
        )
    }
}

impl TabItem for CompareView {
    fn tab_title(&self, _: &App) -> SharedString {
        "Compare".into()
    }

    fn tab_detail(&self, _: &App) -> Option<SharedString> {
        Some(self.label().into())
    }

    fn tab_icon(&self, _: &App) -> TabIcon {
        TabIcon::Icon(IconName::GitCompareArrows)
    }

    fn tab_focus(&self, _: &App) -> Option<FocusHandle> {
        Some(self.focus.clone())
    }
}

impl Render for CompareView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.app_theme().clone();
        let centered = |text: SharedString| {
            div()
                .flex()
                .flex_1()
                .min_h_0()
                .items_center()
                .justify_center()
                .p_6()
                .text_center()
                .text_size(rems(0.78125))
                .text_color(t.muted_foreground)
                .child(text)
        };
        let body = match &self.comparison {
            None => centered("Loading…".into()).into_any_element(),
            Some(Err(e)) => centered(e.clone()).into_any_element(),
            Some(Ok(c)) if c.base == c.head => centered(format!("{} and {} are at the same commit. Pick two points that differ to compare them.", self.head.label, self.base.label).into())
                .into_any_element(),
            Some(Ok(c)) => {
                let c = c.clone();
                let side = self.side();
                let seg = |id: &'static str, label: String, s: Side, cx: &mut Context<Self>| {
                    Button::new(id).xsmall().ghost().label(label).when(side == s, |b| b.selected(true)).on_click(cx.listener(move |this, _, _, cx| {
                        this.side = Some(s);
                        this.list_side(cx);
                        cx.notify();
                    }))
                };
                let (b, h) = (self.base.label.clone(), self.head.label.clone());
                let between = if self.merge_base && !c.unrelated { format!("{h} since it parted from {b}") } else { format!("{b} → {h}") };
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .child(self.render_summary(&c, cx))
                    .child(
                        div()
                            .flex()
                            .flex_1()
                            .min_h_0()
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .flex_1()
                                    .min_w_0()
                                    .border_r_1()
                                    .border_color(t.border)
                                    .child(
                                        div()
                                            .flex()
                                            .flex_none()
                                            .h(px(32.))
                                            .items_center()
                                            .gap_2()
                                            .px_3()
                                            .border_b_1()
                                            .border_color(t.border)
                                            .child(div().flex().gap_px().p_px().rounded(px(6.)).bg(t.elevated).child(seg("ahead", format!("Ahead {}", c.ahead), Side::Ahead, cx)).child(seg(
                                                "behind",
                                                format!("Behind {}", c.behind),
                                                Side::Behind,
                                                cx,
                                            )))
                                            .child(div().min_w_0().truncate().text_size(rems(0.6875)).text_color(t.subtle).child(match side {
                                                Side::Ahead => format!("in {h}, not {b}"),
                                                Side::Behind => format!("in {b}, not {h}"),
                                            })),
                                    )
                                    .child(div().flex_1().min_h_0().children(self.commits.clone())),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .flex_1()
                                    .min_w_0()
                                    .child(
                                        div()
                                            .flex()
                                            .flex_none()
                                            .h(px(32.))
                                            .items_center()
                                            .gap_2()
                                            .px_3()
                                            .text_size(rems(0.65625))
                                            .text_color(t.subtle)
                                            .child(div().font_weight(FontWeight::SEMIBOLD).child("CHANGED FILES"))
                                            .child(div().font_family(t.fonts.code.clone()).text_color(t.muted_foreground).child(c.files.len().to_string()))
                                            .child(div().min_w_0().truncate().text_size(rems(0.6875)).child(between)),
                                    )
                                    .child(div().flex_1().min_h_0().child(self.files.clone())),
                            ),
                    )
                    .into_any_element()
            }
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .track_focus(&self.focus)
            .bg(t.background)
            .child(self.render_toolbar(cx))
            .child(body)
    }
}
