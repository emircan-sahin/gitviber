//! The Changes panel: the conflicted, staged and changed files (flat or as a folder tree, by
//! name or newest first), what each row does (stage, unstage, discard, resolve, mark viewed),
//! the stashes and submodules below them, the banner of a merge or rebase in progress, a branch
//! reviewed against its base in place of the uncommitted files, and the commit box.

use crate::commit::{CommitBox, RepoFacts};
use crate::dialogs::{self, Ask, Choice};
use crate::model::list::{
    ChangesView, Item, Kind, Lists, Mtimes, NESTED_EXPLAINED, file_sig, files, folder_key,
    items_of, kept_by_restore, left_out, range_of, stageable, sum_lines, toggled, viewed_key,
};
use crate::model::text::{describe_stash, has_conflict_markers, ignore_pattern};
use crate::model::tree::folders_of;
use crate::store;
use gitviber_core::api;
use gitviber_core::git::{Commit, FileChange, Side as GitSide};
use gitviber_diff::{Compare, DiffSource};
use gitviber_ui::actions::{changes as changes_actions, git, review};
use gitviber_ui::memory::Live;
use gitviber_ui::widgets::{ChangeKind, FileIcon, LineCounts, PathLabel, StatusLetter};
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
    AnyElement, App, AppContext as _, ClickEvent, Context, Entity, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, KeyBinding, Modifiers, ParentElement as _, Render,
    SharedString, StatefulInteractiveElement as _, Styled as _, Subscription, Task,
    UniformListScrollHandle, WeakEntity, Window, div, prelude::FluentBuilder as _, px,
    uniform_list,
};
use std::cell::Cell;
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;

const ROW: f32 = 26.;
const CONTEXT: &str = "ChangesList";

gpui_kit::actions!(
    changes_list,
    [Up, Down, ExtendUp, ExtendDown, Open, Keep, Escape]
);

