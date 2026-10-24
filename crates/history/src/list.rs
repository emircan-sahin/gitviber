//! A list of commits read a page at a time, with the graph built as pages arrive: HEAD's history,
//! every branch's, a search's matches, or one side of a comparison. Only the rows on screen are
//! laid out; an open commit's files are rows of their own, so a commit of thousands of files
//! scrolls like the rest.

use std::collections::HashSet;
use std::rc::Rc;

use gitviber_core::{Res, api, git};
use gitviber_refs::RepoState;
use gitviber_ui::memory::Live;
use gitviber_ui::widgets::{
    ChangeKind, FileIcon, LineCounts, PathLabel, StatusLetter, full_date, now_unix, relative_time,
};
use gitviber_ui::{ActiveAppTheme, AppTheme, Workspace};
use gpui_kit::assets::IconName;
use gpui_kit::component::menu::ContextMenuExt as _;
use gpui_kit::component::scroll::Scrollbar;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::{
    AnyElement, App, AppContext as _, Bounds, ClickEvent, Context, DragMoveEvent, Entity,
    EventEmitter, FocusHandle, Focusable, FontWeight, Hsla, InteractiveElement as _, IntoElement,
    KeyDownEvent, ListAlignment, ListOffset, ListState, MouseButton, ParentElement as _,
    PathBuilder, Pixels, Point, Render, SharedString, StatefulInteractiveElement as _, Styled as _,
    Task, WeakEntity, Window, canvas, div, fill, list, point, prelude::FluentBuilder as _, px,
    rems, size,
};

use crate::commit::{Badge, Commit, GraphRefs, RefKind, group_refs};
use crate::edits::{Where, first_parent_line};
use crate::graph::{GraphBuilder, GraphRow, Lane};
use crate::query::Filter;

/// Commits a page.
pub const PAGE: u32 = 200;
/// The key context the list's keys (`/` to search) listen in.
pub const CONTEXT: &str = "HistoryList";

/// What a list shows.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Source {
    /// HEAD's history.
    Head,
    /// Every branch's history as the refs let through; HEAD keeps the first lane.
    All(GraphRefs),
    /// A search's matches, with the commits its SHA-looking words name first.
    Search {
        filter: Filter,
        shas: Vec<String>,
        all: Option<GraphRefs>,
    },
    /// What `with` (a full ref) has that HEAD doesn't (`incoming`), or the other way round.
    Compare { with: String, incoming: bool },
    /// What `head` has that `base` doesn't (commit ids).
    Between { base: String, head: String },
}

impl Source {
    /// Branches and merges drawn; off for lists whose neighbours aren't parent and child.
    pub fn graph(&self) -> bool {
        matches!(self, Source::Head | Source::All(_))
    }

    fn read(&self, core: &gitviber_core::Core, skip: u32, limit: u32) -> Res<Vec<git::Commit>> {
        match self {
            Source::Head => api::history::log(core, None, skip, limit, None, None),
            Source::All(refs) => {
                api::history::log(core, None, skip, limit, None, Some(refs.to_core()))
            }
            Source::Search { filter, all, .. } => api::history::log(
                core,
                None,
                skip,
                limit,
                Some(filter.to_core()),
                all.as_ref().map(GraphRefs::to_core),
            ),
            Source::Compare { with, incoming } => {
                api::history::log_compare(core, with.clone(), *incoming, skip, limit)
            }
            Source::Between { base, head } => {
                api::history::log_between(core, base.clone(), head.clone(), skip, limit)
            }
        }
    }

    /// The commits a SHA in a search names, which go first.
    fn found(&self, core: &gitviber_core::Core) -> Vec<git::Commit> {
        match self {
            Source::Search { shas, .. } => shas
                .iter()
                .filter_map(|s| api::history::find_commit(core, s.clone()).ok().flatten())
                .collect(),
            _ => vec![],
        }
    }
}

/// What a list tells its owner.
#[derive(Clone, Debug)]
pub enum ListEvent {
    /// Narrow the search to this author.
    FilterAuthor {
        name: String,
        email: String,
    },
    /// Show what changed between two commits (`head` None: one and the working tree).
    ComparePoints {
        base: (String, String),
        head: Option<(String, String)>,
    },
    HideRefs(Vec<String>),
    OnlyRef(String),
    /// A page or a reload came in.
    Loaded,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Item {
    Commit(usize),
    Files,
    File(usize),
    /// Reading the open commit's files.
    FilesLoading,
    More,
}

struct Open {
    sha: String,
    files: Option<Rc<Vec<git::FileChange>>>,
    _read: Task<()>,
}

/// Open a commit's first file once its files are in (a file's history: that one), so one click
/// shows code.
const AUTO_OPEN: bool = true;

/// Where dragged commits would land.
#[derive(Clone, Debug, PartialEq, Eq)]
struct DropAt {
    sha: String,
    at: Where,
}

/// What a drag carries: the commits it moves.
#[derive(Clone)]
pub struct Dragged(pub Vec<String>);

pub struct DragLabel(SharedString);

impl Render for DragLabel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.app_theme();
        div()
            .px_2()
            .py_1()
            .rounded(px(6.))
            .border_1()
            .border_color(t.border)
            .bg(t.elevated)
            .text_size(rems(0.75))
            .shadow_md()
            .child(self.0.clone())
    }
}

/// How the list behaves, as its owner sets it up.
#[derive(Clone, Default)]
pub struct ListOptions {
    /// What an empty list says.
    pub empty: SharedString,
    /// HEAD, when the list isn't HEAD's own history (a search, every branch).
    pub head: Option<String>,
    /// Clicking an author narrows the search to them.
    pub filter_author: bool,
    /// The all-branches graph: badges offer Hide and Show only.
    pub ref_menu: bool,
    /// Only these refs' badges show.
    pub show_refs: Option<GraphRefs>,
}

