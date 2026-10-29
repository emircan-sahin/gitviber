//! The Changes panel: the conflicted, staged and changed files (flat or as a folder tree, by
//! name or newest first), what each row does (stage, unstage, discard, resolve, mark viewed),
//! the stashes and submodules below them, the banner of a merge or rebase in progress, a branch
//! reviewed against its base in place of the uncommitted files, and the commit box.

use crate::commit::{CommitBox, RepoFacts};
use crate::dialogs::{self, Ask, Choice};
use crate::model::conflicts::{Unsure, resolve_question, unsure};
use crate::model::list::{
    ChangesView, Item, Kind, Lists, Mtimes, NESTED_EXPLAINED, file_sig, files, folder_key,
    items_of, kept_by_restore, left_out, range_of, stageable, sum_lines, toggled, unstage_paths,
    viewed_key,
};
use crate::model::text::{describe_stash, ignore_pattern};
use crate::model::tree::folders_of;
use crate::store;
use crate::toasts::{git_error, tracked, with_undo};
use gitviber_core::api;
use gitviber_core::git::{Commit, ExternalTools, FileChange, HeadMessage, Side as GitSide};
use gitviber_core::network::CANCELLED;
use gitviber_diff::{Compare, DiffSource};
use gitviber_history::BisectBar;
use gitviber_refs::net::NetActivity;
use gitviber_ui::actions::{git, review};
use gitviber_ui::keymap::editor;
use gitviber_ui::memory::Live;
use gitviber_ui::widgets::{
    ChangeKind, FileIcon, LineCounts, PathLabel, RelativeTime, StatusLetter,
};
use gitviber_ui::workspace::{
    ChangesOf, GuideOf, Panel, PanelContext, ProjectEvent, Selection, Toast, Workspace,
    WorkspaceEvent, selection_key,
};
use gitviber_ui::{ActiveAppTheme as _, AppTheme};
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::{Disableable as _, Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::{
    AnyElement, AnyWindowHandle, App, AppContext as _, ClickEvent, Context, Entity, FocusHandle,
    Focusable, InteractiveElement as _, IntoElement, KeyBinding, KeyDownEvent, Modifiers,
    ParentElement as _, Render, SharedString, StatefulInteractiveElement as _, Styled as _,
    Subscription, Task, UniformListScrollHandle, WeakEntity, Window, div,
    prelude::FluentBuilder as _, px, uniform_list,
};
use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

mod follow;
mod keys;
mod menu;

use follow::Leaving;
use keys::{Move, RowKey};

const ROW: f32 = 26.;
const CONTEXT: &str = "ChangesList";

gpui_kit::actions!(
    changes_list,
    [
        Up, Down, ExtendUp, ExtendDown, Home, End, PageUp, PageDown, Left, Right, Open, Keep,
        Escape, Menu
    ]
);

pub fn bind_keys(cx: &mut App) {
    let c = Some(CONTEXT);
    cx.bind_keys([
        KeyBinding::new("up", Up, c),
        KeyBinding::new("down", Down, c),
        KeyBinding::new("shift-up", ExtendUp, c),
        KeyBinding::new("shift-down", ExtendDown, c),
        KeyBinding::new("home", Home, c),
        KeyBinding::new("end", End, c),
        KeyBinding::new("pageup", PageUp, c),
        KeyBinding::new("pagedown", PageDown, c),
        KeyBinding::new("left", Left, c),
        KeyBinding::new("right", Right, c),
        KeyBinding::new("space", Open, c),
        KeyBinding::new("enter", Keep, c),
        KeyBinding::new("escape", Escape, c),
        KeyBinding::new("shift-f10", Menu, c),
    ]);
}

// MARK: Viewed marks, shared with the diff tabs

/// The key a selection's viewed mark is kept under, and its file.
fn mark_of(sel: &Selection) -> Option<(String, &FileChange)> {
    match sel {
        Selection::Unstaged { file } => Some((viewed_key("unstaged", &file.path), file)),
        Selection::Conflict { file } => Some((viewed_key("conflict", &file.path), file)),
        Selection::Branch { base, file, .. } => {
            Some((format!("branch:{base}:{}", file.path), file))
        }
        _ => None,
    }
}

/// Viewed: marked at the file's content as it is now. Staging is accepting a file, so staged
/// ones always count.
pub fn is_viewed(ws: &Workspace, sel: &Selection) -> bool {
    if matches!(sel, Selection::Staged { .. }) {
        return true;
    }
    mark_of(sel).is_some_and(|(key, f)| ws.viewed(&key) == Some(file_sig(f).as_str()))
}

pub fn set_viewed(ws: &mut Workspace, sel: &Selection, on: bool, cx: &mut Context<Workspace>) {
    if let Some((key, f)) = mark_of(sel) {
        let sig = on.then(|| file_sig(f));
        ws.set_viewed(key, sig, cx);
    }
}

/// Unstages `files`; the project reads the repo again.
pub fn unstage(files: Vec<FileChange>, ws: WeakEntity<Workspace>, cx: &mut App) {
    let core = crate::core(cx);
    let paths = unstage_paths(&files);
    cx.spawn(async move |cx| {
        let result = cx
            .background_spawn(async move { api::changes::unstage(&core, paths) })
            .await;
        cx.update(|cx| {
            let Some(ws) = ws.upgrade() else { return };
            if let Err(e) = result {
                ws.update(cx, |ws, cx| {
                    ws.toast(git_error("Unstage failed", &e, &vec![]), cx)
                });
            }
            if let Some(p) = ws.read(cx).project().cloned() {
                p.update(cx, |p, cx| p.repo_changed(cx));
            }
        });
    })
    .detach();
}

/// The panel of a workspace, while it's built.
fn panel_of(ws: &Entity<Workspace>, cx: &App) -> Option<Entity<ChangesPanel>> {
    ws.read(cx)
        .built_panel("changes")
        .and_then(|p| p.view().downcast::<ChangesPanel>().ok())
}

/// The selection a stacked diff's file stands for, with the file as status has it when it can.
pub fn selection_for(ws: &Entity<Workspace>, source: &DiffSource, cx: &App) -> Option<Selection> {
    let panel = panel_of(ws, cx);
    let known = |kind: Kind| {
        panel.as_ref().and_then(|p| {
            p.read(cx)
                .lists
                .list(kind)
                .iter()
                .find(|f| f.path == source.path)
                .cloned()
        })
    };
    let made = || {
        Arc::new(FileChange {
            path: source.path.clone(),
            old_path: source.old_path.clone(),
            status: source.file.status.clone(),
            additions: None,
            deletions: None,
            oid: None,
            index_oid: None,
            conflict: None,
            mode: source.file.mode.clone(),
            submodule: source.file.submodule.clone(),
            nested: None,
        })
    };
    Some(match &source.compare {
        Compare::Staged => Selection::Staged {
            file: known(Kind::Staged).unwrap_or_else(made),
        },
        Compare::Base { base } => {
            let review = panel.as_ref().and_then(|p| {
                p.read(cx).review.as_ref().map(|r| {
                    (
                        r.label.clone(),
                        r.files.iter().find(|f| f.path == source.path).cloned(),
                    )
                })
            });
            let (label, file) = review.unwrap_or_default();
            Selection::Branch {
                base: base.clone(),
                label,
                file: file.unwrap_or_else(made),
                fixed: false,
            }
        }
        _ => Selection::Unstaged {
            file: known(Kind::Unstaged).unwrap_or_else(made),
        },
    })
}

/// As the file manager is named where the file opens.
const REVEAL_LABEL: &str = if cfg!(target_os = "macos") {
    "Reveal in Finder"
} else if cfg!(windows) {
    "Show in Explorer"
} else {
    "Show in Folder"
};

// MARK: The panel

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Section {
    List(Kind),
    Review,
    Stashes,
    Submodules,
}

impl Section {
    const ALL: [Section; 6] = [
        Section::List(Kind::Conflict),
        Section::List(Kind::Staged),
        Section::List(Kind::Unstaged),
        Section::Review,
        Section::Stashes,
        Section::Submodules,
    ];

    fn title(self) -> &'static str {
        match self {
            Section::List(k) => k.title(),
            Section::Review => "Branch Changes",
            Section::Stashes => "Stashes",
            Section::Submodules => "Submodules",
        }
    }
}

#[derive(Clone)]
enum Row {
    Section(Section, usize),
    Item(Item),
    Nested(Arc<FileChange>, usize),
    Review(Arc<FileChange>),
    Stash(usize),
    /// A file of stash `stash`; `sha`: the commit it's in, the untracked files' own for those.
    StashFile {
        stash: usize,
        sha: String,
        file: Arc<FileChange>,
        untracked: bool,
    },
    Submodule(usize),
    Note(SharedString),
}

struct Op {
    kind: String,
    subject: Option<String>,
    step: Option<u32>,
    total: Option<u32>,
    split: bool,
}

struct StashInfo {
    sha: String,
    index: usize,
    author: String,
    text: String,
    branch: Option<String>,
    timestamp: i64,
}

type StashFile = (String, Arc<FileChange>, bool);

struct Filter {
    input: Entity<InputState>,
    _sub: Subscription,
}

struct Review {
    /// The full ref reviewed against, and its short name.
    reference: String,
    label: String,
    /// The merge base the files' old sides are read at, once loaded.
    base: Option<String>,
    files: Vec<Arc<FileChange>>,
    error: Option<String>,
}

pub struct ChangesPanel {
    ctx: PanelContext,
    /// For work that ends outside a frame and opens a tab.
    window: AnyWindowHandle,
    focus: FocusHandle,
    loaded: bool,
    error: Option<String>,
    root: String,
    branch: Option<String>,
    head: Option<Arc<HeadMessage>>,
    pushed: bool,
    operation: Option<Op>,
    /// A bisect under way: History's bar where a merge's banner goes.
    bisect: Option<Entity<BisectBar>>,
    prepared: Option<String>,
    lists: Lists,
    mtimes: Option<Mtimes>,
    stashes: Vec<StashInfo>,
    submodules: Vec<(String, String, String)>,
    view: ChangesView,
    closed: HashSet<String>,
    collapsed: HashSet<Section>,
    /// The filter row, while ⌘F has it open.
    filter: Option<Filter>,
    /// Rows picked with ⌘ / ⇧ around the open one; None: just the open row.
    picked: Option<(Vec<String>, String)>,
    /// The key of the open tab's file, when it's one of ours.
    open_key: Option<String>,
    /// The row the keys last landed on (a folder or a stash takes it without opening anything);
    /// None: the open tab's row.
    cursor: Option<String>,
    rows: Vec<Row>,
    keys: Vec<String>,
    scroll: UniformListScrollHandle,
    /// The rows the list last drew.
    #[cfg(test)]
    drawn: std::ops::Range<usize>,
    commit: Entity<CommitBox>,
    /// The worktree's review notes, between the lists and the commit box.
    notes: Entity<gitviber_review::NotesSection>,
    review: Option<Review>,
    branches: Vec<String>,
    expanded_stash: Option<String>,
    /// The open stash's files: its sha, then each file with the commit it's in and whether untracked.
    stash_files: Option<(String, Vec<StashFile>)>,
    busy: bool,
    updating_submodules: bool,
    /// git's diff and merge tools, read once a project opens.
    tools: Option<ExternalTools>,
    /// The files open in one of them now, until it's closed.
    in_tool: HashMap<String, Task<()>>,
    /// Rows git actions are taking out of their lists (follow.rs).
    leaving: Vec<Leaving>,
    /// Numbers those actions.
    leaves: u64,
    /// Counts the reads started: an action's rows come back only with a read begun after it.
    reads: u64,
    /// Counts the projects opened and worktrees switched to: a read from before doesn't move
    /// the tabs, which are the other worktree's.
    epoch: u64,
    /// What the commit box is told at the next frame.
    facts: Option<RepoFacts>,
    reading: bool,
    again: bool,
    _read: Task<()>,
    _head: Task<()>,
    _review: Task<()>,
    _action: Task<()>,
    _stash: Task<()>,
    _subscriptions: Vec<Subscription>,
    _live: Live,
}

impl Panel for ChangesPanel {
    fn badge(&self, _: &App) -> Option<SharedString> {
        let n = self.lists.total();
        (n > 0).then(|| n.to_string().into())
    }

    fn panel_focus(&self, _: &App) -> Option<FocusHandle> {
        Some(self.focus.clone())
    }
}

impl Focusable for ChangesPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl ChangesPanel {
    pub fn new(ctx: &PanelContext, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let commit = cx.new(|cx| CommitBox::new(ctx.workspace.clone(), window, cx));
        let notes = cx.new(|cx| gitviber_review::NotesSection::new(ctx, cx));
        let mut subscriptions = vec![];
        if let Some(project) = &ctx.project {
            subscriptions.push(
                cx.subscribe(project, |this, _, e: &ProjectEvent, cx| match e {
                    ProjectEvent::RepoChanged(_) => this.read(cx),
                    ProjectEvent::Opened => {
                        this.tools = None;
                        this.epoch += 1;
                        this.start(cx);
                    }
                    ProjectEvent::Switched { .. } => {
                        this.tools = None;
                        this.epoch += 1;
                        // The workspace brings back that worktree's branch review as it handles
                        // the switch too: once it has.
                        let weak = cx.weak_entity();
                        cx.defer(move |cx| {
                            weak.update(cx, |this, cx| this.follow_review_base(cx)).ok();
                        });
                        this.read(cx);
                    }
                    ProjectEvent::Failed(e) => {
                        this.error = Some(e.clone());
                        cx.notify();
                    }
                }),
            );
        }
        if let Some(ws) = ctx.workspace.upgrade() {
            subscriptions.push(cx.subscribe(&ws, |this, ws, e: &WorkspaceEvent, cx| {
                if *e == WorkspaceEvent::TabsChanged {
                    this.follow_tabs(&ws, cx);
                }
            }));
        }
        let mut panel = ChangesPanel {
            ctx: ctx.clone(),
            window: window.window_handle(),
            focus: cx.focus_handle(),
            loaded: false,
            error: None,
            root: String::new(),
            branch: None,
            head: None,
            pushed: false,
            operation: None,
            bisect: None,
            prepared: None,
            lists: Lists::default(),
            mtimes: None,
            stashes: vec![],
            submodules: vec![],
            view: store::view(cx),
            closed: HashSet::new(),
            collapsed: {
                let titles = store::collapsed(cx);
                Section::ALL
                    .into_iter()
                    .filter(|s| titles.iter().any(|t| t == s.title()))
                    .collect()
            },
            filter: None,
            picked: None,
            open_key: None,
            cursor: None,
            rows: vec![],
            keys: vec![],
            scroll: UniformListScrollHandle::new(),
            #[cfg(test)]
            drawn: 0..0,
            commit,
            notes,
            review: None,
            branches: vec![],
            expanded_stash: None,
            stash_files: None,
            busy: false,
            updating_submodules: false,
            tools: None,
            in_tool: HashMap::new(),
            leaving: vec![],
            leaves: 0,
            reads: 0,
            epoch: 0,
            facts: None,
            reading: false,
            again: false,
            _read: Task::ready(()),
            _head: Task::ready(()),
            _review: Task::ready(()),
            _action: Task::ready(()),
            _stash: Task::ready(()),
            _subscriptions: subscriptions,
            _live: Live::new("ChangesPanel"),
        };
        // Built at launch, while the repo is still opening, it starts on `Opened`.
        if panel.repo_open(cx) {
            panel.start(cx);
        }
        panel
    }

    /// The branch review the worktree was left in, and the first read.
    fn start(&mut self, cx: &mut Context<Self>) {
        if self.review.is_none()
            && let Some(base) = self
                .workspace()
                .and_then(|ws| ws.read(cx).review_base().map(str::to_string))
        {
            self.start_review(base, cx);
        }
        self.read(cx);
    }

    /// The branch review the workspace has for the worktree open now, or none: another
    /// worktree's has its own base, and its own files even against the same one.
    fn follow_review_base(&mut self, cx: &mut Context<Self>) {
        let base = self
            .workspace()
            .and_then(|ws| ws.read(cx).review_base().map(str::to_string));
        match base {
            // The read the switch started loads its files.
            Some(base) => self.show_review(base, cx),
            None if self.review.is_some() => self.end_review(cx),
            None => {}
        }
    }

    /// Whether git can read the repo yet: until the project opens it says "No repository is
    /// open". Only the app's core opens a project; without one (a test), or without a project,
    /// the core the panel reads is open already.
    fn repo_open(&self, cx: &App) -> bool {
        !gitviber_ui::has_core(cx)
            || self
                .ctx
                .project
                .as_ref()
                .is_none_or(|p| p.read(cx).is_open())
    }

    fn workspace(&self) -> Option<Entity<Workspace>> {
        self.ctx.workspace.upgrade()
    }

    /// The bisect bar comes with a bisect and goes with it.
    fn sync_bisect(&mut self, cx: &mut Context<Self>) {
        let on = self.operation.as_ref().is_some_and(|o| o.kind == "bisect");
        if !on {
            self.bisect = None;
        } else if self.bisect.is_none()
            && let Some(project) = &self.ctx.project
        {
            let repo = gitviber_refs::RepoState::get(project, cx);
            self.bisect = Some(cx.new(|_| BisectBar::new(repo)));
        }
    }

    fn toast(&self, toast: Toast, cx: &mut App) {
        if let Some(ws) = self.workspace() {
            ws.update(cx, |ws, cx| ws.toast(toast, cx));
        }
    }

    fn refresh_project(&self, cx: &mut App) {
        if let Some(p) = &self.ctx.project {
            p.update(cx, |p, cx| p.repo_changed(cx));
        }
    }

    // MARK: Reading the repo

    /// Status, stashes and submodules side by side, one read at a time; changes during a read
    /// make one more. HEAD's message follows on its own, once HEAD moved.
    fn read(&mut self, cx: &mut Context<Self>) {
        if !self.repo_open(cx) {
            return;
        }
        if self.reading {
            self.again = true;
            return;
        }
        self.reading = true;
        self.reads += 1;
        let seq = self.reads;
        let core = crate::core(cx);
        let recent = self.view.recent;
        // A git call of its own, and the config rarely changes: once a project.
        let want_tools = self.tools.is_none();
        let epoch = self.epoch;
        self._read = cx.spawn(async move |this, cx| {
            // None of these waits on another's git calls.
            let status = cx.background_spawn({
                let core = core.clone();
                async move {
                    let status = api::changes::status(&core)?;
                    let mtimes = recent.then(|| {
                        let paths: Vec<String> = status
                            .conflicted
                            .iter()
                            .chain(&status.staged)
                            .chain(&status.unstaged)
                            .map(|f| f.path.clone())
                            .collect();
                        let times =
                            api::files::file_mtimes(&core, paths.clone()).unwrap_or_default();
                        paths
                            .into_iter()
                            .zip(times)
                            .filter_map(|(p, t)| t.map(|t| (p, t)))
                            .collect::<Mtimes>()
                    });
                    Ok::<_, String>((status, mtimes))
                }
            });
            let stashes = cx.background_spawn({
                let core = core.clone();
                async move { api::stash::stashes(&core).unwrap_or_default() }
            });
            let submodules = cx.background_spawn({
                let core = core.clone();
                async move { api::sync::submodules(&core).unwrap_or_default() }
            });
            let tools = want_tools.then(|| {
                cx.background_spawn(async move {
                    api::changes::external_tools(&core).unwrap_or_default()
                })
            });
            let read = status.await;
            let (stashes, submodules) = (stashes.await, submodules.await);
            let tools = match tools {
                Some(t) => Some(t.await),
                None => None,
            };
            this.update(cx, |this, cx| {
                this.reading = false;
                match read {
                    Ok((status, mtimes)) => {
                        this.loaded = true;
                        if tools.is_some() {
                            this.tools = tools;
                        }
                        this.error = None;
                        this.root = status.root.clone();
                        this.branch = status.branch.clone();
                        match &status.head_oid {
                            None => {
                                this.head = None;
                                this._head = Task::ready(());
                            }
                            Some(oid) if this.head.as_ref().is_none_or(|h| &h.sha != oid) => {
                                this.load_head(cx)
                            }
                            Some(_) => {}
                        }
                        // HEAD is on the branch `git push` updates (else its upstream).
                        let push = status.push.as_ref().filter(|p| p.branch.is_some());
                        let upstream = (status.upstream.is_some() && !status.upstream_gone)
                            .then_some(status.ahead);
                        this.pushed =
                            status.head.is_some() && push.map(|p| p.ahead).or(upstream) == Some(0);
                        this.operation = status.operation.as_ref().map(|o| Op {
                            kind: o.kind.clone(),
                            subject: o.subject.clone(),
                            step: o.step,
                            total: o.total,
                            split: o.split,
                        });
                        this.sync_bisect(cx);
                        this.prepared = status.prepared_message.clone();
                        this.lists = Lists::of(&status);
                        this.mtimes = mtimes;
                        this.stashes = stashes
                            .into_iter()
                            .map(|s| {
                                let (text, branch) = describe_stash(&s.message);
                                StashInfo {
                                    sha: s.sha,
                                    index: s.index,
                                    author: s.author,
                                    text,
                                    branch,
                                    timestamp: s.timestamp,
                                }
                            })
                            .collect();
                        this.submodules = submodules
                            .into_iter()
                            .map(|s| (s.path, s.sha, s.state))
                            .collect();
                        this.rebuild(cx);
                        this.commit_facts(cx);
                        this.read_landed(seq);
                        if epoch == this.epoch {
                            this.follow_git(cx);
                        }
                    }
                    Err(e) => {
                        this.error = Some(e);
                        cx.notify();
                    }
                }
                if std::mem::take(&mut this.again) {
                    this.read(cx);
                }
                if let Some(r) = &this.review {
                    let reference = r.reference.clone();
                    this.load_review(reference, cx);
                }
            })
            .ok();
        });
    }

    /// HEAD's message, which Amend puts in the box. Nothing shown waits for it.
    fn load_head(&mut self, cx: &mut Context<Self>) {
        let core = crate::core(cx);
        self._head = cx.spawn(async move |this, cx| {
            let head = cx
                .background_spawn(async move { api::history::head_message(&core) })
                .await;
            // A failed read keeps the last one; the next read asks again.
            if let Ok(Some(head)) = head {
                this.update(cx, |this, cx| {
                    this.head = Some(Arc::new(head));
                    this.commit_facts(cx);
                })
                .ok();
            }
        });
    }

