//! The History panel: a search box over the commit list, the all-branches graph and its
//! choices, a bisect under way, and in the list's place a comparison with a branch or between
//! two commits.

use std::rc::Rc;
use std::time::Duration;

use gitviber_core::api;
use gitviber_refs::names::short_ref;
use gitviber_refs::points::branch_point;
use gitviber_refs::{RepoEvent, RepoState};
use gitviber_ui::keymap::history as history_actions;
use gitviber_ui::memory::Live;
use gitviber_ui::workspace::{Panel, PanelContext};
use gitviber_ui::{ActiveAppTheme, Project, Workspace};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Icon, Selectable as _, Sizable as _};
use gpui_kit::{
    Anchor, App, AppContext as _, Context, Entity, FocusHandle, Focusable, InteractiveElement as _,
    IntoElement, KeyDownEvent, ParentElement as _, Render, StatefulInteractiveElement as _,
    Styled as _, Subscription, Task, WeakEntity, Window, div, prelude::FluentBuilder as _, px,
    rems,
};

use crate::bisect::BisectBar;
use crate::commit::GraphRefs;
use crate::files::FileList;
use crate::list::{CommitList, ListEvent, ListOptions, Source};
use crate::query::{parse_log_query, with_author};
use crate::{open, store};

const DEBOUNCE: Duration = Duration::from_millis(250);
const SYNTAX: &str = "Words match the message (all of them, any case).\nauthor:name  path:src/app  code:text a commit added or removed\nA SHA or prefix finds that commit. Quotes keep spaces.";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Pane {
    Behind,
    Ahead,
    Files,
}

/// A branch (full ref) against HEAD: what each has that the other lacks, and what it changed.
struct RefCompare {
    with: String,
    pane: Pane,
    behind: Entity<CommitList>,
    ahead: Entity<CommitList>,
    files: Entity<FileList>,
    counts: Option<(u32, u32)>,
    _read: Task<()>,
}

/// Two commits, or one and the working tree (`head` None): every file they differ in.
struct Points {
    base: (String, String),
    head: Option<(String, String)>,
    files: Entity<FileList>,
    _read: Task<()>,
}

enum Mode {
    List,
    Ref(RefCompare),
    Points(Points),
}

pub struct HistoryPanel {
    repo: Entity<RepoState>,
    workspace: WeakEntity<Workspace>,
    search: Entity<InputState>,
    query: String,
    /// "Show History" of a file (followed through renames) or a folder.
    scope: Option<(String, bool)>,
    all: bool,
    refs: GraphRefs,
    list: Entity<CommitList>,
    mode: Mode,
    bisect: Option<Entity<BisectBar>>,
    /// What the branch comparison's files are diffed between: merge base and its tip.
    compared_range: Option<(String, String)>,
    debounce: Option<Task<()>>,
    focus: FocusHandle,
    _subs: Vec<Subscription>,
    _live: Live,
}