pub struct CommitList {
    pub(crate) repo: Entity<RepoState>,
    pub(crate) workspace: WeakEntity<Workspace>,
    source: Source,
    pub(crate) options: ListOptions,
    pub(crate) commits: Vec<Rc<Commit>>,
    rows: Vec<GraphRow>,
    builder: GraphBuilder,
    has_more: bool,
    loaded: bool,
    error: Option<SharedString>,
    seq: u64,
    reading: Option<Task<()>>,
    paging: Option<Task<()>>,
    items: Vec<Item>,
    state: ListState,
    open: Option<Open>,
    /// Picked together with ⌘/⇧-click, newest first as listed.
    pub(crate) picked: Vec<String>,
    anchor: Option<String>,
    /// The row the keys move.
    cursor: Option<usize>,
    /// The row under the pointer: its branches come forward.
    lit: Option<usize>,
    drop_at: Option<DropAt>,
    dragging: Option<Vec<String>>,
    /// Scroll to this commit and open it once it's listed.
    jump: Option<String>,
    /// Loading pages until there's no more (a memory check, a jump far back).
    to_end: bool,
    focus: FocusHandle,
    pub(crate) flow: Option<Task<()>>,
    _repo: gpui_kit::Subscription,
    _live: Live,
}

impl EventEmitter<ListEvent> for CommitList {}

impl Focusable for CommitList {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

// Lane geometry, matching the dot: 12px row padding, 9px dot, its centre 13.5px down.
const LANE: f32 = 10.;
const LANE_X: f32 = 16.5;
const DOT_Y: f32 = 13.5;
const BEND_Y: f32 = DOT_Y + 10.;
// Past this many lanes the rest are cut off; their commits sit on the last one shown.
const MAX_LANES: usize = 8;

fn lane_of(i: usize) -> usize {
    i.min(MAX_LANES - 1)
}

fn lanes_of(row: &GraphRow) -> usize {
    row.width.min(MAX_LANES)
}

/// Colour follows the branch, not the column. The list's own branch is grey like its dots.
pub fn lane_color(id: usize, t: &AppTheme) -> Hsla {
    if id == 0 {
        return t.subtle;
    }
    [t.renamed, t.modified, t.primary, t.conflict, t.added][(id - 1) % 5]
}

impl CommitList {
    pub fn new(
        repo: Entity<RepoState>,
        workspace: WeakEntity<Workspace>,
        source: Source,
        options: ListOptions,
        cx: &mut Context<Self>,
    ) -> Self {
        let state = ListState::new(0, ListAlignment::Top, px(300.));
        let weak = cx.weak_entity();
        state.set_scroll_handler(move |e, _, cx| {
            // Near the end: the next page, before the user gets there.
            if e.visible_range.end + 20 >= e.count {
                let _ = weak.update(cx, |this, cx| this.load_more(cx));
            }
        });
        let sub = cx.subscribe(
            &repo,
            |this: &mut Self, _, event: &gitviber_refs::RepoEvent, cx| {
                if let gitviber_refs::RepoEvent::Changed = event {
                    this.repo_changed(cx);
                }
            },
        );
        let mut list = CommitList {
            repo,
            workspace,
            source,
            options,
            commits: vec![],
            rows: vec![],
            builder: GraphBuilder::default(),
            has_more: false,
            loaded: false,
            error: None,
            seq: 0,
            reading: None,
            paging: None,
            items: vec![],
            state,
            open: None,
            picked: vec![],
            anchor: None,
            cursor: None,
            lit: None,
            drop_at: None,
            dragging: None,
            jump: None,
            to_end: false,
            focus: cx.focus_handle(),
            flow: None,
            _repo: sub,
            _live: Live::new("CommitList"),
        };
        // Before the project opens core has no repo: its first read comes with RepoEvent::Changed.
        if list.repo.read(cx).status.is_some() {
            list.reload(cx);
        }
        list
    }

    /// Reads page after page until the history ends.
    pub fn load_to_end(&mut self, cx: &mut Context<Self>) {
        self.to_end = true;
        self.load_more(cx);
    }

    /// The first commit's tab of all its files.
    pub fn open_first_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(c) = self.commits.first().cloned() {
            crate::open::open_commit(self, &c, window, cx);
        }
    }

    /// Opens the first commit as a click does, its first file's tab too.
    pub fn open_first(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(c) = self.commits.first().cloned() {
            self.cursor = Some(0);
            self.toggle_open(&c, Some(window), cx);
        }
    }

    pub fn source(&self) -> &Source {
        &self.source
    }

    pub fn is_loaded(&self) -> bool {
        self.loaded
    }

    pub fn len(&self) -> usize {
        self.commits.len()
    }

    pub fn is_empty(&self) -> bool {
        self.commits.is_empty()
    }

    pub fn has_more(&self) -> bool {
        self.has_more
    }

    /// HEAD as the history shows it: core refuses to move HEAD if it moved since.
    pub fn head(&self, cx: &App) -> String {
        self.options
            .head
            .clone()
            .or_else(|| self.repo.read(cx).head().map(str::to_string))
            .unwrap_or_default()
    }

    pub fn commit(&self, sha: &str) -> Option<&Rc<Commit>> {
        self.commits.iter().find(|c| c.sha == sha)
    }

    /// Shows another list in place of this one: what's loaded goes.
    pub fn set_source(&mut self, source: Source, options: ListOptions, cx: &mut Context<Self>) {
        if source == self.source && options.show_refs == self.options.show_refs {
            self.options = options;
            return;
        }
        self.source = source;
        self.options = options;
        self.commits.clear();
        self.rows.clear();
        self.open = None;
        self.picked.clear();
        self.loaded = false;
        self.state.reset(0);
        self.items.clear();
        self.reload(cx);
    }