    fn commit_facts(&mut self, cx: &mut Context<Self>) {
        let main = self
            .ctx
            .project
            .as_ref()
            .and_then(|p| {
                p.read(cx)
                    .main_worktree()
                    .map(|m| m.to_string_lossy().into_owned())
            })
            .unwrap_or_else(|| self.root.clone());
        let facts = RepoFacts {
            root: self.root.clone(),
            main,
            branch: self.branch.clone(),
            head: self.head.clone(),
            lists: self.lists.clone(),
            prepared: self.prepared.clone(),
            pushed: self.pushed,
            shown: self.filtered_lists(cx),
        };
        // The box's fields take a window to change; the next frame has one.
        self.facts = Some(facts);
        cx.notify();
    }

    /// The open tab: its row is the active one.
    fn follow_tabs(&mut self, ws: &Entity<Workspace>, cx: &mut Context<Self>) {
        let key = ws
            .read(cx)
            .tabs()
            .active()
            .and_then(|t| t.item.selection())
            .filter(|s| matches!(s.kind(), "unstaged" | "staged" | "conflict" | "branch"))
            .map(selection_key);
        if key != self.open_key {
            self.open_key = key;
            // Opened from elsewhere: the keys go on from there.
            self.cursor = None;
            // In the tree, the folders around the file opened (J/K, a tab) open too.
            if self.view.tree
                && let Some((kind, path)) =
                    self.open_key.as_deref().and_then(|k| k.split_once("::"))
            {
                let kind = match kind {
                    "staged" => Kind::Staged,
                    "conflict" => Kind::Conflict,
                    _ => Kind::Unstaged,
                };
                reveal_folders(&mut self.closed, kind, path);
                self.rebuild(cx);
            }
            if let Some(k) = &self.open_key
                && let Some(ix) = self.keys.iter().position(|x| x == k)
            {
                self.scroll
                    .scroll_to_item(ix, gpui_kit::ScrollStrategy::Nearest);
            }
        }
        // A viewed mark changed: the checks follow.
        cx.notify();
    }

    // MARK: What shows

    fn shown_lists(&self, cx: &App) -> Lists {
        self.filtered_lists(cx)
            .unwrap_or_else(|| self.lists.clone())
            .ordered(self.view, self.mtimes.as_ref())
    }

    /// What the filter leaves, while it has text.
    fn filtered_lists(&self, cx: &App) -> Option<Lists> {
        let needle = self.needle(cx);
        (!needle.is_empty()).then(|| {
            self.lists.filtered(|f| {
                f.path.to_lowercase().contains(&needle)
                    || f.old_path
                        .as_deref()
                        .is_some_and(|o| o.to_lowercase().contains(&needle))
            })
        })
    }

    fn filtering(&self, cx: &App) -> bool {
        !self.needle(cx).is_empty()
    }

    /// The filter's text, trimmed and lowercased; empty while it lets everything through.
    fn needle(&self, cx: &App) -> String {
        self.filter
            .as_ref()
            .map(|f| f.input.read(cx).value().trim().to_lowercase())
            .unwrap_or_default()
    }