pub fn bind_keys(cx: &mut App) {
    let c = Some(CONTEXT);
    cx.bind_keys([
        KeyBinding::new("up", Up, c),
        KeyBinding::new("down", Down, c),
        KeyBinding::new("shift-up", ExtendUp, c),
        KeyBinding::new("shift-down", ExtendDown, c),
        KeyBinding::new("space", Open, c),
        KeyBinding::new("enter", Keep, c),
        KeyBinding::new("escape", Escape, c),
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
    let paths: Vec<String> = files.into_iter().map(|f| f.path).collect();
    cx.spawn(async move |cx| {
        let result = cx
            .background_spawn(async move { api::changes::unstage(&core, paths) })
            .await;
        cx.update(|cx| {
            let Some(ws) = ws.upgrade() else { return };
            if let Err(e) = result {
                ws.update(cx, |ws, cx| {
                    ws.toast(Toast::error(e).title("Unstage failed"), cx)
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
    StashFile {
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
    text: String,
    branch: Option<String>,
    timestamp: i64,
}

type StashFile = (String, Arc<FileChange>, bool);

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
    focus: FocusHandle,
    loaded: bool,
    error: Option<String>,
    root: String,
    branch: Option<String>,
    head: Option<Arc<Commit>>,
    pushed: bool,
    operation: Option<Op>,
    prepared: Option<String>,
    lists: Lists,
    mtimes: Option<Mtimes>,
    stashes: Vec<StashInfo>,
    submodules: Vec<(String, String, String)>,
    view: ChangesView,
    closed: HashSet<String>,
    collapsed: HashSet<Section>,
    filter: Entity<InputState>,
    /// Rows picked with ⌘ / ⇧ around the open one; None: just the open row.
    picked: Option<(Vec<String>, String)>,
    /// The key of the open tab's file, when it's one of ours.
    open_key: Option<String>,
    rows: Vec<Row>,
    keys: Vec<String>,
    scroll: UniformListScrollHandle,
    commit: Entity<CommitBox>,
    /// The worktree's review notes, between the lists and the commit box.
    notes: Entity<gitviber_review::NotesSection>,
    review: Option<Review>,
    branches: Vec<String>,
    expanded_stash: Option<String>,
    /// The open stash's files: its sha, then each file with the commit it's in and whether untracked.
    stash_files: Option<(String, Vec<StashFile>)>,
    busy: bool,
    /// What the commit box is told at the next frame.
    facts: Option<RepoFacts>,
    reading: bool,
    again: bool,
    _read: Task<()>,
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
        let filter = cx.new(|cx| InputState::new(window, cx).placeholder("Filter changed files"));
        let commit = cx.new(|cx| CommitBox::new(ctx.workspace.clone(), window, cx));
        let notes = cx.new(|cx| gitviber_review::NotesSection::new(ctx, cx));
        let mut subscriptions = vec![cx.subscribe(&filter, |this, _, e: &InputEvent, cx| {
            if matches!(e, InputEvent::Change) {
                this.rebuild(cx);
            }
        })];
        if let Some(project) = &ctx.project {
            subscriptions.push(
                cx.subscribe(project, |this, _, e: &ProjectEvent, cx| match e {
                    ProjectEvent::RepoChanged(_)
                    | ProjectEvent::Opened
                    | ProjectEvent::Switched { .. } => this.read(cx),
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
            focus: cx.focus_handle(),
            loaded: false,
            error: None,
            root: String::new(),
            branch: None,
            head: None,
            pushed: false,
            operation: None,
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
            filter,
            picked: None,
            open_key: None,
            rows: vec![],
            keys: vec![],
            scroll: UniformListScrollHandle::new(),
            commit,
            notes,
            review: None,
            branches: vec![],
            expanded_stash: None,
            stash_files: None,
            busy: false,
            facts: None,
            reading: false,
            again: false,
            _read: Task::ready(()),
            _review: Task::ready(()),
            _action: Task::ready(()),
            _stash: Task::ready(()),
            _subscriptions: subscriptions,
            _live: Live::new("ChangesPanel"),
        };
        if let Some(base) = ctx
            .workspace
            .upgrade()
            .and_then(|ws| ws.read(cx).review_base().map(str::to_string))
        {
            panel.start_review(base, cx);
        }
        panel.read(cx);
        panel
    }

    fn workspace(&self) -> Option<Entity<Workspace>> {
        self.ctx.workspace.upgrade()
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

    /// Status, stashes and submodules, one read at a time; changes during a read make one more.
    fn read(&mut self, cx: &mut Context<Self>) {
        if self.reading {
            self.again = true;
            return;
        }
        self.reading = true;
        let core = crate::core(cx);
        let recent = self.view.recent;
        let head_before = self.head.as_ref().map(|h| h.sha.clone());
        self._read = cx.spawn(async move |this, cx| {
            let read = cx
                .background_spawn(async move {
                    let status = api::changes::status(&core)?;
                    let stashes = api::stash::stashes(&core).unwrap_or_default();
                    let submodules = api::sync::submodules(&core).unwrap_or_default();
                    let paths: Vec<String> = status
                        .conflicted
                        .iter()
                        .chain(&status.staged)
                        .chain(&status.unstaged)
                        .map(|f| f.path.clone())
                        .collect();
                    let mtimes = recent.then(|| {
                        let times =
                            api::files::file_mtimes(&core, paths.clone()).unwrap_or_default();
                        paths
                            .into_iter()
                            .zip(times)
                            .filter_map(|(p, t)| t.map(|t| (p, t)))
                            .collect::<Mtimes>()
                    });
                    let head = match (&status.head, head_before) {
                        (Some(sha), Some(before)) if *sha == before => None,
                        (Some(sha), _) => api::history::find_commit(&core, sha.clone())
                            .ok()
                            .flatten()
                            .map(Arc::new),
                        _ => None,
                    };
                    Ok::<_, String>((status, stashes, submodules, mtimes, head))
                })
                .await;
            this.update(cx, |this, cx| {
                this.reading = false;
                match read {
                    Ok((status, stashes, submodules, mtimes, head)) => {
                        this.loaded = true;
                        this.error = None;
                        this.root = status.root.clone();
                        this.branch = status.branch.clone();
                        if status.head.is_none() {
                            this.head = None;
                        } else if let Some(h) = head {
                            this.head = Some(h);
                        }
                        this.pushed = this.head.as_ref().is_some_and(|h| !h.unpushed)
                            && (status.upstream.is_some() || status.push.is_some());
                        this.operation = status.operation.as_ref().map(|o| Op {
                            kind: o.kind.clone(),
                            subject: o.subject.clone(),
                            step: o.step,
                            total: o.total,
                            split: o.split,
                        });
                        this.prepared = status.prepared_message.clone();
                        this.lists = Lists::of(&status);
                        this.mtimes = mtimes;
                        this.stashes = stashes
                            .into_iter()
                            .map(|s| {
                                let (text, branch) = describe_stash(&s.message);
                                StashInfo {
                                    sha: s.sha,
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
        let needle = self.filter.read(cx).value().to_lowercase();
        let lists = if needle.trim().is_empty() {
            self.lists.clone()
        } else {
            let needle = needle.trim().to_string();
            self.lists.filtered(|f| {
                f.path.to_lowercase().contains(&needle)
                    || f.old_path
                        .as_deref()
                        .is_some_and(|o| o.to_lowercase().contains(&needle))
            })
        };
        lists.ordered(self.view, self.mtimes.as_ref())
    }

    fn filtering(&self, cx: &App) -> bool {
        !self.filter.read(cx).value().trim().is_empty()
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

    /// ↑ / ↓ along the files shown, ⇧ to pick the range.
    fn step(&mut self, dir: isize, extend: bool, window: &mut Window, cx: &mut Context<Self>) {
        let keys = self.file_keys();
        if keys.is_empty() {
            return;
        }
        let at = self
            .open_key
            .as_ref()
            .and_then(|k| keys.iter().position(|x| x == k));
        let to = match at {
            Some(i) => (i as isize + dir).clamp(0, keys.len() as isize - 1) as usize,
            None => 0,
        };
        let key = keys[to].clone();
        if extend {
            let anchor = self
                .picked
                .as_ref()
                .map(|p| p.1.clone())
                .or(self.open_key.clone())
                .unwrap_or(key.clone());
            self.picked = Some((range_of(&keys, &anchor, &key), anchor));
        } else {
            self.picked = None;
        }
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
    fn run(
        &mut self,
        failed: &'static str,
        done: Option<String>,
        work: impl FnOnce(&gitviber_core::Core) -> Result<(), String> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        let core = crate::core(cx);
        self.busy = true;
        cx.notify();
        self._action = cx.spawn(async move |this, cx| {
            let result = cx.background_spawn(async move { work(&core) }).await;
            this.update(cx, |this, cx| {
                this.busy = false;
                match result {
                    Ok(()) => {
                        if let Some(done) = done {
                            this.toast(Toast::success(done), cx);
                        }
                    }
                    Err(e) => this.toast(Toast::error(e).title(failed), cx),
                }
                this.refresh_project(cx);
                cx.notify();
            })
            .ok();
        });
    }

    fn stage(&mut self, files: Vec<Arc<FileChange>>, cx: &mut Context<Self>) {
        let (paths, nested) = stageable(&files);
        if nested > 0 {
            self.toast(Toast::info(NESTED_EXPLAINED).title(left_out(nested)), cx);
        }
        if !paths.is_empty() {
            self.run(
                "Stage failed",
                None,
                move |core| api::changes::stage(core, paths, false),
                cx,
            );
        }
    }

    fn unstage_files(&mut self, files: Vec<Arc<FileChange>>, cx: &mut Context<Self>) {
        let paths: Vec<String> = files.iter().map(|f| f.path.clone()).collect();
        if !paths.is_empty() {
            self.run(
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
        let done = if restorable.is_empty() {
            None
        } else {
            Some(format!(
                "Discarded {}",
                one.clone().unwrap_or_else(|| files(restorable.len()))
            ))
        };
        dialogs::confirm(window, cx, title, question, ok, move |_, cx| {
            weak.update(cx, |this, cx| {
                this.discard_now(restorable, untracked, done, cx)
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
        done: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.run(
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
            let flagged = cx
                .background_spawn(async move {
                    check
                        .iter()
                        .filter(|f| matches!(f.conflict.as_deref(), Some("UU" | "AA")))
                        .filter(|f| {
                            api::files::read_file(&core, f.path.clone()).is_ok_and(|t| {
                                t.exists
                                    && !t.binary
                                    && (t.too_large || has_conflict_markers(&t.text))
                            })
                        })
                        .map(|f| f.path.clone())
                        .collect::<Vec<_>>()
                })
                .await;
            cx.update(|window, cx| {
                let go = move |cx: &mut App| {
                    weak.update(cx, |this, cx| this.stage(rows, cx)).ok();
                };
                if flagged.is_empty() {
                    go(cx);
                } else {
                    let name = if flagged.len() == 1 {
                        flagged[0].clone()
                    } else {
                        files(flagged.len())
                    };
                    let q = format!(
                        "{name} still {} conflict markers. Mark {} resolved anyway?",
                        if flagged.len() == 1 { "has" } else { "have" },
                        if flagged.len() == 1 { "it" } else { "them" }
                    );
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
                        Some(done),
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
                    Err(e) => this.toast(Toast::error(e).title("Could not load stash"), cx),
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
                        Toast::error(e).title(if pop { "Pop failed" } else { "Apply failed" }),
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
                        Some(format!(
                            "Stash dropped. To get it back: git stash apply {short}"
                        )),
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
                        Some("Stash renamed".into()),
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
                weak.update(cx, |this, cx| this.run("Branch from stash failed", Some(done), move |core| api::stash::stash_branch(core, name, sha).map(|_| ()), cx)).ok();
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
            Some(done),
            move |core| api::stash::stash_restore_file(core, sha, path, untracked),
            cx,
        );
    }

    fn update_submodules(&mut self, cx: &mut Context<Self>) {
        self.run(
            "Could not update submodules",
            Some("Submodules updated".into()),
            |core| api::sync::submodule_update(core, "submodules".into(), |_| {}),
            cx,
        );
    }

    fn operation(&mut self, which: &'static str, window: &mut Window, cx: &mut Context<Self>) {
        let kind = self
            .operation
            .as_ref()
            .map(|o| o.kind.clone())
            .unwrap_or_default();
        let go = move |this: &mut Self, cx: &mut Context<Self>| {
            let core = crate::core(cx);
            this.busy = true;
            let kind = kind.clone();
            this._action = cx.spawn(async move |this, cx| {
                let result = cx
                    .background_spawn(async move {
                        match which {
                            "continue" => api::sync::op_continue(&core),
                            "skip" => api::sync::rebase_skip(&core),
                            _ => api::sync::op_abort(&core).map(|_| false),
                        }
                    })
                    .await;
                this.update(cx, |this, cx| {
                    this.busy = false;
                    match result {
                        Ok(true) => this.toast(
                            Toast::info("Resolve them to continue.")
                                .title("Stopped on new conflicts"),
                            cx,
                        ),
                        Ok(false) if which != "abort" => this.toast(
                            Toast::success(format!(
                                "{}{} finished",
                                kind[..1.min(kind.len())].to_uppercase(),
                                &kind[1.min(kind.len())..]
                            )),
                            cx,
                        ),
                        Ok(false) => {}
                        Err(e) => this.toast(
                            Toast::error(e).title(format!(
                                "{} failed",
                                which[..1].to_uppercase() + &which[1..]
                            )),
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
    fn apply_patch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
                    weak.update(cx, |this, cx| this.toast(Toast::error(e).title("Can't apply that patch"), cx)).ok();
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
                        weak.update(cx, |this, cx| this.run("Apply patch failed", Some(done), move |core| api::patch::apply_patch(core, patch).map(|_| ()), cx)).ok();
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
        let label = reference
            .trim_start_matches("refs/heads/")
            .trim_start_matches("refs/remotes/")
            .to_string();
        self.review = Some(Review {
            reference: reference.clone(),
            label,
            base: None,
            files: vec![],
            error: None,
        });
        if let Some(ws) = self.workspace() {
            ws.update(cx, |ws, cx| ws.set_review_base(Some(reference.clone()), cx));
        }
        self.load_review(reference, cx);
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

    fn open_all(&mut self, of: ChangesOf, window: &mut Window, cx: &mut Context<Self>) {
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
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let t = cx.app_theme().clone();
        let ws = self.workspace();
        range
            .filter_map(|ix| {
                self.rows
                    .get(ix)
                    .cloned()
                    .map(|row| self.render_row(ix, row, &t, ws.as_ref(), cx))
            })
            .collect()
    }

    fn render_row(
        &self,
        ix: usize,
        row: Row,
        t: &AppTheme,
        ws: Option<&Entity<Workspace>>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
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
                    .child(
                        h_flex()
                            .invisible()
                            .group_hover("row", |s| s.visible())
                            .children(self.actions_for(kind, rows.clone(), true, cx)),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
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
                    cx,
                )
            }
            Row::Review(file) => match self.review_selection(&file) {
                Some(sel) => self.file_row(ix, sel, Kind::Unstaged, file, 0, None, t, ws, cx),
                None => base().into_any_element(),
            },
            Row::Stash(i) => self.stash_row(ix, i, t, cx),
            Row::StashFile {
                sha,
                file,
                untracked,
            } => {
                let (s, p) = (sha.clone(), file.path.clone());
                base()
                    .pl_8()
                    .child(FileIcon::file(&file.path))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(PathLabel(file.path.clone().into())),
                    )
                    .child(
                        div()
                            .group_hover("row", |s| s.invisible())
                            .child(LineCounts::new(file.additions, file.deletions)),
                    )
                    .child(
                        h_flex()
                            .invisible()
                            .group_hover("row", |s| s.visible())
                            .when(file.status != "D", |el| {
                                el.child(row_button(
                                    format!("take-{ix}"),
                                    Lucide::FileDown,
                                    "Take this file (replaces the working copy)",
                                    cx.listener(move |this, _, _, cx| {
                                        this.take_from_stash(s.clone(), p.clone(), untracked, cx)
                                    }),
                                ))
                            }),
                    )
                    .child(StatusLetter(change_kind(&file.status)))
                    .into_any_element()
            }
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
                    buttons.push(small("open-all", "Open All".into()).on_click(
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
                    buttons.push(small("open-all-staged", "Open All".into()).on_click(
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
                buttons.push(small("open-all-branch", "Open All".into()).on_click(
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
            menu.separator()
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
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let key = selection_key(&sel);
        let active = self.open_key.as_deref() == Some(key.as_str());
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
        let weak = cx.entity().downgrade();
        let menu_targets = targets.clone();
        let root = self.root.clone();
        let can_stash = self.operation.is_none();
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
            .child(
                div()
                    .group_hover("row", |s| s.invisible())
                    .child(LineCounts::new(file.additions, file.deletions)),
            )
            .when(!is_review, |el| {
                el.child(
                    h_flex()
                        .absolute()
                        .right(px(22.))
                        .invisible()
                        .group_hover("row", |s| s.visible())
                        .children(self.actions_for(kind, targets.clone(), false, cx)),
                )
            })
            .child(StatusLetter(change_kind(&file.status)))
            .on_click(cx.listener(move |this, e: &ClickEvent, window, cx| {
                this.click_file(
                    sel_click.clone(),
                    e.modifiers(),
                    e.click_count(),
                    window,
                    cx,
                )
            }))
            .context_menu(move |menu, _, cx| {
                let w = weak.clone();
                let rows = menu_targets.clone();
                let n = rows.len();
                let path = rows.first().map(|f| f.path.clone()).unwrap_or_default();
                let mut menu = menu;
                let (w1, s1) = (w.clone(), sel_menu.clone());
                menu = menu.item(PopupMenuItem::new("Open").on_click(move |_, window, cx| {
                    w1.update(cx, |this, cx| this.open(s1.clone(), false, window, cx))
                        .ok();
                }));
                let (w2, p2) = (w.clone(), path.clone());
                menu = menu.item(
                    PopupMenuItem::new("Open File").on_click(move |_, window, cx| {
                        w2.update(cx, |this, cx| {
                            this.open(Selection::File { path: p2.clone() }, false, window, cx)
                        })
                        .ok();
                    }),
                );
                menu = menu.separator();
                if is_review {
                    return menu;
                }
                match kind {
                    Kind::Unstaged => {
                        let (w3, r3) = (w.clone(), rows.clone());
                        menu = menu.item(
                            PopupMenuItem::new(if n > 1 {
                                format!("Stage {}", files(n))
                            } else {
                                "Stage".into()
                            })
                            .on_click(move |_, _, cx| {
                                w3.update(cx, |this, cx| this.stage(r3.clone(), cx)).ok();
                            }),
                        );
                        let (w4, r4) = (w.clone(), rows.clone());
                        menu = menu.item(
                            PopupMenuItem::new(if n > 1 {
                                format!("Discard {}…", files(n))
                            } else {
                                "Discard Changes…".into()
                            })
                            .on_click(move |_, window, cx| {
                                w4.update(cx, |this, cx| this.discard(r4.clone(), window, cx))
                                    .ok();
                            }),
                        );
                        if can_stash {
                            let (w5, p5) = (
                                w.clone(),
                                rows.iter().map(|f| f.path.clone()).collect::<Vec<_>>(),
                            );
                            menu = menu.item(
                                PopupMenuItem::new(if n > 1 {
                                    format!("Stash {}…", files(n))
                                } else {
                                    "Stash This File…".into()
                                })
                                .on_click(move |_, window, cx| {
                                    w5.update(cx, |this, cx| {
                                        this.stash_dialog(p5.clone(), window, cx)
                                    })
                                    .ok();
                                }),
                            );
                        }
                        if rows.iter().all(|f| f.status == "?") {
                            let (w6, r6) = (w.clone(), rows.clone());
                            menu = menu.item(PopupMenuItem::new("Add to .gitignore").on_click(
                                move |_, _, cx| {
                                    w6.update(cx, |this, cx| this.ignore(r6.clone(), cx)).ok();
                                },
                            ));
                        }
                    }
                    Kind::Staged => {
                        let (w3, r3) = (w.clone(), rows.clone());
                        menu = menu.item(
                            PopupMenuItem::new(if n > 1 {
                                format!("Unstage {}", files(n))
                            } else {
                                "Unstage".into()
                            })
                            .on_click(move |_, _, cx| {
                                w3.update(cx, |this, cx| this.unstage_files(r3.clone(), cx))
                                    .ok();
                            }),
                        );
                        if can_stash {
                            let (w5, p5) = (
                                w.clone(),
                                rows.iter().map(|f| f.path.clone()).collect::<Vec<_>>(),
                            );
                            menu = menu.item(
                                PopupMenuItem::new(if n > 1 {
                                    format!("Stash {}…", files(n))
                                } else {
                                    "Stash This File…".into()
                                })
                                .on_click(move |_, window, cx| {
                                    w5.update(cx, |this, cx| {
                                        this.stash_dialog(p5.clone(), window, cx)
                                    })
                                    .ok();
                                }),
                            );
                        }
                    }
                    Kind::Conflict => {
                        let (w3, r3) = (w.clone(), rows.clone());
                        menu = menu.item(PopupMenuItem::new("Mark Resolved").on_click(
                            move |_, window, cx| {
                                w3.update(cx, |this, cx| {
                                    this.mark_resolved(r3.clone(), window, cx)
                                })
                                .ok();
                            },
                        ));
                        let (w4, r4) = (w.clone(), rows.clone());
                        menu = menu.item(PopupMenuItem::new("Take Current Version").on_click(
                            move |_, _, cx| {
                                w4.update(cx, |this, cx| {
                                    this.resolve(r4.clone(), GitSide::Ours, cx)
                                })
                                .ok();
                            },
                        ));
                        let (w5, r5) = (w.clone(), rows.clone());
                        menu = menu.item(PopupMenuItem::new("Take Incoming Version").on_click(
                            move |_, _, cx| {
                                w5.update(cx, |this, cx| {
                                    this.resolve(r5.clone(), GitSide::Theirs, cx)
                                })
                                .ok();
                            },
                        ));
                    }
                }
                let abs = format!("{}/{}", root.trim_end_matches('/'), path);
                menu.separator()
                    .item(PopupMenuItem::new("Copy Path").on_click(move |_, _, cx| {
                        cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(abs.clone()))
                    }))
                    .item(
                        PopupMenuItem::new("Copy Relative Path").on_click(move |_, _, cx| {
                            cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(path.clone()))
                        }),
                    )
                    .when(cx.has_global::<gitviber_ui::Settings>(), |m| m)
            })
            .into_any_element()
    }

    fn stash_row(&self, ix: usize, i: usize, t: &AppTheme, cx: &mut Context<Self>) -> AnyElement {
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
        let when =
            gitviber_ui::widgets::relative_time(s.timestamp, gitviber_ui::widgets::now_unix());
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
                div()
                    .group_hover("row", |s| s.invisible())
                    .text_size(px(10.5))
                    .text_color(t.subtle)
                    .child(match &s.branch {
                        Some(b) => format!("{b} · {when}"),
                        None => when,
                    }),
            )
            .child(
                h_flex()
                    .absolute()
                    .right_2()
                    .invisible()
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
            .on_click(cx.listener(move |this, _, _, cx| this.toggle_stash(toggle.clone(), cx)))
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

    fn render_banner(&self, t: &AppTheme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let op = self.operation.as_ref()?;
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
        let view = self.view;
        let weak = cx.entity().downgrade();
        let toolbar = h_flex()
            .flex_none()
            .gap_1()
            .px_2()
            .py_1p5()
            .child(
                div()
                    .flex_1()
                    .child(Input::new(&self.filter).small().cleanable(true)),
            )
            .child(
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
                    }),
            );
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
            .bg(t.chrome(t.sidebar))
            .text_color(t.foreground)
            .child(toolbar)
            .children(banner)
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
            .on_action(cx.listener(|this, _: &Up, window, cx| this.step(-1, false, window, cx)))
            .on_action(cx.listener(|this, _: &Down, window, cx| this.step(1, false, window, cx)))
            .on_action(
                cx.listener(|this, _: &ExtendUp, window, cx| this.step(-1, true, window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &ExtendDown, window, cx| this.step(1, true, window, cx)),
            )
            .on_action(cx.listener(|this, _: &Keep, window, cx| {
                if let Some(sel) = this.open_key.clone().and_then(|k| this.selection_at(&k)) {
                    this.open(sel, false, window, cx);
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
                cx.listener(|this, _: &review::NextFile, window, cx| {
                    this.step(1, false, window, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &review::PrevFile, window, cx| {
                this.step(-1, false, window, cx)
            }))
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
            .on_action(
                cx.listener(|this, _: &git::ApplyPatch, window, cx| this.apply_patch(window, cx)),
            )
            .on_action(cx.listener(|this, _: &review::OpenAll, window, cx| {
                this.open_all(ChangesOf::Unstaged, window, cx)
            }))
            .on_action(cx.listener(|this, _: &review::OpenAllStaged, window, cx| {
                this.open_all(ChangesOf::Staged, window, cx)
            }))
            .on_action(cx.listener(|this, _: &review::OpenAllBranch, window, cx| {
                this.open_all(ChangesOf::Branch, window, cx)
            }))
            .on_action(cx.listener(|this, _: &review::Branch, _, cx| {
                let reference = this
                    .review
                    .as_ref()
                    .map(|r| r.reference.clone())
                    .unwrap_or_else(|| "refs/remotes/origin/main".into());
                this.start_review(reference, cx);
            }))
            .on_action(
                cx.listener(|this, _: &changes_actions::FocusMessage, window, cx| {
                    this.focus_message(window, cx)
                }),
            )
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
        let dir =
            std::env::temp_dir().join(format!("gitviber-changes-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
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
        let config =
            std::env::temp_dir().join(format!("gitviber-changes-config-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&config);
        // SAFETY: these tests point the config dir at a temporary folder, as gitviber-ui's do.
        unsafe { std::env::set_var("GITVIBER_CONFIG_DIR", &config) };
        cx.update(|cx| {
            gitviber_ui::init(cx);
            // Its events go nowhere: the app's would come in on the watcher's thread.
            let core = Arc::new(gitviber_core::Core::new(Arc::new(|_| {})));
            cx.set_global(crate::TestCore(core));
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
            p.discard_now(vec![], vec!["c.txt".into()], None, cx)
        });
        cx.run_until_parked();
        p.update(cx, |p, cx| p.read(cx));
        cx.run_until_parked();
        assert!(!dir.join("c.txt").exists());
        assert_eq!(p.read_with(cx, |p, _| p.lists.total()), 0);
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
        let dir =
            std::env::temp_dir().join(format!("gitviber-changes-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        git(&dir, &["init", "-q", "-b", "main"]);
        git(&dir, &["config", "user.name", "Test"]);
        git(&dir, &["config", "user.email", "test@example.com"]);
        std::fs::write(dir.join("a.txt"), "a\nb\nc\nd\ne\nf\ng\nh\ni\nj\n").unwrap();
        git(&dir, &["add", "."]);
        git(&dir, &["commit", "-q", "-m", "base"]);
        git(&dir, &["checkout", "-q", "-b", "other"]);
        std::fs::write(dir.join("a.txt"), theirs).unwrap();
        git(&dir, &["commit", "-q", "-am", "theirs"]);
        git(&dir, &["checkout", "-q", "main"]);
        std::fs::write(dir.join("a.txt"), ours).unwrap();
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
        let view = cx.update(|window, cx| {
            let view = cx.new(|cx| {
                crate::conflict::ConflictView::new("a.txt".into(), code.into(), window, cx)
            });
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
        let dir = std::env::temp_dir().join(format!("gitviber-changes-ud-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
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
}