    /// HEAD or the refs may have moved: read again what's loaded.
    fn repo_changed(&mut self, cx: &mut Context<Self>) {
        let tips = self.repo.read(cx).head().map(str::to_string);
        // A list of fixed commits never changes.
        if matches!(self.source, Source::Between { .. }) {
            return;
        }
        let _ = tips;
        self.reload(cx);
    }

    /// Reads the list again, as many as are loaded; what's on screen stays where it was.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        self.seq += 1;
        let seq = self.seq;
        let limit = PAGE.max(self.commits.len() as u32);
        let source = self.source.clone();
        let core = gitviber_ui::core(cx);
        self.paging = None;
        let read =
            cx.background_spawn(async move { (source.read(&core, 0, limit), source.found(&core)) });
        self.reading = Some(cx.spawn(async move |this, cx| {
            let (log, found) = read.await;
            let _ = this.update(cx, |this, cx| {
                if this.seq != seq {
                    return;
                }
                this.reading = None;
                match log {
                    Ok(log) => {
                        let has_more = log.len() as u32 == limit;
                        this.replace(found, log, has_more, cx);
                        this.error = None;
                    }
                    Err(e) => {
                        this.error = Some(e.into());
                        if !this.loaded {
                            this.replace(vec![], vec![], false, cx);
                        }
                    }
                }
                this.loaded = true;
                cx.emit(ListEvent::Loaded);
                cx.notify();
                if this.to_end {
                    this.load_more(cx);
                }
            });
        }));
    }

    fn graph_head(&self, cx: &App) -> Option<String> {
        // Every branch is listed: HEAD's keeps the first lane even below newer branches' tips.
        match &self.source {
            Source::All(_) => self.repo.read(cx).head().map(str::to_string),
            _ => None,
        }
    }

    fn replace(
        &mut self,
        found: Vec<git::Commit>,
        log: Vec<git::Commit>,
        has_more: bool,
        cx: &mut Context<Self>,
    ) {
        let mut seen = HashSet::new();
        let next: Vec<Rc<Commit>> = found
            .into_iter()
            .chain(log)
            .filter(|c| seen.insert(c.sha.clone()))
            .map(|c| Rc::new(Commit::from(c)))
            .collect();
        // A refresh that moved nothing shown (a staged file, a focus) leaves the list alone.
        if self.loaded
            && next.len() == self.commits.len()
            && next.iter().zip(&self.commits).all(|(a, b)| **a == **b)
        {
            self.has_more = has_more;
            return;
        }
        let top = self.state.logical_scroll_top();
        let top_sha = self.item_sha(top.item_ix);
        self.commits = next;
        let listed: HashSet<&str> = self.commits.iter().map(|c| c.sha.as_str()).collect();
        // An undo can bring rewritten commits back: their old picks don't come with them.
        self.picked.retain(|s| listed.contains(s.as_str()));
        if self
            .open
            .as_ref()
            .is_some_and(|o| !listed.contains(o.sha.as_str()))
        {
            self.open = None;
        }
        self.has_more = has_more;
        self.builder = GraphBuilder::new(
            self.graph_head(cx)
                .filter(|h| listed.contains(h.as_str()))
                .as_deref(),
        );
        self.rows.clear();
        if self.source.graph() {
            let mut builder = std::mem::take(&mut self.builder);
            self.rows = self
                .commits
                .iter()
                .map(|c| builder.push(&c.sha, &c.parents))
                .collect();
            self.builder = builder;
        }
        let old = self.items.len();
        self.rebuild_items();
        self.state.splice(0..old, self.items.len());
        if let Some(sha) = top_sha
            && let Some(ix) = self
                .items
                .iter()
                .position(|i| matches!(i, Item::Commit(c) if self.commits[*c].sha == sha))
        {
            self.state.scroll_to(ListOffset {
                item_ix: ix,
                offset_in_item: top.offset_in_item,
            });
        }
        self.try_jump(cx);
    }

    fn item_sha(&self, ix: usize) -> Option<String> {
        match self.items.get(ix)? {
            Item::Commit(c) => Some(self.commits[*c].sha.clone()),
            _ => None,
        }
    }

    fn rebuild_items(&mut self) {
        let mut items = Vec::with_capacity(self.commits.len() + 1);
        for (i, c) in self.commits.iter().enumerate() {
            items.push(Item::Commit(i));
            if let Some(open) = self.open.as_ref().filter(|o| o.sha == c.sha) {
                match &open.files {
                    None => items.push(Item::FilesLoading),
                    Some(files) => {
                        items.push(Item::Files);
                        items.extend((0..files.len()).map(Item::File));
                    }
                }
            }
        }
        if self.has_more {
            items.push(Item::More);
        }
        self.items = items;
    }

    /// The next page, unless one is on its way or there's no more.
    pub fn load_more(&mut self, cx: &mut Context<Self>) {
        if !self.has_more || self.paging.is_some() || self.reading.is_some() {
            return;
        }
        let seq = self.seq;
        let skip = self.commits.len() as u32;
        let source = self.source.clone();
        let core = gitviber_ui::core(cx);
        let read = cx.background_spawn(async move { source.read(&core, skip, PAGE) });
        self.paging = Some(cx.spawn(async move |this, cx| {
            let more = read.await;
            let _ = this.update(cx, |this, cx| {
                this.paging = None;
                if this.seq != seq {
                    return;
                }
                match more {
                    Ok(more) => this.append(more, cx),
                    Err(e) => this.error = Some(e.into()),
                }
                cx.emit(ListEvent::Loaded);
                cx.notify();
            });
        }));
    }

    fn append(&mut self, more: Vec<git::Commit>, cx: &mut Context<Self>) {
        self.has_more = more.len() as u32 == PAGE;
        // A refresh may have shifted the page boundary meanwhile; never show a commit twice.
        let seen: HashSet<&str> = self.commits.iter().map(|c| c.sha.as_str()).collect();
        let fresh: Vec<Rc<Commit>> = more
            .into_iter()
            .filter(|c| !seen.contains(c.sha.as_str()))
            .map(|c| Rc::new(Commit::from(c)))
            .collect();
        let at = self.items.len() - usize::from(self.items.last() == Some(&Item::More));
        let first = self.commits.len();
        if self.source.graph() {
            for c in &fresh {
                self.rows.push(self.builder.push(&c.sha, &c.parents));
            }
        }
        self.commits.extend(fresh);
        let mut tail: Vec<Item> = (first..self.commits.len()).map(Item::Commit).collect();
        if self.has_more {
            tail.push(Item::More);
        }
        let old = self.items.len() - at;
        let count = tail.len();
        self.items.truncate(at);
        self.items.extend(tail);
        self.state.splice(at..at + old, count);
        self.try_jump(cx);
        if self.to_end {
            self.load_more(cx);
        }
    }

    /// Scrolls to a commit and opens it, loading pages until it's listed, up to a point.
    pub fn jump_to(&mut self, sha: String, cx: &mut Context<Self>) {
        self.jump = Some(sha);
        self.try_jump(cx);
    }

    fn try_jump(&mut self, cx: &mut Context<Self>) {
        let Some(sha) = self.jump.clone() else { return };
        if let Some(i) = self.commits.iter().position(|c| c.sha == sha) {
            self.jump = None;
            let commit = self.commits[i].clone();
            if self.open.as_ref().is_none_or(|o| o.sha != sha) {
                self.toggle_open(&commit, None, cx);
            }
            if let Some(ix) = self.items.iter().position(|it| *it == Item::Commit(i)) {
                self.state.scroll_to_reveal_item(ix);
            }
            cx.notify();
        } else if self.has_more && self.commits.len() < 20 * PAGE as usize {
            self.load_more(cx);
        } else {
            self.jump = None;
        }
    }

    // MARK: Open, pick

    pub fn open_sha(&self) -> Option<&str> {
        self.open.as_ref().map(|o| o.sha.as_str())
    }

    /// Opens a commit's files under it, or closes it; with `window`, its first file's tab too.
    fn toggle_open(
        &mut self,
        commit: &Rc<Commit>,
        window: Option<&mut Window>,
        cx: &mut Context<Self>,
    ) {
        if self.open.as_ref().is_some_and(|o| o.sha == commit.sha) {
            self.open = None;
        } else {
            let core = gitviber_ui::core(cx);
            let sha = commit.sha.clone();
            let read = cx.background_spawn({
                let sha = sha.clone();
                async move { api::history::commit_files(&core, sha) }
            });
            // From a click, the first file opens too, in the window clicked.
            let handle = window.filter(|_| AUTO_OPEN).map(|w| w.window_handle());
            let commit2 = commit.clone();
            let task = cx.spawn(async move |this, cx| {
                let files = read.await;
                let _ = this.update(cx, |this, cx| {
                    let Some(open) = this.open.as_mut().filter(|o| o.sha == sha) else {
                        return;
                    };
                    let files = Rc::new(files.unwrap_or_default());
                    open.files = Some(files.clone());
                    this.relayout(cx);
                    let file = files
                        .iter()
                        .find(|f| Some(&f.path) == commit2.file.as_ref())
                        .or(files.first())
                        .cloned();
                    if let (Some(handle), Some(f)) = (handle, file) {
                        let this = cx.weak_entity();
                        cx.defer(move |cx| {
                            let _ = handle.update(cx, |_, window, cx| {
                                let _ = this.update(cx, |this, cx| {
                                    crate::open::open_commit_file(
                                        this, &commit2, &f, true, window, cx,
                                    )
                                });
                            });
                        });
                    }
                });
            });
            self.open = Some(Open {
                sha: commit.sha.clone(),
                files: None,
                _read: task,
            });
        }
        self.relayout(cx);
    }

    /// The items changed around the open commit: laid out again, the scroll kept on its row.
    fn relayout(&mut self, cx: &mut Context<Self>) {
        let top = self.state.logical_scroll_top();
        let top_sha = self.item_sha(top.item_ix);
        let old = self.items.len();
        self.rebuild_items();
        self.state.splice(0..old, self.items.len());
        if let Some(sha) = top_sha
            && let Some(ix) = self
                .items
                .iter()
                .position(|i| matches!(i, Item::Commit(c) if self.commits[*c].sha == sha))
        {
            self.state.scroll_to(ListOffset {
                item_ix: ix,
                offset_in_item: top.offset_in_item,
            });
        }
        cx.notify();
    }

    fn files_of_open(&self) -> Option<(Rc<Commit>, Rc<Vec<git::FileChange>>)> {
        let open = self.open.as_ref()?;
        Some((self.commit(&open.sha)?.clone(), open.files.clone()?))
    }

    /// A plain click opens the commit; ⌘ picks one more, ⇧ the range from the last clicked,
    /// along commits on the same side of HEAD.
    fn click_commit(
        &mut self,
        i: usize,
        e: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let commit = self.commits[i].clone();
        self.cursor = Some(i);
        let m = e.modifiers();
        if m.secondary() {
            if self.picked.is_empty()
                && let Some(open) = self.open_sha()
            {
                self.picked.push(open.to_string());
            }
            match self.picked.iter().position(|s| *s == commit.sha) {
                Some(at) => {
                    self.picked.remove(at);
                }
                None => self.picked.push(commit.sha.clone()),
            }
            self.order_picked();
            self.anchor = Some(commit.sha.clone());
        } else if m.shift {
            self.pick_range(i);
        } else {
            self.picked.clear();
            self.anchor = Some(commit.sha.clone());
            self.toggle_open(&commit, Some(window), cx);
        }
        cx.notify();
    }

    fn order_picked(&mut self) {
        let picked: HashSet<String> = self.picked.drain(..).collect();
        self.picked = self
            .commits
            .iter()
            .filter(|c| picked.contains(&c.sha))
            .map(|c| c.sha.clone())
            .collect();
    }

    fn pick_range(&mut self, to: usize) {
        let from = self
            .anchor
            .clone()
            .or_else(|| self.open_sha().map(str::to_string))
            .and_then(|a| self.commits.iter().position(|c| c.sha == a))
            .unwrap_or(to);
        let side = self.commits[to].not_in_head;
        let (lo, hi) = (from.min(to), from.max(to));
        self.anchor = Some(self.commits[from].sha.clone());
        self.picked = self.commits[lo..=hi]
            .iter()
            .filter(|c| c.not_in_head == side)
            .map(|c| c.sha.clone())
            .collect();
    }

    /// The picked commits as listed; one when nothing's picked but `sha`.
    pub fn picked_commits(&self) -> Vec<Rc<Commit>> {
        let set: HashSet<&str> = self.picked.iter().map(String::as_str).collect();
        self.commits
            .iter()
            .filter(|c| set.contains(c.sha.as_str()))
            .cloned()
            .collect()
    }

    fn key_down(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let rows: Vec<usize> = (0..self.commits.len()).collect();
        if rows.is_empty() {
            return;
        }
        let k = e.keystroke.key.as_str();
        let shift = e.keystroke.modifiers.shift;
        let at = self.cursor.unwrap_or(0);
        let to = match k {
            "down" => (at + 1).min(rows.len() - 1),
            "up" => at.saturating_sub(1),
            "enter" => {
                let commit = self.commits[at].clone();
                self.toggle_open(&commit, Some(window), cx);
                cx.stop_propagation();
                return;
            }
            "escape" if !self.picked.is_empty() => {
                self.picked.clear();
                cx.stop_propagation();
                cx.notify();
                return;
            }
            _ => return,
        };
        cx.stop_propagation();
        if shift {
            if self.picked.is_empty() {
                self.anchor = Some(self.commits[at].sha.clone());
            }
            self.pick_range(to);
        } else {
            self.picked.clear();
            self.anchor = Some(self.commits[to].sha.clone());
        }
        self.cursor = Some(to);
        if let Some(ix) = self.items.iter().position(|it| *it == Item::Commit(to)) {
            self.state.scroll_to_reveal_item(ix);
        }
        cx.notify();
    }

    // MARK: Drag

    /// Only the branch's own history as the graph draws it, with nothing running.
    fn can_drag(&self, cx: &App) -> bool {
        matches!(self.source, Source::Head) && !self.repo.read(cx).locked()
    }

    fn line(&self, cx: &App) -> HashSet<String> {
        first_parent_line(&self.commits, &self.head(cx))
    }

    fn drag_move(&mut self, i: usize, e: &DragMoveEvent<Dragged>, cx: &mut Context<Self>) {
        let dragged = e.drag(cx).0.clone();
        let sha = self.commits[i].sha.clone();
        let target = (!dragged.contains(&sha) && self.line(cx).contains(&sha)).then(|| {
            let (y, top, h) = (e.event.position.y, e.bounds.origin.y, e.bounds.size.height);
            let at = if y < top + h / 4. {
                Where::Above
            } else if y > top + h - h / 4. {
                Where::Below
            } else {
                Where::Onto
            };
            DropAt { sha, at }
        });
        if target != self.drop_at {
            self.drop_at = target;
            self.dragging = Some(dragged);
            cx.notify();
        }
    }

    fn drop(&mut self, dragged: &Dragged, window: &mut Window, cx: &mut Context<Self>) {
        let target = self.drop_at.take();
        self.dragging = None;
        cx.notify();
        if let Some(target) = target {
            crate::act::drop_commits(self, dragged.0.clone(), target.sha, target.at, window, cx);
        }
    }

    // MARK: Render

    fn render_item(
        &mut self,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match self.items.get(ix).copied() {
            Some(Item::Commit(i)) => self.render_commit(i, window, cx),
            Some(Item::Files) => self.render_files_header(cx),
            Some(Item::File(f)) => self.render_file(f, cx),
            Some(Item::FilesLoading) => {
                let t = cx.app_theme();
                div()
                    .py_1()
                    .pl_8()
                    .text_size(rems(0.71875))
                    .text_color(t.subtle)
                    .child("Loading…")
                    .into_any_element()
            }
            Some(Item::More) => {
                let t = cx.app_theme();
                div()
                    .p_2()
                    .text_center()
                    .text_size(rems(0.71875))
                    .text_color(t.subtle)
                    .child("Loading more…")
                    .into_any_element()
            }
            None => div().into_any_element(),
        }
    }

    fn row_graph(&self, i: usize) -> GraphRow {
        if self.source.graph() {
            self.rows.get(i).cloned().unwrap_or_default()
        } else {
            GraphRow::straight(i == 0, i + 1 == self.commits.len() && !self.has_more)
        }
    }

    fn render_commit(
        &mut self,
        i: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = cx.app_theme().clone();
        let commit = self.commits[i].clone();
        let row = self.row_graph(i);
        let head = self.head(cx);
        let is_head = commit.sha == head;
        let open = self.open_sha() == Some(commit.sha.as_str());
        let picked = self.picked.iter().any(|s| *s == commit.sha);
        let cursor = self.cursor == Some(i) && self.focus.is_focused(window);
        let lit: Option<Vec<usize>> = self.lit.map(|l| {
            let r = self.row_graph(l);
            std::iter::once(r.id)
                .chain(r.out.iter().map(|o| o.id))
                .collect()
        });
        let drop = self
            .drop_at
            .as_ref()
            .filter(|d| d.sha == commit.sha)
            .map(|d| d.at);
        let dragged = self
            .dragging
            .as_ref()
            .is_some_and(|d| d.contains(&commit.sha));
        let lanes = lanes_of(&row);
        let mark = if commit.unpushed {
            Some("Not pushed yet")
        } else if commit.not_in_head {
            Some("Not in your branch yet")
        } else {
            None
        };
        let dot_tip: Option<SharedString> = if is_head {
            Some(
                std::iter::once("HEAD")
                    .chain(mark)
                    .collect::<Vec<_>>()
                    .join(" · ")
                    .into(),
            )
        } else {
            mark.map(SharedString::from)
        };
        let (dot_border, dot_bg) = if commit.unpushed {
            (t.primary, t.primary)
        } else if commit.not_in_head {
            (t.added, t.added)
        } else if row.id == 0 {
            (t.subtle, t.sidebar)
        } else {
            (lane_color(row.id, &t), t.sidebar)
        };
        let dot = div()
            .id(("dot", i))
            .relative()
            .flex_none()
            .mt(px(3.))
            .ml(px(lane_of(row.col) as f32 * LANE))
            .mr(px((lanes - 1 - lane_of(row.col)) as f32 * LANE))
            .size(px(9.))
            .rounded_full()
            .border_2()
            .border_color(dot_border)
            .bg(dot_bg)
            // Where you are, among every branch's commits.
            .when(is_head, |d| {
                d.child(
                    div()
                        .absolute()
                        .top(px(-4.))
                        .left(px(-4.))
                        .size(px(13.))
                        .rounded_full()
                        .border_1()
                        .border_color(t.primary),
                )
            })
            .when_some(dot_tip, |d, tip| {
                d.tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
            });
        let badges = group_refs(
            &commit.refs,
            &self.remotes(cx),
            self.options.show_refs.as_ref(),
        );
        let author: AnyElement = if self.options.filter_author && !commit.author.is_empty() {
            let (name, email) = (commit.author.to_string(), commit.email.to_string());
            div()
                .id(("author", i))
                .min_w_0()
                .truncate()
                .cursor_pointer()
                .hover(|s| s.text_color(t.foreground).underline())
                .child(commit.author.clone())
                .tooltip({
                    let tip: SharedString = format!("Filter by author {}", commit.author).into();
                    move |window, cx| Tooltip::new(tip.clone()).build(window, cx)
                })
                .on_click(cx.listener(move |_, _, _, cx| {
                    cx.stop_propagation();
                    cx.emit(ListEvent::FilterAuthor {
                        name: name.clone(),
                        email: email.clone(),
                    });
                }))
                .into_any_element()
        } else {
            div()
                .min_w_0()
                .truncate()
                .child(commit.author.clone())
                .into_any_element()
        };
        let now = now_unix();
        let committed = relative_time(commit.committed_at, now);
        let authored = relative_time(commit.timestamp, now);
        let moved = committed != authored;
        let time_tip: Option<SharedString> = moved.then(|| {
            format!(
                "Committed {committed} by {} ({})\nAuthored {authored} by {} ({})",
                commit.committer,
                full_date(commit.committed_at),
                commit.author,
                full_date(commit.timestamp)
            )
            .into()
        });
        let time = div()
            .id(("time", i))
            .flex_none()
            .when(moved, |d| d.underline())
            .child(committed)
            .when_some(time_tip, |d, tip| {
                d.tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
            });
        let bg = if drop == Some(Where::Onto) || picked {
            Some(t.primary.opacity(0.15))
        } else if open {
            Some(t.active)
        } else {
            None
        };
        let draggable = self.can_drag(cx) && self.line(cx).contains(&commit.sha);
        let sha = commit.sha.clone();
        let list = cx.weak_entity();
        let menu_commit = commit.clone();
        let mut el = div()
            .id(("commit", i))
            .relative()
            .w_full()
            .overflow_hidden()
            .flex()
            .items_start()
            .gap(px(10.))
            .py(px(6.))
            .pr_2()
            .pl(px(12.))
            .cursor_pointer()
            .when_some(bg, |d, bg| d.bg(bg))
            .when(bg.is_none(), |d| d.hover(|s| s.bg(t.hover)))
            .when(cursor, |d| d.border_1().border_color(t.ring))
            .when(dragged, |d| d.opacity(0.5))
            .when(drop == Some(Where::Onto), |d| {
                d.border_1().border_color(t.primary)
            })
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                let lit = hovered.then_some(i);
                if this.lit != lit && (lit.is_some() || this.lit == Some(i)) {
                    this.lit = lit;
                    cx.notify();
                }
            }))
            .on_click(cx.listener(move |this, e: &ClickEvent, window, cx| {
                this.click_commit(i, e, window, cx)
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, _, _, cx| {
                    // A right-click off the picked commits lets them go: its menu is that commit's.
                    let sha = &this.commits[i].sha;
                    if !this.picked.iter().any(|s| s == sha) {
                        this.picked.clear();
                        cx.notify();
                    }
                }),
            )
            .child(graph_lines(row.clone(), lit, &t))
            .child(dot)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .truncate()
                            .text_size(rems(0.75))
                            .line_height(px(16.))
                            .text_color(if open {
                                t.foreground
                            } else {
                                t.foreground.opacity(0.9)
                            })
                            .when(open, |d| d.font_weight(FontWeight::MEDIUM))
                            .child(commit.subject.clone()),
                    )
                    .child(
                        div()
                            .mt(px(2.))
                            .flex()
                            .min_w_0()
                            .items_center()
                            .gap(px(6.))
                            .text_size(rems(0.65625))
                            .text_color(t.subtle)
                            .child(author)
                            .child("·")
                            .child(time)
                            .child(
                                div().ml_auto().flex_none().children(
                                    commit
                                        .on_origin
                                        .then(|| {
                                            crate::commit_checks(&commit.sha, false, window, cx)
                                        })
                                        .flatten(),
                                ),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .font_family(t.fonts.code.clone())
                                    .child(commit.short.clone()),
                            ),
                    )
                    .when(!badges.is_empty(), |d| d.child(render_badges(&badges, &t))),
            )
            .when(drop == Some(Where::Above), |d| {
                d.child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .top(px(-1.))
                        .h(px(2.))
                        .bg(t.primary),
                )
            })
            .when(drop == Some(Where::Below), |d| {
                d.child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .bottom(px(-1.))
                        .h(px(2.))
                        .bg(t.primary),
                )
            });
        if draggable {
            // A drag takes the picked commits it starts on, else its own.
            let shas = if self.picked.len() > 1 && picked {
                self.picked.clone()
            } else {
                vec![sha.clone()]
            };
            let label_n = shas.len();
            el = el
                .on_drag(Dragged(shas), move |_, _, _, cx| {
                    cx.new(|_| {
                        DragLabel(if label_n == 1 {
                            "Move 1 commit".into()
                        } else {
                            format!("Move {label_n} commits").into()
                        })
                    })
                })
                .on_drag_move(cx.listener(move |this, e: &DragMoveEvent<Dragged>, _, cx| {
                    this.drag_move(i, e, cx)
                }))
                .on_drop(cx.listener(|this, d: &Dragged, window, cx| this.drop(d, window, cx)));
        }
        el.context_menu(move |menu, window, cx| {
            let Some(list) = list.upgrade() else {
                return menu;
            };
            crate::menu::commit_menu(menu, &list, &menu_commit, window, cx)
        })
        .into_any_element()
    }

    pub(crate) fn remotes(&self, cx: &App) -> Vec<String> {
        self.repo
            .read(cx)
            .branches
            .iter()
            .filter(|b| b.remote)
            .map(|b| b.name.clone())
            .collect()
    }

    fn render_files_header(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.app_theme().clone();
        let Some((commit, files)) = self.files_of_open() else {
            return div().into_any_element();
        };
        let (add, del) = files.iter().fold((0, 0), |(a, d), f| {
            (a + f.additions.unwrap_or(0), d + f.deletions.unwrap_or(0))
        });
        div()
            .w_full()
            .flex()
            .h(px(24.))
            .items_center()
            .gap_2()
            .pr_2()
            .pl_8()
            .border_t_1()
            .border_color(t.border)
            .text_size(rems(0.65625))
            .text_color(t.subtle)
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("CHANGED FILES"),
            )
            .child(
                div()
                    .font_family(t.fonts.code.clone())
                    .text_color(t.muted_foreground)
                    .child(files.len().to_string()),
            )
            .when(!files.is_empty(), |d| {
                d.child(
                    div()
                        .id("open-all")
                        .px_1()
                        .rounded(px(3.))
                        .text_color(t.primary)
                        .cursor_pointer()
                        .hover(|s| s.bg(t.hover))
                        .child("Open All")
                        .on_click(cx.listener(move |this, _, window, cx| {
                            crate::open::open_commit(this, &commit, window, cx)
                        })),
                )
            })
            .child(
                div()
                    .ml_auto()
                    .flex()
                    .gap_1()
                    .font_family(t.fonts.code.clone())
                    .child(div().text_color(t.added).child(format!("+{add}")))
                    .child(div().text_color(t.removed).child(format!("-{del}"))),
            )
            .into_any_element()
    }

    fn render_file(&mut self, f: usize, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.app_theme().clone();
        let Some((commit, files)) = self.files_of_open() else {
            return div().into_any_element();
        };
        let file = files[f].clone();
        let last = f + 1 == files.len();
        let menu_file = file.clone();
        let menu_commit = commit.clone();
        let list = cx.weak_entity();
        div()
            .id(("file", f))
            .w_full()
            .flex()
            .h(px(26.))
            .items_center()
            .gap_2()
            .pr_2()
            .pl_8()
            .text_size(rems(0.75))
            .cursor_pointer()
            .hover(|s| s.bg(t.hover))
            .when(last, |d| d.border_b_1().border_color(t.border))
            .child(FileIcon::file(&file.path))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(PathLabel(file.path.clone().into())),
            )
            .child(LineCounts::new(file.additions, file.deletions))
            .child(StatusLetter(ChangeKind::from_letter(
                file.status.chars().next().unwrap_or('M'),
            )))
            .on_click(cx.listener(move |this, e: &ClickEvent, window, cx| {
                crate::open::open_commit_file(
                    this,
                    &commit,
                    &file,
                    e.click_count() < 2,
                    window,
                    cx,
                );
            }))
            .context_menu(move |menu, window, cx| {
                let Some(list) = list.upgrade() else {
                    return menu;
                };
                crate::menu::file_menu(menu, &list, &menu_commit, &menu_file, window, cx)
            })
            .into_any_element()
    }
}