    /// Find (⌘F) in the panel opens the filter row, or puts the cursor back in it.
    fn open_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(f) = &self.filter {
            f.input.update(cx, |s, cx| {
                s.focus(window, cx);
                s.select_all(window, cx);
            });
            return;
        }
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Filter changed files"));
        input.update(cx, |s, cx| s.focus(window, cx));
        let sub = cx.subscribe(&input, |this, _, e: &InputEvent, cx| {
            if matches!(e, InputEvent::Change) {
                this.rebuild(cx);
                // The commit button says how many of its files the filter hides.
                this.commit_facts(cx);
            }
        });
        self.filter = Some(Filter { input, _sub: sub });
        cx.notify();
    }

    /// Clears and hides the row; the keys go back to the list.
    fn close_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(filter) = self.filter.take() else {
            return;
        };
        let had_text = !filter.input.read(cx).value().trim().is_empty();
        self.focus.focus(window, cx);
        if had_text {
            self.rebuild(cx);
            self.commit_facts(cx);
        }
        cx.notify();
    }

    fn rebuild(&mut self, cx: &mut Context<Self>) {
        let lists = self.shown_lists(cx);
        let mut rows = vec![];
        if let Some(review) = &self.review {
            rows.push(Row::Section(Section::Review, review.files.len()));
            if !self.collapsed.contains(&Section::Review) {
                if review.files.is_empty() {
                    let note = review.error.clone().unwrap_or_else(|| {
                        if review.base.is_none() {
                            "Loading…".into()
                        } else {
                            format!("No changes since {}.", review.label)
                        }
                    });
                    rows.push(Row::Note(note.into()));
                }
                rows.extend(review.files.iter().cloned().map(Row::Review));
            }
        } else {
            for kind in [Kind::Conflict, Kind::Staged, Kind::Unstaged] {
                let list = lists.list(kind);
                if list.is_empty() {
                    continue;
                }
                let section = Section::List(kind);
                rows.push(Row::Section(section, list.len()));
                if self.collapsed.contains(&section) {
                    continue;
                }
                for item in items_of(kind, list, self.view, self.mtimes.as_ref(), &self.closed) {
                    match &item {
                        Item::File { change, depth, .. } if change.file.nested.is_some() => {
                            rows.push(Row::Nested(change.file.clone(), *depth))
                        }
                        _ => rows.push(Row::Item(item)),
                    }
                }
            }
            if lists.total() == 0 {
                rows.push(Row::Note(
                    if self.filtering(cx) {
                        "No changed files match."
                    } else {
                        "All caught up: no changes."
                    }
                    .into(),
                ));
            }
        }
        if !self.stashes.is_empty() || self.lists.total() > 0 {
            rows.push(Row::Section(Section::Stashes, self.stashes.len()));
            if !self.collapsed.contains(&Section::Stashes) {
                if self.stashes.is_empty() {
                    rows.push(Row::Note("Nothing stashed".into()));
                }
                for (i, s) in self.stashes.iter().enumerate() {
                    rows.push(Row::Stash(i));
                    if self.expanded_stash.as_deref() == Some(s.sha.as_str()) {
                        match &self.stash_files {
                            Some((sha, list)) if *sha == s.sha => {
                                rows.extend(list.iter().map(|(sha, f, untracked)| Row::StashFile {
                                    stash: i,
                                    sha: sha.clone(),
                                    file: f.clone(),
                                    untracked: *untracked,
                                }))
                            }
                            _ => rows.push(Row::Note("Loading…".into())),
                        }
                    }
                }
            }
        }
        if !self.submodules.is_empty() {
            rows.push(Row::Section(Section::Submodules, self.submodules.len()));
            if !self.collapsed.contains(&Section::Submodules) {
                rows.extend((0..self.submodules.len()).map(Row::Submodule));
            }
        }
        self.keys = rows.iter().map(row_key).collect();
        self.rows = rows;
        cx.notify();
    }

    /// ⌃⌘C: into the commit message, the uncommitted files shown rather than a branch review.
    pub fn focus_message(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.review.is_some() {
            self.end_review(cx);
        }
        let commit = self.commit.clone();
        commit.update(cx, |b, cx| b.focus_message(window, cx));
    }

    /// A section closed or opened again, as it'll be at the next launch.
    fn toggle_section(&mut self, section: Section, cx: &mut Context<Self>) {
        if !self.collapsed.remove(&section) {
            self.collapsed.insert(section);
        }
        let titles = Section::ALL
            .into_iter()
            .filter(|s| self.collapsed.contains(s))
            .map(|s| s.title().to_string())
            .collect();
        store::set_collapsed(titles, cx);
        self.rebuild(cx);
    }

    /// List or tree, by name or newest first; kept for the next launch.
    pub fn set_view(&mut self, view: ChangesView, cx: &mut Context<Self>) {
        let reread = view.recent && !self.view.recent;
        self.view = view;
        store::set_view(view, cx);
        if reread {
            self.read(cx);
        }
        self.rebuild(cx);
    }

    /// The J/K order of `sel`'s list: the branch review's files, else every change as shown.
    pub fn walk_list(&self, sel: &Selection) -> Vec<Selection> {
        if let (Selection::Branch { .. }, Some(r)) = (sel, &self.review)
            && let Some(base) = &r.base
        {
            return r
                .files
                .iter()
                .map(|f| Selection::Branch {
                    base: base.clone(),
                    label: r.label.clone(),
                    file: f.clone(),
                    fixed: false,
                })
                .collect();
        }
        self.lists
            .ordered(self.view, self.mtimes.as_ref())
            .changes()
            .into_iter()
            .map(|c| selection_of(c.kind, c.file))
            .collect()
    }

    // MARK: Picking rows

    /// The files the rows picked stand for, by kind; just `key`'s when it isn't picked.
    fn targets(&self, kind: Kind, key: &str) -> Vec<Arc<FileChange>> {
        let picked: Vec<&String> = match &self.picked {
            Some((rows, _)) if rows.iter().any(|r| r == key) => rows.iter().collect(),
            _ => vec![],
        };
        let lists = &self.lists;
        if picked.is_empty() {
            let path = key.split_once("::").map(|x| x.1).unwrap_or(key);
            return lists
                .list(kind)
                .iter()
                .filter(|f| f.path == path)
                .cloned()
                .collect();
        }
        picked
            .iter()
            .filter_map(|k| k.split_once("::"))
            .filter(|(k, _)| *k == kind.name())
            .filter_map(|(_, path)| lists.list(kind).iter().find(|f| f.path == path).cloned())
            .collect()
    }

    fn file_keys(&self) -> Vec<String> {
        self.rows
            .iter()
            .zip(&self.keys)
            .filter(|(r, _)| matches!(r, Row::Item(Item::File { .. }) | Row::Review(_)))
            .map(|(_, k)| k.clone())
            .collect()
    }

    fn click_file(
        &mut self,
        sel: Selection,
        mods: Modifiers,
        count: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = selection_key(&sel);
        if mods.secondary() {
            let now = self
                .picked
                .as_ref()
                .map(|p| p.0.clone())
                .unwrap_or_else(|| self.open_key.iter().cloned().collect());
            self.picked = Some((toggled(&now, &key), key.clone()));
        } else if mods.shift {
            let anchor = self
                .picked
                .as_ref()
                .map(|p| p.1.clone())
                .or(self.open_key.clone())
                .unwrap_or(key.clone());
            self.picked = Some((range_of(&self.file_keys(), &anchor, &key), anchor));
        } else {
            self.picked = None;
        }
        window.focus(&self.focus, cx);
        self.open(sel, count < 2, window, cx);
        cx.notify();
    }

    fn open(&mut self, sel: Selection, preview: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.open_key = Some(selection_key(&sel));
        if let Some(ws) = self.workspace() {
            ws.update(cx, |ws, cx| ws.open(sel, preview, window, cx));
        }
    }

    /// `open` once the current update is over, from work that has no window at hand.
    fn open_later(&self, sel: Selection, preview: bool, cx: &mut Context<Self>) {
        let (handle, weak) = (self.window, cx.weak_entity());
        cx.defer(move |cx| {
            handle
                .update(cx, |_, window, cx| {
                    weak.update(cx, |this, cx| this.open(sel, preview, window, cx))
                        .ok();
                })
                .ok();
        });
    }

    /// J / K: the next or previous file after the one the keys are on, else the open one.
    fn step(&mut self, dir: isize, window: &mut Window, cx: &mut Context<Self>) {
        let keys = self.file_keys();
        if keys.is_empty() {
            return;
        }
        let at = [self.cursor_ix(cx), self.open_ix(cx)]
            .into_iter()
            .flatten()
            .find_map(|ix| keys.iter().position(|x| *x == self.keys[ix]));
        let to = match at {
            Some(i) => (i as isize + dir).clamp(0, keys.len() as isize - 1) as usize,
            None => 0,
        };
        let key = keys[to].clone();
        self.picked = None;
        self.cursor = Some(key.clone());
        if let Some(sel) = self.selection_at(&key) {
            self.open(sel, true, window, cx);
        }
        if let Some(ix) = self.keys.iter().position(|k| *k == key) {
            self.scroll
                .scroll_to_item(ix, gpui_kit::ScrollStrategy::Nearest);
        }
        cx.notify();
    }

    fn selection_at(&self, key: &str) -> Option<Selection> {
        let ix = self.keys.iter().position(|k| k == key)?;
        match &self.rows[ix] {
            Row::Item(Item::File { change, .. }) => {
                Some(selection_of(change.kind, change.file.clone()))
            }
            Row::Review(f) => self.review_selection(f),
            _ => None,
        }
    }

    /// A stash's file opens as a commit's: `sha`, against its first parent.
    fn stash_file_selection(&self, stash: usize, sha: &str, file: Arc<FileChange>) -> Selection {
        Selection::Commit {
            commit: Arc::new(stash_commit(&self.stashes[stash], sha)),
            file,
            url: None,
        }
    }

    fn review_selection(&self, f: &Arc<FileChange>) -> Option<Selection> {
        let r = self.review.as_ref()?;
        Some(Selection::Branch {
            base: r.base.clone()?,
            label: r.label.clone(),
            file: f.clone(),
            fixed: false,
        })
    }

    // MARK: Git actions

    /// Runs `work` off the main thread and reads the repo again; `failed` titles its error.
    /// `done` says it went through, with Undo when it went into the journal (a discard).
    fn run(
        &mut self,
        failed: &'static str,
        done: Option<Toast>,
        work: impl FnOnce(&gitviber_core::Core) -> Result<(), String> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        self.run_leaving(None, failed, done, work, cx);
    }

    /// `run`, for the action `leave` numbered `leaving`.
    fn run_leaving(
        &mut self,
        leaving: Option<u64>,
        failed: &'static str,
        done: Option<Toast>,
        work: impl FnOnce(&gitviber_core::Core) -> Result<(), String> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        let core = crate::core(cx);
        let repo = core.repo().ok();
        self.busy = true;
        cx.notify();
        // Short, and holding the panel weakly: one more S before git is done mustn't cancel it.
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { tracked(&core, work).map(|((), entry)| entry) })
                .await;
            this.update(cx, |this, cx| {
                this.busy = false;
                this.settle_leave(leaving, result.is_err(), cx);
                match result {
                    Ok(entry) => {
                        if let Some(done) = done {
                            let ws = this.ctx.workspace.clone();
                            this.toast(with_undo(done, entry, repo, ws), cx);
                        }
                    }
                    Err(e) => this.toast(git_error(failed, &e, &vec![]), cx),
                }
                this.refresh_project(cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn stage(&mut self, files: Vec<Arc<FileChange>>, cx: &mut Context<Self>) {
        let (paths, nested) = stageable(&files);
        if nested > 0 {
            self.toast(Toast::info(NESTED_EXPLAINED).title(left_out(nested)), cx);
        }
        if !paths.is_empty() {
            let leaving = self.leave(Kind::Unstaged, &files, cx);
            self.run_leaving(
                leaving,
                "Stage failed",
                None,
                move |core| api::changes::stage(core, paths, false),
                cx,
            );
        }
    }

    fn unstage_files(&mut self, files: Vec<Arc<FileChange>>, cx: &mut Context<Self>) {
        let paths = unstage_paths(files.iter().map(Arc::as_ref));
        if !paths.is_empty() {
            let leaving = self.leave(Kind::Staged, &files, cx);
            self.run_leaving(
                leaving,
                "Unstage failed",
                None,
                move |core| api::changes::unstage(core, paths),
                cx,
            );
        }
    }

    /// Untracked files have nothing to restore: discarding one moves it to the Trash. Tracked
    /// ones keep a copy of what they were in the Trash, which Undo writes back.
    fn discard(
        &mut self,
        picked: Vec<Arc<FileChange>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let submodules = picked.iter().filter(|f| kept_by_restore(f)).count();
        if submodules > 0 {
            self.toast(Toast::info("Discard in the submodule itself: git restore leaves its commit and files as they are.").title(format!("Left out {}", if submodules == 1 { "a submodule".into() } else { format!("{submodules} submodules") })), cx);
        }
        let list: Vec<Arc<FileChange>> =
            picked.into_iter().filter(|f| !kept_by_restore(f)).collect();
        if list.is_empty() {
            return;
        }
        let restorable: Vec<String> = list
            .iter()
            .filter(|f| f.status != "?")
            .map(|f| f.path.clone())
            .collect();
        let untracked: Vec<String> = list
            .iter()
            .filter(|f| f.status == "?")
            .map(|f| f.path.clone())
            .collect();
        let one = (list.len() == 1).then(|| list[0].path.clone());
        let copied = list
            .iter()
            .filter(|f| f.status != "?" && f.status != "D")
            .count();
        let (title, question, ok) = if restorable.is_empty() {
            let q = match &one {
                Some(p) => {
                    format!("Move {p} to the Trash? It is untracked, so git has no copy of it.")
                }
                None => format!(
                    "Move {} untracked files to the Trash? Git has no copy of them.",
                    untracked.len()
                ),
            };
            (
                if one.is_some() {
                    "Delete file"
                } else {
                    "Delete files"
                },
                q,
                "Move to Trash",
            )
        } else {
            let what = one.clone().unwrap_or_else(|| files(restorable.len()));
            let mut q = format!("Discard changes to {what}?");
            if copied > 0 {
                q.push_str(if restorable.len() == 1 {
                    " Its current version is moved to the Trash."
                } else {
                    " Their current versions are moved to the Trash."
                });
            }
            if !untracked.is_empty() {
                q.push_str(&format!(
                    " {} git doesn't track will be moved to the Trash too.",
                    files(untracked.len())
                ));
            }
            ("Discard changes", q, "Discard")
        };
        let weak = cx.entity().downgrade();
        let done = Some(discarded(
            one.as_deref(),
            restorable.len(),
            copied,
            untracked.len(),
        ));
        dialogs::confirm(window, cx, title, question, ok, move |_, cx| {
            weak.update(cx, |this, cx| {
                let leaving = this.leave(Kind::Unstaged, &list, cx);
                this.discard_now(restorable, untracked, done, leaving, cx)
            })
            .ok();
        });
    }

    /// What Discard does once asked: tracked files back to the index (their current version to
    /// the Trash), untracked ones to the Trash.
    fn discard_now(
        &mut self,
        restorable: Vec<String>,
        untracked: Vec<String>,
        done: Option<Toast>,
        leaving: Option<u64>,
        cx: &mut Context<Self>,
    ) {
        self.run_leaving(
            leaving,
            "Discard failed",
            done,
            move |core| {
                if !restorable.is_empty() {
                    api::changes::discard(core, restorable)?;
                }
                for p in untracked {
                    api::files::trash_path(core, p)?;
                }
                Ok(())
            },
            cx,
        )
    }

    /// Both sides edited (UU) or added (AA) the file, so git wrote markers into it; staging
    /// them as they are would commit them.
    fn mark_resolved(
        &mut self,
        rows: Vec<Arc<FileChange>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let core = crate::core(cx);
        let weak = cx.entity().downgrade();
        self._action = cx.spawn_in(window, async move |_, cx| {
            let check = rows.clone();
            // Both sides edited or added these: a file that can't be read can't be checked.
            let found = cx
                .background_spawn(async move {
                    check
                        .iter()
                        .filter(|f| matches!(f.conflict.as_deref(), Some("UU" | "AA")))
                        .filter_map(|f| {
                            let read = api::files::read_file(&core, f.path.clone());
                            unsure(&read).map(|u| (f.path.clone(), u))
                        })
                        .collect::<Vec<_>>()
                })
                .await;
            cx.update(|window, cx| {
                let go = move |cx: &mut App| {
                    weak.update(cx, |this, cx| this.stage(rows, cx)).ok();
                };
                if found.is_empty() {
                    go(cx);
                } else {
                    let of = |kind: Unsure| -> Vec<String> {
                        found
                            .iter()
                            .filter(|(_, u)| *u == kind)
                            .map(|(p, _)| p.clone())
                            .collect()
                    };
                    let q = resolve_question(&of(Unsure::Markers), &of(Unsure::Unread));
                    dialogs::confirm(
                        window,
                        cx,
                        "Mark resolved",
                        q,
                        "Mark Resolved",
                        move |_, cx| go(cx),
                    );
                }
            })
            .ok();
        });
    }

    fn resolve(&mut self, rows: Vec<Arc<FileChange>>, side: GitSide, cx: &mut Context<Self>) {
        let paths: Vec<String> = rows.iter().map(|f| f.path.clone()).collect();
        let ours = matches!(side, GitSide::Ours);
        self.run(
            "Resolve failed",
            None,
            move |core| {
                for p in paths {
                    api::changes::resolve_side(
                        core,
                        p,
                        if ours { GitSide::Ours } else { GitSide::Theirs },
                    )?;
                }
                Ok(())
            },
            cx,
        );
    }

    fn ignore(&mut self, rows: Vec<Arc<FileChange>>, cx: &mut Context<Self>) {
        let mut lines: Vec<String> = rows.iter().map(|f| ignore_pattern(&f.path)).collect();
        lines.dedup();
        self.run(
            "Could not update .gitignore",
            None,
            move |core| {
                let cur = api::files::read_file(core, ".gitignore".into())?;
                if cur.exists && (cur.binary || cur.lossy || cur.too_large) {
                    return Err(".gitignore is not a plain text file".into());
                }
                let sep = if !cur.text.is_empty() && !cur.text.ends_with('\n') {
                    "\n"
                } else {
                    ""
                };
                api::files::write_file(
                    core,
                    ".gitignore".into(),
                    format!("{}{sep}{}\n", cur.text, lines.join("\n")),
                )
            },
            cx,
        );
    }

    fn mark_viewed(&mut self, sels: Vec<Selection>, on: bool, cx: &mut Context<Self>) {
        let Some(ws) = self.workspace() else { return };
        // Unchecking a staged file takes it back out of the commit.
        let staged: Vec<FileChange> = sels
            .iter()
            .filter_map(|s| {
                if let Selection::Staged { file } = s {
                    Some(file.as_ref().clone())
                } else {
                    None
                }
            })
            .collect();
        if !on && !staged.is_empty() {
            unstage(staged, ws.downgrade(), cx);
            return;
        }
        ws.update(cx, |ws, cx| {
            for s in &sels {
                set_viewed(ws, s, on, cx);
            }
        });
        cx.notify();
    }

    fn stash_dialog(&mut self, paths: Vec<String>, window: &mut Window, cx: &mut Context<Self>) {
        let some = !paths.is_empty();
        let untracked_count = self
            .lists
            .unstaged
            .iter()
            .filter(|f| f.status == "?")
            .count();
        let untracked = Rc::new(Cell::new(false));
        let staged_only = Rc::new(Cell::new(false));
        let mut choices = vec![];
        if !some {
            choices.push(Choice {
                label: format!(
                    "Include {}",
                    if untracked_count > 0 {
                        format!(
                            "{untracked_count} untracked {}",
                            if untracked_count == 1 {
                                "file"
                            } else {
                                "files"
                            }
                        )
                    } else {
                        "untracked files".into()
                    }
                )
                .into(),
                enabled: untracked_count > 0,
                checked: untracked.clone(),
            });
            choices.push(Choice {
                label: "Only staged changes".into(),
                enabled: !self.lists.staged.is_empty(),
                checked: staged_only.clone(),
            });
        }
        let title = if some {
            format!(
                "Stash {}",
                if paths.len() == 1 {
                    paths[0].clone()
                } else {
                    files(paths.len())
                }
            )
        } else {
            "Stash changes".into()
        };
        let description = if some {
            "Sets the changes to these files aside and leaves the rest as they are."
        } else {
            "Sets your uncommitted changes aside and leaves the working tree clean."
        };
        let weak = cx.entity().downgrade();
        let some_untracked = some
            && paths.iter().any(|p| {
                self.lists
                    .unstaged
                    .iter()
                    .any(|f| f.path == *p && f.status == "?")
            });
        dialogs::ask_text(
            window,
            cx,
            Ask {
                title: title.into(),
                description: format!("{description} Apply or pop them later from Stashes.").into(),
                placeholder: "Message (optional)",
                initial: String::new(),
                ok_label: "Stash",
                allow_empty: true,
                choices,
            },
            move |message, _, cx| {
                let (paths, staged) = (paths.clone(), staged_only.get());
                let with_untracked = if some {
                    some_untracked
                } else {
                    untracked.get() && !staged
                };
                let done = if message.is_empty() {
                    "Changes stashed".to_string()
                } else {
                    format!("Stashed: {message}")
                };
                weak.update(cx, |this, cx| {
                    this.run(
                        "Stash failed",
                        Some(Toast::success(done)),
                        move |core| {
                            api::stash::stash_push(
                                core,
                                message,
                                with_untracked,
                                Some(!some && staged),
                                (!paths.is_empty()).then_some(paths),
                            )
                        },
                        cx,
                    )
                })
                .ok();
            },
        );
    }

    fn toggle_stash(&mut self, sha: String, cx: &mut Context<Self>) {
        if self.expanded_stash.as_deref() == Some(sha.as_str()) {
            self.expanded_stash = None;
            self.rebuild(cx);
            return;
        }
        self.expanded_stash = Some(sha.clone());
        self.rebuild(cx);
        let core = crate::core(cx);
        self._stash = cx.spawn(async move |this, cx| {
            let files = cx
                .background_spawn({
                    let sha = sha.clone();
                    async move { api::stash::stash_files(&core, sha) }
                })
                .await;
            this.update(cx, |this, cx| {
                match files {
                    Ok(f) => {
                        let mut list: Vec<(String, Arc<FileChange>, bool)> = f
                            .files
                            .into_iter()
                            .map(|x| (sha.clone(), Arc::new(x), false))
                            .collect();
                        if let Some(u) = f.untracked_sha {
                            list.extend(
                                f.untracked
                                    .into_iter()
                                    .map(|x| (u.clone(), Arc::new(x), true)),
                            );
                        }
                        this.stash_files = Some((sha.clone(), list));
                    }
                    Err(e) => this.toast(git_error("Could not load stash", &e, &vec![]), cx),
                }
                this.rebuild(cx);
            })
            .ok();
        });
    }

    fn apply_stash(&mut self, sha: String, pop: bool, cx: &mut Context<Self>) {
        let done = if pop { "Stash popped" } else { "Stash applied" };
        let weak = cx.entity().downgrade();
        let core = crate::core(cx);
        self.busy = true;
        self._action = cx.spawn(async move |_, cx| {
            let result = cx
                .background_spawn(async move { api::stash::stash_apply(&core, sha, pop) })
                .await;
            weak.update(cx, |this, cx| {
                this.busy = false;
                match result {
                    Ok(true) => this.toast(
                        Toast::warning(
                            "Resolve them in Changes. The stash is kept; drop it once you're done.",
                        )
                        .title("Stopped on conflicts"),
                        cx,
                    ),
                    Ok(false) => this.toast(Toast::success(done), cx),
                    Err(e) => this.toast(
                        git_error(if pop { "Pop failed" } else { "Apply failed" }, &e, &vec![]),
                        cx,
                    ),
                }
                this.refresh_project(cx);
            })
            .ok();
        });
    }

    fn drop_stash(
        &mut self,
        sha: String,
        text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let weak = cx.entity().downgrade();
        let short = sha[..sha.len().min(10)].to_string();
        dialogs::confirm(
            window,
            cx,
            "Drop stash",
            format!("Drop \"{text}\"? Its changes are thrown away; GitViber can't undo this."),
            "Drop",
            move |_, cx| {
                weak.update(cx, |this, cx| {
                    this.run(
                        "Drop failed",
                        Some(Toast::success(format!(
                            "Stash dropped. To get it back: git stash apply {short}"
                        ))),
                        move |core| api::stash::stash_drop(core, sha),
                        cx,
                    )
                })
                .ok();
            },
        );
    }

    fn rename_stash(
        &mut self,
        sha: String,
        text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let weak = cx.entity().downgrade();
        dialogs::ask_text(
            window,
            cx,
            Ask {
                title: "Rename stash".into(),
                description: "The stash keeps its changes and moves to the top of the list.".into(),
                placeholder: "Message",
                initial: text,
                ok_label: "Rename",
                allow_empty: false,
                choices: vec![],
            },
            move |message, _, cx| {
                let sha = sha.clone();
                weak.update(cx, |this, cx| {
                    this.run(
                        "Rename failed",
                        Some(Toast::success("Stash renamed")),
                        move |core| api::stash::stash_rename(core, sha, message),
                        cx,
                    )
                })
                .ok();
            },
        );
    }

    fn branch_from_stash(&mut self, sha: String, window: &mut Window, cx: &mut Context<Self>) {
        let weak = cx.entity().downgrade();
        dialogs::ask_text(
            window,
            cx,
            Ask {
                title: "Create branch from stash".into(),
                description: "A new branch at the commit the stash was made on, with its changes applied there. The stash is dropped once applied.".into(),
                placeholder: "Branch name",
                initial: String::new(),
                ok_label: "Create",
                allow_empty: false,
                choices: vec![],
            },
            move |name, _, cx| {
                let sha = sha.clone();
                let done = format!("Switched to new branch {name}");
                weak.update(cx, |this, cx| this.run("Branch from stash failed", Some(Toast::success(done)), move |core| api::stash::stash_branch(core, name, sha).map(|_| ()), cx)).ok();
            },
        );
    }

    fn take_from_stash(
        &mut self,
        sha: String,
        path: String,
        untracked: bool,
        cx: &mut Context<Self>,
    ) {
        let done = format!("Took {path} from the stash");
        self.run(
            "Could not take the file",
            Some(Toast::success(done)),
            move |core| api::stash::stash_restore_file(core, sha, path, untracked),
            cx,
        );
    }

    /// Sets the submodules up and brings them to the commits recorded, as the top bar's activity:
    /// a clone of one can take minutes, and Cancel stops it.
    fn update_submodules(&mut self, cx: &mut Context<Self>) {
        if self.updating_submodules {
            return;
        }
        self.updating_submodules = true;
        let net = NetActivity::global(cx);
        let op = net.update(cx, |a, cx| a.start("Update submodules".into(), cx));
        let (id, progress) = (op.id.clone(), op.progress());
        let core = crate::core(cx);
        let work =
            cx.background_spawn(async move { api::sync::submodule_update(&core, id, progress) });
        // Detached, holding the panel weakly: dropping it wouldn't stop git, only leave the top
        // bar showing an update that ended.
        cx.spawn(async move |this, cx| {
            let result = work.await;
            net.update(cx, |a, cx| a.finish(&op.id, cx));
            this.update(cx, |this, cx| {
                this.updating_submodules = false;
                let toast = match result {
                    Ok(()) => Toast::success("Submodules updated"),
                    Err(e) if e == CANCELLED => Toast::info("Submodule update cancelled"),
                    Err(e) => git_error("Could not update submodules", &e, &vec![]),
                };
                this.toast(toast, cx);
                this.refresh_project(cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn operation(&mut self, which: &'static str, window: &mut Window, cx: &mut Context<Self>) {
        let kind = self
            .operation
            .as_ref()
            .map(|o| o.kind.clone())
            .unwrap_or_default();
        let go = move |this: &mut Self, cx: &mut Context<Self>| {
            let core = crate::core(cx);
            let repo = core.repo().ok();
            this.busy = true;
            let kind = kind.clone();
            this._action = cx.spawn(async move |this, cx| {
                // Finished, the journal's entry is the whole merge or rebase: Undo takes it back.
                let result = cx
                    .background_spawn(async move {
                        tracked(&core, |core| match which {
                            "continue" => api::sync::op_continue(core),
                            "skip" => api::sync::rebase_skip(core),
                            _ => api::sync::op_abort(core).map(|_| false),
                        })
                    })
                    .await;
                this.update(cx, |this, cx| {
                    this.busy = false;
                    match result {
                        Ok((true, _)) => this.toast(
                            Toast::info("Resolve them to continue.")
                                .title("Stopped on new conflicts"),
                            cx,
                        ),
                        Ok((false, entry)) if which != "abort" => {
                            let done = Toast::success(format!(
                                "{}{} finished",
                                kind[..1.min(kind.len())].to_uppercase(),
                                &kind[1.min(kind.len())..]
                            ));
                            let ws = this.ctx.workspace.clone();
                            this.toast(with_undo(done, entry, repo, ws), cx)
                        }
                        Ok((false, _)) => {}
                        Err(e) => this.toast(
                            git_error(
                                format!("{} failed", which[..1].to_uppercase() + &which[1..]),
                                &e,
                                &vec![],
                            ),
                            cx,
                        ),
                    }
                    this.refresh_project(cx);
                })
                .ok();
            });
        };
        if which == "abort" {
            let weak = cx.entity().downgrade();
            let kind = self
                .operation
                .as_ref()
                .map(|o| o.kind.clone())
                .unwrap_or_default();
            dialogs::confirm(
                window,
                cx,
                format!("Abort {kind}"),
                format!("Abort the {kind}? Your branch goes back to how it was before it started."),
                "Abort",
                move |_, cx| {
                    weak.update(cx, |this, cx| go(this, cx)).ok();
                },
            );
        } else {
            go(self, cx);
        }
    }

    /// Apply Patch from Clipboard: what it would do, then Apply. The working tree only.
    pub(crate) fn apply_patch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let patch = cx
            .read_from_clipboard()
            .and_then(|c| c.text())
            .unwrap_or_default();
        if patch.trim().is_empty() {
            self.toast(
                Toast::info("Copy a patch first, such as one from Copy as Patch.")
                    .title("The clipboard is empty"),
                cx,
            );
            return;
        }
        let core = crate::core(cx);
        let weak = cx.entity().downgrade();
        self._action = cx.spawn_in(window, async move |_, cx| {
            let text = patch.clone();
            let preview = cx.background_spawn(async move { api::patch::patch_preview(&core, text) }).await;
            cx.update(|window, cx| match preview {
                Err(e) => {
                    weak.update(cx, |this, cx| this.toast(git_error("Can't apply that patch", &e, &vec![]), cx)).ok();
                }
                Ok(p) => {
                    let n = p.files.len();
                    let how = match p.applies.as_deref() {
                        Some("clean") => "It applies cleanly.".to_string(),
                        Some(_) => "It applies only by merging with what changed here since, which may leave conflicts to resolve.".to_string(),
                        None => format!("It doesn't apply here: {}", p.error.clone().unwrap_or_default()),
                    };
                    let listed: Vec<String> = p.files.iter().take(12).map(|f| f.path.clone()).collect();
                    let question = format!("{} from the clipboard, changed in the working tree; nothing is staged. {how}\n\n{}", if n == 1 { "One file".into() } else { format!("{n} files") }, listed.join("\n"));
                    if p.applies.is_none() {
                        weak.update(cx, |this, cx| this.toast(Toast::error(how).title("Can't apply that patch"), cx)).ok();
                        return;
                    }
                    let done = if n == 1 { format!("Applied patch to {}", p.files[0].path) } else { format!("Applied patch to {n} files") };
                    dialogs::confirm(window, cx, "Apply patch", question, "Apply", move |_, cx| {
                        weak.update(cx, |this, cx| this.run("Apply patch failed", Some(Toast::success(done)), move |core| api::patch::apply_patch(core, patch).map(|_| ()), cx)).ok();
                    });
                }
            })
            .ok();
        });
    }

    // MARK: Branch review

    /// Changes in place of the uncommitted list: what the branch changed since it left
    /// `reference`, committed or not.
    pub fn start_review(&mut self, reference: String, cx: &mut Context<Self>) {
        self.show_review(reference.clone(), cx);
        self.load_review(reference, cx);
    }

    /// The review against `reference` in place of the list, its files still to load.
    fn show_review(&mut self, reference: String, cx: &mut Context<Self>) {
        let mut label = reference
            .trim_start_matches("refs/heads/")
            .trim_start_matches("refs/remotes/")
            .to_string();
        if label.is_empty() {
            label = "a base branch".into();
        }
        // The filter narrows the uncommitted lists, which the review takes the place of.
        if self.filter.take().is_some() {
            self.commit_facts(cx);
        }
        self.review = Some(Review {
            reference: reference.clone(),
            label,
            base: None,
            files: vec![],
            error: None,
        });
        if let Some(ws) = self.workspace() {
            ws.update(cx, |ws, cx| ws.set_review_base(Some(reference), cx));
        }
        self.load_branches(cx);
        self.rebuild(cx);
    }

    fn end_review(&mut self, cx: &mut Context<Self>) {
        self.review = None;
        self._review = Task::ready(());
        if let Some(ws) = self.workspace() {
            ws.update(cx, |ws, cx| ws.set_review_base(None, cx));
        }
        self.rebuild(cx);
    }

    fn load_review(&mut self, reference: String, cx: &mut Context<Self>) {
        // No main branch here: the bar's picker names one.
        if reference.is_empty() {
            if let Some(r) = self.review.as_mut() {
                r.error = Some("Pick a branch to review against.".into());
            }
            return;
        }
        let core = crate::core(cx);
        self._review = cx.spawn(async move |this, cx| {
            let r = reference.clone();
            let review = cx
                .background_spawn(async move { api::changes::branch_review(&core, r) })
                .await;
            this.update(cx, |this, cx| {
                let Some(state) = this.review.as_mut().filter(|s| s.reference == reference) else {
                    return;
                };
                match review {
                    Ok(r) => {
                        state.base = Some(r.base);
                        state.files = r.files.into_iter().map(Arc::new).collect();
                        state.error = None;
                        this.follow_review(cx);
                    }
                    Err(e) => state.error = Some(e),
                }
                this.rebuild(cx);
            })
            .ok();
        });
    }

    fn load_branches(&mut self, cx: &mut Context<Self>) {
        let core = crate::core(cx);
        cx.spawn(async move |this, cx| {
            let branches = cx
                .background_spawn(async move { api::branches::branches(&core) })
                .await
                .unwrap_or_default();
            this.update(cx, |this, cx| {
                this.branches = branches
                    .into_iter()
                    .filter(|b| !b.current)
                    .map(|b| {
                        if b.remote {
                            format!("refs/remotes/{}", b.name)
                        } else {
                            format!("refs/heads/{}", b.name)
                        }
                    })
                    .collect();
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Review Branch Against Base: the review in progress, else one against origin's default
    /// branch (or a local one of its name). `open_all`: then every file of it in one tab.
    pub(crate) fn review_branch(&mut self, open_all: bool, cx: &mut Context<Self>) {
        let all = Selection::Changes {
            of: ChangesOf::Branch,
        };
        if self.review.is_some() {
            if open_all {
                self.open_later(all, false, cx);
            }
            return;
        }
        let core = crate::core(cx);
        cx.spawn(async move |this, cx| {
            let branches = cx
                .background_spawn(async move { api::branches::branches(&core) })
                .await
                .unwrap_or_default();
            let branches: Vec<gitviber_refs::names::Branch> =
                branches.into_iter().map(Into::into).collect();
            let base = gitviber_refs::names::review_base(&branches).unwrap_or_default();
            this.update(cx, |this, cx| {
                if this.review.is_none() {
                    this.start_review(base, cx);
                }
                if open_all {
                    this.open_later(all, false, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// Open All (Staged) Changes: a list's files in one tab; an empty list says so instead.
    pub(crate) fn open_all(&mut self, of: ChangesOf, window: &mut Window, cx: &mut Context<Self>) {
        let empty = match of {
            ChangesOf::Unstaged => self.lists.unstaged.is_empty(),
            ChangesOf::Staged => self.lists.staged.is_empty(),
            _ => false,
        };
        if self.loaded && empty {
            let what = if matches!(of, ChangesOf::Staged) {
                "Nothing is staged"
            } else {
                "No changes"
            };
            self.toast(Toast::info(format!("{what} to open.")), cx);
            return;
        }
        self.open(Selection::Changes { of }, false, window, cx);
    }
}

fn row_key(row: &Row) -> String {
    match row {
        Row::Section(s, _) => format!("section:{}", s.title()),
        Row::Item(item) => item.key(),
        Row::Nested(f, _) => format!("nested:{}", f.path),
        Row::Review(f) => format!("review:{}", f.path),
        Row::Stash(i) => format!("stash:{i}"),
        Row::StashFile { sha, file, .. } => format!("stashfile:{sha}:{}", file.path),
        Row::Submodule(i) => format!("submodule:{i}"),
        Row::Note(n) => format!("note:{n}"),
    }
}

pub fn selection_of(kind: Kind, file: Arc<FileChange>) -> Selection {
    match kind {
        Kind::Conflict => Selection::Conflict { file },
        Kind::Staged => Selection::Staged { file },
        Kind::Unstaged => Selection::Unstaged { file },
    }
}

fn change_kind(status: &str) -> ChangeKind {
    ChangeKind::from_letter(status.chars().next().unwrap_or('M'))
}

/// What a discard says once done, `one` naming the file when it's alone: what went, and that
/// the old versions (`copied` of the `restorable` files) and untracked files are in the Trash.
fn discarded(one: Option<&str>, restorable: usize, copied: usize, untracked: usize) -> Toast {
    if restorable == 0 {
        let what = one.map_or_else(|| files(untracked), str::to_string);
        return Toast::success(format!("Moved {what} to the Trash"));
    }
    let what = format!(
        "Discarded {}",
        one.map_or_else(|| files(restorable), str::to_string)
    );
    let trash = match (copied, untracked) {
        (0, 0) => return Toast::success(what),
        (0, n) => format!("{} git didn't track went to the Trash.", files(n)),
        (1, 0) => "The old version is in the Trash.".to_string(),
        (_, 0) => "The old versions are in the Trash.".to_string(),
        (c, n) => format!(
            "The old {} in the Trash, with {} git didn't track.",
            if c == 1 { "version is" } else { "versions are" },
            files(n)
        ),
    };
    Toast::success(trash).title(what)
}

/// A stash as the commit its files' diffs are read from: `sha`, against its first parent.
fn stash_commit(s: &StashInfo, sha: &str) -> Commit {
    Commit {
        sha: sha.to_string(),
        short_sha: format!("stash@{{{}}}", s.index),
        author_name: s.author.clone(),
        author_email: String::new(),
        timestamp: s.timestamp,
        committer_name: s.author.clone(),
        committed_at: s.timestamp,
        parents: vec![],
        refs: vec![],
        subject: s.text.clone(),
        body: String::new(),
        unpushed: false,
        on_origin: false,
        not_in_head: false,
        file: None,
    }
}

// MARK: Drawing

/// A row's small button, shown while the row is hovered.
fn row_button(
    id: impl Into<SharedString>,
    icon: Lucide,
    tip: impl Into<SharedString>,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Button {
    Button::new(gpui_kit::ElementId::Name(id.into()))
        .ghost()
        .xsmall()
        .icon(icon)
        .tooltip(tip.into())
        .on_click(move |e, w, cx| {
            cx.stop_propagation();
            on_click(e, w, cx)
        })
}

impl ChangesPanel {
    fn render_rows(
        &mut self,
        range: std::ops::Range<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        #[cfg(test)]
        {
            self.drawn = range.clone();
        }
        let t = cx.app_theme().clone();
        let ws = self.workspace();
        // Where the keys are, while they're the list's.
        let cursor = self
            .focus
            .is_focused(window)
            .then(|| self.cursor_ix(cx))
            .flatten();
        range
            .filter_map(|ix| {
                self.rows
                    .get(ix)
                    .cloned()
                    .map(|row| self.render_row(ix, row, &t, ws.as_ref(), cursor == Some(ix), cx))
            })
            .collect()
    }

    fn render_row(
        &self,
        ix: usize,
        row: Row,
        t: &AppTheme,
        ws: Option<&Entity<Workspace>>,
        at_cursor: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let row_key = self.keys[ix].clone();
        let base = || {
            h_flex()
                .id(("changes-row", ix))
                .h(px(ROW))
                .w_full()
                .gap_1p5()
                .pr_2()
                .text_size(px(12.))
                .group("row")
        };
        match row {
            Row::Section(section, count) => self.section_row(ix, section, count, t, cx),
            Row::Note(text) => base()
                .pl_6()
                .text_color(t.subtle)
                .text_size(px(11.5))
                .child(text)
                .into_any_element(),
            Row::Nested(file, depth) => base()
                .pl(px(10. + depth as f32 * 12.))
                .text_color(t.subtle)
                .child(Icon::new(Lucide::FolderGit2).xsmall())
                .child(PathLabel(file.path.clone().into()))
                .child(div().flex_1())
                .child(div().text_size(px(11.)).child("nested repo"))
                .into_any_element(),
            Row::Item(Item::Folder {
                kind,
                path,
                label,
                depth,
                open,
                files: inside,
            }) => {
                let key = folder_key(kind, &path);
                let rows: Vec<Arc<FileChange>> = inside
                    .iter()
                    .filter(|f| f.nested.is_none())
                    .cloned()
                    .collect();
                base()
                    .pl(px(6. + depth as f32 * 12.))
                    .cursor_pointer()
                    .hover(|s| s.bg(t.hover))
                    .when(at_cursor, |el| el.border_1().border_color(t.ring))
                    .child(
                        Icon::new(if open {
                            Lucide::ChevronDown
                        } else {
                            Lucide::ChevronRight
                        })
                        .xsmall()
                        .text_color(t.subtle),
                    )
                    .child(FileIcon::folder(&label, open))
                    .child(div().min_w_0().truncate().child(label.clone()))
                    .child(
                        div()
                            .text_color(t.subtle)
                            .text_size(px(11.))
                            .child(inside.len().to_string()),
                    )
                    .child(div().flex_1())
                    // On the hovered row, and on the keys' row.
                    .child(
                        h_flex()
                            .when(!at_cursor, |el| el.invisible())
                            .group_hover("row", |s| s.visible())
                            .children(self.actions_for(kind, rows.clone(), true, cx)),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        window.focus(&this.focus, cx);
                        this.cursor = Some(row_key.clone());
                        if !this.closed.remove(&key) {
                            this.closed.insert(key.clone());
                        }
                        this.rebuild(cx);
                    }))
                    .context_menu(self.folder_menu(kind, path, rows, ws, cx))
                    .into_any_element()
            }
            Row::Item(Item::File {
                change,
                depth,
                name,
            }) => {
                let sel = selection_of(change.kind, change.file.clone());
                self.file_row(
                    ix,
                    sel,
                    change.kind,
                    change.file.clone(),
                    depth,
                    name,
                    t,
                    ws,
                    at_cursor,
                    cx,
                )
            }
            Row::Review(file) => match self.review_selection(&file) {
                Some(sel) => {
                    self.file_row(ix, sel, Kind::Unstaged, file, 0, None, t, ws, at_cursor, cx)
                }
                None => base().into_any_element(),
            },
            Row::Stash(i) => self.stash_row(ix, i, t, at_cursor, cx),
            Row::StashFile {
                stash,
                sha,
                file,
                untracked,
            } => self.stash_file_row(ix, stash, sha, file, untracked, t, ws, at_cursor, cx),
            Row::Submodule(i) => {
                let (path, sha, state) = self.submodules[i].clone();
                let (note, color) = match state.as_str() {
                    "missing" => ("not set up", t.modified),
                    "moved" => ("on another commit", t.modified),
                    "conflict" => ("conflicted", t.removed),
                    _ => ("", t.subtle),
                };
                base()
                    .pl_4()
                    .child(Icon::new(Lucide::FolderGit2).xsmall().text_color(t.subtle))
                    .child(div().flex_1().min_w_0().truncate().child(path))
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(color)
                            .child(if note.is_empty() {
                                sha[..sha.len().min(7)].to_string()
                            } else {
                                note.to_string()
                            }),
                    )
                    .into_any_element()
            }
        }
    }

    fn section_row(
        &self,
        ix: usize,
        section: Section,
        count: usize,
        t: &AppTheme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let collapsed = self.collapsed.contains(&section);
        let filtering = self.filtering(cx);
        let mut buttons: Vec<Button> = vec![];
        let small = |id: &str, label: String| {
            Button::new(SharedString::from(format!("{id}-{ix}")))
                .ghost()
                .xsmall()
                .label(label)
        };
        let shown = self.shown_lists(cx);
        let picked_of = |kind: Kind| -> Vec<Arc<FileChange>> {
            match &self.picked {
                Some((rows, _)) if rows.len() > 1 => rows
                    .iter()
                    .filter_map(|k| k.split_once("::"))
                    .filter(|(k, _)| *k == kind.name())
                    .filter_map(|(_, p)| {
                        self.lists.list(kind).iter().find(|f| f.path == p).cloned()
                    })
                    .collect(),
                _ => vec![],
            }
        };
        match section {
            Section::List(Kind::Unstaged) => {
                let picked = picked_of(Kind::Unstaged);
                if picked.len() > 1 {
                    let (p1, p2) = (picked.clone(), picked.clone());
                    buttons.push(
                        small(
                            "discard-picked",
                            format!("Discard {}…", files(picked.len())),
                        )
                        .on_click(
                            cx.listener(move |this, _, w, cx| this.discard(p1.clone(), w, cx)),
                        ),
                    );
                    buttons.push(
                        small("stage-picked", format!("Stage {}", files(picked.len()))).on_click(
                            cx.listener(move |this, _, _, cx| this.stage(p2.clone(), cx)),
                        ),
                    );
                } else {
                    let discardable: Vec<Arc<FileChange>> = shown
                        .unstaged
                        .iter()
                        .filter(|f| f.status != "?" && !kept_by_restore(f))
                        .cloned()
                        .collect();
                    let viewed: Vec<Arc<FileChange>> = match self.workspace() {
                        Some(ws) => shown
                            .unstaged
                            .iter()
                            .filter(|f| {
                                f.nested.is_none()
                                    && is_viewed(
                                        ws.read(cx),
                                        &Selection::Unstaged { file: (*f).clone() },
                                    )
                            })
                            .cloned()
                            .collect(),
                        None => vec![],
                    };
                    let all = shown.unstaged.clone();
                    buttons.push(small("open-all", "Open all".into()).on_click(
                        cx.listener(|this, _, w, cx| this.open_all(ChangesOf::Unstaged, w, cx)),
                    ));
                    if !discardable.is_empty() {
                        buttons.push(
                            small(
                                "discard-all",
                                if filtering {
                                    format!("Discard {} shown…", discardable.len())
                                } else {
                                    "Discard".into()
                                },
                            )
                            .on_click(cx.listener(
                                move |this, _, w, cx| this.discard(discardable.clone(), w, cx),
                            )),
                        );
                    }
                    if !viewed.is_empty() {
                        let n = viewed.len();
                        buttons.push(small("stage-viewed", format!("Stage {n} viewed")).on_click(
                            cx.listener(move |this, _, _, cx| this.stage(viewed.clone(), cx)),
                        ));
                    }
                    buttons.push(
                        small(
                            "stage-all",
                            if filtering {
                                format!("Stage {} shown", stageable(&all).0.len())
                            } else {
                                "Stage all".into()
                            },
                        )
                        .on_click(cx.listener(move |this, _, _, cx| this.stage(all.clone(), cx))),
                    );
                }
            }
            Section::List(Kind::Staged) => {
                let picked = picked_of(Kind::Staged);
                if picked.len() > 1 {
                    let n = picked.len();
                    buttons.push(
                        small("unstage-picked", format!("Unstage {}", files(n))).on_click(
                            cx.listener(move |this, _, _, cx| {
                                this.unstage_files(picked.clone(), cx)
                            }),
                        ),
                    );
                } else {
                    let all = shown.staged.clone();
                    buttons.push(small("open-all-staged", "Open all".into()).on_click(
                        cx.listener(|this, _, w, cx| this.open_all(ChangesOf::Staged, w, cx)),
                    ));
                    buttons.push(
                        small(
                            "unstage-all",
                            if filtering {
                                format!("Unstage {} shown", all.len())
                            } else {
                                "Unstage all".into()
                            },
                        )
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.unstage_files(all.clone(), cx)),
                        ),
                    );
                }
            }
            Section::List(Kind::Conflict) => {
                let picked = picked_of(Kind::Conflict);
                if picked.len() > 1 {
                    let n = picked.len();
                    buttons.push(
                        small("resolve-picked", format!("Mark {} resolved", files(n))).on_click(
                            cx.listener(move |this, _, w, cx| {
                                this.mark_resolved(picked.clone(), w, cx)
                            }),
                        ),
                    );
                }
            }
            Section::Review => {
                buttons.push(small("open-all-branch", "Open all".into()).on_click(
                    cx.listener(|this, _, w, cx| this.open_all(ChangesOf::Branch, w, cx)),
                ));
            }
            Section::Stashes => {
                if self.lists.total() > 0 && self.operation.is_none() {
                    buttons.push(
                        small("stash", "Stash…".into()).on_click(
                            cx.listener(|this, _, w, cx| this.stash_dialog(vec![], w, cx)),
                        ),
                    );
                }
            }
            Section::Submodules => buttons.push(
                small("update-submodules", "Update".into())
                    .disabled(self.updating_submodules)
                    .on_click(cx.listener(|this, _, _, cx| this.update_submodules(cx))),
            ),
        }
        let tone = if section == Section::List(Kind::Conflict) {
            t.conflict
        } else {
            t.muted_foreground
        };
        h_flex()
            .id(("changes-row", ix))
            .h(px(ROW))
            .w_full()
            .gap_1()
            .pl_1p5()
            .pr_1()
            .border_t_1()
            .border_color(t.border)
            .text_size(px(11.))
            .cursor_pointer()
            .child(
                Icon::new(if collapsed {
                    Lucide::ChevronRight
                } else {
                    Lucide::ChevronDown
                })
                .xsmall()
                .text_color(t.subtle),
            )
            .child(
                div()
                    .text_color(tone)
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .child(section.title().to_uppercase()),
            )
            .child(div().text_color(t.subtle).child(count.to_string()))
            .child(div().flex_1())
            .children(buttons)
            .on_click(cx.listener(move |this, _, _, cx| this.toggle_section(section, cx)))
            .into_any_element()
    }

    /// A tree folder's right-click menu: its git actions cover `rows`, every file under it.
    fn folder_menu(
        &self,
        kind: Kind,
        path: String,
        rows: Vec<Arc<FileChange>>,
        ws: Option<&Entity<Workspace>>,
        cx: &mut Context<Self>,
    ) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
        let weak = cx.entity().downgrade();
        let sels: Vec<Selection> = rows.iter().map(|f| selection_of(kind, f.clone())).collect();
        let all_viewed = ws.is_some_and(|ws| sels.iter().all(|s| is_viewed(ws.read(cx), s)));
        let can_stash = self.operation.is_none();
        let root = self.root.clone();
        let workspace = self.ctx.workspace.clone();
        move |menu, _, _| {
            let n = rows.len();
            let act = |f: fn(
                &mut ChangesPanel,
                Vec<Arc<FileChange>>,
                &mut Window,
                &mut Context<ChangesPanel>,
            )| {
                let (w, rows) = (weak.clone(), rows.clone());
                move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                    w.update(cx, |this, cx| f(this, rows.clone(), window, cx))
                        .ok();
                }
            };
            let mut menu = menu;
            match kind {
                Kind::Unstaged => {
                    menu = menu
                        .item(
                            PopupMenuItem::new(format!("Stage {}", files(n)))
                                .disabled(n == 0)
                                .on_click(act(|this, rows, _, cx| this.stage(rows, cx))),
                        )
                        .item(
                            PopupMenuItem::new(format!("Discard {}…", files(n)))
                                .disabled(rows.iter().all(|f| kept_by_restore(f)))
                                .on_click(act(|this, rows, window, cx| {
                                    this.discard(rows, window, cx)
                                })),
                        );
                }
                Kind::Staged => {
                    menu = menu.item(
                        PopupMenuItem::new(format!("Unstage {}", files(n)))
                            .on_click(act(|this, rows, _, cx| this.unstage_files(rows, cx))),
                    );
                }
                Kind::Conflict => {
                    menu = menu
                        .item(
                            PopupMenuItem::new(format!("Mark {} as Resolved", files(n))).on_click(
                                act(|this, rows, window, cx| this.mark_resolved(rows, window, cx)),
                            ),
                        )
                        .item(
                            PopupMenuItem::new(format!("Take Current Version of {}", files(n)))
                                .on_click(act(|this, rows, _, cx| {
                                    this.resolve(rows, GitSide::Ours, cx)
                                })),
                        )
                        .item(
                            PopupMenuItem::new(format!("Take Incoming Version of {}", files(n)))
                                .on_click(act(|this, rows, _, cx| {
                                    this.resolve(rows, GitSide::Theirs, cx)
                                })),
                        );
                }
            }
            if kind != Kind::Conflict && can_stash {
                menu = menu.item(
                    PopupMenuItem::new(format!("Stash {}…", files(n)))
                        .disabled(n == 0)
                        .on_click(act(|this, rows, window, cx| {
                            let paths = rows.iter().map(|f| f.path.clone()).collect();
                            this.stash_dialog(paths, window, cx)
                        })),
                );
            }
            if kind == Kind::Unstaged && n > 0 {
                let (w, sels) = (weak.clone(), sels.clone());
                menu = menu.item(
                    PopupMenuItem::new(format!(
                        "Mark {} as {}",
                        files(n),
                        if all_viewed { "Not Viewed" } else { "Viewed" }
                    ))
                    .on_click(move |_, _, cx| {
                        w.update(cx, |this, cx| {
                            this.mark_viewed(sels.clone(), !all_viewed, cx)
                        })
                        .ok();
                    }),
                );
            }
            let (abs, rel, reveal) = (
                format!("{}/{}", root.trim_end_matches('/'), path),
                path.clone(),
                path.clone(),
            );
            let (w, shown) = (workspace.clone(), path.clone());
            menu.separator()
                .item(PopupMenuItem::new("Reveal in Explorer View").on_click(
                    move |_, window, cx| gitviber_files::reveal_in_explorer(&w, &shown, window, cx),
                ))
                .item(PopupMenuItem::new(REVEAL_LABEL).on_click(move |_, _, cx| {
                    let (core, path) = (crate::core(cx), reveal.clone());
                    cx.background_spawn(async move { api::files::reveal_path(&core, path) })
                        .detach();
                }))
                .separator()
                .item(PopupMenuItem::new("Copy Path").on_click(move |_, _, cx| {
                    cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(abs.clone()))
                }))
                .item(
                    PopupMenuItem::new("Copy Relative Path").on_click(move |_, _, cx| {
                        cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(rel.clone()))
                    }),
                )
        }
    }

    /// A row's buttons, for its own file or a folder's (`files`, every file under it).
    fn actions_for(
        &self,
        kind: Kind,
        rows: Vec<Arc<FileChange>>,
        many: bool,
        cx: &mut Context<Self>,
    ) -> Vec<Button> {
        let many = many || rows.len() > 1;
        let n = files(rows.len());
        let id = rows.first().map(|f| f.path.clone()).unwrap_or_default();
        let (r1, r2, r3) = (rows.clone(), rows.clone(), rows.clone());
        match kind {
            Kind::Conflict => vec![
                row_button(
                    format!("resolved-{id}"),
                    Lucide::Check,
                    if many {
                        format!("Mark {n} resolved as they are")
                    } else {
                        "Mark resolved as it is".into()
                    },
                    cx.listener(move |this, _, w, cx| this.mark_resolved(r1.clone(), w, cx)),
                ),
                row_button(
                    format!("ours-{id}"),
                    Lucide::ArrowLeftToLine,
                    if many {
                        format!("Take current version of {n}")
                    } else {
                        "Take current version".into()
                    },
                    cx.listener(move |this, _, _, cx| this.resolve(r2.clone(), GitSide::Ours, cx)),
                ),
                row_button(
                    format!("theirs-{id}"),
                    Lucide::ArrowRightToLine,
                    if many {
                        format!("Take incoming version of {n}")
                    } else {
                        "Take incoming version".into()
                    },
                    cx.listener(move |this, _, _, cx| {
                        this.resolve(r3.clone(), GitSide::Theirs, cx)
                    }),
                ),
            ],
            Kind::Staged => vec![row_button(
                format!("unstage-{id}"),
                Lucide::Minus,
                if many {
                    format!("Unstage {n}")
                } else {
                    "Unstage".into()
                },
                cx.listener(move |this, _, _, cx| this.unstage_files(r1.clone(), cx)),
            )],
            Kind::Unstaged => {
                // A file's own button leaves untracked files out (deleting is in its menu); a folder's covers them.
                let discardable = if many {
                    rows.iter().any(|f| !kept_by_restore(f))
                } else {
                    rows.first()
                        .is_some_and(|f| f.status != "?" && !kept_by_restore(f))
                };
                let mut out = vec![];
                if discardable {
                    out.push(row_button(
                        format!("discard-{id}"),
                        Lucide::Undo2,
                        if many {
                            format!("Discard {n}…")
                        } else {
                            "Discard changes".into()
                        },
                        cx.listener(move |this, _, w, cx| this.discard(r1.clone(), w, cx)),
                    ));
                }
                out.push(row_button(
                    format!("stage-{id}"),
                    Lucide::Plus,
                    if many {
                        format!("Stage {n}")
                    } else {
                        "Stage".into()
                    },
                    cx.listener(move |this, _, _, cx| this.stage(r2.clone(), cx)),
                ));
                out
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn file_row(
        &self,
        ix: usize,
        sel: Selection,
        kind: Kind,
        file: Arc<FileChange>,
        depth: usize,
        name: Option<String>,
        t: &AppTheme,
        ws: Option<&Entity<Workspace>>,
        at_cursor: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let key = selection_key(&sel);
        let row_key = self.keys[ix].clone();
        let active = self.open_key.as_deref() == Some(key.as_str());
        let shown = active || at_cursor;
        let picked = self.picked.as_ref().is_some_and(|p| p.0.contains(&key));
        let viewed = ws.is_some_and(|ws| is_viewed(ws.read(cx), &sel));
        let is_review = matches!(sel, Selection::Branch { .. });
        let targets = if is_review {
            vec![file.clone()]
        } else {
            self.targets(kind, &key)
        };
        let check_tip = if kind == Kind::Staged && !is_review {
            "Unstage"
        } else if viewed {
            "Mark as not viewed"
        } else {
            "Mark as viewed"
        };
        let (sel_click, sel_check, sel_menu) = (sel.clone(), sel.clone(), sel.clone());
        let picked_sels: Vec<Selection> = if is_review {
            vec![sel.clone()]
        } else {
            targets
                .iter()
                .map(|f| selection_of(kind, f.clone()))
                .collect()
        };
        let check_sels = picked_sels.clone();
        h_flex()
            .id(("changes-row", ix))
            .relative()
            .h(px(ROW))
            .w_full()
            .gap_1p5()
            .pl(px(8. + depth as f32 * 12.))
            .pr_2()
            .text_size(px(12.))
            .group("row")
            .cursor_pointer()
            .when(active, |el| el.bg(t.primary.opacity(0.15)))
            .when(picked && !active, |el| el.bg(t.active))
            .when(!active && !picked, |el| el.hover(|s| s.bg(t.hover)))
            .when(at_cursor && !active, |el| {
                el.border_1().border_color(t.ring)
            })
            .when(active, |el| {
                el.child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .w(px(2.))
                        .h_full()
                        .bg(t.primary),
                )
            })
            .when(kind != Kind::Conflict || is_review, |el| {
                el.child(
                    div()
                        .id(("check", ix))
                        .size(px(13.))
                        .flex_none()
                        .rounded(px(3.))
                        .border_1()
                        .border_color(if viewed {
                            t.added_fill
                        } else {
                            t.border_strong
                        })
                        .when(viewed, |el| {
                            el.bg(t.added_fill).child(
                                Icon::new(Lucide::Check)
                                    .size(px(10.))
                                    .text_color(t.on_status),
                            )
                        })
                        .tooltip(move |window, cx| {
                            gpui_kit::component::tooltip::Tooltip::new(check_tip).build(window, cx)
                        })
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            let _ = &sel_check;
                            this.mark_viewed(check_sels.clone(), !viewed, cx);
                        })),
                )
            })
            .child(FileIcon::file(&file.path))
            .child(div().min_w_0().child(match &name {
                Some(n) => div().truncate().child(n.clone()).into_any_element(),
                None => PathLabel(file.path.clone().into()).into_any_element(),
            }))
            .when_some(file.old_path.clone(), |el, old| {
                el.child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_size(px(11.))
                        .text_color(t.subtle)
                        .child(format!("← {old}")),
                )
            })
            .child(div().flex_1())
            // The actions show on the hovered row, the keys' row and the open file's, in place
            // of the line counts.
            .child(
                div()
                    .when(shown && !is_review, |el| el.invisible())
                    .group_hover("row", |s| s.invisible())
                    .child(LineCounts::new(file.additions, file.deletions)),
            )
            .when(!is_review, |el| {
                el.child(
                    h_flex()
                        .absolute()
                        .right(px(22.))
                        .when(!shown, |el| el.invisible())
                        .group_hover("row", |s| s.visible())
                        .children(self.actions_for(kind, targets.clone(), false, cx)),
                )
            })
            .child(StatusLetter(change_kind(&file.status)))
            .on_click(cx.listener(move |this, e: &ClickEvent, window, cx| {
                this.cursor = Some(row_key.clone());
                this.click_file(
                    sel_click.clone(),
                    e.modifiers(),
                    e.click_count(),
                    window,
                    cx,
                )
            }))
            .context_menu(self.file_menu(sel_menu, kind, file.clone(), targets.clone(), viewed, cx))
            .into_any_element()
    }

    fn stash_row(
        &self,
        ix: usize,
        i: usize,
        t: &AppTheme,
        at_cursor: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let row_key = self.keys[ix].clone();
        let s = &self.stashes[i];
        let expanded = self.expanded_stash.as_deref() == Some(s.sha.as_str());
        let (sha, text) = (s.sha.clone(), s.text.clone());
        let (a, p, d, toggle) = (
            sha.clone(),
            sha.clone(),
            (sha.clone(), text.clone()),
            sha.clone(),
        );
        let weak = cx.entity().downgrade();
        let (menu_sha, menu_text) = (sha.clone(), text.clone());
        h_flex()
            .id(("changes-row", ix))
            .h(px(ROW))
            .w_full()
            .gap_1p5()
            .pl_2()
            .pr_2()
            .text_size(px(12.))
            .group("row")
            .cursor_pointer()
            .when(expanded, |el| el.bg(t.active))
            .hover(|s| s.bg(t.hover))
            .when(at_cursor, |el| el.border_1().border_color(t.ring))
            .child(
                Icon::new(if expanded {
                    Lucide::ChevronDown
                } else {
                    Lucide::ChevronRight
                })
                .xsmall()
                .text_color(t.subtle),
            )
            .child(Icon::new(Lucide::Archive).xsmall().text_color(t.subtle))
            .child(div().flex_1().min_w_0().truncate().child(text.clone()))
            .child(
                h_flex()
                    .gap_1()
                    .when(at_cursor, |el| el.invisible())
                    .group_hover("row", |s| s.invisible())
                    .text_size(px(10.5))
                    .text_color(t.subtle)
                    .when_some(s.branch.clone(), |el, b| el.child(b).child("·"))
                    // Kept current: "just now" turns into "5m ago" while the window sits idle.
                    .child(RelativeTime::new(s.timestamp)),
            )
            .child(
                h_flex()
                    .absolute()
                    .right_2()
                    .when(!at_cursor, |el| el.invisible())
                    .group_hover("row", |s| s.visible())
                    .child(row_button(
                        format!("apply-{ix}"),
                        Lucide::PackageOpen,
                        "Apply (keep the stash)",
                        cx.listener(move |this, _, _, cx| this.apply_stash(a.clone(), false, cx)),
                    ))
                    .child(row_button(
                        format!("pop-{ix}"),
                        Lucide::ArchiveRestore,
                        "Pop (apply, then drop it)",
                        cx.listener(move |this, _, _, cx| this.apply_stash(p.clone(), true, cx)),
                    ))
                    .child(row_button(
                        format!("drop-{ix}"),
                        Lucide::Trash,
                        "Drop…",
                        cx.listener(move |this, _, w, cx| {
                            this.drop_stash(d.0.clone(), d.1.clone(), w, cx)
                        }),
                    )),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                window.focus(&this.focus, cx);
                this.cursor = Some(row_key.clone());
                this.toggle_stash(toggle.clone(), cx)
            }))
            .context_menu(move |menu, _, _| {
                let (w1, w2, w3, w4, w5, w6) = (
                    weak.clone(),
                    weak.clone(),
                    weak.clone(),
                    weak.clone(),
                    weak.clone(),
                    weak.clone(),
                );
                let (s1, s2, s3, s4, s5, s6) = (
                    menu_sha.clone(),
                    menu_sha.clone(),
                    menu_sha.clone(),
                    menu_sha.clone(),
                    menu_sha.clone(),
                    menu_sha.clone(),
                );
                let (t4, t6) = (menu_text.clone(), menu_text.clone());
                menu.item(PopupMenuItem::new("Apply Stash").on_click(move |_, _, cx| {
                    w1.update(cx, |this, cx| this.apply_stash(s1.clone(), false, cx))
                        .ok();
                }))
                .item(PopupMenuItem::new("Pop Stash").on_click(move |_, _, cx| {
                    w2.update(cx, |this, cx| this.apply_stash(s2.clone(), true, cx))
                        .ok();
                }))
                .item(PopupMenuItem::new("Create Branch from Stash…").on_click(
                    move |_, window, cx| {
                        w3.update(cx, |this, cx| {
                            this.branch_from_stash(s3.clone(), window, cx)
                        })
                        .ok();
                    },
                ))
                .item(
                    PopupMenuItem::new("Rename…").on_click(move |_, window, cx| {
                        w4.update(cx, |this, cx| {
                            this.rename_stash(s4.clone(), t4.clone(), window, cx)
                        })
                        .ok();
                    }),
                )
                .item(
                    PopupMenuItem::new("Copy as Patch").on_click(move |_, _, cx| {
                        let core = crate::core(cx);
                        let (sha, w) = (s5.clone(), w5.clone());
                        cx.spawn(async move |cx| {
                            let patch = cx
                                .background_spawn(
                                    async move { api::patch::stash_patch(&core, sha) },
                                )
                                .await;
                            cx.update(|cx| match patch {
                                Ok(p) => {
                                    cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(p));
                                    w.update(cx, |this, cx| {
                                        this.toast(Toast::success("Patch copied"), cx)
                                    })
                                    .ok();
                                }
                                Err(e) => {
                                    w.update(cx, |this, cx| {
                                        this.toast(
                                            Toast::error(e).title("Could not copy the patch"),
                                            cx,
                                        )
                                    })
                                    .ok();
                                }
                            });
                        })
                        .detach();
                    }),
                )
                .separator()
                .item(
                    PopupMenuItem::new("Drop Stash…").on_click(move |_, window, cx| {
                        w6.update(cx, |this, cx| {
                            this.drop_stash(s6.clone(), t6.clone(), window, cx)
                        })
                        .ok();
                    }),
                )
            })
            .into_any_element()
    }

    /// A stash's file: its diff opens as a commit's, Take This File puts it in the working tree.
    #[allow(clippy::too_many_arguments)]
    fn stash_file_row(
        &self,
        ix: usize,
        stash: usize,
        sha: String,
        file: Arc<FileChange>,
        untracked: bool,
        t: &AppTheme,
        ws: Option<&Entity<Workspace>>,
        at_cursor: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let row_key = self.keys[ix].clone();
        let s = &self.stashes[stash];
        let sel = self.stash_file_selection(stash, &sha, file.clone());
        let key = selection_key(&sel);
        let active = ws.is_some_and(|ws| {
            ws.read(cx).tabs().active_key().map(|k| k.as_ref()) == Some(key.as_str())
        });
        // The stash names it: git takes an untracked file from the stash's third parent.
        let (stash_sha, path) = (s.sha.clone(), file.path.clone());
        let can_take = !self.busy && file.status != "D";
        let take = Rc::new(
            move |this: &mut ChangesPanel, cx: &mut Context<ChangesPanel>| {
                this.take_from_stash(stash_sha.clone(), path.clone(), untracked, cx)
            },
        );
        let (take_button, take_menu) = (take.clone(), take);
        let weak = cx.entity().downgrade();
        h_flex()
            .id(("changes-row", ix))
            .relative()
            .h(px(ROW))
            .w_full()
            .gap_1p5()
            .pl_8()
            .pr_2()
            .text_size(px(12.))
            .group("row")
            .cursor_pointer()
            .when(active, |el| {
                el.bg(t.primary.opacity(0.15)).child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .w(px(2.))
                        .h_full()
                        .bg(t.primary),
                )
            })
            .when(!active, |el| el.hover(|s| s.bg(t.hover)))
            .when(at_cursor && !active, |el| {
                el.border_1().border_color(t.ring)
            })
            .child(FileIcon::file(&file.path))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(PathLabel(file.path.clone().into())),
            )
            .child(
                div()
                    .when(active || at_cursor, |el| el.invisible())
                    .group_hover("row", |s| s.invisible())
                    .child(LineCounts::new(file.additions, file.deletions)),
            )
            .child(
                h_flex()
                    .when(!active && !at_cursor, |el| el.invisible())
                    .group_hover("row", |s| s.visible())
                    .child(
                        row_button(
                            format!("take-{ix}"),
                            Lucide::FileDown,
                            "Take this file (replaces the working copy)",
                            cx.listener(move |this, _, _, cx| take_button(this, cx)),
                        )
                        .disabled(!can_take),
                    ),
            )
            .child(StatusLetter(change_kind(&file.status)))
            .on_click(cx.listener(move |this, e: &ClickEvent, window, cx| {
                window.focus(&this.focus, cx);
                this.cursor = Some(row_key.clone());
                this.picked = None;
                this.open(sel.clone(), e.click_count() < 2, window, cx);
            }))
            .context_menu(move |menu, _, _| {
                let (w, take) = (weak.clone(), take_menu.clone());
                menu.item(
                    PopupMenuItem::new("Take This File")
                        .disabled(!can_take)
                        .on_click(move |_, _, cx| {
                            w.update(cx, |this, cx| take(this, cx)).ok();
                        }),
                )
            })
            .into_any_element()
    }

    fn render_banner(&self, t: &AppTheme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let op = self.operation.as_ref()?;
        // The bisect bar instead.
        if op.kind == "bisect" {
            return None;
        }
        let left = self.lists.conflicted.len();
        let uncommitted = self.lists.staged.len() + self.lists.unstaged.len();
        let label = if op.split {
            "Splitting a commit".to_string()
        } else {
            match op.kind.as_str() {
                "merge" => "Merging",
                "rebase" => "Rebasing",
                "cherry-pick" => "Cherry-picking",
                "revert" => "Reverting",
                _ => "In progress",
            }
            .to_string()
        };
        let note = if op.split {
            format!(
                "Commit the pieces with line staging, then Continue.{}",
                if uncommitted > 0 {
                    format!(" {} not committed yet.", files(uncommitted))
                } else {
                    String::new()
                }
            )
        } else if left > 0 {
            format!(
                "{left} {} left. Resolve them, then continue.",
                if left == 1 { "conflict" } else { "conflicts" }
            )
        } else {
            "All conflicts resolved. Continue to finish.".into()
        };
        Some(
            v_flex()
                .flex_none()
                .gap_1()
                .px_3()
                .py_2()
                .border_b_1()
                .border_color(t.conflict.opacity(0.4))
                .bg(t.conflict.opacity(0.1))
                .text_size(px(12.))
                .child(
                    h_flex()
                        .gap_2()
                        .child(Icon::new(Lucide::GitMerge).xsmall().text_color(t.conflict))
                        .child(
                            div()
                                .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                .child(label),
                        )
                        .when_some(op.step.zip(op.total), |el, (s, n)| {
                            el.child(
                                div()
                                    .text_color(t.muted_foreground)
                                    .child(format!("{s}/{n}")),
                            )
                        })
                        .when_some(op.subject.clone(), |el, s| {
                            el.child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_color(t.muted_foreground)
                                    .child(s),
                            )
                        }),
                )
                .child(
                    div()
                        .text_size(px(11.5))
                        .text_color(t.muted_foreground)
                        .child(note),
                )
                .child(
                    h_flex()
                        .gap_1()
                        .child(
                            Button::new("op-continue")
                                .small()
                                .primary()
                                .flex_1()
                                .label("Continue")
                                .disabled(self.busy || left > 0)
                                .on_click(
                                    cx.listener(|this, _, w, cx| this.operation("continue", w, cx)),
                                ),
                        )
                        .when(op.kind == "rebase" && !op.split, |el| {
                            el.child(
                                Button::new("op-skip")
                                    .small()
                                    .label("Skip commit")
                                    .disabled(self.busy)
                                    .on_click(
                                        cx.listener(|this, _, w, cx| this.operation("skip", w, cx)),
                                    ),
                            )
                        })
                        .child(
                            Button::new("op-abort")
                                .small()
                                .danger()
                                .label("Abort")
                                .disabled(self.busy)
                                .on_click(
                                    cx.listener(|this, _, w, cx| this.operation("abort", w, cx)),
                                ),
                        ),
                )
                .into_any_element(),
        )
    }

    fn render_review_bar(&self, t: &AppTheme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let review = self.review.as_ref()?;
        let weak = cx.entity().downgrade();
        let branches = self.branches.clone();
        Some(
            h_flex()
                .flex_none()
                .h(px(28.))
                .gap_1p5()
                .px_2p5()
                .border_b_1()
                .border_color(t.border)
                .text_size(px(11.))
                .text_color(t.muted_foreground)
                .child(
                    Icon::new(Lucide::GitCompareArrows)
                        .xsmall()
                        .text_color(t.subtle),
                )
                .child("Review against")
                .child(
                    Button::new("review-base")
                        .ghost()
                        .xsmall()
                        .label(review.label.clone())
                        .dropdown_menu(move |menu, _, _| {
                            branches.iter().fold(menu, |menu, b| {
                                let (w, b) = (weak.clone(), b.clone());
                                let label = b
                                    .trim_start_matches("refs/heads/")
                                    .trim_start_matches("refs/remotes/")
                                    .to_string();
                                menu.item(PopupMenuItem::new(label).on_click(move |_, _, cx| {
                                    w.update(cx, |this, cx| this.start_review(b.clone(), cx))
                                        .ok();
                                }))
                            })
                        }),
                )
                .child({
                    let (base, label) = (review.reference.clone(), review.label.clone());
                    Button::new("guide-branch")
                        .ghost()
                        .xsmall()
                        .icon(Lucide::Sparkles)
                        .tooltip(format!(
                            "Guided review of the branch's commits since {label}"
                        ))
                        .on_click(move |_, window, cx| {
                            let of = GuideOf::Branch {
                                base: base.clone(),
                                label: label.clone(),
                            };
                            gitviber_review::open_guide(of, window, cx)
                        })
                })
                .child(div().flex_1())
                .child(
                    Button::new("review-close")
                        .ghost()
                        .xsmall()
                        .icon(Lucide::X)
                        .tooltip("Back to uncommitted changes")
                        .on_click(cx.listener(|this, _, _, cx| this.end_review(cx))),
                )
                .into_any_element(),
        )
    }

    /// List or tree, by name or newest first.
    fn view_menu(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let view = self.view;
        let weak = cx.entity().downgrade();
        Button::new("view-menu")
            .ghost()
            .xsmall()
            .icon(if view.tree {
                Lucide::ListTree
            } else {
                Lucide::List
            })
            .tooltip("View and sort")
            .dropdown_menu(move |menu, _, _| {
                let set = |w: WeakEntity<ChangesPanel>, f: fn(&mut ChangesView)| {
                    move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                        w.update(cx, |this, cx| {
                            let mut next = this.view;
                            f(&mut next);
                            this.set_view(next, cx);
                        })
                        .ok();
                    }
                };
                menu.label("View")
                    .item(
                        PopupMenuItem::new("List")
                            .checked(!view.tree)
                            .on_click(set(weak.clone(), |v| v.tree = false)),
                    )
                    .item(
                        PopupMenuItem::new("Tree")
                            .checked(view.tree)
                            .on_click(set(weak.clone(), |v| v.tree = true)),
                    )
                    .separator()
                    .label("Sort by")
                    .item(
                        PopupMenuItem::new("Name")
                            .checked(!view.recent)
                            .on_click(set(weak.clone(), |v| v.recent = false)),
                    )
                    .item(
                        PopupMenuItem::new("Recently Modified")
                            .checked(view.recent)
                            .on_click(set(weak.clone(), |v| v.recent = true)),
                    )
            })
    }

    fn render_summary(&self, t: &AppTheme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let ws = self.workspace()?;
        let (sels, list): (Vec<Selection>, Vec<Arc<FileChange>>) = match &self.review {
            Some(r) => (
                r.files
                    .iter()
                    .filter_map(|f| self.review_selection(f))
                    .collect(),
                r.files.clone(),
            ),
            None => {
                let changes = self.lists.changes();
                (
                    changes
                        .iter()
                        .map(|c| selection_of(c.kind, c.file.clone()))
                        .collect(),
                    changes.into_iter().map(|c| c.file).collect(),
                )
            }
        };
        let n = sels.len();
        if n == 0 {
            return None;
        }
        let (add, del) = sum_lines(list.iter().map(|f| f.as_ref()));
        let reviewed = sels.iter().filter(|s| is_viewed(ws.read(cx), s)).count();
        let frac = reviewed as f32 / n as f32;
        // It orders the uncommitted lists; a review's stays as git gives it.
        let view_menu = self.review.is_none().then(|| self.view_menu(cx));
        Some(
            v_flex()
                .flex_none()
                .gap_1()
                .px_3()
                .pt_2()
                .pb_1p5()
                .text_size(px(12.))
                .child(
                    h_flex()
                        .gap_2()
                        .child(div().child(if n == 1 {
                            "1 file".to_string()
                        } else {
                            format!("{n} files")
                        }))
                        .child(LineCounts::new(Some(add), Some(del)))
                        .when(self.review.is_none(), |row| {
                            row.child(
                                Button::new("guide-uncommitted")
                                    .ghost()
                                    .xsmall()
                                    .icon(Lucide::Sparkles)
                                    .tooltip("Guided review of the uncommitted changes")
                                    .on_click(|_, window, cx| {
                                        window
                                            .dispatch_action(Box::new(review::GuideUncommitted), cx)
                                    }),
                            )
                        })
                        .children(view_menu)
                        .child(div().flex_1())
                        .child(
                            h_flex()
                                .child(
                                    div()
                                        .text_color(if reviewed == n {
                                            t.added
                                        } else {
                                            t.foreground
                                        })
                                        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                        .child(reviewed.to_string()),
                                )
                                .child(
                                    div()
                                        .text_color(t.muted_foreground)
                                        .child(format!("/{n} reviewed")),
                                ),
                        ),
                )
                .child(
                    div()
                        .h(px(3.))
                        .w_full()
                        .rounded_full()
                        .bg(t.elevated)
                        .child(
                            div()
                                .h_full()
                                .rounded_full()
                                .bg(t.added_fill)
                                .w(gpui_kit::relative(frac)),
                        ),
                )
                .into_any_element(),
        )
    }
}

impl Render for ChangesPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(facts) = self.facts.take() {
            self.commit
                .update(cx, |b, cx| b.set_facts(facts, window, cx));
        }
        let t = cx.app_theme().clone();
        let filter = self.filter.as_ref().map(|f| {
            h_flex()
                .id("changes-filter")
                .flex_none()
                .px_2p5()
                .border_b_1()
                .border_color(t.border)
                // Esc clears, then closes; ↓ or ↵ go on to the rows it leaves.
                .capture_key_down(cx.listener(|this, e: &KeyDownEvent, window, cx| {
                    let Some(f) = &this.filter else { return };
                    match e.keystroke.key.as_str() {
                        "escape" if f.input.read(cx).value().is_empty() => {
                            this.close_filter(window, cx)
                        }
                        "escape" => {
                            let input = f.input.clone();
                            // set_value says nothing of the change.
                            input.update(cx, |s, cx| s.set_value("", window, cx));
                            this.rebuild(cx);
                            this.commit_facts(cx);
                        }
                        "down" | "enter" => this.focus.focus(window, cx),
                        _ => return,
                    }
                    cx.stop_propagation();
                }))
                // The field's own padding spaces it from the icon.
                .child(Icon::new(Lucide::Search).small().text_color(t.subtle))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(Input::new(&f.input).small().appearance(false)),
                )
                .child(
                    Button::new("close-filter")
                        .ghost()
                        .xsmall()
                        .icon(Lucide::X)
                        .tooltip("Close filter")
                        .on_click(cx.listener(|this, _, window, cx| this.close_filter(window, cx))),
                )
        });
        let count = self.rows.len();
        let list = uniform_list(
            "changes-rows",
            count,
            cx.processor(|this, range, window, cx| this.render_rows(range, window, cx)),
        )
        .track_scroll(&self.scroll)
        .flex_1()
        .min_h_0();
        let banner = self.render_banner(&t, cx);
        let review_bar = self.render_review_bar(&t, cx);
        let summary = self.render_summary(&t, cx);
        let split = self
            .operation
            .as_ref()
            .is_some_and(|o| o.split || o.kind == "bisect");
        let blocked_by_op = self.operation.is_some() && !split;
        let _ = window;
        v_flex()
            .id("changes-panel")
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .size_full()
            .bg(t.chrome(t.panel))
            .text_color(t.foreground)
            .children(filter)
            .children(banner)
            .children(self.bisect.clone())
            .children(review_bar)
            .children(summary)
            .child(match (&self.error, self.loaded) {
                (Some(e), false) => div()
                    .flex_1()
                    .p_4()
                    .text_color(t.muted_foreground)
                    .text_size(px(12.))
                    .child(format!("Could not read the repository: {e}"))
                    .into_any_element(),
                (_, false) => div().flex_1().into_any_element(),
                _ => list.into_any_element(),
            })
            .child(self.notes.clone())
            .child(if blocked_by_op {
                // Committing by hand mid-rebase would splice an extra commit into the history.
                div()
                    .flex_none()
                    .border_t_1()
                    .border_color(t.border)
                    .bg(t.panel)
                    .px_3()
                    .py_2p5()
                    .text_size(px(11.5))
                    .text_color(t.muted_foreground)
                    .child(format!(
                        "A {} is in progress. Resolve the conflicts, then use Continue above.",
                        self.operation
                            .as_ref()
                            .map(|o| o.kind.as_str())
                            .unwrap_or("merge")
                    ))
                    .into_any_element()
            } else {
                self.commit.clone().into_any_element()
            })
            .on_action(cx.listener(|this, _: &Up, window, cx| {
                this.nav_move(Move::By(-1), false, window, cx)
            }))
            .on_action(cx.listener(|this, _: &Down, window, cx| {
                this.nav_move(Move::By(1), false, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ExtendUp, window, cx| {
                this.nav_move(Move::By(-1), true, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ExtendDown, window, cx| {
                this.nav_move(Move::By(1), true, window, cx)
            }))
            // The rest only on the list itself: the filter and the commit box have their own.
            .on_action(cx.listener(|this, _: &Home, window, cx| {
                this.list_key(window, cx, |this, window, cx| {
                    this.nav_move(Move::Home, false, window, cx);
                    true
                })
            }))
            .on_action(cx.listener(|this, _: &End, window, cx| {
                this.list_key(window, cx, |this, window, cx| {
                    this.nav_move(Move::End, false, window, cx);
                    true
                })
            }))
            .on_action(cx.listener(|this, _: &PageUp, window, cx| {
                this.list_key(window, cx, |this, window, cx| {
                    this.nav_move(Move::PageUp, false, window, cx);
                    true
                })
            }))
            .on_action(cx.listener(|this, _: &PageDown, window, cx| {
                this.list_key(window, cx, |this, window, cx| {
                    this.nav_move(Move::PageDown, false, window, cx);
                    true
                })
            }))
            .on_action(cx.listener(|this, _: &Left, window, cx| {
                this.list_key(window, cx, |this, window, cx| {
                    this.row_key(RowKey::Left, window, cx)
                })
            }))
            .on_action(cx.listener(|this, _: &Right, window, cx| {
                this.list_key(window, cx, |this, window, cx| {
                    this.row_key(RowKey::Right, window, cx)
                })
            }))
            .on_action(cx.listener(|this, _: &Open, window, cx| {
                this.list_key(window, cx, |this, window, cx| {
                    this.row_key(RowKey::Space, window, cx)
                })
            }))
            .on_action(cx.listener(|this, _: &Keep, window, cx| {
                this.list_key(window, cx, |this, window, cx| {
                    this.row_key(RowKey::Enter, window, cx)
                })
            }))
            .on_action(cx.listener(|this, _: &Menu, window, cx| {
                this.list_key(window, cx, |this, window, cx| {
                    this.open_row_menu(window, cx);
                    true
                })
            }))
            .on_action(cx.listener(|this, _: &editor::Find, window, cx| {
                if this.review.is_none() {
                    this.open_filter(window, cx);
                } else {
                    cx.propagate();
                }
            }))
            .on_action(cx.listener(|this, _: &Escape, _, cx| {
                if this.picked.take().is_some() {
                    cx.notify();
                } else {
                    cx.propagate();
                }
            }))
            .on_action(
                cx.listener(|this, _: &review::NextFile, window, cx| this.step(1, window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &review::PrevFile, window, cx| this.step(-1, window, cx)),
            )
            .on_action(cx.listener(|this, _: &review::ToggleViewed, _, cx| {
                let Some(key) = this.open_key.clone() else {
                    return;
                };
                let Some(sel) = this.selection_at(&key) else {
                    return;
                };
                let Some(ws) = this.workspace() else { return };
                let on = !is_viewed(ws.read(cx), &sel);
                let sels = match &sel {
                    Selection::Unstaged { .. } => this
                        .targets(Kind::Unstaged, &key)
                        .into_iter()
                        .map(|f| selection_of(Kind::Unstaged, f))
                        .collect(),
                    _ => vec![sel],
                };
                this.mark_viewed(sels, on, cx);
            }))
            .on_action(cx.listener(|this, _: &git::ToggleStage, _, cx| {
                let Some(key) = this.open_key.clone() else {
                    return;
                };
                match key.split_once("::").map(|x| x.0) {
                    Some("unstaged") => {
                        let rows = this.targets(Kind::Unstaged, &key);
                        this.stage(rows, cx);
                    }
                    Some("staged") => {
                        let rows = this.targets(Kind::Staged, &key);
                        this.unstage_files(rows, cx);
                    }
                    _ => {}
                }
            }))
            .on_action(cx.listener(|this, _: &git::Discard, window, cx| {
                let Some(key) = this
                    .open_key
                    .clone()
                    .filter(|k| k.starts_with("unstaged::"))
                else {
                    return;
                };
                let rows = this.targets(Kind::Unstaged, &key);
                this.discard(rows, window, cx);
            }))
            .on_action(cx.listener(|this, _: &git::SelectAllChanges, _, cx| {
                let keys = this.file_keys();
                if let Some(first) = keys.first().cloned() {
                    this.picked = Some((keys, this.open_key.clone().unwrap_or(first)));
                    cx.notify();
                }
            }))
    }
}

/// Every folder around the open file opens in the tree.
pub fn reveal_folders(closed: &mut HashSet<String>, kind: Kind, path: &str) {
    for f in folders_of(path) {
        closed.remove(&folder_key(kind, &f));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tabs::ChangeTab;
    use gitviber_ui::actions::changes as changes_actions;
    use gitviber_ui::workspace::{Workspace, selection_key};
    use gpui_kit::{TestAppContext, VisualTestContext};
    use std::path::{Path, PathBuf};
    use std::process::Command;

    fn git(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .expect("git runs");
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    /// A throwaway repo: a.txt and b.txt changed, c.txt new.
    fn repo(name: &str) -> PathBuf {
        let dir = gitviber_ui::test_support::temp_dir(&format!("changes-{name}"));
        git(&dir, &["init", "-q", "-b", "main"]);
        git(&dir, &["config", "user.name", "Test"]);
        git(&dir, &["config", "user.email", "test@example.com"]);
        for f in ["a.txt", "b.txt"] {
            std::fs::write(dir.join(f), "one\n").unwrap();
        }
        git(&dir, &["add", "."]);
        git(&dir, &["commit", "-q", "-m", "first"]);
        std::fs::write(dir.join("a.txt"), "one\ntwo\n").unwrap();
        std::fs::write(dir.join("b.txt"), "uno\n").unwrap();
        std::fs::write(dir.join("c.txt"), "new\n").unwrap();
        dir.canonicalize().unwrap()
    }

    struct Host {
        panel: Option<Entity<ChangesPanel>>,
        /// A tab's view, drawn beside the panel.
        center: Option<gpui_kit::AnyView>,
    }

    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .children(self.panel.clone())
                .children(self.center.clone())
        }
    }

    fn setup(cx: &mut TestAppContext) {
        gitviber_ui::test_support::config_dir("changes-config");
        cx.update(|cx| {
            gitviber_ui::init(cx);
            gitviber_ui::install_core(cx);
            crate::init(cx);
        });
    }

    /// A window with a workspace and the panel in it on `dir`, the status read. No Project: the
    /// test reads the repo again itself after each action.
    fn open<'a>(
        cx: &'a mut TestAppContext,
        dir: &Path,
    ) -> (Entity<Workspace>, Entity<Host>, &'a mut VisualTestContext) {
        let core = cx.update(|cx| crate::core(cx));
        gitviber_core::api::repo::open_repo(&core, dir.to_string_lossy().into_owned()).unwrap();
        let ws = cx.update(|cx| cx.new(Workspace::new));
        let (host, cx) = cx.add_window_view(|_, _| Host {
            panel: None,
            center: None,
        });
        cx.update(|window, cx| Workspace::register_window(&ws, window, cx));
        cx.update(|window, cx| {
            let ctx = ws.update(cx, |ws, cx| ws.context(cx));
            let panel = cx.new(|cx| ChangesPanel::new(&ctx, window, cx));
            host.update(cx, |h, cx| {
                h.panel = Some(panel);
                cx.notify();
            });
        });
        cx.run_until_parked();
        (ws, host, cx)
    }

    fn panel(host: &Entity<Host>, cx: &mut VisualTestContext) -> Entity<ChangesPanel> {
        host.read_with(cx, |h, _| h.panel.clone().unwrap())
    }

    fn paths(list: &[Arc<FileChange>]) -> Vec<String> {
        list.iter().map(|f| f.path.clone()).collect()
    }

    #[gpui_kit::test]
    fn stages_commits_stashes_and_discards(cx: &mut TestAppContext) {
        let dir = repo("flow");
        setup(cx);
        let (_ws, host, cx) = open(cx, &dir);
        let p = panel(&host, cx);
        assert_eq!(
            p.read_with(cx, |p, _| paths(&p.lists.unstaged)),
            ["a.txt", "b.txt", "c.txt"]
        );

        // Stage a.txt; it moves to Staged.
        p.update(cx, |p, cx| {
            let a = p.lists.unstaged[0].clone();
            p.stage(vec![a], cx);
        });
        cx.run_until_parked();
        p.update(cx, |p, cx| p.read(cx));
        cx.run_until_parked();
        assert_eq!(p.read_with(cx, |p, _| paths(&p.lists.staged)), ["a.txt"]);

        // Commit it: the box commits what's staged.
        let commit = p.read_with(cx, |p, _| p.commit.clone());
        cx.update(|window, cx| {
            commit.update(cx, |b, cx| {
                b.write_message("Add two", "", window, cx);
                b.commit(None, window, cx);
            })
        });
        cx.run_until_parked();
        p.update(cx, |p, cx| p.read(cx));
        cx.run_until_parked();
        assert_eq!(git(&dir, &["log", "-1", "--format=%s"]).trim(), "Add two");
        assert_eq!(p.read_with(cx, |p, _| p.lists.staged.len()), 0);

        // Stash b.txt alone.
        p.update(cx, |p, cx| {
            p.run(
                "Stash failed",
                None,
                |core| {
                    api::stash::stash_push(
                        core,
                        "b only".into(),
                        false,
                        Some(false),
                        Some(vec!["b.txt".into()]),
                    )
                },
                cx,
            )
        });
        cx.run_until_parked();
        assert!(git(&dir, &["stash", "list"]).contains("b only"));
        assert_eq!(std::fs::read_to_string(dir.join("b.txt")).unwrap(), "one\n");

        // Discard what's left of the tracked changes: none; the untracked file goes to the Trash
        // (one file, of this throwaway repo).
        p.update(cx, |p, cx| {
            p.discard_now(vec![], vec!["c.txt".into()], None, None, cx)
        });
        cx.run_until_parked();
        p.update(cx, |p, cx| p.read(cx));
        cx.run_until_parked();
        assert!(!dir.join("c.txt").exists());
        assert_eq!(p.read_with(cx, |p, _| p.lists.total()), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[gpui_kit::test]
    fn cmd_enter_commits_the_message_as_written(cx: &mut TestAppContext) {
        let dir = repo("cmd-enter");
        setup(cx);
        let (_ws, host, cx) = open(cx, &dir);
        let commit = panel(&host, cx).read_with(cx, |p, _| p.commit.clone());
        let write = |summary: &'static str, cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                commit.update(cx, |b, cx| {
                    b.write_message(summary, "First\nSecond", window, cx);
                    b.put_cursor_in_body(0, 3, window, cx);
                })
            });
            cx.simulate_keystrokes("cmd-enter");
            cx.run_until_parked();
        };
        // No summary: nothing commits, and the description is as it was, no newline at the cursor.
        write("", cx);
        assert_eq!(
            commit.read_with(cx, |b, cx| b.body_text(cx)),
            "First\nSecond"
        );
        assert_eq!(git(&dir, &["rev-list", "--count", "HEAD"]).trim(), "1");
        write("Add things", cx);
        assert_eq!(
            git(&dir, &["log", "-1", "--format=%B"]).trim_end(),
            "Add things\n\nFirst\nSecond"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[gpui_kit::test]
    fn closing_the_panel_frees_it(cx: &mut TestAppContext) {
        let dir = repo("leak");
        setup(cx);
        let (ws, host, cx) = open(cx, &dir);
        cx.update(|_, cx| {
            host.update(cx, |h, cx| {
                h.panel = None;
                cx.notify();
            })
        });
        cx.run_until_parked();
        let snapshot = cx.update(|_, cx| cx.leak_detector_snapshot());
        for _ in 0..5 {
            cx.update(|window, cx| {
                let ctx = ws.update(cx, |ws, cx| ws.context(cx));
                let panel = cx.new(|cx| ChangesPanel::new(&ctx, window, cx));
                host.update(cx, |h, cx| {
                    h.panel = Some(panel);
                    cx.notify();
                });
            });
            cx.run_until_parked();
            cx.update(|_, cx| {
                host.update(cx, |h, cx| {
                    h.panel = None;
                    cx.notify();
                })
            });
            cx.run_until_parked();
        }
        // The last frame's listeners hold the panel's inputs until the next frame, and a test
        // flush draws after it frees, so two more frames let the last panel's inputs go.
        cx.executor()
            .advance_clock(std::time::Duration::from_secs(5));
        for _ in 0..3 {
            cx.update(|window, _| window.refresh());
            cx.run_until_parked();
        }
        cx.update(|_, cx| cx.assert_no_new_leaks(&snapshot));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A restart: what's in memory goes, the files on disk stay.
    fn restart(cx: &mut VisualTestContext) {
        cx.update(|_, cx| {
            gitviber_ui::store::flush(cx);
            if cx.has_global::<store::Saved>() {
                cx.remove_global::<store::Saved>();
            }
            if cx.has_global::<store::Edits>() {
                cx.remove_global::<store::Edits>();
            }
        });
    }

    #[gpui_kit::test]
    fn the_view_sections_and_draft_outlive_the_app(cx: &mut TestAppContext) {
        let dir = repo("keep");
        setup(cx);
        let (ws, host, cx) = open(cx, &dir);
        let root = dir.to_string_lossy().into_owned();
        let p = panel(&host, cx);
        cx.update(|window, cx| {
            p.update(cx, |p, cx| {
                p.set_view(
                    ChangesView {
                        tree: true,
                        recent: false,
                    },
                    cx,
                );
                p.toggle_section(Section::Stashes, cx);
                p.commit
                    .clone()
                    .update(cx, |b, cx| b.write_message("Half done", "why", window, cx));
            })
        });
        cx.run_until_parked();
        restart(cx);
        assert!(
            gitviber_ui::store::config_dir()
                .join("changes.json")
                .exists()
        );
        cx.update(|window, cx| {
            let ctx = ws.update(cx, |ws, cx| ws.context(cx));
            let next = cx.new(|cx| ChangesPanel::new(&ctx, window, cx));
            host.update(cx, |h, cx| {
                h.panel = Some(next);
                cx.notify();
            });
        });
        cx.run_until_parked();
        let p = panel(&host, cx);
        cx.update(|_, cx| {
            let p = p.read(cx);
            assert!(p.view.tree && !p.view.recent);
            assert!(p.collapsed.contains(&Section::Stashes) && p.collapsed.len() == 1);
            let b = p.commit.read(cx);
            assert_eq!(
                (b.summary_text(cx), b.body_text(cx)),
                ("Half done".into(), "why".into())
            );
        });
        // Back to how it started: nothing is kept for the worktree.
        cx.update(|window, cx| {
            p.read(cx)
                .commit
                .clone()
                .update(cx, |b, cx| b.write_message("", "", window, cx))
        });
        assert_eq!(cx.update(|_, cx| store::draft(&root, cx)), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[gpui_kit::test]
    fn typing_in_a_diff_comes_back_after_a_restart(cx: &mut TestAppContext) {
        let dir = repo("edit");
        setup(cx);
        let (ws, host, cx) = open(cx, &dir);
        let root = dir.to_string_lossy().into_owned();
        cx.update(|window, cx| ws.update(cx, |ws, cx| ws.open_project(dir.clone(), window, cx)));
        let p = panel(&host, cx);
        let sel = cx.update(|_, cx| {
            p.read(cx)
                .walk_list(&Selection::File {
                    path: String::new(),
                })
                .into_iter()
                .find(|s| matches!(s, Selection::Unstaged { file } if file.path == "a.txt"))
                .expect("a.txt is changed")
        });
        // The tab of the file's diff, drawn.
        let open_tab = |cx: &mut VisualTestContext| {
            let tab = cx.update(|window, cx| {
                ws.update(cx, |ws, cx| {
                    ws.open(sel.clone(), false, window, cx);
                    ws.active_view(window, cx)
                })
            });
            let tab = tab
                .and_then(|v| v.downcast::<ChangeTab>().ok())
                .expect("a diff tab");
            cx.update(|_, cx| {
                host.update(cx, |h, cx| {
                    h.center = Some(tab.clone().into());
                    cx.notify();
                })
            });
            cx.run_until_parked();
            cx.update(|window, _| window.refresh());
            cx.run_until_parked();
            cx.update(|_, cx| tab.read(cx).diff().cloned().expect("a diff"))
        };
        let settle = |cx: &mut VisualTestContext| {
            cx.executor()
                .advance_clock(std::time::Duration::from_secs(1));
            cx.run_until_parked();
        };
        let diff = open_tab(cx);
        cx.update(|window, cx| {
            diff.update(cx, |v, cx| {
                v.toggle_edit(window, cx);
                v.set_edit_text("mine\n", window, cx);
            })
        });
        settle(cx);
        let kept = store::Edit {
            text: "mine\n".into(),
            base: "one\ntwo\n".into(),
        };
        assert_eq!(
            cx.update(|_, cx| store::edit(&root, "a.txt", cx)),
            Some(kept.clone())
        );
        // The tab closes and the app restarts: the typing comes back in the editor.
        cx.update(|_, cx| {
            ws.update(cx, |ws, cx| {
                ws.force_close_tabs(&[selection_key(&sel).into()], cx)
            });
            host.update(cx, |h, _| h.center = None);
        });
        cx.run_until_parked();
        restart(cx);
        assert!(gitviber_ui::store::config_dir().join("edits.json").exists());
        let diff = open_tab(cx);
        cx.update(|_, cx| {
            let v = diff.read(cx);
            assert!(v.is_editing());
            assert_eq!(
                v.unsaved_edit(cx),
                Some((kept.text.clone(), kept.base.clone()))
            );
        });
        // Saved: nothing is left to keep.
        cx.update(|window, cx| diff.update(cx, |v, cx| v.save(window, cx)));
        settle(cx);
        assert_eq!(
            std::fs::read_to_string(dir.join("a.txt")).unwrap(),
            "mine\n"
        );
        assert_eq!(cx.update(|_, cx| store::edit(&root, "a.txt", cx)), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A throwaway repo mid-merge, a.txt conflicted in git's default style: `ours` and `theirs`
    /// are each side's file.
    fn conflicted(name: &str, ours: &str, theirs: &str) -> PathBuf {
        conflicted_at(
            name,
            "a.txt",
            "a\nb\nc\nd\ne\nf\ng\nh\ni\nj\n",
            ours,
            theirs,
        )
    }

    /// `path` conflicted, from `base`.
    fn conflicted_at(name: &str, path: &str, base: &str, ours: &str, theirs: &str) -> PathBuf {
        let dir = gitviber_ui::test_support::temp_dir(&format!("changes-{name}"));
        git(&dir, &["init", "-q", "-b", "main"]);
        git(&dir, &["config", "user.name", "Test"]);
        git(&dir, &["config", "user.email", "test@example.com"]);
        std::fs::write(dir.join(path), base).unwrap();
        git(&dir, &["add", "."]);
        git(&dir, &["commit", "-q", "-m", "base"]);
        git(&dir, &["checkout", "-q", "-b", "other"]);
        std::fs::write(dir.join(path), theirs).unwrap();
        git(&dir, &["commit", "-q", "-am", "theirs"]);
        git(&dir, &["checkout", "-q", "main"]);
        std::fs::write(dir.join(path), ours).unwrap();
        git(&dir, &["commit", "-q", "-am", "ours"]);
        // It conflicts: git says so with a failure.
        let _ = Command::new("git")
            .args(["-c", "merge.conflictStyle=merge", "merge", "-q", "other"])
            .current_dir(&dir)
            .output();
        dir.canonicalize().unwrap()
    }

    fn conflict_view(
        host: &Entity<Host>,
        code: &str,
        cx: &mut VisualTestContext,
    ) -> Entity<crate::conflict::ConflictView> {
        conflict_view_at(host, "a.txt", code, cx)
    }

    fn conflict_view_at(
        host: &Entity<Host>,
        path: &str,
        code: &str,
        cx: &mut VisualTestContext,
    ) -> Entity<crate::conflict::ConflictView> {
        let view = cx.update(|window, cx| {
            let view = cx
                .new(|cx| crate::conflict::ConflictView::new(path.into(), code.into(), window, cx));
            host.update(cx, |h, cx| {
                h.center = Some(view.clone().into());
                cx.notify();
            });
            view
        });
        cx.run_until_parked();
        view
    }

    const OURS: &str = "a\nB1\nc\nd\ne\nf\ng\nh\nI1\nj\n";
    const THEIRS: &str = "a\nB2\nc\nd\ne\nf\ng\nh\nI2\nj\n";

    #[gpui_kit::test]
    fn the_conflict_view_shows_each_blocks_base_and_hands_the_prompt_over(cx: &mut TestAppContext) {
        let dir = conflicted("conflict", OURS, THEIRS);
        setup(cx);
        let (ws, host, cx) = open(cx, &dir);
        let view = conflict_view(&host, "UU", cx);
        cx.update(|_, cx| {
            assert_eq!(
                view.read(cx).bases_for_test(),
                vec![Some(vec!["b".to_string()]), Some(vec!["i".to_string()])],
                "the bases come from the index stages"
            );
        });
        // No terminal runs an agent here: the toast offers the prompt to copy.
        cx.update(|window, cx| view.update(cx, |v, cx| v.ask_agent_for_test(window, cx)));
        cx.run_until_parked();
        let toast = cx
            .update(|_, cx| ws.update(cx, |ws, _| ws.take_toasts()))
            .pop()
            .expect("a toast");
        assert_eq!(
            toast.title.as_deref(),
            Some("No agent running in this worktree")
        );
        let action = toast.action.expect("Copy prompt");
        assert_eq!(action.label.as_ref(), "Copy prompt");
        cx.update(|window, cx| (action.run)(window, cx));
        let copied = cx.update(|_, cx| cx.read_from_clipboard().and_then(|c| c.text()));
        assert_eq!(
            copied.as_deref(),
            Some(
                "Resolve the merge conflicts in a.txt (merging other into main); keep both sides' intent, then stage the files."
            )
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[gpui_kit::test]
    fn choices_and_an_edit_by_hand_resolve_the_file(cx: &mut TestAppContext) {
        use crate::conflict::TestAct;
        use crate::model::conflicts::Pick;
        let dir = conflicted("resolve", OURS, THEIRS);
        setup(cx);
        let (_ws, host, cx) = open(cx, &dir);
        let view = conflict_view(&host, "UU", cx);
        cx.update(|window, cx| {
            view.update(cx, |v, cx| {
                v.act_for_test(TestAct::All(Pick::Theirs), window, cx);
                v.act_for_test(TestAct::Choose(0, Pick::Both), window, cx);
                v.act_for_test(TestAct::Edit(1, "I by hand".into()), window, cx);
                v.act_for_test(TestAct::Save, window, cx);
            })
        });
        cx.run_until_parked();
        assert_eq!(
            std::fs::read_to_string(dir.join("a.txt")).unwrap(),
            "a\nB1\nB2\nc\nd\ne\nf\ng\nh\nI by hand\nj\n"
        );
        let status = git(&dir, &["status", "--porcelain"]);
        assert_eq!(status.trim(), "M  a.txt", "staged as resolved");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[gpui_kit::test]
    fn a_save_over_an_outside_edit_starts_the_choices_over(cx: &mut TestAppContext) {
        use crate::conflict::TestAct;
        use crate::model::conflicts::Pick;
        let dir = conflicted("outside", OURS, THEIRS);
        setup(cx);
        let (ws, host, cx) = open(cx, &dir);
        let view = conflict_view(&host, "UU", cx);
        // An agent fixes the file meanwhile.
        std::fs::write(dir.join("a.txt"), "agent\n").unwrap();
        cx.update(|window, cx| {
            view.update(cx, |v, cx| {
                v.act_for_test(TestAct::All(Pick::Ours), window, cx);
                v.act_for_test(TestAct::Save, window, cx);
            })
        });
        cx.run_until_parked();
        assert_eq!(
            std::fs::read_to_string(dir.join("a.txt")).unwrap(),
            "agent\n"
        );
        let toast = cx
            .update(|_, cx| ws.update(cx, |ws, _| ws.take_toasts()))
            .pop()
            .expect("a toast");
        assert_eq!(toast.title.as_deref(), Some("File changed on disk"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[gpui_kit::test]
    fn a_deleted_side_keeps_or_deletes_the_whole_file(cx: &mut TestAppContext) {
        use crate::conflict::TestAct;
        // Theirs deletes the file, ours changes it: UD.
        let dir = gitviber_ui::test_support::temp_dir("changes-ud");
        git(&dir, &["init", "-q", "-b", "main"]);
        git(&dir, &["config", "user.name", "Test"]);
        git(&dir, &["config", "user.email", "test@example.com"]);
        std::fs::write(dir.join("a.txt"), "one\n").unwrap();
        git(&dir, &["add", "."]);
        git(&dir, &["commit", "-q", "-m", "base"]);
        git(&dir, &["checkout", "-q", "-b", "other"]);
        git(&dir, &["rm", "-q", "a.txt"]);
        git(&dir, &["commit", "-q", "-m", "gone"]);
        git(&dir, &["checkout", "-q", "main"]);
        std::fs::write(dir.join("a.txt"), "two\n").unwrap();
        git(&dir, &["commit", "-q", "-am", "ours"]);
        let _ = Command::new("git")
            .args(["merge", "-q", "other"])
            .current_dir(&dir)
            .output();
        let dir = dir.canonicalize().unwrap();
        setup(cx);
        let (_ws, host, cx) = open(cx, &dir);
        let view = conflict_view(&host, "UD", cx);
        let whole = cx
            .update(|_, cx| view.read(cx).whole_file_for_test())
            .expect("no blocks");
        assert_eq!(whole.sides[1].0, "Delete the file");
        assert!(!whole.as_is);
        cx.update(|window, cx| {
            view.update(cx, |v, cx| {
                v.act_for_test(TestAct::Whole(Some(GitSide::Theirs)), window, cx)
            })
        });
        cx.run_until_parked();
        assert!(
            !dir.join("a.txt").exists(),
            "taking the incoming side deletes it"
        );
        assert_eq!(git(&dir, &["status", "--porcelain"]).trim(), "D  a.txt");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[gpui_kit::test]
    fn focus_message_works_from_outside_the_panel(cx: &mut TestAppContext) {
        let dir = repo("focus");
        setup(cx);
        let (ws, host, cx) = open(cx, &dir);
        cx.update(|window, cx| ws.update(cx, |ws, cx| ws.open_project(dir.clone(), window, cx)));
        // The workspace's own panel drawn, and the left side closed, focus nowhere in it.
        let panel = cx.update(|window, cx| {
            ws.update(cx, |ws, cx| {
                ws.set_open(gitviber_ui::workspace::Slot::Left, false, cx)
            });
            let panel = Workspace::ensure_panel(&ws, "changes", window, cx)
                .and_then(|p| p.view().downcast::<ChangesPanel>().ok())
                .expect("the panel is built");
            host.update(cx, |h, cx| {
                h.panel = Some(panel.clone());
                cx.notify();
            });
            window.blur(cx);
            window.activate_window();
            panel
        });
        cx.run_until_parked();
        cx.dispatch_action(changes_actions::FocusMessage);
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(ws.read(cx).is_open(gitviber_ui::workspace::Slot::Left));
            assert!(panel.read(cx).commit.read(cx).message_focused(window, cx));
        });
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[gpui_kit::test]
    fn open_all_and_review_branch_work_from_outside_the_panel(cx: &mut TestAppContext) {
        let dir = repo("global");
        setup(cx);
        let (ws, _host, cx) = open(cx, &dir);
        cx.update(|window, cx| {
            ws.update(cx, |ws, cx| ws.open_project(dir.clone(), window, cx));
            window.blur(cx);
            window.activate_window();
        });
        cx.run_until_parked();
        cx.dispatch_action(review::OpenAll);
        cx.run_until_parked();
        let active = ws.read_with(cx, |ws, _| {
            ws.tabs().active().and_then(|t| t.item.selection().cloned())
        });
        assert!(matches!(
            active,
            Some(Selection::Changes {
                of: ChangesOf::Unstaged
            })
        ));
        // No origin here: the review is against the local main.
        cx.dispatch_action(review::Branch);
        cx.run_until_parked();
        assert_eq!(
            ws.read_with(cx, |ws, _| ws.review_base().map(str::to_string)),
            Some("refs/heads/main".to_string())
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[gpui_kit::test]
    fn tabs_follow_their_files_through_git(cx: &mut TestAppContext) {
        let dir = repo("follow");
        setup(cx);
        let (ws, host, cx) = open(cx, &dir);
        let p = panel(&host, cx);
        let keys = |cx: &mut VisualTestContext| {
            ws.read_with(cx, |ws, _| {
                let keys = ws.tabs().tabs().iter().map(|t| t.key.to_string());
                (
                    keys.collect::<Vec<_>>(),
                    ws.tabs().active_key().map(|k| k.to_string()),
                )
            })
        };
        let reread = |cx: &mut VisualTestContext| {
            p.update(cx, |p, cx| p.read(cx));
            cx.run_until_parked();
        };
        // a.txt open in the preview tab: staging it opens b.txt in its place, before git is done.
        cx.update(|window, cx| {
            p.update(cx, |p, cx| {
                let a = p.lists.unstaged[0].clone();
                p.open(selection_of(Kind::Unstaged, a.clone()), true, window, cx);
                p.stage(vec![a], cx);
            })
        });
        cx.run_until_parked();
        reread(cx);
        let b = "unstaged::b.txt".to_string();
        assert_eq!(keys(cx), (vec![b.clone()], Some(b.clone())));
        // c.txt kept open beside it, then staged outside the panel: its tab follows it into
        // Staged, in its place, and the tab in front stays in front.
        cx.update(|window, cx| {
            p.update(cx, |p, cx| {
                let c = p.lists.unstaged.iter().find(|f| f.path == "c.txt").cloned();
                p.open(selection_of(Kind::Unstaged, c.unwrap()), false, window, cx);
            });
            ws.update(cx, |ws, cx| ws.activate_tab(&b, window, cx));
        });
        git(&dir, &["add", "c.txt"]);
        reread(cx);
        assert_eq!(
            keys(cx),
            (vec![b.clone(), "staged::c.txt".into()], Some(b.clone()))
        );
        // Moved, not closed: nothing to reopen.
        assert!(!ws.read_with(cx, |ws, _| ws.can_reopen_tab()));
        // Committed: its tab closes.
        git(&dir, &["commit", "-q", "-m", "a and c"]);
        reread(cx);
        assert_eq!(keys(cx), (vec![b.clone()], Some(b.clone())));
        // b.txt typed into, then staged and committed outside: its tab stays as it is, the
        // typing with it.
        let tab = cx
            .update(|window, cx| ws.update(cx, |ws, cx| ws.active_view(window, cx)))
            .and_then(|v| v.downcast::<ChangeTab>().ok())
            .expect("b.txt's diff tab");
        cx.update(|_, cx| {
            host.update(cx, |h, cx| {
                h.center = Some(tab.clone().into());
                cx.notify();
            })
        });
        cx.run_until_parked();
        let diff = cx.update(|_, cx| tab.read(cx).diff().cloned().expect("a diff"));
        cx.update(|window, cx| {
            diff.update(cx, |v, cx| {
                v.toggle_edit(window, cx);
                v.set_edit_text("mine\n", window, cx);
            })
        });
        cx.run_until_parked();
        git(&dir, &["add", "b.txt"]);
        reread(cx);
        assert_eq!(keys(cx), (vec![b.clone()], Some(b.clone())));
        git(&dir, &["commit", "-q", "-m", "b"]);
        reread(cx);
        assert_eq!(keys(cx), (vec![b.clone()], Some(b)));
        assert!(cx.update(|_, cx| diff.read(cx).has_unsaved_edits(cx)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// windowing.test.ts: of a long list only the rows on screen are drawn, wherever it's
    /// scrolled to.
    #[gpui_kit::test]
    fn a_long_list_draws_only_the_rows_on_screen(cx: &mut TestAppContext) {
        let dir = repo("long-list");
        for i in 0..3000 {
            std::fs::write(dir.join(format!("f{i:04}.txt")), "x\n").unwrap();
        }
        setup(cx);
        let (_ws, host, cx) = open(cx, &dir);
        let p = panel(&host, cx);
        let rows = p.read_with(cx, |p, _| p.rows.len());
        assert!(rows > 3000, "{rows} rows");
        let drawn = p.read_with(cx, |p, _| p.drawn.clone());
        assert!(
            drawn.start == 0 && (1..100).contains(&drawn.len()),
            "{drawn:?}"
        );
        p.update(cx, |p, cx| {
            p.scroll.scroll_to_item(2000, gpui_kit::ScrollStrategy::Top);
            cx.notify();
        });
        cx.run_until_parked();
        let drawn = p.read_with(cx, |p, _| p.drawn.clone());
        assert!(drawn.contains(&2000) && drawn.len() < 100, "{drawn:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// leaving.test.ts: the row now in the place of the open one that left passes over rows
    /// still on their way out; an action's rows stay out until a read begun after it's done,
    /// so a status read while it runs doesn't bring the open row back.
    #[gpui_kit::test]
    fn rows_on_their_way_out_stay_out_until_a_read_after_their_action(cx: &mut TestAppContext) {
        let dir = repo("leaving");
        setup(cx);
        let (_ws, host, cx) = open(cx, &dir);
        let p = panel(&host, cx);
        let row = |p: &ChangesPanel, path: &str| {
            p.lists
                .unstaged
                .iter()
                .find(|f| f.path == path)
                .cloned()
                .unwrap()
        };
        let leave = |path: &'static str, cx: &mut VisualTestContext| {
            let id = p.update(cx, |p, cx| p.leave(Kind::Unstaged, &[row(p, path)], cx));
            cx.run_until_parked();
            id
        };
        let open_key = |cx: &mut VisualTestContext| p.read_with(cx, |p, _| p.open_key.clone());
        let out = |key: &str, cx: &mut VisualTestContext| p.read_with(cx, |p, _| p.is_leaving(key));
        let reread = |cx: &mut VisualTestContext| {
            p.update(cx, |p, cx| p.read(cx));
            cx.run_until_parked();
        };
        // b.txt open, then on its way out (git still at it): c.txt opens in its place.
        cx.update(|window, cx| {
            p.update(cx, |p, cx| {
                let b = row(p, "b.txt");
                p.open(selection_of(Kind::Unstaged, b), true, window, cx)
            })
        });
        let b = leave("b.txt", cx);
        assert_eq!(open_key(cx).as_deref(), Some("unstaged::c.txt"));
        // A status lands while it runs: b.txt is still listed, and still on its way out.
        reread(cx);
        assert!(out("unstaged::b.txt", cx));
        // c.txt goes too: nothing after it, and b.txt is passed over for a.txt.
        let c = leave("c.txt", cx);
        assert_eq!(open_key(cx).as_deref(), Some("unstaged::a.txt"));
        // b's action done while a read was on its way: that read doesn't let it back, the next does.
        p.update(cx, |p, cx| {
            p.read(cx);
            p.settle_leave(b, false, cx);
        });
        cx.run_until_parked();
        assert!(out("unstaged::b.txt", cx));
        reread(cx);
        assert!(!out("unstaged::b.txt", cx));
        assert!(out("unstaged::c.txt", cx), "its action still runs");
        // c's fails: c.txt stays, and the tab goes back to it from the row opened in its place.
        p.update(cx, |p, cx| p.settle_leave(c, true, cx));
        cx.run_until_parked();
        assert!(!out("unstaged::c.txt", cx));
        assert_eq!(open_key(cx).as_deref(), Some("unstaged::c.txt"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[gpui_kit::test]
    fn a_bisect_shows_its_bar_in_the_banners_place(cx: &mut TestAppContext) {
        let dir = repo("bisect");
        let first = git(&dir, &["rev-parse", "HEAD"]).trim().to_string();
        // Commits of a file of their own: the changed files stay as they are at each checkout.
        for n in 1..=3 {
            std::fs::write(dir.join("d.txt"), format!("{n}\n")).unwrap();
            git(&dir, &["add", "d.txt"]);
            git(&dir, &["commit", "-q", "-m", &format!("d{n}")]);
        }
        git(&dir, &["bisect", "start", "HEAD", &first]);
        setup(cx);
        let (ws, host, cx) = open(cx, &dir);
        // The bar marks through the project's repo state: a panel made with the project open.
        let p = cx.update(|window, cx| {
            ws.update(cx, |ws, cx| ws.open_project(dir.clone(), window, cx));
            let ctx = ws.update(cx, |ws, cx| ws.context(cx));
            let panel = cx.new(|cx| ChangesPanel::new(&ctx, window, cx));
            host.update(cx, |h, cx| {
                h.panel = Some(panel.clone());
                cx.notify();
            });
            panel
        });
        cx.run_until_parked();
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
        let bar = p
            .read_with(cx, |p, _| p.bisect.clone())
            .expect("the bisect bar");
        // Stopped outside: the bar goes with its entity.
        let weak = bar.downgrade();
        drop(bar);
        git(&dir, &["bisect", "reset"]);
        p.update(cx, |p, cx| p.read(cx));
        cx.run_until_parked();
        assert!(p.read_with(cx, |p, _| p.bisect.is_none()));
        cx.update(|_, _| {});
        cx.run_until_parked();
        assert!(weak.upgrade().is_none(), "the bar is released");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[gpui_kit::test]
    fn a_stashed_untracked_file_is_taken_by_the_stash(cx: &mut TestAppContext) {
        let dir = repo("stash-file");
        setup(cx);
        let (_ws, host, cx) = open(cx, &dir);
        let p = panel(&host, cx);
        git(&dir, &["stash", "push", "-q", "-u", "-m", "all"]);
        assert!(!dir.join("c.txt").exists());
        p.update(cx, |p, cx| p.read(cx));
        cx.run_until_parked();
        let sha = p.read_with(cx, |p, _| p.stashes[0].sha.clone());
        p.update(cx, |p, cx| p.toggle_stash(sha.clone(), cx));
        cx.run_until_parked();
        // The row's file is in the untracked files' own commit; git takes it by the stash's.
        let (commit, untracked) = p.read_with(cx, |p, _| {
            let (_, files) = p.stash_files.as_ref().unwrap();
            let (commit, _, untracked) = files.iter().find(|f| f.1.path == "c.txt").unwrap();
            (commit.clone(), *untracked)
        });
        assert!(untracked && commit != sha);
        p.update(cx, |p, cx| p.take_from_stash(sha, "c.txt".into(), true, cx));
        cx.run_until_parked();
        assert_eq!(std::fs::read_to_string(dir.join("c.txt")).unwrap(), "new\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[gpui_kit::test]
    fn a_pasted_message_splits_into_summary_and_description(cx: &mut TestAppContext) {
        let dir = repo("paste");
        setup(cx);
        let (_ws, host, cx) = open(cx, &dir);
        let commit = panel(&host, cx).read_with(cx, |p, _| p.commit.clone());
        cx.update(|window, cx| commit.update(cx, |b, cx| b.focus_message(window, cx)));
        let paste = |text: &str, cx: &mut VisualTestContext| {
            let item = gpui_kit::ClipboardItem::new_string(text.to_string());
            cx.update(|_, cx| cx.write_to_clipboard(item));
            cx.simulate_keystrokes("secondary-v");
            cx.run_until_parked();
            commit.read_with(cx, |b, cx| (b.summary_text(cx), b.body_text(cx)))
        };
        assert_eq!(
            paste("Fix the parser\r\n\r\nIt dropped the last line.\n", cx),
            ("Fix the parser".into(), "It dropped the last line.".into())
        );
        // One line goes in at the caret, as the field pastes it.
        assert_eq!(paste("!", cx).0, "Fix the parser!");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The filter row is there only after Find (⌘F) in the panel; Esc clears it, then closes it.
    #[gpui_kit::test]
    fn find_opens_the_filter_and_the_commit_button_counts_what_it_hides(cx: &mut TestAppContext) {
        let dir = repo("hidden");
        setup(cx);
        let (_ws, host, cx) = open(cx, &dir);
        let p = panel(&host, cx);
        let commit = p.read_with(cx, |p, _| p.commit.clone());
        let hidden = |cx: &mut VisualTestContext| commit.read_with(cx, |b, _| b.hidden());
        assert!(p.read_with(cx, |p, _| p.filter.is_none()));
        cx.update(|window, cx| {
            let focus = p.read(cx).focus.clone();
            focus.focus(window, cx)
        });
        // ⌘F, which the app's root binds.
        cx.dispatch_action(editor::Find);
        cx.simulate_input("a.txt");
        cx.run_until_parked();
        // Commit all takes a.txt, b.txt and c.txt; only a.txt is shown.
        assert_eq!(hidden(cx), 2);
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        assert_eq!(hidden(cx), 0);
        assert!(
            p.read_with(cx, |p, _| p.filter.is_some()),
            "the first Esc clears it"
        );
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        cx.update(|window, cx| {
            let p = p.read(cx);
            assert!(p.filter.is_none() && p.focus.is_focused(window));
        });
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// At launch the panel is built while the repo is still opening: it reads once it's open,
    /// and "No repository is open" never shows.
    #[gpui_kit::test]
    fn a_panel_built_while_the_repo_opens_reads_once_open(cx: &mut TestAppContext) {
        let dir = repo("launch");
        setup(cx);
        let ws = cx.update(|cx| cx.new(Workspace::new));
        let (host, cx) = cx.add_window_view(|_, _| Host {
            panel: None,
            center: None,
        });
        let errors = Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
        cx.update(|window, cx| {
            Workspace::register_window(&ws, window, cx);
            ws.update(cx, |ws, cx| ws.open_project(dir.clone(), window, cx));
            let ctx = ws.update(cx, |ws, cx| ws.context(cx));
            assert!(!ctx.project.as_ref().unwrap().read(cx).is_open());
            let panel = cx.new(|cx| ChangesPanel::new(&ctx, window, cx));
            // Each state it's drawn in.
            let seen = errors.clone();
            cx.observe(&panel, move |p, cx| {
                seen.borrow_mut().extend(p.read(cx).error.clone())
            })
            .detach();
            // A read now beats the open on real threads and fails; this executor would run the
            // open first, so what counts is that none started.
            assert!(!panel.read(cx).reading);
            host.update(cx, |h, cx| {
                h.panel = Some(panel);
                cx.notify();
            });
        });
        cx.run_until_parked();
        let p = panel(&host, cx);
        assert_eq!(
            p.read_with(cx, |p, _| paths(&p.lists.unstaged)),
            ["a.txt", "b.txt", "c.txt"]
        );
        assert!(errors.borrow().is_empty(), "{:?}", errors.borrow());
        assert!(p.read_with(cx, |p, _| p.error.is_none()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// HEAD's commit comes in after the lists, and again only once HEAD moves; "pushed" (an
    /// amend needs a force push) comes from the branch's place against its push target.
    #[gpui_kit::test]
    fn head_and_pushed_follow_the_branch(cx: &mut TestAppContext) {
        let dir = repo("pushed");
        let remote = dir.with_extension("remote");
        let _ = std::fs::remove_dir_all(&remote);
        let url = remote.to_string_lossy().into_owned();
        git(&dir, &["init", "-q", "--bare", &url]);
        git(&dir, &["remote", "add", "origin", &url]);
        setup(cx);
        let (_ws, host, cx) = open(cx, &dir);
        let p = panel(&host, cx);
        let state = |cx: &mut VisualTestContext| {
            p.read_with(cx, |p, _| {
                (p.head.as_ref().map(|h| h.sha.clone()), p.pushed)
            })
        };
        let head = || git(&dir, &["rev-parse", "HEAD"]).trim().to_string();
        let first = head();
        assert_eq!(state(cx), (Some(first.clone()), false), "not on origin yet");
        git(&dir, &["push", "-q", "-u", "origin", "main"]);
        p.update(cx, |p, cx| p.read(cx));
        cx.run_until_parked();
        assert_eq!(state(cx), (Some(first), true));
        git(&dir, &["commit", "-q", "--allow-empty", "-m", "second"]);
        p.update(cx, |p, cx| p.read(cx));
        cx.run_until_parked();
        assert_eq!(state(cx), (Some(head()), false));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&remote);
    }

    #[gpui_kit::test]
    fn a_co_author_is_found_or_typed(cx: &mut TestAppContext) {
        let dir = repo("co-author");
        git(
            &dir,
            &["-c", "user.name=Ada Lovelace", "-c", "user.email=ada@x.dev"]
                .into_iter()
                .chain(["commit", "-q", "--allow-empty", "-m", "by Ada"])
                .collect::<Vec<_>>(),
        );
        setup(cx);
        let (_ws, host, cx) = open(cx, &dir);
        let added = Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
        let pick = |taken: Vec<String>, typed: &str, cx: &mut VisualTestContext| {
            let a = added.clone();
            cx.update(|window, cx| {
                let on_add: crate::co_authors::OnAdd =
                    Rc::new(move |author, _, _| a.borrow_mut().push(author));
                let picker =
                    cx.new(|cx| crate::co_authors::CoAuthorPicker::new(taken, on_add, window, cx));
                picker.focus_handle(cx).focus(window, cx);
                host.update(cx, |h, cx| {
                    h.center = Some(picker.into());
                    cx.notify();
                });
            });
            cx.run_until_parked();
            cx.simulate_input(typed);
            cx.simulate_keystrokes("enter");
            cx.run_until_parked();
        };
        let ada = "Ada Lovelace <ada@x.dev>".to_string();
        pick(vec![], "lovelace", cx);
        assert_eq!(*added.borrow(), vec![ada.clone()]);
        // Taken already, and no "Name <email>" typed: nothing to add.
        pick(vec![ada.clone()], "ada", cx);
        assert_eq!(added.borrow().len(), 1);
        pick(vec![ada.clone()], "Bob Ross <bob@x.io>", cx);
        assert_eq!(added.borrow()[1], "Bob Ross <bob@x.io>");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[gpui_kit::test]
    fn a_failing_hook_offers_committing_without_hooks(cx: &mut TestAppContext) {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = repo("hooks");
        let hook = dir.join(".git/hooks/pre-commit");
        // Silent: a hook's output is progress, sent from core's reader thread, which the test
        // scheduler refuses.
        std::fs::write(&hook, "#!/bin/sh\nexit 1\n").unwrap();
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        setup(cx);
        let (ws, host, cx) = open(cx, &dir);
        let commit = panel(&host, cx).read_with(cx, |p, _| p.commit.clone());
        cx.update(|window, cx| {
            commit.update(cx, |b, cx| {
                b.write_message("Add things", "", window, cx);
                b.commit(None, window, cx);
            })
        });
        cx.run_until_parked();
        assert_eq!(git(&dir, &["rev-list", "--count", "HEAD"]).trim(), "1");
        // The top bar's activity ended with it.
        assert!(!cx.update(|_, cx| NetActivity::global(cx).read(cx).is_running()));
        let toasts = ws.update(cx, |ws, _| ws.take_toasts());
        let failed = toasts
            .iter()
            .find(|t| t.title.as_deref() == Some("Commit failed"))
            .expect("a failure toast");
        assert!(failed.message.contains("A commit hook stopped the commit"));
        let action = failed.action.clone().expect("a way out");
        assert_eq!(action.label, "Commit without hooks");
        cx.update(|window, cx| (action.run)(window, cx));
        cx.run_until_parked();
        assert_eq!(
            git(&dir, &["log", "-1", "--format=%s"]).trim(),
            "Add things"
        );
        assert_eq!(git(&dir, &["rev-list", "--count", "HEAD"]).trim(), "2");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[gpui_kit::test]
    fn the_keys_reach_folders_and_stashes(cx: &mut TestAppContext) {
        let dir = repo("keys");
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/x.txt"), "x\n").unwrap();
        git(&dir, &["stash", "push", "-q", "-m", "saved", "--", "b.txt"]);
        setup(cx);
        let (ws, host, cx) = open(cx, &dir);
        let p = panel(&host, cx);
        p.update(cx, |p, cx| {
            p.set_view(
                ChangesView {
                    tree: true,
                    recent: false,
                },
                cx,
            )
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            let focus = p.read(cx).focus.clone();
            focus.focus(window, cx)
        });
        let at = |cx: &mut VisualTestContext| {
            p.read_with(cx, |p, cx| {
                p.cursor_ix(cx)
                    .map(|ix| p.keys[ix].clone())
                    .unwrap_or_default()
            })
        };
        let tab = |cx: &mut VisualTestContext| {
            ws.read_with(cx, |ws, _| ws.tabs().active_key().map(|k| k.to_string()))
        };
        let key = |k: &str, cx: &mut VisualTestContext| {
            cx.simulate_keystrokes(k);
            cx.run_until_parked();
            at(cx)
        };
        let folder = "folder:unstaged:src".to_string();
        // ↓ from the top stops on the folder, which takes the cursor and opens nothing.
        let mut now = key("home", cx);
        let mut before = tab(cx);
        for _ in 0..6 {
            if now == folder {
                break;
            }
            before = tab(cx);
            now = key("down", cx);
        }
        assert_eq!(now, folder);
        assert_eq!(tab(cx), before);
        // ← closes it, → opens it again, then → goes on to its file, which opens.
        key("left", cx);
        assert!(p.read_with(cx, |p, _| p.closed.contains("unstaged:src")));
        key("right", cx);
        assert!(!p.read_with(cx, |p, _| p.closed.contains("unstaged:src")));
        assert_eq!(key("right", cx), "unstaged::src/x.txt");
        assert_eq!(tab(cx).as_deref(), Some("unstaged::src/x.txt"));
        // ← from the file: back up to its folder, the tab stays.
        assert_eq!(key("left", cx), folder);
        assert_eq!(tab(cx).as_deref(), Some("unstaged::src/x.txt"));
        // End: the stash. Space opens it, → steps into its files, ← back out, ↵ closes it.
        assert_eq!(key("end", cx), "stash:0");
        key("space", cx);
        assert!(p.read_with(cx, |p, _| p.expanded_stash.is_some()));
        assert!(key("right", cx).starts_with("stashfile:"));
        assert_eq!(key("left", cx), "stash:0");
        key("enter", cx);
        assert!(p.read_with(cx, |p, _| p.expanded_stash.is_none()));
        // PgUp from the bottom goes up a page, never past the first row.
        assert_ne!(key("pageup", cx), "stash:0");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[gpui_kit::test]
    fn branch_review_tabs_follow_the_review(cx: &mut TestAppContext) {
        let dir = repo("review-follow");
        git(&dir, &["checkout", "-q", "-b", "feature"]);
        git(&dir, &["commit", "-q", "-am", "feature work"]);
        git(&dir, &["branch", "later"]);
        std::fs::write(dir.join("b.txt"), "dos\n").unwrap();
        setup(cx);
        let (ws, host, cx) = open(cx, &dir);
        let p = panel(&host, cx);
        let review = |reference: &str, cx: &mut VisualTestContext| {
            p.update(cx, |p, cx| p.start_review(reference.into(), cx));
            cx.run_until_parked();
            p.read_with(cx, |p, _| p.review.as_ref().unwrap().base.clone().unwrap())
        };
        let main = review("refs/heads/main", cx);
        for path in ["a.txt", "b.txt"] {
            let sel = p.read_with(cx, |p, _| {
                let r = p.review.as_ref().unwrap();
                p.review_selection(r.files.iter().find(|f| f.path == path).unwrap())
                    .unwrap()
            });
            cx.update(|window, cx| p.update(cx, |p, cx| p.open(sel, false, window, cx)));
        }
        cx.run_until_parked();
        let tabs = |cx: &mut VisualTestContext| {
            ws.read_with(cx, |ws, _| {
                ws.tabs()
                    .tabs()
                    .iter()
                    .map(|t| t.key.to_string())
                    .collect::<Vec<_>>()
            })
        };
        assert_eq!(
            tabs(cx),
            vec![
                format!("branch:{main}:a.txt"),
                format!("branch:{main}:b.txt")
            ]
        );
        // Against `later`, a.txt's change is in the base already: its tab closes, and b.txt's
        // moves onto the new base.
        let later = review("refs/heads/later", cx);
        assert_ne!(later, main);
        assert_eq!(tabs(cx), vec![format!("branch:{later}:b.txt")]);
        // Only the closed one comes back: the moved one's old key isn't left to reopen.
        cx.update(|window, cx| ws.update(cx, |ws, cx| ws.reopen_closed_tab(window, cx)));
        cx.run_until_parked();
        assert_eq!(
            tabs(cx),
            vec![
                format!("branch:{main}:a.txt"),
                format!("branch:{later}:b.txt")
            ]
        );
        assert!(!ws.read_with(cx, |ws, _| ws.can_reopen_tab()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The box's message and description.
    fn message(commit: &Entity<CommitBox>, cx: &mut VisualTestContext) -> (String, String) {
        commit.read_with(cx, |b, cx| (b.summary_text(cx), b.body_text(cx)))
    }

    #[gpui_kit::test]
    fn a_commit_stays_with_the_worktree_it_was_made_in(cx: &mut TestAppContext) {
        let dir = repo("commit-stays");
        setup(cx);
        let (ws, host, cx) = open(cx, &dir);
        let p = panel(&host, cx);
        let commit = p.read_with(cx, |p, _| p.commit.clone());
        let pushed = Rc::new(Cell::new(0));
        let count = pushed.clone();
        cx.update(|_, cx| {
            cx.on_action(move |_: &git::Push, _| count.set(count.get() + 1));
        });
        let a = dir.to_string_lossy().into_owned();

        // Running, the commit holds the worktree open (the switch guard waits for it).
        cx.update(|window, cx| {
            commit.update(cx, |b, cx| {
                b.write_message("Add things", "", window, cx);
                b.commit(None, window, cx);
                assert!(b.committing());
            })
        });
        cx.run_until_parked();
        assert!(!commit.read_with(cx, |b, _| b.committing()));
        assert_eq!(message(&commit, cx), (String::new(), String::new()));
        let toasts = ws.update(cx, |ws, _| ws.take_toasts());
        let done = toasts
            .iter()
            .find(|t| t.title.as_deref() == Some("Committed"));
        assert_eq!(
            done.and_then(|t| t.action.as_ref())
                .map(|a| a.label.clone()),
            Some("Undo".into()),
            "the commit's toast takes it back"
        );

        // Another worktree came in while the next one ran (a path past the guard): its box and
        // draft stay, the commit's draft goes, and nothing is pushed.
        let b_root = "/nonexistent/gitviber-worktree-b".to_string();
        cx.update(|window, cx| {
            commit.update(cx, |b, cx| {
                b.write_message("A's message", "", window, cx);
                let mut facts = b.facts().clone();
                facts.root = b_root.clone();
                b.set_facts(facts, window, cx);
                b.write_message("B's draft", "", window, cx);
            })
        });
        let started = || crate::commit::Started {
            repo: Some(PathBuf::from("/nonexistent/gitviber-worktree-a")),
            root: a.clone(),
            summary: "A's message".into(),
            then: Some(crate::commit::Then::Push),
            skipped: 0,
            amended: false,
        };
        cx.update(|window, cx| {
            commit.update(cx, |b, cx| b.committed(Ok(Some(7)), started(), window, cx))
        });
        cx.run_until_parked();
        assert_eq!(message(&commit, cx).0, "B's draft");
        assert_eq!(
            cx.update(|_, cx| store::draft(&b_root, cx))
                .map(|d| d.summary),
            Some("B's draft".into())
        );
        assert_eq!(cx.update(|_, cx| store::draft(&a, cx)), None);
        assert_eq!(pushed.get(), 0, "B isn't A's to push");
        let toasts = ws.update(cx, |ws, _| ws.take_toasts());
        assert!(
            toasts
                .iter()
                .any(|t| t.title.as_deref() == Some("Not pushed"))
        );
        // Its Undo is for A's journal: not offered in B.
        let done = toasts
            .iter()
            .find(|t| t.title.as_deref() == Some("Committed"));
        assert!(done.is_some_and(|t| t.action.is_none()));

        // Where it was made, Commit and Push pushes.
        let here = crate::commit::Started {
            repo: cx.update(|_, cx| crate::core(cx).repo().ok()),
            root: b_root.clone(),
            ..started()
        };
        cx.update(|window, cx| commit.update(cx, |b, cx| b.committed(Ok(None), here, window, cx)));
        cx.run_until_parked();
        assert_eq!(pushed.get(), 1);
        assert_eq!(message(&commit, cx).0, "");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[gpui_kit::test]
    fn unstaging_a_rename_unstages_both_paths(cx: &mut TestAppContext) {
        let dir = repo("unrename");
        git(&dir, &["mv", "b.txt", "d.txt"]);
        setup(cx);
        let (_ws, host, cx) = open(cx, &dir);
        let p = panel(&host, cx);
        let renamed = p.read_with(cx, |p, _| p.lists.staged.clone());
        assert_eq!(
            renamed
                .iter()
                .map(|f| (f.status.as_str(), f.old_path.as_deref()))
                .collect::<Vec<_>>(),
            [("R", Some("b.txt"))]
        );
        p.update(cx, |p, cx| p.unstage_files(renamed, cx));
        cx.run_until_parked();
        p.update(cx, |p, cx| p.read(cx));
        cx.run_until_parked();
        assert!(
            p.read_with(cx, |p, _| p.lists.staged.is_empty()),
            "b.txt's deletion stayed staged"
        );
        let status = git(&dir, &["status", "--porcelain"]);
        assert!(
            status.contains(" D b.txt") && status.contains("?? d.txt"),
            "{status}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Open All Changes reads a file again when what its diff is against moves, though the file
    /// and its status don't: the index under a reset, HEAD under a soft one.
    #[test]
    fn a_reset_moves_the_list_diffs_signature() {
        use crate::model::list::diff_sig;
        let dir = repo("reset-sig");
        git(&dir, &["commit", "-q", "--allow-empty", "-m", "second"]);
        let status = || gitviber_core::git::status(&dir).unwrap();
        let entry = |list: Vec<FileChange>| list.into_iter().find(|f| f.path == "a.txt").unwrap();
        let sigs = |f: &FileChange| {
            let against = f.index_oid.as_deref().unwrap_or("");
            (file_sig(f), diff_sig(f, against))
        };
        // Staged, then edited again: the unstaged change is against the staged version.
        git(&dir, &["add", "a.txt"]);
        std::fs::write(dir.join("a.txt"), "one\ntwo\nthree\n").unwrap();
        let before = sigs(&entry(status().unstaged));
        git(&dir, &["reset", "-q"]);
        let after = sigs(&entry(status().unstaged));
        assert_eq!(before.0, after.0, "the file itself didn't change");
        assert_ne!(before.1, after.1);
        // Staged under a soft reset: the same blob, against another HEAD.
        git(&dir, &["add", "a.txt"]);
        let s = status();
        let before = diff_sig(&entry(s.staged), s.head.as_deref().unwrap());
        git(&dir, &["reset", "-q", "--soft", "HEAD~1"]);
        let s = status();
        let after = diff_sig(&entry(s.staged), s.head.as_deref().unwrap());
        assert_ne!(before, after);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[gpui_kit::test]
    fn an_aborted_squash_takes_its_message_with_it(cx: &mut TestAppContext) {
        let dir = repo("squash-abort");
        git(&dir, &["checkout", "-q", "-b", "x"]);
        std::fs::write(dir.join("x.txt"), "x\n").unwrap();
        git(&dir, &["add", "x.txt"]);
        git(&dir, &["commit", "-q", "-m", "Add x"]);
        git(&dir, &["checkout", "-q", "main"]);
        git(&dir, &["merge", "-q", "--squash", "x"]);
        setup(cx);
        let (_ws, host, cx) = open(cx, &dir);
        let p = panel(&host, cx);
        let commit = p.read_with(cx, |p, _| p.commit.clone());
        let root = dir.to_string_lossy().into_owned();
        assert!(
            message(&commit, cx).0.starts_with("Squashed commit"),
            "{:?}",
            message(&commit, cx)
        );
        // As git prepared it: not the user's draft.
        assert_eq!(cx.update(|_, cx| store::draft(&root, cx)), None);
        git(&dir, &["reset", "-q", "--merge"]);
        p.update(cx, |p, cx| p.read(cx));
        cx.run_until_parked();
        assert_eq!(message(&commit, cx), (String::new(), String::new()));
        assert_eq!(cx.update(|_, cx| store::draft(&root, cx)), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[gpui_kit::test]
    fn a_held_index_lock_offers_removing_it(cx: &mut TestAppContext) {
        let dir = repo("index-lock");
        setup(cx);
        let (ws, host, cx) = open(cx, &dir);
        std::fs::write(dir.join(".git/index.lock"), "").unwrap();
        let p = panel(&host, cx);
        p.update(cx, |p, cx| {
            let a = p.lists.unstaged[0].clone();
            p.stage(vec![a], cx);
        });
        cx.run_until_parked();
        let toasts = ws.update(cx, |ws, _| ws.take_toasts());
        let failed = toasts
            .iter()
            .find(|t| t.title.as_deref() == Some("Stage failed"))
            .expect("a failure toast");
        assert!(
            failed
                .message
                .starts_with("Another git command holds the index lock"),
            "{}",
            failed.message
        );
        assert!(
            failed
                .detail
                .as_deref()
                .is_some_and(|d| d.contains("index.lock"))
        );
        assert_eq!(
            failed.action.as_ref().map(|a| a.label.clone()),
            Some("Remove lock".into())
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[gpui_kit::test]
    fn a_discard_says_where_the_old_version_went(cx: &mut TestAppContext) {
        assert_eq!(
            (
                discarded(Some("a.txt"), 1, 1, 0).title,
                discarded(Some("a.txt"), 1, 1, 0).message
            ),
            (
                Some("Discarded a.txt".into()),
                "The old version is in the Trash.".into()
            )
        );
        assert_eq!(
            discarded(None, 2, 1, 1).message.as_ref(),
            "The old version is in the Trash, with 1 file git didn't track."
        );
        assert_eq!(
            discarded(Some("gone.txt"), 1, 0, 0).message.as_ref(),
            "Discarded gone.txt"
        );
        assert_eq!(
            discarded(None, 0, 0, 2).message.as_ref(),
            "Moved 2 files to the Trash"
        );

        let dir = repo("discard-undo");
        setup(cx);
        let (ws, host, cx) = open(cx, &dir);
        let p = panel(&host, cx);
        // One file of this throwaway repo goes to the Trash.
        p.update(cx, |p, cx| {
            let done = discarded(Some("a.txt"), 1, 1, 0);
            p.discard_now(vec!["a.txt".into()], vec![], Some(done), None, cx)
        });
        cx.run_until_parked();
        assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), "one\n");
        let toasts = ws.update(cx, |ws, _| ws.take_toasts());
        let done = toasts
            .iter()
            .find(|t| t.title.as_deref() == Some("Discarded a.txt"))
            .expect("a done toast");
        assert_eq!(done.message.as_ref(), "The old version is in the Trash.");
        assert_eq!(
            done.action.as_ref().map(|a| a.label.clone()),
            Some("Undo".into())
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A worktree switch brings back that worktree's branch review (or none), and leaves the
    /// one it left saved with it.
    #[gpui_kit::test]
    fn a_branch_review_follows_the_worktree_switch(cx: &mut TestAppContext) {
        let dir = repo("review-switch");
        git(&dir, &["checkout", "-q", "-b", "feature"]);
        git(&dir, &["branch", "later"]);
        setup(cx);
        let ws = cx.update(|cx| cx.new(Workspace::new));
        let (host, cx) = cx.add_window_view(|_, _| Host {
            panel: None,
            center: None,
        });
        let core = cx.update(|_, cx| crate::core(cx));
        api::repo::open_repo(&core, dir.to_string_lossy().into_owned()).unwrap();
        let (a, b) = (dir.clone(), PathBuf::from("/nonexistent/gitviber-review-b"));
        cx.update(|window, cx| {
            Workspace::register_window(&ws, window, cx);
            // No core of the app's: the project stays as it is, and the panel reads TestCore.
            ws.update(cx, |ws, cx| ws.open_project(a.clone(), window, cx));
            let ctx = ws.update(cx, |ws, cx| ws.context(cx));
            let panel = cx.new(|cx| ChangesPanel::new(&ctx, window, cx));
            host.update(cx, |h, cx| {
                h.panel = Some(panel);
                cx.notify();
            });
        });
        cx.run_until_parked();
        let p = panel(&host, cx);
        let project = ws.read_with(cx, |ws, _| ws.project().cloned().unwrap());
        let switch = |from: &Path, to: &Path, cx: &mut VisualTestContext| {
            let (from, to) = (from.to_path_buf(), to.to_path_buf());
            project.update(cx, |_, cx| cx.emit(ProjectEvent::Switched { from, to }));
            cx.run_until_parked();
        };
        let reviewed = |cx: &mut VisualTestContext| {
            p.read_with(cx, |p, _| p.review.as_ref().map(|r| r.reference.clone()))
        };
        p.update(cx, |p, cx| p.start_review("refs/heads/main".into(), cx));
        cx.run_until_parked();
        switch(&a, &b, cx);
        assert_eq!(reviewed(cx), None, "B has no review");
        p.update(cx, |p, cx| p.start_review("refs/heads/later".into(), cx));
        cx.run_until_parked();
        switch(&b, &a, cx);
        assert_eq!(reviewed(cx).as_deref(), Some("refs/heads/main"));
        assert!(
            p.read_with(cx, |p, _| p.review.as_ref().unwrap().base.is_some()),
            "its files are read"
        );
        switch(&a, &b, cx);
        assert_eq!(reviewed(cx).as_deref(), Some("refs/heads/later"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A Claude Code too old for the lean flags refuses them: ✦ asks once more without them,
    /// the model kept, as a guided review does.
    #[gpui_kit::test]
    fn a_suggestion_retries_an_old_claude_without_the_lean_flags(cx: &mut TestAppContext) {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = repo("suggest-old");
        let bin = dir.with_extension("bin");
        let _ = std::fs::remove_dir_all(&bin);
        std::fs::create_dir_all(&bin).unwrap();
        let fake = bin.join("claude");
        std::fs::write(
            &fake,
            "#!/bin/sh\nhere=\"$(cd \"$(dirname \"$0\")\" && pwd)\"\ncat >/dev/null\nlean=no; model=no\nfor a in \"$@\"; do [ \"$a\" = --strict-mcp-config ] && lean=yes; [ \"$a\" = --model ] && model=yes; done\necho \"$lean $model\" >> \"$here/calls\"\n[ $lean = yes ] && { echo \"error: unknown option '--strict-mcp-config'\" >&2; exit 1; }\nprintf 'Fix the thing\\n\\nWhy it was broken.\\n'\n",
        )
        .unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        let path = format!("{}:/usr/bin:/bin", bin.display());
        // SAFETY: each test runs in its own process (nextest), before any thread reads it.
        unsafe { std::env::set_var("PATH", &path) };
        // As core searches: Homebrew's, then this PATH. The real claude must never run.
        let first = ["/opt/homebrew/bin", "/usr/local/bin"]
            .into_iter()
            .map(PathBuf::from)
            .chain(std::env::split_paths(&path))
            .map(|d| d.join("claude"))
            .find(|p| p.exists());
        assert_eq!(
            first.as_deref(),
            Some(fake.as_path()),
            "a real claude would run"
        );
        setup(cx);
        let (_ws, host, cx) = open(cx, &dir);
        let line = cx.update(|_, cx| gitviber_ui::Settings::get(cx).suggest_command.clone());
        assert_eq!(line, "claude -p");
        let commit = panel(&host, cx).read_with(cx, |p, _| p.commit.clone());
        cx.update(|window, cx| commit.update(cx, |b, cx| b.suggest(window, cx)));
        cx.run_until_parked();
        assert_eq!(
            message(&commit, cx),
            ("Fix the thing".into(), "Why it was broken.".into())
        );
        let calls = std::fs::read_to_string(bin.join("calls")).unwrap();
        assert_eq!(
            calls, "yes yes\nno yes\n",
            "a refused try, then one without"
        );
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&bin);
    }

    /// Code in a conflict is drawn as in the diffs: a 2-space file widened to 4 columns a level.
    #[gpui_kit::test]
    fn the_conflict_view_widens_indentation_as_the_diffs_do(cx: &mut TestAppContext) {
        let dir = conflicted(
            "conflict-indent",
            "a\n  B1\n    c\n  d\n",
            "a\n  B2\n    c\n  d\n",
        );
        setup(cx);
        let (_ws, host, cx) = open(cx, &dir);
        let view = conflict_view(&host, "UU", cx);
        assert_eq!(view.read_with(cx, |v, _| v.indent_unit_for_test()), Some(2));
        assert_eq!(
            gitviber_diff::model::indent::Shown::new("  B1", 2).text,
            "    B1"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Code in a conflict is in its syntax colors, as in the diffs: of the language its name
    /// tells, or its first line without one, and each block read with the code around it, so a
    /// side inside a string is string.
    #[gpui_kit::test]
    fn the_conflict_view_colors_code_with_the_code_around_it(cx: &mut TestAppContext) {
        use crate::model::versions::Version;
        let file = |line: &str| format!("#!/usr/bin/env node\nconst a = `\n{line}\n`;\n");
        let dir = conflicted_at(
            "conflict-colors",
            "run",
            &file("zero"),
            &file("one"),
            &file("two"),
        );
        setup(cx);
        let (_ws, host, cx) = open(cx, &dir);
        let view = conflict_view_at(&host, "run", "UU", cx);
        let color = |at: (usize, Version, usize), byte: usize, cx: &mut VisualTestContext| {
            view.read_with(cx, |v, cx| v.styles_for_test(at, cx))
                .expect("colored")
                .iter()
                .find(|(r, _)| r.contains(&byte))
                .and_then(|(_, s)| s.color)
        };
        // The backtick opening the string, then each side's line inside it.
        let string = color((0, Version::Ours, 1), 10, cx);
        assert!(string.is_some());
        assert_eq!(color((1, Version::Ours, 0), 0, cx), string);
        assert_eq!(color((1, Version::Theirs, 0), 0, cx), string);
        assert_eq!(color((1, Version::Base, 0), 0, cx), string);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