impl HistoryPanel {
    pub fn new(
        ctx: &PanelContext,
        project: Entity<Project>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let repo = RepoState::get(&project, cx);
        let all = store::get(cx).all_branches;
        let main = Self::main_of(&project, cx);
        let refs = store::graph_refs(&main, cx);
        let search = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Search commits  ·  author: path: code:")
        });
        let (source, options) = Self::source_for("", &None, all, &refs);
        let list =
            cx.new(|cx| CommitList::new(repo.clone(), ctx.workspace.clone(), source, options, cx));
        let subs = vec![
            cx.subscribe_in(
                &search,
                window,
                |this: &mut Self, _, e: &InputEvent, window, cx| match e {
                    InputEvent::Change => this.typed(window, cx),
                    InputEvent::PressEnter { .. } => this.apply(cx),
                    _ => {}
                },
            ),
            cx.subscribe_in(&list, window, Self::list_event),
            cx.observe(&repo, |this: &mut Self, _, cx| this.sync_bisect(cx)),
            cx.subscribe(&repo, |this: &mut Self, _, e: &RepoEvent, cx| {
                if let (RepoEvent::Changed, Mode::Ref(r)) = (e, &mut this.mode) {
                    let with = r.with.clone();
                    this.read_ref_compare(with, cx);
                }
            }),
        ];
        let mut panel = HistoryPanel {
            repo,
            workspace: ctx.workspace.clone(),
            search,
            query: String::new(),
            scope: None,
            all,
            refs,
            list,
            mode: Mode::List,
            bisect: None,
            compared_range: None,
            debounce: None,
            focus: cx.focus_handle(),
            _subs: subs,
            _live: Live::new("HistoryPanel"),
        };
        panel.sync_bisect(cx);
        panel
    }

    /// The graph's hidden branches are the repository's, across its worktrees.
    fn main_of(project: &Entity<Project>, cx: &App) -> String {
        let p = project.read(cx);
        p.main_worktree()
            .unwrap_or(p.root())
            .to_string_lossy()
            .into_owned()
    }

    fn main(&self, cx: &App) -> String {
        self.repo
            .read(cx)
            .project()
            .map(|p| Self::main_of(&p, cx))
            .unwrap_or_default()
    }

    fn source_for(
        query: &str,
        scope: &Option<(String, bool)>,
        all: bool,
        refs: &GraphRefs,
    ) -> (Source, ListOptions) {
        let (mut filter, shas) = parse_log_query(query);
        if let Some((path, file)) = scope {
            // git follows one path only: a typed `path:` next to the file's turns following off.
            filter.follow = *file && filter.paths.is_empty();
            filter.paths.insert(0, path.clone());
        }
        if !filter.is_empty() {
            let options = ListOptions {
                empty: "No commits match.".into(),
                filter_author: true,
                ..Default::default()
            };
            return (
                Source::Search {
                    filter,
                    shas,
                    all: all.then(|| refs.clone()),
                },
                options,
            );
        }
        if all {
            let options = ListOptions {
                empty: "No commits yet.".into(),
                filter_author: true,
                ref_menu: true,
                show_refs: Some(refs.clone()),
            };
            return (Source::All(refs.clone()), options);
        }
        (
            Source::Head,
            ListOptions {
                empty: "No commits yet.".into(),
                filter_author: true,
                ..Default::default()
            },
        )
    }

    fn apply(&mut self, cx: &mut Context<Self>) {
        self.debounce = None;
        // Undo and reset act on HEAD, which the list reads with each of its pages.
        let (source, options) = Self::source_for(&self.query, &self.scope, self.all, &self.refs);
        self.list
            .update(cx, |l, cx| l.set_source(source, options, cx));
        cx.notify();
    }

    fn typed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let q = self.search.read(cx).value().to_string();
        if q == self.query {
            return;
        }
        self.query = q;
        // A search goes first: a comparison shown comes back once it's closed.
        let timer = cx.background_executor().timer(DEBOUNCE);
        self.debounce = Some(cx.spawn_in(window, async move |this, cx| {
            timer.await;
            let _ = this.update(cx, |this, cx| this.apply(cx));
        }));
        cx.notify();
    }

    fn set_query(&mut self, q: String, window: &mut Window, cx: &mut Context<Self>) {
        self.search
            .update(cx, |s, cx| s.set_value(q.clone(), window, cx));
        self.query = q;
        self.apply(cx);
    }

    #[cfg(test)]
    pub(crate) fn list(&self) -> &Entity<CommitList> {
        &self.list
    }

    /// Selects commit `sha` and scrolls to it, reading pages until it's listed (up to
    /// `list::JUMP_PAGES`). A search or a comparison shown gives way to the history first.
    pub fn jump_to(&mut self, sha: String, window: &mut Window, cx: &mut Context<Self>) {
        if !self.query.is_empty() || self.scope.is_some() {
            self.scope = None;
            self.set_query(String::new(), window, cx);
        }
        self.mode = Mode::List;
        self.list.update(cx, |l, cx| {
            l.jump_to(sha, cx);
        });
        self.list.focus_handle(cx).focus(window, cx);
        cx.notify();
    }

    pub fn focus_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search.update(cx, |s, cx| {
            s.focus(window, cx);
            s.select_all(window, cx);
        });
    }

    pub fn search_for(&mut self, q: String, window: &mut Window, cx: &mut Context<Self>) {
        self.set_query(q, window, cx);
    }

    pub fn load_to_end(&mut self, cx: &mut Context<Self>) {
        self.list.update(cx, |l, cx| l.load_to_end(cx));
    }

    pub fn loaded_to_end(&self, cx: &App) -> bool {
        let l = self.list.read(cx);
        l.is_loaded() && !l.has_more()
    }

    /// Commits listed.
    pub fn loaded(&self, cx: &App) -> usize {
        self.list.read(cx).len()
    }

    pub fn open_first(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.list.update(cx, |l, cx| l.open_first(window, cx));
    }

    pub fn open_first_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.list.update(cx, |l, cx| l.open_first_all(window, cx));
    }

    pub fn open_compare(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        open::open_compare(&self.workspace, &self.repo, None, None, window, cx);
    }

    /// "Show History" of a file or folder: the list narrows to it, a chip says so.
    pub fn show_history_of(&mut self, path: String, file: bool, cx: &mut Context<Self>) {
        self.scope = Some((path, file));
        self.mode = Mode::List;
        self.apply(cx);
    }

    fn set_all(&mut self, all: bool, cx: &mut Context<Self>) {
        self.all = all;
        store::update(cx, |s| s.all_branches = all);
        self.apply(cx);
    }

    fn set_refs(&mut self, refs: GraphRefs, cx: &mut Context<Self>) {
        let main = self.main(cx);
        store::set_graph_refs(&main, refs.clone(), cx);
        self.refs = refs;
        self.apply(cx);
    }

    fn list_event(
        &mut self,
        _: &Entity<CommitList>,
        e: &ListEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match e {
            ListEvent::FilterAuthor { name, email } => {
                let q = with_author(
                    &self.query,
                    name,
                    (!email.is_empty()).then_some(email.as_str()),
                );
                self.set_query(q, window, cx);
            }
            ListEvent::ComparePoints { base, head } => {
                self.compare_points(base.clone(), head.clone(), cx)
            }
            ListEvent::HideRefs(r) => {
                let refs = self.refs.hide(r);
                self.set_refs(refs, cx);
            }
            ListEvent::OnlyRef(r) => {
                let refs = GraphRefs {
                    only: Some(r.clone()),
                    ..self.refs.clone()
                };
                self.set_refs(refs, cx);
            }
            ListEvent::Loaded => {}
        }
    }

    fn sync_bisect(&mut self, cx: &mut Context<Self>) {
        let on = self
            .repo
            .read(cx)
            .status
            .as_ref()
            .and_then(|s| s.operation.as_ref())
            .is_some_and(|o| o.kind == "bisect");
        match (on, self.bisect.is_some()) {
            (true, false) => self.bisect = Some(cx.new(|_| BisectBar::new(self.repo.clone()))),
            (false, true) => self.bisect = None,
            _ => {}
        }
    }

    // MARK: Comparisons in the list's place

    fn compare_ref(&mut self, with: String, cx: &mut Context<Self>) {
        let options = |empty: &str| ListOptions {
            empty: empty.to_string().into(),
            ..Default::default()
        };
        let (repo, ws) = (self.repo.clone(), self.workspace.clone());
        let behind = cx.new(|cx| {
            CommitList::new(
                repo.clone(),
                ws.clone(),
                Source::Compare {
                    with: with.clone(),
                    incoming: true,
                },
                options("Nothing here that your branch lacks."),
                cx,
            )
        });
        let ahead = cx.new(|cx| {
            CommitList::new(
                repo.clone(),
                ws.clone(),
                Source::Compare {
                    with: with.clone(),
                    incoming: false,
                },
                options("Nothing here that it lacks."),
                cx,
            )
        });
        let files = self.range_files_list(cx);
        self.mode = Mode::Ref(RefCompare {
            with: with.clone(),
            pane: Pane::Behind,
            behind,
            ahead,
            files,
            counts: None,
            _read: Task::ready(()),
        });
        self.read_ref_compare(with, cx);
        cx.notify();
    }

    /// A file list whose rows open a range's diff: `(base, head, label)` is read when clicked.
    fn range_files_list(&self, cx: &mut Context<Self>) -> Entity<FileList> {
        let weak = cx.weak_entity();
        cx.new(|_| {
            FileList::new(
                "No changes",
                Rc::new(move |f, pin, window, cx| {
                    let _ = weak.update(cx, |this, cx| this.open_compared_file(f, pin, window, cx));
                }),
            )
        })
    }

    fn read_ref_compare(&mut self, with: String, cx: &mut Context<Self>) {
        let core = gitviber_ui::core(cx);
        let read = cx.background_spawn(async move {
            (
                api::history::compare_counts(&core, with.clone()).ok(),
                api::history::compare_files(&core, with),
            )
        });
        if let Mode::Ref(r) = &mut self.mode {
            r._read = cx.spawn(async move |this, cx| {
                let (counts, files) = read.await;
                let _ = this.update(cx, |this, cx| {
                    if let Mode::Ref(r) = &mut this.mode {
                        r.counts = counts;
                        let range = files
                            .as_ref()
                            .ok()
                            .map(|f| (f.base.clone(), f.head.clone()));
                        r.files
                            .update(cx, |l, cx| l.set(files.map(|f| f.files), cx));
                        this.compared_range = range;
                    }
                    cx.notify();
                });
            });
        }
    }

    fn compare_points(
        &mut self,
        base: (String, String),
        head: Option<(String, String)>,
        cx: &mut Context<Self>,
    ) {
        let files = self.range_files_list(cx);
        let core = gitviber_ui::core(cx);
        let (b, h) = (base.0.clone(), head.as_ref().map(|h| h.0.clone()));
        let read = cx.background_spawn(async move {
            match h {
                Some(h) => api::history::range_files(&core, b, h),
                // The working tree: its uncommitted work too.
                None => api::history::compare_worktree(&core, b).map(|r| r.files),
            }
        });
        let list = files.clone();
        let task = cx.spawn(async move |_, cx| {
            let files = read.await;
            list.update(cx, |l, cx| l.set(files, cx));
        });
        self.mode = Mode::Points(Points {
            base,
            head,
            files,
            _read: task,
        });
        cx.notify();
    }

    fn open_compared_file(
        &mut self,
        f: &gitviber_core::git::FileChange,
        pin: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match &self.mode {
            Mode::Ref(r) => {
                let Some((base, head)) = self.compared_range.clone() else {
                    return;
                };
                let label = short_ref(&r.with).to_string();
                open::open_range_file(
                    &self.workspace,
                    open::Range { base, head, label },
                    f,
                    !pin,
                    window,
                    cx,
                );
            }
            Mode::Points(p) => match &p.head {
                Some(h) => {
                    let range = open::Range {
                        base: p.base.0.clone(),
                        head: h.0.clone(),
                        label: format!("{}..{}", p.base.1, h.1),
                    };
                    open::open_range_file(&self.workspace, range, f, !pin, window, cx)
                }
                None => open::open_worktree_file(
                    &self.workspace,
                    p.base.0.clone(),
                    p.base.1.clone(),
                    f,
                    !pin,
                    window,
                    cx,
                ),
            },
            Mode::List => {}
        }
    }

    fn close_compare(&mut self, cx: &mut Context<Self>) {
        self.mode = Mode::List;
        cx.notify();
    }

    // MARK: Render

    fn render_search(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.app_theme().clone();
        let all = self.all;
        let searching =
            self.list.read(cx).is_loaded().then_some(()).is_none() && !self.query.is_empty();
        let weak = cx.weak_entity();
        let menu = Button::new("graph-options")
            .ghost()
            .xsmall()
            .icon(IconName::ChevronDown)
            .tooltip("Graph options")
            .dropdown_menu_with_anchor(Anchor::TopRight, move |mut menu, _, cx| {
                let Some(this) = weak.upgrade() else {
                    return menu;
                };
                let (all, refs) = {
                    let p = this.read(cx);
                    (p.all, p.refs.clone())
                };
                let toggle = |which: &'static str| {
                    let this = this.downgrade();
                    move |_: &gpui_kit::ClickEvent, _: &mut Window, cx: &mut App| {
                        let _ = this.update(cx, |p, cx| {
                            let mut refs = p.refs.clone();
                            match which {
                                "local" => refs.local = !refs.local,
                                "remote" => refs.remote = !refs.remote,
                                _ => refs.tags = !refs.tags,
                            }
                            refs.only = None;
                            p.set_refs(refs, cx);
                        });
                    }
                };
                let w = this.downgrade();
                if all {
                    menu = menu
                        .item(
                            PopupMenuItem::new("Local branches")
                                .checked(refs.local)
                                .on_click(toggle("local")),
                        )
                        .item(
                            PopupMenuItem::new("Remote branches")
                                .checked(refs.remote)
                                .on_click(toggle("remote")),
                        )
                        .item(
                            PopupMenuItem::new("Tags")
                                .checked(refs.tags)
                                .on_click(toggle("tags")),
                        )
                        .separator()
                        .item(
                            PopupMenuItem::new("Go to HEAD")
                                .icon(Icon::new(IconName::Crosshair))
                                .on_click({
                                    let w = w.clone();
                                    move |_, _, cx| {
                                        let _ = w.update(cx, |p, cx| {
                                            let head = p.repo.read(cx).head().map(str::to_string);
                                            if let Some(h) = head {
                                                p.list.update(cx, |l, cx| l.jump_to(h, cx));
                                            }
                                        });
                                    }
                                }),
                        )
                        .item(
                            PopupMenuItem::new("Go to branch…")
                                .icon(Icon::new(IconName::Crosshair))
                                .on_click({
                                    let w = w.clone();
                                    move |_, window, cx| {
                                        let _ = w.update(cx, |p, cx| p.pick_ref(false, window, cx));
                                    }
                                }),
                        )
                        .separator();
                }
                menu.item(
                    PopupMenuItem::new("Compare with…")
                        .icon(Icon::new(IconName::GitCompareArrows))
                        .on_click({
                            let w = w.clone();
                            move |_, window, cx| {
                                let _ = w.update(cx, |p, cx| p.pick_ref(true, window, cx));
                            }
                        }),
                )
                .item(
                    PopupMenuItem::new("Reflog…")
                        .icon(Icon::new(IconName::RotateCcwClock))
                        .on_click({
                            let w = w.clone();
                            move |_, window, cx| {
                                let _ = w.update(cx, |p, cx| {
                                    crate::reflog::open_reflog(p.repo.clone(), window, cx)
                                });
                            }
                        }),
                )
            });
        div()
            .flex()
            .flex_none()
            .h(px(32.))
            .items_center()
            .gap_2()
            .px(px(10.))
            .border_b_1()
            .border_color(t.border)
            .child(
                Icon::new(if searching {
                    IconName::LoaderCircle
                } else {
                    IconName::Search
                })
                .small()
                .text_color(t.subtle),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(Input::new(&self.search).appearance(false).xsmall()),
            )
            .child(
                div()
                    .id("syntax")
                    .flex_none()
                    .child(
                        Icon::new(IconName::CircleQuestionMark)
                            .xsmall()
                            .text_color(t.subtle),
                    )
                    .tooltip(|w, cx| Tooltip::new(SYNTAX).build(w, cx)),
            )
            .when(!self.query.is_empty(), |d| {
                d.child(
                    Button::new("clear")
                        .ghost()
                        .xsmall()
                        .icon(IconName::X)
                        .tooltip("Clear search")
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.set_query(String::new(), window, cx)
                        })),
                )
            })
            .child(
                Button::new("all-branches")
                    .ghost()
                    .xsmall()
                    .icon(IconName::GitGraph)
                    .when(all, |b| b.selected(true))
                    .tooltip(if all {
                        "Showing all branches"
                    } else {
                        "Show all branches"
                    })
                    .on_click(cx.listener(move |this, _, _, cx| this.set_all(!all, cx))),
            )
            .child(menu)
    }

    fn pick_ref(&mut self, compare: bool, window: &mut Window, cx: &mut Context<Self>) {
        let repo = self.repo.read(cx);
        let current = repo.branch().map(str::to_string);
        // Comparing a branch with itself shows nothing.
        let branches: Vec<_> = repo
            .branches
            .iter()
            .filter(|b| !compare || b.remote || Some(&b.name) != current.as_ref())
            .cloned()
            .collect();
        let weak = cx.weak_entity();
        let title = if compare {
            "Compare with…"
        } else {
            "Go to branch…"
        };
        let offer = crate::picker::Offer {
            title: title.into(),
            branches: Rc::new(branches),
            detached: false,
            with_tags: false,
            picked: None,
        };
        crate::picker::open_ref_picker(
            offer,
            Rc::new(move |point, window, cx| {
                let _ = weak.update(cx, |this, cx| {
                    if compare {
                        this.compare_ref(point.r#ref.clone(), cx);
                    } else {
                        this.go_to(point.r#ref.clone(), point.label.clone(), window, cx);
                    }
                });
            }),
            window,
            cx,
        );
    }

    fn go_to(&mut self, r: String, name: String, window: &mut Window, cx: &mut Context<Self>) {
        let core = gitviber_ui::core(cx);
        let find = cx.background_spawn(async move { api::history::find_commit(&core, r) });
        self.debounce = Some(cx.spawn_in(window, async move |this, cx| {
            let tip = find.await.ok().flatten();
            let _ = this.update_in(cx, |this, window, cx| match tip {
                Some(c) => this.list.update(cx, |l, cx| l.jump_to(c.sha, cx)),
                None => gitviber_refs::toast::Note::error(format!("Could not find {name}"))
                    .show(window, cx),
            });
        }));
    }

    fn render_notice(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        if !self.all || !self.query.is_empty() || self.scope.is_some() {
            return None;
        }
        let t = cx.app_theme().clone();
        let r = &self.refs;
        let off: Vec<&str> = [
            (!r.local).then_some("local branches"),
            (!r.remote).then_some("remote branches"),
            (!r.tags).then_some("tags"),
        ]
        .into_iter()
        .flatten()
        .collect();
        let parts: Vec<String> = match &r.only {
            Some(only) => vec![format!("Only {}", short_ref(only))],
            None => [
                (!off.is_empty()).then(|| format!("No {}", off.join(", "))),
                (!r.hidden.is_empty()).then(|| format!("{} hidden", r.hidden.len())),
            ]
            .into_iter()
            .flatten()
            .collect(),
        };
        if parts.is_empty() {
            return None;
        }
        Some(
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(px(6.))
                .px(px(10.))
                .py_1()
                .border_b_1()
                .border_color(t.border)
                .text_size(rems(0.6875))
                .text_color(t.muted_foreground)
                .child(div().min_w_0().truncate().child(parts.join(" · ")))
                .child(
                    div()
                        .id("show-all")
                        .flex_none()
                        .px_1()
                        .text_color(t.primary)
                        .cursor_pointer()
                        .hover(|s| s.underline())
                        .child("Show all")
                        .on_click(
                            cx.listener(|this, _, _, cx| this.set_refs(GraphRefs::default(), cx)),
                        ),
                ),
        )
    }

    fn render_scope(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let (path, file) = self.scope.clone()?;
        let t = cx.app_theme().clone();
        Some(
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap_1()
                .px_2()
                .py(px(6.))
                .border_b_1()
                .border_color(t.border)
                .child(
                    div()
                        .flex()
                        .min_w_0()
                        .items_center()
                        .gap_1()
                        .rounded_full()
                        .border_1()
                        .border_color(t.border_strong)
                        .bg(t.active)
                        .pl(px(6.))
                        .pr(px(2.))
                        .text_size(rems(0.65625))
                        .child(
                            Icon::new(if file {
                                IconName::FileClock
                            } else {
                                IconName::FolderClock
                            })
                            .xsmall()
                            .text_color(t.subtle),
                        )
                        .child(
                            div()
                                .truncate()
                                .font_family(t.fonts.code.clone())
                                .child(path),
                        )
                        .child(
                            Button::new("scope-x")
                                .ghost()
                                .xsmall()
                                .icon(IconName::X)
                                .tooltip("Show all history")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.scope = None;
                                    this.apply(cx);
                                })),
                        ),
                ),
        )
    }

    fn compare_header(
        &self,
        text: AnyEl,
        swap: bool,
        open_screen: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let t = cx.app_theme().clone();
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(6.))
            .px(px(10.))
            .py_1()
            .border_b_1()
            .border_color(t.border)
            .text_size(rems(0.6875))
            .text_color(t.muted_foreground)
            .child(
                Icon::new(IconName::GitCompareArrows)
                    .xsmall()
                    .text_color(t.subtle),
            )
            .child(div().min_w_0().flex_1().truncate().child(text))
            .when(swap, |d| {
                d.child(
                    Button::new("swap")
                        .ghost()
                        .xsmall()
                        .icon(IconName::ArrowLeftRight)
                        .tooltip("Swap sides")
                        .on_click(cx.listener(|this, _, _, cx| {
                            if let Mode::Points(p) = &this.mode
                                && let Some(h) = p.head.clone()
                            {
                                let base = p.base.clone();
                                this.compare_points(h, Some(base), cx);
                            }
                        })),
                )
            })
            .when(open_screen, |d| {
                d.child(
                    Button::new("to-screen")
                        .ghost()
                        .xsmall()
                        .icon(IconName::PanelTop)
                        .tooltip("Open in the Compare screen")
                        .on_click(cx.listener(|this, _, window, cx| {
                            if let Mode::Ref(r) = &this.mode {
                                let current =
                                    this.repo.read(cx).branch().map(|b| branch_point(b, false));
                                let head = gitviber_refs::points::ComparePoint {
                                    r#ref: r.with.clone(),
                                    label: short_ref(&r.with).to_string(),
                                };
                                open::open_compare(
                                    &this.workspace,
                                    &this.repo,
                                    current,
                                    Some(head),
                                    window,
                                    cx,
                                );
                            }
                        })),
                )
            })
            .child(
                Button::new("stop-compare")
                    .ghost()
                    .xsmall()
                    .icon(IconName::X)
                    .tooltip("Stop comparing")
                    .on_click(cx.listener(|this, _, _, cx| this.close_compare(cx))),
            )
    }

    fn render_body(&mut self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let t = cx.app_theme().clone();
        // A search goes first; closing it comes back to the comparison.
        let searching = !self.query.is_empty() || self.scope.is_some();
        match &self.mode {
            Mode::Ref(r) if !searching => {
                let current = self.repo.read(cx).branch().unwrap_or("HEAD").to_string();
                let name = short_ref(&r.with).to_string();
                let text = div()
                    .child(
                        div()
                            .flex()
                            .gap_1()
                            .child(
                                div()
                                    .font_family(t.fonts.code.clone())
                                    .text_color(t.foreground)
                                    .child(current.clone()),
                            )
                            .child("compared with")
                            .child(
                                div()
                                    .font_family(t.fonts.code.clone())
                                    .text_color(t.foreground)
                                    .child(name.clone()),
                            ),
                    )
                    .into_any_element();
                let pane = r.pane;
                let (behind, ahead) = r.counts.map_or((None, None), |(a, b)| (Some(b), Some(a)));
                let tab = |id: &'static str, label: String, p: Pane, cx: &mut Context<Self>| {
                    let on = pane == p;
                    div()
                        .id(id)
                        .px_2()
                        .py(px(2.))
                        .rounded(px(4.))
                        .cursor_pointer()
                        .when(on, |d| d.bg(t.active).text_color(t.foreground))
                        .when(!on, |d| d.hover(|s| s.bg(t.hover)))
                        .child(label)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Mode::Ref(r) = &mut this.mode {
                                r.pane = p;
                                cx.notify();
                            }
                        }))
                };
                let count = |n: Option<u32>| n.map(|n| format!(" {n}")).unwrap_or_default();
                let tabs = div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap_1()
                    .px_2()
                    .py_1()
                    .border_b_1()
                    .border_color(t.border)
                    .text_size(rems(0.6875))
                    .text_color(t.muted_foreground)
                    .child(tab(
                        "behind",
                        format!("Behind{}", count(behind)),
                        Pane::Behind,
                        cx,
                    ))
                    .child(tab(
                        "ahead",
                        format!("Ahead{}", count(ahead)),
                        Pane::Ahead,
                        cx,
                    ))
                    .child(tab("files", "Files".into(), Pane::Files, cx))
                    .child(
                        div()
                            .ml_auto()
                            .min_w_0()
                            .truncate()
                            .text_color(t.subtle)
                            .child(match pane {
                                Pane::Behind => format!("in {name}, not {current}"),
                                Pane::Ahead => format!("in {current}, not {name}"),
                                Pane::Files => format!("what {name} changed since they parted"),
                            }),
                    );
                let body = match pane {
                    Pane::Behind => r.behind.clone().into_any_element(),
                    Pane::Ahead => r.ahead.clone().into_any_element(),
                    Pane::Files => r.files.clone().into_any_element(),
                };
                div()
                    .flex()
                    .flex_col()
                    .size_full()
                    .child(self.compare_header(text, false, true, cx))
                    .child(tabs)
                    .child(div().flex_1().min_h_0().child(body))
                    .into_any_element()
            }
            Mode::Points(p) if !searching => {
                let text = div()
                    .flex()
                    .gap_1()
                    .child(
                        div()
                            .font_family(t.fonts.code.clone())
                            .text_color(t.foreground)
                            .child(p.base.1.clone()),
                    )
                    .child("→")
                    .child(match &p.head {
                        Some(h) => div()
                            .font_family(t.fonts.code.clone())
                            .text_color(t.foreground)
                            .child(h.1.clone()),
                        None => div().text_color(t.foreground).child("working tree"),
                    })
                    .into_any_element();
                let swap = p.head.is_some();
                let files = p.files.clone();
                let (add, del) = files.read(cx).counts();
                let n = files.read(cx).files().map(|f| f.len());
                div()
                    .flex()
                    .flex_col()
                    .size_full()
                    .child(self.compare_header(text, swap, false, cx))
                    .when_some(n.filter(|n| *n > 0), |d, n| {
                        d.child(
                            div()
                                .flex()
                                .flex_none()
                                .h(px(24.))
                                .items_center()
                                .gap_2()
                                .px_4()
                                .border_b_1()
                                .border_color(t.border)
                                .text_size(rems(0.65625))
                                .text_color(t.subtle)
                                .child("CHANGED FILES")
                                .child(
                                    div()
                                        .font_family(t.fonts.code.clone())
                                        .text_color(t.muted_foreground)
                                        .child(n.to_string()),
                                )
                                .child(
                                    div()
                                        .ml_auto()
                                        .flex()
                                        .gap_1()
                                        .font_family(t.fonts.code.clone())
                                        .child(div().text_color(t.added).child(format!("+{add}")))
                                        .child(
                                            div().text_color(t.removed).child(format!("-{del}")),
                                        ),
                                ),
                        )
                    })
                    .child(div().flex_1().min_h_0().child(files))
                    .into_any_element()
            }
            _ => self.list.clone().into_any_element(),
        }
    }
}