/// The colored lines of one row: lanes passing by, ending at the dot, and leaving it for its
/// parents. `lit`: the hovered row's branches, which stay bright while the others dim.
fn graph_lines(row: GraphRow, lit: Option<Vec<usize>>, t: &AppTheme) -> impl IntoElement {
    let colors: Vec<Hsla> = (0..=row
        .through
        .iter()
        .chain(&row.into)
        .chain(&row.out)
        .map(|l| l.id)
        .max()
        .unwrap_or(0)
        .max(row.id))
        .map(|id| {
            let c = lane_color(id, t);
            if lit.as_ref().is_some_and(|l| !l.contains(&id)) {
                c.opacity(0.25)
            } else {
                c
            }
        })
        .collect();
    let width = LANE_X + (lanes_of(&row) as f32 - 1.) * LANE + LANE / 2.;
    canvas(
        |_, _, _| {},
        move |bounds: Bounds<Pixels>, _, window, _| {
            let x0 = bounds.origin.x;
            let top = bounds.origin.y;
            let bottom = top + bounds.size.height;
            let lx = |col: usize| x0 + px(LANE_X + lane_of(col) as f32 * LANE);
            let x = lx(row.col);
            let shown = |l: &Lane| l.col < MAX_LANES || l.col == row.col;
            let color = |l: &Lane| colors.get(l.id).copied().unwrap_or(colors[0]);
            let vline = |window: &mut Window, at: Pixels, from: Pixels, to: Pixels, c: Hsla| {
                window.paint_quad(fill(
                    Bounds::new(point(at - px(0.5), from), size(px(1.), to - from)),
                    c,
                ));
            };
            let curve = |window: &mut Window,
                         from: Point<Pixels>,
                         c1: Point<Pixels>,
                         c2: Point<Pixels>,
                         to: Point<Pixels>,
                         c: Hsla| {
                let mut path = PathBuilder::stroke(px(1.));
                path.move_to(from);
                path.cubic_bezier_to(to, c1, c2);
                if let Ok(p) = path.build() {
                    window.paint_path(p, c);
                }
            };
            let dot = top + px(DOT_Y);
            let bend = top + px(BEND_Y);
            for l in row.through.iter().filter(|l| shown(l)) {
                vline(window, lx(l.col), top, bottom, color(l));
            }
            for l in row.into.iter().filter(|l| shown(l)) {
                if l.col == row.col {
                    vline(window, x, top, dot, color(l));
                } else {
                    let from = point(lx(l.col), top);
                    curve(
                        window,
                        from,
                        point(lx(l.col), dot),
                        point(x, top),
                        point(x, dot),
                        color(l),
                    );
                }
            }
            for l in row.out.iter().filter(|l| shown(l)) {
                if l.col == row.col {
                    vline(window, x, dot, bottom, color(l));
                } else {
                    curve(
                        window,
                        point(x, dot),
                        point(x, bend),
                        point(lx(l.col), dot),
                        point(lx(l.col), bend),
                        color(l),
                    );
                    vline(window, lx(l.col), bend, bottom, color(l));
                }
            }
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .h_full()
    .w(px(width))
}

fn render_badges(badges: &[Badge], t: &AppTheme) -> impl IntoElement {
    div()
        .mt_1()
        .flex()
        .flex_wrap()
        .gap_1()
        .children(badges.iter().map(|b| {
            let (bg, fg, border) = match b.kind {
                RefKind::Head => (Some(t.primary), t.primary_foreground, None),
                RefKind::Local => (Some(t.renamed.opacity(0.15)), t.renamed, None),
                RefKind::Remote => (None, t.muted_foreground, Some(t.border_strong)),
                RefKind::Tag => (Some(t.modified.opacity(0.15)), t.modified, None),
            };
            let icon = match b.kind {
                RefKind::Tag => Some(IconName::Tag),
                RefKind::Remote => Some(IconName::Cloud),
                _ => None,
            };
            div()
                .flex()
                .max_w_full()
                .items_center()
                .gap_1()
                .rounded(px(3.))
                .px(px(6.))
                .font_family(t.fonts.code.clone())
                .text_size(rems(0.625))
                .line_height(px(18.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(fg)
                .when_some(bg, |d, bg| d.bg(bg))
                .when_some(border, |d, c| d.border_1().border_color(c))
                .when_some(icon, |d, i| d.child(Icon::new(i).xsmall()))
                .child(div().truncate().child(b.name.clone()))
                .when(b.synced, |d| {
                    d.child(Icon::new(IconName::Cloud).xsmall().opacity(0.75))
                })
        }))
}

impl Render for CommitList {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.app_theme().clone();
        if self.commits.is_empty() {
            let text: SharedString = match (&self.error, self.loaded) {
                (Some(e), _) => e.clone(),
                (None, true) => self.options.empty.clone(),
                (None, false) => "Loading…".into(),
            };
            return div()
                .size_full()
                .track_focus(&self.focus)
                .px_6()
                .pt(px(80.))
                .text_center()
                .text_size(rems(0.75))
                .text_color(t.subtle)
                .child(text)
                .into_any_element();
        }
        let render = cx.processor(|this, ix: usize, window, cx| this.render_item(ix, window, cx));
        div()
            .id("commit-list")
            .relative()
            .size_full()
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .on_key_down(
                cx.listener(|this, e: &KeyDownEvent, window, cx| this.key_down(e, window, cx)),
            )
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                if !hovered && this.lit.is_some() {
                    this.lit = None;
                    cx.notify();
                }
            }))
            .child(
                list(self.state.clone(), move |ix, window, cx| {
                    render(ix, window, cx)
                })
                .size_full()
                .py_1(),
            )
            .child(Scrollbar::vertical(&self.state))
            .into_any_element()
    }
}