type AnyEl = gpui_kit::AnyElement;

impl Panel for HistoryPanel {
    fn panel_focus(&self, cx: &App) -> Option<FocusHandle> {
        Some(self.list.focus_handle(cx))
    }
}

impl Focusable for HistoryPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for HistoryPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let search_focus = self.search.focus_handle(cx);
        let search_focus2 = search_focus.clone();
        div()
            .size_full()
            .flex()
            .flex_col()
            .track_focus(&self.focus)
            .on_action(
                cx.listener(move |_, _: &history_actions::Search, window, cx| {
                    search_focus.focus(window, cx);
                }),
            )
            .on_action(
                cx.listener(move |_, _: &history_actions::Find, window, cx| {
                    search_focus2.focus(window, cx);
                }),
            )
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, window, cx| {
                // The words first, then the file.
                if e.keystroke.key == "escape" && this.search.focus_handle(cx).is_focused(window) {
                    if !this.query.is_empty() {
                        this.set_query(String::new(), window, cx);
                    } else if this.scope.is_some() {
                        this.scope = None;
                        this.apply(cx);
                    } else {
                        this.list.focus_handle(cx).focus(window, cx);
                    }
                    cx.stop_propagation();
                }
            }))
            .children(self.bisect.clone())
            .child(self.render_search(cx))
            .children(self.render_notice(cx))
            .children(self.render_scope(cx))
            .child(div().flex_1().min_h_0().child(self.render_body(cx)))
            .when(window.is_window_active(), |d| d)
    }
}
