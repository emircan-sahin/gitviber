//! The contract feature crates plug into. A window's `Workspace` holds the open project, the
//! center's tabs and which panels show; the app draws it. A feature:
//!
//! - registers its panels and bar items once at startup (`register_panel`, `register_bar_item`,
//!   `register_quick_open`), built per project the first time they show;
//! - registers an opener per `Selection` kind it shows (`register_opener`), and opens tabs with
//!   `Workspace::open(selection, preview, ..)`: an open one is shown rather than built again,
//!   a preview open reuses the preview tab, and each worktree's tabs are saved as selections;
//! - toasts with `Workspace::toast`, shows or focuses panels by id, toggles the terminal;
//! - follows the project's `ProjectEvent::RepoChanged` to refresh.
//!
//! Get the window's workspace with `Workspace::for_window(window, cx)`, or keep the
//! `PanelContext::workspace` a panel was built with.

mod item;
mod project;
mod registry;
mod selection;
pub mod tabs;
mod toast;

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::{
    AnyView, AnyWindowHandle, App, AppContext as _, Context, Entity, EventEmitter, FocusHandle,
    Global, SharedString, Subscription, WeakEntity, Window,
};

pub use item::{Panel, PanelHandle, SaveDone, Tab, TabHandle, TabIcon, TabItem, save_signal};
pub use project::{Project, ProjectEvent, RepoState};
pub use registry::{
    BarItem, BarSlot, PanelContext, PanelSpec, QuickItem, QuickOpenSource, Registry, RenderBarItem,
    RunItem, Slot, register_bar_item, register_bar_item_default, register_panel,
    register_panel_default, register_quick_open,
};
pub use selection::{
    ChangeList, ChangesOf, ComparePoint, GuideOf, Opener, PullRange, Selection, opener, page_host,
    page_label, register_opener, selection_key, selection_path,
};
pub use tabs::{ClosedTab, Opened, TabEntry, TabGroup, Tabs};
pub use toast::{Toast, ToastAction, ToastKind, ToastRun};

use crate::memory::Live;
use crate::state::{TabSnapshot, UiState, WorkspaceSnapshot};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorkspaceEvent {
    /// Another project (or none) is open; tabs and per-project panels started over.
    ProjectChanged,
    TabsChanged,
    /// A panel showed, hid or switched.
    LayoutChanged,
}

#[derive(Default)]
struct Windows(HashMap<AnyWindowHandle, WeakEntity<Workspace>>);
impl Global for Windows {}

/// Asked about tabs with unsaved edits before they close: the keys, and the workspace to call
/// `force_close_tabs` on once the user chose. Runs after the close call returns.
pub type CloseGuard = Rc<dyn Fn(Vec<SharedString>, WeakEntity<Workspace>, &mut App)>;

#[derive(Default)]
struct CloseGuardGlobal(Option<CloseGuard>);
impl Global for CloseGuardGlobal {}

/// Sets what asks before tabs with unsaved edits close (the editor's save / discard / keep).
pub fn set_close_guard(cx: &mut App, guard: CloseGuard) {
    cx.set_global(CloseGuardGlobal(Some(guard)));
}

/// Asked about tabs' unsaved edits before something would drop them (a switch, a quit): the
/// keys, the workspace, and what to run once they're settled: true when they were saved or
/// dropped, false when the user kept them (Cancel) and nothing goes ahead. The app's dialog sets it.
pub type SettleEdits =
    Rc<dyn Fn(Vec<SharedString>, WeakEntity<Workspace>, Box<dyn FnOnce(bool, &mut App)>, &mut App)>;

#[derive(Default)]
struct SettleEditsGlobal(Option<SettleEdits>);
impl Global for SettleEditsGlobal {}

pub fn set_settle_edits(cx: &mut App, settle: SettleEdits) {
    cx.set_global(SettleEditsGlobal(Some(settle)));
}

/// What `set_settle_edits` set, for the app's quit.
pub fn settle_edits(cx: &App) -> Option<SettleEdits> {
    cx.try_global::<SettleEditsGlobal>()
        .and_then(|g| g.0.clone())
}

/// Asked before the window's repo changes (another project, another worktree of it): a reason
/// keeps it where it is, said in a toast. The git actions' feature sets it, so an action still
/// running isn't carried on in the other repo, core having one repo open at a time.
pub type SwitchGuard = Rc<dyn Fn(&Entity<Project>, &App) -> Option<SharedString>>;

#[derive(Default)]
struct SwitchGuardGlobal(Option<SwitchGuard>);
impl Global for SwitchGuardGlobal {}

pub fn set_switch_guard(cx: &mut App, guard: SwitchGuard) {
    cx.set_global(SwitchGuardGlobal(Some(guard)));
}

struct SideState {
    open: bool,
    active: Option<&'static str>,
    focus: FocusHandle,
}

/// Tab views kept while out of sight; older ones go and are built again from their selection
/// when shown. A tab with unsaved edits, or none to build from, keeps its view.
const LIVE_TABS: usize = 8;
/// Closed tabs' selections kept for Reopen Closed Tab, as `Tabs` keeps their places.
const CLOSED_KEPT: usize = 20;

pub struct Workspace {
    project: Option<Entity<Project>>,
    /// The worktree the tabs belong to and are saved under: set once its repo is open, so a
    /// switch still on its way or one that failed never saves them under another root.
    tabs_root: Option<String>,
    tabs: Tabs<Tab>,
    /// Tab keys, the latest shown first: which views stay built.
    shown: Vec<SharedString>,
    /// What closed tabs showed, for Reopen Closed Tab; never their views.
    closed: Vec<(SharedString, Selection)>,
    /// Viewed marks of this worktree: `kind:path` → the file's content signature when marked.
    viewed: BTreeMap<String, String>,
    /// The full ref the branch is reviewed against, while Changes shows that review.
    review: Option<String>,
    left: SideState,
    right: SideState,
    bottom: SideState,
    /// The terminal over the whole workspace (⌘↵ in it).
    bottom_maximized: bool,
    center_focus: FocusHandle,
    panels: HashMap<&'static str, PanelHandle>,
    toasts: Vec<Toast>,
    _project_events: Option<Subscription>,
    _live: Live,
}

impl EventEmitter<WorkspaceEvent> for Workspace {}

impl Workspace {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let sizes = UiState::get(cx).panels.clone();
        let side = |open: bool, cx: &mut Context<Self>| SideState {
            open,
            active: None,
            focus: cx.focus_handle(),
        };
        Self {
            project: None,
            tabs_root: None,
            tabs: Tabs::default(),
            shown: Vec::new(),
            closed: Vec::new(),
            viewed: BTreeMap::new(),
            review: None,
            left: side(!sizes.left.collapsed, cx),
            right: side(!sizes.right.collapsed, cx),
            // The terminal starts as it was left; hidden the first time, which is when it has no
            // height yet (the app gives it one when it's first drawn).
            bottom: side(sizes.bottom.size.is_some() && !sizes.bottom.collapsed, cx),
            bottom_maximized: false,
            center_focus: cx.focus_handle(),
            panels: HashMap::new(),
            toasts: Vec::new(),
            _project_events: None,
            _live: Live::new("Workspace"),
        }
    }

    /// Makes `workspace` the one `for_window` finds for `window`, until it's released.
    pub fn register_window(workspace: &Entity<Workspace>, window: &Window, cx: &mut App) {
        let handle = window.window_handle();
        cx.default_global::<Windows>()
            .0
            .insert(handle, workspace.downgrade());
        cx.observe_release(workspace, move |_, cx| {
            cx.default_global::<Windows>().0.remove(&handle);
        })
        .detach();
    }

    pub fn for_window(window: &Window, cx: &App) -> Option<Entity<Workspace>> {
        cx.try_global::<Windows>()?
            .0
            .get(&window.window_handle())?
            .upgrade()
    }

    pub fn context(&self, cx: &Context<Self>) -> PanelContext {
        PanelContext {
            workspace: cx.weak_entity(),
            project: self.project.clone(),
        }
    }

    // MARK: Project

    pub fn project(&self) -> Option<&Entity<Project>> {
        self.project.as_ref()
    }

    /// Opens `root` in this window: the old project's tabs are saved and closed, its per-project
    /// panels dropped, the new one's saved workspace restored.
    pub fn open_project(&mut self, root: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .project
            .as_ref()
            .is_some_and(|p| p.read(cx).root() == root)
        {
            return;
        }
        if self.switch_refused(cx) {
            return;
        }
        // Its tabs go: unsaved edits that wouldn't come back are asked about first.
        let handle = window.window_handle();
        let again = root.clone();
        if self.settle_first(cx, move |ws, cx| {
            let _ = handle.update(cx, |_, window, cx| {
                ws.update(cx, |ws, cx| ws.open_project_settled(again, window, cx))
            });
        }) {
            return;
        }
        self.open_project_settled(root, window, cx);
    }

    fn open_project_settled(&mut self, root: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .project
            .as_ref()
            .is_some_and(|p| p.read(cx).root() == root)
        {
            return;
        }
        self.drop_project(cx);
        let project = cx.new(|cx| Project::new(root, cx));
        self._project_events = Some(cx.subscribe_in(&project, window, Self::on_project_event));
        self.project = Some(project);
        // Without core (a test) nothing opens it: it's taken as open where it was asked.
        if !crate::has_core(cx) {
            self.opened(cx);
        }
        cx.emit(WorkspaceEvent::ProjectChanged);
        cx.notify();
    }

    /// The repo is open: it goes first in the recent projects by its main worktree (agent
    /// worktrees don't pile up there), and its worktree's tabs come back, by git's top level
    /// rather than the folder asked for (`gitviber .` from a subfolder).
    fn opened(&mut self, cx: &mut Context<Self>) {
        let Some(project) = &self.project else { return };
        let project = project.read(cx);
        let root = project.root().to_string_lossy().into_owned();
        let main = project
            .main_worktree()
            .map(|m| m.to_string_lossy().into_owned())
            .unwrap_or_else(|| root.clone());
        UiState::update(cx, |s| s.add_recent_project(&main));
        // Tabs opened while it was opening stay; a saved set comes back only into none.
        if self.tabs.is_empty() {
            let snapshot = UiState::get(cx)
                .workspace(&root)
                .cloned()
                .unwrap_or_default();
            self.restore(snapshot, cx);
        }
        self.tabs_root = Some(root);
        self.changed(WorkspaceEvent::TabsChanged, cx);
    }

    fn on_project_event(
        &mut self,
        _: &Entity<Project>,
        event: &ProjectEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            // Another worktree's workspace: this one's tabs are saved under its root, the other's come back.
            ProjectEvent::Switched { from, to } => {
                let from = self
                    .tabs_root
                    .clone()
                    .unwrap_or_else(|| from.to_string_lossy().into_owned());
                self.save_snapshot_as(&from, cx);
                self.clear_tabs();
                let to = to.to_string_lossy().into_owned();
                let snapshot = UiState::get(cx).workspace(&to).cloned().unwrap_or_default();
                self.restore(snapshot, cx);
                self.tabs_root = Some(to);
                self.changed(WorkspaceEvent::TabsChanged, cx);
            }
            ProjectEvent::Failed(err) => self.toast(
                Toast::error(err.clone()).title("Couldn't open the repository"),
                cx,
            ),
            ProjectEvent::Opened => self.opened(cx),
            ProjectEvent::RepoChanged(_) => {}
        }
    }

    /// Opens another worktree of the open repo in this window (the worktree picker).
    pub fn switch_worktree(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        if self.switch_refused(cx) {
            return;
        }
        let again = root.clone();
        if self.settle_first(cx, move |ws, cx| {
            ws.update(cx, |ws, cx| ws.switch_worktree_settled(again, cx))
        }) {
            return;
        }
        self.switch_worktree_settled(root, cx);
    }

    fn switch_worktree_settled(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        if let Some(project) = &self.project {
            project.update(cx, |p, cx| p.switch_worktree(root, cx));
        }
    }

    /// The tabs whose unsaved edits would be lost with them: dirty, and not kept by the tab.
    pub fn unkept_dirty_tabs(&self, cx: &App) -> Vec<SharedString> {
        self.tabs
            .tabs()
            .iter()
            .filter(|t| t.item.is_dirty(cx))
            .filter(|t| !t.item.handle.as_ref().is_some_and(|h| h.keeps_edits(cx)))
            .map(|t| t.key.clone())
            .collect()
    }

    /// True when `then` waits for the unsaved edits to be settled (the app asks); false when
    /// there are none, or nothing to ask with, and the caller goes on now.
    fn settle_first(
        &mut self,
        cx: &mut Context<Self>,
        then: impl FnOnce(Entity<Workspace>, &mut App) + 'static,
    ) -> bool {
        let dirty = self.unkept_dirty_tabs(cx);
        let Some(settle) = settle_edits(cx).filter(|_| !dirty.is_empty()) else {
            return false;
        };
        let weak = cx.weak_entity();
        let after = weak.clone();
        cx.defer(move |cx| {
            settle(
                dirty,
                weak,
                // After the update that settled them (a dialog's button, in this window).
                Box::new(move |go, cx| {
                    if go {
                        cx.defer(move |cx| {
                            if let Some(ws) = after.upgrade() {
                                then(ws, cx);
                            }
                        })
                    }
                }),
                cx,
            )
        });
        true
    }

    /// The switch guard's reason to stay, toasted; false when the repo may change.
    fn switch_refused(&mut self, cx: &mut Context<Self>) -> bool {
        let guard = cx
            .try_global::<SwitchGuardGlobal>()
            .and_then(|g| g.0.clone());
        let why = match (&self.project, guard) {
            (Some(project), Some(guard)) => guard(project, cx),
            _ => None,
        };
        let Some(why) = why else { return false };
        self.toast(Toast::warning(why).title("Not switched"), cx);
        true
    }

    /// Closes the project: its tabs and per-project panels go, with all they held.
    pub fn close_project(&mut self, cx: &mut Context<Self>) {
        if self.project.is_none() {
            return;
        }
        self.drop_project(cx);
        cx.emit(WorkspaceEvent::ProjectChanged);
        cx.notify();
    }

    fn drop_project(&mut self, cx: &mut Context<Self>) {
        self.save_snapshot(cx);
        let per_project: Vec<&'static str> = Registry::get(cx)
            .all_panels()
            .iter()
            .filter(|p| p.per_project)
            .map(|p| p.id)
            .collect();
        self.panels.retain(|id, _| !per_project.contains(id));
        self.clear_tabs();
        self.tabs_root = None;
        self._project_events = None;
        if self.project.take().is_some() && crate::has_core(cx) {
            // Its watcher stops; a project opened next overtakes this in core's queue.
            crate::backend::close_repo(cx);
        }
    }

    fn clear_tabs(&mut self) {
        self.tabs = Tabs::default();
        self.shown.clear();
        self.closed.clear();
        self.viewed.clear();
        self.review = None;
    }

    /// Puts a saved workspace back: its tabs as selections only, built as they're shown. A tab
    /// this build can't read is left out.
    fn restore(&mut self, snapshot: WorkspaceSnapshot, cx: &mut Context<Self>) {
        let pick = |id: Option<String>, slot: Slot, cx: &App| {
            let panels = Registry::get(cx).panels(slot);
            id.and_then(|id| panels.iter().find(|p| p.id == id).map(|p| p.id))
                .or_else(|| panels.first().map(|p| p.id))
        };
        self.left.active = pick(snapshot.list_tab, Slot::Left, cx);
        self.right.active = pick(snapshot.right_panel, Slot::Right, cx);
        self.bottom.active = pick(None, Slot::Bottom, cx);
        let mut active = None;
        for tab in snapshot.tabs {
            let Ok(selection) = serde_json::from_value::<Selection>(tab.sel) else {
                continue;
            };
            // Keys are derived again: their format can change, and a stale one would stop a
            // list row from finding its tab. The saved active key follows its tab's new one.
            let key: SharedString = selection_key(&selection).into();
            if snapshot.active.as_deref() == Some(tab.key.as_str()) {
                active = Some(key.clone());
            }
            self.tabs.open(key, tab.preview, || Tab {
                handle: None,
                selection: Some(selection),
            });
        }
        match active {
            Some(active) if self.tabs.activate(&active) => {}
            _ => self.tabs.go_to(0),
        }
        self.viewed = snapshot.viewed.into_iter().collect();
        self.review = snapshot.review;
    }

    /// This workspace as it would be restored: the tabs that have a selection.
    pub fn snapshot(&self) -> WorkspaceSnapshot {
        WorkspaceSnapshot {
            tabs: self
                .tabs
                .tabs()
                .iter()
                .filter_map(|t| {
                    let sel = serde_json::to_value(t.item.selection.as_ref()?).ok()?;
                    Some(TabSnapshot {
                        key: t.key.to_string(),
                        sel,
                        preview: t.preview,
                    })
                })
                .collect(),
            active: self.tabs.active_key().map(|k| k.to_string()),
            list_tab: self.left.active.map(str::to_string),
            right_panel: self.right.active.map(str::to_string),
            viewed: self
                .viewed
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            review: self.review.clone(),
        }
    }

    fn save_snapshot(&self, cx: &mut App) {
        if let Some(root) = &self.tabs_root {
            self.save_snapshot_as(root, cx);
        }
    }

    fn save_snapshot_as(&self, root: &str, cx: &mut App) {
        let snapshot = self.snapshot();
        let root = root.to_string();
        UiState::update_later(cx, move |s| s.save_workspace(&root, snapshot));
    }

    /// Which panels are closed, for every project and the next launch: saved on each change,
    /// not with the tabs, which wait for a repo to be open.
    fn save_layout(&self, cx: &mut App) {
        let closed = (!self.left.open, !self.right.open, !self.bottom.open);
        UiState::update_later(cx, move |s| {
            (
                s.panels.left.collapsed,
                s.panels.right.collapsed,
                s.panels.bottom.collapsed,
            ) = closed;
        });
    }

    fn changed(&mut self, event: WorkspaceEvent, cx: &mut Context<Self>) {
        if event == WorkspaceEvent::LayoutChanged {
            self.save_layout(cx);
        }
        self.save_snapshot(cx);
        cx.emit(event);
        cx.notify();
    }

    // MARK: Viewed marks

    /// The signature a file had when it was marked viewed, under `key` (`kind:path`).
    pub fn viewed(&self, key: &str) -> Option<&str> {
        self.viewed.get(key).map(String::as_str)
    }

    pub fn viewed_marks(&self) -> &BTreeMap<String, String> {
        &self.viewed
    }

    /// Marks a file viewed at its `signature`, or not (None). Saved with the worktree.
    pub fn set_viewed(
        &mut self,
        key: impl Into<String>,
        signature: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let key = key.into();
        match signature {
            Some(sig) => self.viewed.insert(key, sig),
            None => self.viewed.remove(&key),
        };
        self.changed(WorkspaceEvent::TabsChanged, cx);
    }

    pub fn review_base(&self) -> Option<&str> {
        self.review.as_deref()
    }

    pub fn set_review_base(&mut self, base: Option<String>, cx: &mut Context<Self>) {
        self.review = base;
        self.changed(WorkspaceEvent::TabsChanged, cx);
    }

    // MARK: Tabs

    pub fn tabs(&self) -> &Tabs<Tab> {
        &self.tabs
    }

    /// Opens what `selection` shows, by its key: an open tab is shown (and kept when `preview`
    /// is false) and takes the fresher selection; else the opener registered for its kind builds
    /// the view. False when no opener knows the kind or it couldn't build one.
    pub fn open(
        &mut self,
        selection: Selection,
        preview: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        self.open_at(selection, preview, None, window, cx)
    }

    /// Opens a popup's `selection` kept, right after its opener's tab `beside` and the popups
    /// that one opened before; closed while active, it hands back to `beside`.
    pub fn open_beside(
        &mut self,
        selection: Selection,
        beside: SharedString,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        self.open_at(selection, false, Some(beside), window, cx)
    }

    fn open_at(
        &mut self,
        selection: Selection,
        preview: bool,
        beside: Option<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let key: SharedString = selection_key(&selection).into();
        if self.tabs.get(&key).is_some() {
            self.tabs.open(key.clone(), preview, || unreachable!());
            self.tabs.set(&key, |tab| tab.selection = Some(selection));
            self.shown_now(&key, window, cx);
            self.changed(WorkspaceEvent::TabsChanged, cx);
            return true;
        }
        let Some(open) = opener(selection.opener_kind(), cx) else {
            return false;
        };
        let Some(handle) = open(&selection, window, cx) else {
            return false;
        };
        let tab = || Tab {
            handle: Some(handle),
            selection: Some(selection),
        };
        match beside {
            Some(beside) => self.tabs.open_beside(key.clone(), beside, tab),
            None => self.tabs.open(key.clone(), preview, tab),
        };
        self.shown_now(&key, window, cx);
        self.changed(WorkspaceEvent::TabsChanged, cx);
        true
    }

    /// The tab showing `selection`, when one is open.
    pub fn tab_for(&self, selection: &Selection) -> Option<&TabEntry<Tab>> {
        self.tabs.get(&selection_key(selection))
    }

    /// Changes what a tab under `key` saves as its selection (a page that moved to another address)
    /// without showing it or opening anything; its key stays.
    pub fn set_selection(
        &mut self,
        key: &SharedString,
        selection: Selection,
        cx: &mut Context<Self>,
    ) {
        if self.tabs.get(key).is_none() {
            return;
        }
        self.tabs.set(key, |tab| tab.selection = Some(selection));
        self.changed(WorkspaceEvent::TabsChanged, cx);
    }

    /// Opens a center tab under `key` that no selection describes (it isn't saved or reopened),
    /// or shows it when it's open already, without building it again. `preview`: a single
    /// click's tab, which the next preview open replaces; false keeps it.
    pub fn open_tab<V: TabItem>(
        &mut self,
        key: impl Into<SharedString>,
        preview: bool,
        build: impl FnOnce(&mut Window, &mut Context<Self>) -> Entity<V>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = key.into();
        if self.tabs.get(&key).is_some() {
            self.tabs.open(key.clone(), preview, || unreachable!());
        } else {
            let handle = TabHandle::new(build(window, cx));
            self.tabs.open(key.clone(), preview, || Tab {
                handle: Some(handle),
                selection: None,
            });
        }
        self.shown_now(&key, window, cx);
        self.changed(WorkspaceEvent::TabsChanged, cx);
    }

    /// Opens a tab already built; an open `key` is shown and `item` dropped.
    pub fn open_item(
        &mut self,
        key: impl Into<SharedString>,
        preview: bool,
        item: impl Into<TabHandle>,
        cx: &mut Context<Self>,
    ) {
        let item = item.into();
        self.tabs.open(key.into(), preview, || Tab {
            handle: Some(item),
            selection: None,
        });
        self.changed(WorkspaceEvent::TabsChanged, cx);
    }

    /// A tab at the end, the active one left as it is (one an agent opened).
    pub fn add_tab(
        &mut self,
        key: impl Into<SharedString>,
        item: impl Into<TabHandle>,
        selection: Option<Selection>,
        cx: &mut Context<Self>,
    ) {
        let item = item.into();
        self.tabs.add(key.into(), || Tab {
            handle: Some(item),
            selection,
        });
        self.changed(WorkspaceEvent::TabsChanged, cx);
    }

    pub fn activate_tab(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.tabs.activate(key) {
            let key = SharedString::from(key.to_string());
            self.shown_now(&key, window, cx);
            self.changed(WorkspaceEvent::TabsChanged, cx);
        }
    }

    /// Shows the tab under `key`, focus staying where it is: the tab strip's arrow keys.
    pub fn show_tab(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.tabs.activate(key) {
            let key = SharedString::from(key.to_string());
            self.ensure_built(&key, window, cx);
            self.note_shown(&key, cx);
            self.changed(WorkspaceEvent::TabsChanged, cx);
        }
    }

    /// The tab under `key` shows `selection` now, where it stands (a staged file's diff tab moves
    /// to the staged list): nothing goes to Reopen Closed Tab, a tab already open on `selection`
    /// takes this one in, and the view is built again when it's shown. False when `key` isn't
    /// open or has unsaved edits, which a new view would lose.
    pub fn retarget_tab(
        &mut self,
        key: &str,
        selection: Selection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(tab) = self.tabs.get(key) else {
            return false;
        };
        if tab.item.is_dirty(cx) {
            return false;
        }
        let new_key: SharedString = selection_key(&selection).into();
        if new_key.as_ref() == key {
            self.tabs.set(key, |tab| tab.selection = Some(selection));
            self.changed(WorkspaceEvent::TabsChanged, cx);
            return true;
        }
        let had_focus = tab
            .item
            .focus_handle(cx)
            .is_some_and(|f| f.contains_focused(window, cx));
        // Dropped once the new view is focused, so focus has somewhere to go.
        let _old = self.tabs.replace(key, new_key.clone(), || Tab {
            handle: None,
            selection: Some(selection),
        });
        self.shown.retain(|k| k.as_ref() != key);
        if self.tabs.active_key() == Some(&new_key) {
            self.ensure_built(&new_key, window, cx);
            self.note_shown(&new_key, cx);
            if had_focus && let Some(f) = self.tabs.active().and_then(|t| t.item.focus_handle(cx)) {
                f.focus(window, cx);
            }
        }
        self.changed(WorkspaceEvent::TabsChanged, cx);
        true
    }

    /// Keeps a preview tab (it was typed into, or double-clicked).
    pub fn pin_tab(&mut self, key: &str, cx: &mut Context<Self>) {
        self.tabs.pin(key);
        self.changed(WorkspaceEvent::TabsChanged, cx);
    }

    /// The active tab is shown: its view is built if it has none, it goes first among the
    /// shown, and views past LIVE_TABS that can be built again are let go.
    fn shown_now(&mut self, key: &SharedString, window: &mut Window, cx: &mut Context<Self>) {
        self.ensure_built(key, window, cx);
        self.note_shown(key, cx);
        self.focus_active_tab(window, cx);
    }

    /// Puts `key` first among the shown and lets go of views past LIVE_TABS that can be built
    /// again.
    fn note_shown(&mut self, key: &SharedString, cx: &mut Context<Self>) {
        if self.shown.first() == Some(key) {
            return;
        }
        self.shown.retain(|k| k != key);
        self.shown.insert(0, key.clone());
        let app: &App = cx;
        let release: Vec<SharedString> = self
            .shown
            .iter()
            .skip(LIVE_TABS)
            .filter(|k| {
                self.tabs
                    .get(k)
                    .is_some_and(|t| t.item.selection.is_some() && !t.item.is_dirty(app))
            })
            .cloned()
            .collect();
        for k in &release {
            self.tabs.set(k, |tab| tab.handle = None);
        }
        self.shown
            .retain(|k| !release.contains(k) && self.tabs.get(k).is_some());
    }

    /// Builds the view of the tab under `key` from its selection, when it has none.
    pub fn ensure_built(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(key) else {
            return;
        };
        if tab.item.handle.is_some() {
            return;
        }
        let Some(selection) = tab.item.selection.clone() else {
            return;
        };
        let Some(open) = opener(selection.opener_kind(), cx) else {
            return;
        };
        if let Some(handle) = open(&selection, window, cx) {
            self.tabs.set(key, |tab| tab.handle = Some(handle));
        }
    }

    /// The active tab's view, built now if it was let go or restored.
    pub fn active_view(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyView> {
        let key = self.tabs.active_key()?.clone();
        self.ensure_built(&key, window, cx);
        // Shown now, however it became active (a close handing over, a restore).
        self.note_shown(&key, cx);
        self.tabs.get(&key)?.item.view()
    }

    /// Closes `keys`. Tabs with unsaved edits stay open when a close guard is set: it's handed
    /// them to ask about, and closes them with `force_close_tabs`.
    pub fn close_tabs(&mut self, keys: &[SharedString], cx: &mut Context<Self>) {
        let guard = cx
            .try_global::<CloseGuardGlobal>()
            .and_then(|g| g.0.clone());
        if let Some(guard) = guard {
            let app: &App = cx;
            let dirty: Vec<SharedString> = keys
                .iter()
                .filter(|k| self.tabs.get(k).is_some_and(|t| t.item.is_dirty(app)))
                .cloned()
                .collect();
            if !dirty.is_empty() {
                let clean: Vec<SharedString> = keys
                    .iter()
                    .filter(|k| !dirty.contains(k))
                    .cloned()
                    .collect();
                let weak = cx.weak_entity();
                cx.defer(move |cx| guard(dirty, weak, cx));
                return self.force_close_tabs(&clean, cx);
            }
        }
        self.force_close_tabs(keys, cx);
    }

    /// The tabs under `keys` with unsaved edits, for a switch or a close that needs to ask first.
    pub fn dirty_tabs(&self, cx: &App) -> Vec<SharedString> {
        self.tabs
            .tabs()
            .iter()
            .filter(|t| t.item.is_dirty(cx))
            .map(|t| t.key.clone())
            .collect()
    }

    /// Closes `keys` whatever they hold: for a close guard once it asked, or after saving.
    pub fn force_close_tabs(&mut self, keys: &[SharedString], cx: &mut Context<Self>) {
        let app: &App = cx;
        let mut again = Vec::new();
        let mut once = Vec::new();
        for t in self.tabs.tabs().iter().filter(|t| keys.contains(&t.key)) {
            let reopenable = t.item.selection.is_some()
                && t.item.handle.as_ref().is_none_or(|h| h.reopenable(app));
            if reopenable {
                again.push(t.key.clone())
            } else {
                once.push(t.key.clone())
            }
        }
        let kept: Vec<(SharedString, Selection)> = again
            .iter()
            .filter_map(|k| Some((k.clone(), self.tabs.get(k)?.item.selection.clone()?)))
            .collect();
        // The views go here: nothing keeps a closed tab's entity.
        let hidden = |t: &Tab| t.hidden(app);
        drop(self.tabs.close(&again, true, hidden));
        drop(self.tabs.close(&once, false, hidden));
        self.closed.retain(|(k, _)| !keys.contains(k));
        self.closed.extend(kept);
        let overflow = self.closed.len().saturating_sub(CLOSED_KEPT);
        self.closed.drain(..overflow);
        self.shown.retain(|k| !keys.contains(k));
        self.changed(WorkspaceEvent::TabsChanged, cx);
    }

    pub fn close_tab(&mut self, key: &str, cx: &mut Context<Self>) {
        self.close_tabs(&[SharedString::from(key.to_string())], cx);
    }

    pub fn close_active_tab(&mut self, cx: &mut Context<Self>) {
        if let Some(key) = self.tabs.active_key().cloned() {
            self.close_tabs(&[key], cx);
        }
    }

    /// Close Others, to the Left, to the Right, All, around the active tab.
    pub fn close_group(&mut self, which: TabGroup, cx: &mut Context<Self>) {
        let Some(at) = self.tabs.active_index() else {
            return;
        };
        let keys = self.tabs.group(at, which);
        self.close_tabs(&keys, cx);
    }

    pub fn can_reopen_tab(&self) -> bool {
        self.tabs.can_reopen() && !self.closed.is_empty()
    }

    /// Opens the last closed tab again where it was, from what it showed.
    pub fn reopen_closed_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        while let Some(closed) = self.tabs.take_reopen() {
            let Some(at) = self.closed.iter().rposition(|(k, _)| *k == closed.key) else {
                continue;
            };
            let (key, selection) = self.closed.remove(at);
            let Some(open) = opener(selection.opener_kind(), cx) else {
                continue;
            };
            let Some(handle) = open(&selection, window, cx) else {
                continue;
            };
            self.tabs.insert(
                closed.index,
                key.clone(),
                Tab {
                    handle: Some(handle),
                    selection: Some(selection),
                },
            );
            self.shown_now(&key, window, cx);
            self.changed(WorkspaceEvent::TabsChanged, cx);
            return;
        }
    }

    pub fn step_tab(&mut self, dir: isize, window: &mut Window, cx: &mut Context<Self>) {
        self.tabs.step(dir);
        self.after_switch(window, cx);
    }

    pub fn go_to_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.tabs.go_to(index);
        self.after_switch(window, cx);
    }

    pub fn go_to_last_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.tabs.go_to_last();
        self.after_switch(window, cx);
    }

    fn after_switch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(key) = self.tabs.active_key().cloned() {
            self.shown_now(&key, window, cx);
        }
        self.changed(WorkspaceEvent::TabsChanged, cx);
    }

    pub fn move_tab(&mut self, from: usize, to: usize, cx: &mut Context<Self>) {
        self.tabs.move_tab(from, to);
        self.changed(WorkspaceEvent::TabsChanged, cx);
    }

    pub fn move_active_tab(&mut self, dir: isize, cx: &mut Context<Self>) {
        self.tabs.move_active(dir);
        self.changed(WorkspaceEvent::TabsChanged, cx);
    }

    fn focus_active_tab(&self, window: &mut Window, cx: &mut App) {
        if !self.center_focus.contains_focused(window, cx) {
            return;
        }
        let handle = self.tabs.active().and_then(|t| t.item.focus_handle(cx));
        handle
            .unwrap_or_else(|| self.center_focus.clone())
            .focus(window, cx);
    }

    // MARK: Panels

    fn side(&self, slot: Slot) -> &SideState {
        match slot {
            Slot::Left => &self.left,
            Slot::Right => &self.right,
            Slot::Bottom => &self.bottom,
        }
    }

    fn side_mut(&mut self, slot: Slot) -> &mut SideState {
        match slot {
            Slot::Left => &mut self.left,
            Slot::Right => &mut self.right,
            Slot::Bottom => &mut self.bottom,
        }
    }

    pub fn is_open(&self, slot: Slot) -> bool {
        self.side(slot).open
    }

    /// The panel the slot shows, by id; the slot's first when none was picked.
    pub fn active_panel(&self, slot: Slot, cx: &App) -> Option<&'static str> {
        self.side(slot)
            .active
            .or_else(|| Registry::get(cx).panels(slot).first().map(|p| p.id))
    }

    /// The focus handle the app tracks on a slot's container, for F6 and focus commands.
    pub fn slot_focus(&self, slot: Slot) -> &FocusHandle {
        &self.side(slot).focus
    }

    pub fn center_focus(&self) -> &FocusHandle {
        &self.center_focus
    }

    pub fn set_open(&mut self, slot: Slot, open: bool, cx: &mut Context<Self>) {
        if self.side(slot).open != open {
            self.side_mut(slot).open = open;
            if slot == Slot::Bottom && !open {
                self.bottom_maximized = false;
            }
            self.changed(WorkspaceEvent::LayoutChanged, cx);
        }
    }

    pub fn toggle(&mut self, slot: Slot, cx: &mut Context<Self>) {
        let open = !self.is_open(slot);
        self.set_open(slot, open, cx);
    }

    /// Shows a panel by id: its slot opens and switches to it.
    pub fn show_panel(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(spec) = Registry::get(cx).panel(id).cloned() else {
            return;
        };
        let side = self.side_mut(spec.slot);
        side.open = true;
        side.active = Some(spec.id);
        self.changed(WorkspaceEvent::LayoutChanged, cx);
    }

    /// A panel's view, built the first time it's asked for, outside the workspace's update: a
    /// panel's constructor may read the workspace, which panics during one. Views call this one.
    pub fn ensure_panel(
        workspace: &Entity<Self>,
        id: &str,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<PanelHandle> {
        let ws = workspace.read(cx);
        if let Some(panel) = ws.panels.get(id) {
            return Some(panel.clone());
        }
        let spec = Registry::get(cx).panel(id).cloned()?;
        if spec.per_project && ws.project.is_none() {
            return None;
        }
        // A per-window panel outlives projects: it reads the current one from the workspace, so
        // it never keeps one alive past a switch.
        let ctx = PanelContext {
            workspace: workspace.downgrade(),
            project: if spec.per_project {
                ws.project.clone()
            } else {
                None
            },
        };
        let panel = spec.build(&ctx, window, cx);
        workspace.update(cx, |ws, _| ws.panels.insert(spec.id, panel.clone()));
        Some(panel)
    }

    /// A panel's view, built inside the workspace's update. Test-only: a real panel's constructor
    /// reads the workspace, and building one here leased it twice (how ⌘T crashed). The app,
    /// examples and other crates' tests use `ensure_panel`.
    #[cfg(test)]
    pub fn panel(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<PanelHandle> {
        if let Some(panel) = self.panels.get(id) {
            return Some(panel.clone());
        }
        let spec = Registry::get(cx).panel(id).cloned()?;
        if spec.per_project && self.project.is_none() {
            return None;
        }
        let ctx = PanelContext {
            workspace: cx.weak_entity(),
            project: if spec.per_project {
                self.project.clone()
            } else {
                None
            },
        };
        let panel = spec.build(&ctx, window, cx);
        self.panels.insert(spec.id, panel.clone());
        Some(panel)
    }

    /// The panel's view if it was built; no building.
    pub fn built_panel(&self, id: &str) -> Option<&PanelHandle> {
        self.panels.get(id)
    }

    /// Opens a slot and moves focus into its panel.
    pub fn focus_slot(&mut self, slot: Slot, window: &mut Window, cx: &mut Context<Self>) {
        self.set_open(slot, true, cx);
        let id = self.active_panel(slot, cx);
        if let Some(id) = id.filter(|id| !self.panels.contains_key(id)) {
            // Built on the next frame, outside this update (ensure_panel), then focused.
            let ws = cx.weak_entity();
            let fallback = self.side(slot).focus.clone();
            window.on_next_frame(move |window, cx| {
                let panel = ws
                    .upgrade()
                    .and_then(|ws| Self::ensure_panel(&ws, id, window, cx));
                let handle = panel.and_then(|p| p.focus_handle(cx)).unwrap_or(fallback);
                handle.focus(window, cx);
            });
            return;
        }
        let handle = id
            .and_then(|id| self.panels.get(id))
            .and_then(|p| p.focus_handle(cx))
            .unwrap_or_else(|| self.side(slot).focus.clone());
        handle.focus(window, cx);
    }

    /// Moves focus into the active tab (⌘E).
    pub fn focus_center(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let handle = self.tabs.active().and_then(|t| t.item.focus_handle(cx));
        handle
            .unwrap_or_else(|| self.center_focus.clone())
            .focus(window, cx);
    }

    /// Which part holds focus now, in F6 order.
    pub fn focused_part(&self, window: &Window, cx: &App) -> Option<Part> {
        Part::ORDER.into_iter().find(|part| match part {
            Part::Center => self.center_focus.contains_focused(window, cx),
            Part::Slot(slot) => self.side(*slot).focus.contains_focused(window, cx),
        })
    }

    /// F6 and ⇧F6: the next open part after the focused one, wrapping.
    pub fn focus_next_part(&mut self, dir: isize, window: &mut Window, cx: &mut Context<Self>) {
        let order = Part::ORDER;
        let at = self
            .focused_part(window, cx)
            .and_then(|p| order.iter().position(|o| *o == p))
            .unwrap_or(1);
        for step in 1..=order.len() as isize {
            let next = order[(at as isize + dir * step).rem_euclid(order.len() as isize) as usize];
            match next {
                Part::Center => return self.focus_center(window, cx),
                Part::Slot(slot) if self.is_open(slot) => return self.focus_slot(slot, window, cx),
                _ => {}
            }
        }
    }

    pub fn terminal_maximized(&self) -> bool {
        self.bottom_maximized && self.bottom.open
    }

    pub fn set_terminal_maximized(&mut self, maximized: bool, cx: &mut Context<Self>) {
        self.bottom_maximized = maximized;
        if maximized {
            self.bottom.open = true;
        }
        self.changed(WorkspaceEvent::LayoutChanged, cx);
    }

    // MARK: Toasts

    /// Shows a toast in this workspace's window at its next frame; needs no window to call.
    pub fn toast(&mut self, toast: Toast, cx: &mut Context<Self>) {
        self.toasts.push(toast);
        cx.notify();
    }

    /// For the app's view, which shows them.
    pub fn take_toasts(&mut self) -> Vec<Toast> {
        std::mem::take(&mut self.toasts)
    }
}

/// A part of the workspace focus moves between.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Center,
    Slot(Slot),
}

impl Part {
    /// F6 order: git, code, explorer, terminal.
    pub const ORDER: [Part; 4] = [
        Part::Slot(Slot::Left),
        Part::Center,
        Part::Slot(Slot::Right),
        Part::Slot(Slot::Bottom),
    ];
}

pub(crate) fn init(cx: &mut App) {
    cx.default_global::<Registry>();
    cx.default_global::<Windows>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{IntoElement, Render, TestAppContext, VisualTestContext, div};

    struct Doc;
    impl Render for Doc {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
        }
    }
    impl TabItem for Doc {
        fn tab_title(&self, _: &App) -> SharedString {
            "doc".into()
        }
    }

    /// Keeps what it was built with, as a real panel does.
    struct Side(#[allow(dead_code)] PanelContext);
    impl Render for Side {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
        }
    }
    impl Panel for Side {}

    struct Host(Entity<Workspace>);
    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
        }
    }

    fn setup(cx: &mut TestAppContext) {
        crate::test_support::config_dir("workspace-test");
        cx.update(|cx| {
            crate::settings::init(cx);
            crate::state::init(cx);
            init(cx);
            register_panel(
                cx,
                PanelSpec::new("side", Slot::Left, "Side", |ctx, _, cx| {
                    cx.new(|_| Side(ctx.clone()))
                }),
            );
            register_panel(
                cx,
                PanelSpec::new("term", Slot::Bottom, "Term", |ctx, _, cx| {
                    cx.new(|_| Side(ctx.clone()))
                })
                .per_window(),
            );
        });
    }

    #[gpui_kit::test]
    fn tabs_panels_and_projects_give_their_entities_back(cx: &mut TestAppContext) {
        setup(cx);
        let (host, cx) = cx.add_window_view(|_, cx| Host(cx.new(Workspace::new)));
        let ws = host.read_with(cx, |h, _| h.0.clone());
        // A per-window panel outlives projects; it must not keep one alive.
        cx.update(|window, cx| ws.update(cx, |ws, cx| ws.panel("term", window, cx)));
        let snapshot = cx.update(|_, cx| cx.leak_detector_snapshot());
        for round in 0..6 {
            cx.update(|window, cx| {
                ws.update(cx, |ws, cx| {
                    ws.open_project(
                        PathBuf::from(format!("/tmp/gitviber-project-{}", round % 2)),
                        window,
                        cx,
                    );
                    assert!(ws.panel("side", window, cx).is_some());
                    for i in 0..4 {
                        ws.open_tab(
                            format!("tab-{i}"),
                            i % 2 == 0,
                            |_, cx| cx.new(|_| Doc),
                            window,
                            cx,
                        );
                    }
                    ws.close_tab("tab-1", cx);
                    ws.close_group(TabGroup::All, cx);
                    ws.open_tab("kept", false, |_, cx| cx.new(|_| Doc), window, cx);
                })
            });
        }
        cx.update(|_, cx| ws.update(cx, |ws, cx| ws.close_project(cx)));
        cx.run_until_parked();
        cx.update(|_, cx| cx.assert_no_new_leaks(&snapshot));
    }

    #[gpui_kit::test]
    fn a_per_window_panel_lets_the_old_project_go(cx: &mut TestAppContext) {
        setup(cx);
        let (host, cx) = cx.add_window_view(|_, cx| Host(cx.new(Workspace::new)));
        let ws = host.read_with(cx, |h, _| h.0.clone());
        let first = cx.update(|window, cx| {
            ws.update(cx, |ws, cx| {
                ws.open_project(PathBuf::from("/tmp/gitviber-project-a"), window, cx);
                ws.panel("term", window, cx);
                ws.panel("side", window, cx);
                let first = ws.project().unwrap().downgrade();
                ws.open_project(PathBuf::from("/tmp/gitviber-project-b"), window, cx);
                first
            })
        });
        cx.run_until_parked();
        first.assert_released();
    }

    #[gpui_kit::test]
    fn the_panels_come_back_as_left_through_projects_and_launches(cx: &mut TestAppContext) {
        setup(cx);
        let (host, cx) = cx.add_window_view(|_, cx| Host(cx.new(Workspace::new)));
        let ws = host.read_with(cx, |h, _| h.0.clone());
        let open = |ws: &Entity<Workspace>, cx: &mut VisualTestContext| {
            ws.read_with(cx, |ws, _| {
                [Slot::Left, Slot::Right, Slot::Bottom].map(|s| ws.is_open(s))
            })
        };
        // The terminal is hidden the first time.
        assert_eq!(open(&ws, cx), [true, true, false]);
        // Set before any repo is open, so with no tabs to be saved along.
        cx.update(|_, cx| {
            ws.update(cx, |ws, cx| {
                ws.set_open(Slot::Left, false, cx);
                ws.set_open(Slot::Bottom, true, cx);
            });
            // The height the app gives it when it's first drawn.
            UiState::update_later(cx, |s| s.panels.bottom.size = Some(240.));
        });
        // Projects opened, a tab opened, a project closed: none of them shows or hides a panel.
        cx.update(|window, cx| {
            ws.update(cx, |ws, cx| {
                ws.open_project(PathBuf::from("/tmp/gitviber-project-p"), window, cx);
                ws.open_tab("doc", false, |_, cx| cx.new(|_| Doc), window, cx);
                ws.open_project(PathBuf::from("/tmp/gitviber-project-q"), window, cx);
            })
        });
        assert_eq!(open(&ws, cx), [false, true, true]);
        cx.update(|_, cx| ws.update(cx, |ws, cx| ws.close_project(cx)));
        assert_eq!(open(&ws, cx), [false, true, true]);
        // The next launch's workspace.
        let next = cx.update(|_, cx| cx.new(Workspace::new));
        assert_eq!(open(&next, cx), [false, true, true]);
        cx.update(|_, cx| {
            next.update(cx, |ws, cx| {
                ws.set_open(Slot::Left, true, cx);
                ws.set_open(Slot::Right, false, cx);
                ws.set_open(Slot::Bottom, false, cx);
            })
        });
        let after = cx.update(|_, cx| cx.new(Workspace::new));
        assert_eq!(open(&after, cx), [true, false, false]);
    }

    #[gpui_kit::test]
    fn a_tab_opens_once_and_closed_ones_keep_only_keys(cx: &mut TestAppContext) {
        setup(cx);
        let (host, cx) = cx.add_window_view(|_, cx| Host(cx.new(Workspace::new)));
        let ws = host.read_with(cx, |h, _| h.0.clone());
        let built = std::rc::Rc::new(std::cell::Cell::new(0));
        let weak = cx.update(|window, cx| {
            ws.update(cx, |ws, cx| {
                ws.open_project(PathBuf::from("/tmp/gitviber-project-x"), window, cx);
                let doc = cx.new(|_| Doc);
                let weak = doc.downgrade();
                ws.open_item("same", true, doc, cx);
                for _ in 0..3 {
                    let built = built.clone();
                    let build = move |_: &mut Window, cx: &mut Context<Workspace>| {
                        built.set(built.get() + 1);
                        cx.new(|_| Doc)
                    };
                    ws.open_tab("same", true, build, window, cx);
                }
                assert_eq!(ws.tabs().len(), 1);
                ws.close_tab("same", cx);
                assert!(ws.tabs().is_empty());
                // It has no selection: nothing can build it again, so ⇧⌘T doesn't apply.
                assert!(!ws.can_reopen_tab());
                weak
            })
        });
        assert_eq!(built.get(), 0);
        cx.run_until_parked();
        weak.assert_released();
    }

    fn file_tab(path: &str) -> Selection {
        serde_json::from_value(serde_json::json!({ "kind": "file", "path": path })).unwrap()
    }

    fn register_file_opener(
        cx: &mut VisualTestContext,
        built: std::rc::Rc<std::cell::Cell<usize>>,
    ) {
        cx.update(|_, cx| {
            register_opener(cx, "file", move |_, _, cx| {
                built.set(built.get() + 1);
                Some(TabHandle::new(cx.new(|_| Doc)))
            })
        });
    }

    #[gpui_kit::test]
    fn selections_open_once_save_and_come_back_lazily(cx: &mut TestAppContext) {
        setup(cx);
        let (host, cx) = cx.add_window_view(|_, cx| Host(cx.new(Workspace::new)));
        let ws = host.read_with(cx, |h, _| h.0.clone());
        let built = std::rc::Rc::new(std::cell::Cell::new(0));
        register_file_opener(cx, built.clone());
        let root = PathBuf::from("/tmp/gitviber-project-sel");
        cx.update(|window, cx| {
            ws.update(cx, |ws, cx| {
                ws.open_project(root.clone(), window, cx);
                assert!(ws.open(file_tab("src/a.rs"), false, window, cx));
                assert!(ws.open(file_tab("src/a.rs"), true, window, cx));
                assert!(ws.open(file_tab("src/b.rs"), true, window, cx));
                assert!(!ws.open(
                    Selection::Compare {
                        base: ComparePoint {
                            reference: "a".into(),
                            label: "a".into()
                        },
                        head: ComparePoint {
                            reference: "b".into(),
                            label: "b".into()
                        },
                        merge_base: true,
                    },
                    false,
                    window,
                    cx
                ));
                assert_eq!(ws.tabs().len(), 2);
                ws.close_tab("file::src/b.rs", cx);
                assert!(ws.can_reopen_tab());
                ws.reopen_closed_tab(window, cx);
                assert_eq!(ws.tabs().len(), 2);
                ws.activate_tab("file::src/a.rs", window, cx);
            })
        });
        assert_eq!(built.get(), 3);
        // Away and back: the tabs come back from the saved selections, built only when shown.
        cx.update(|window, cx| {
            ws.update(cx, |ws, cx| {
                ws.open_project(PathBuf::from("/tmp/gitviber-project-other"), window, cx);
                assert!(ws.tabs().is_empty());
                ws.open_project(root.clone(), window, cx);
                assert_eq!(ws.tabs().len(), 2);
                assert_eq!(ws.tabs().active_key().unwrap(), "file::src/a.rs");
                assert!(ws.tabs().tabs().iter().all(|t| t.item.handle().is_none()));
                assert_eq!(
                    ws.tabs().get("file::src/b.rs").unwrap().item.title(cx),
                    "b.rs"
                );
                assert!(ws.active_view(window, cx).is_some());
            })
        });
        assert_eq!(built.get(), 4);
    }

    #[gpui_kit::test]
    fn a_retargeted_tab_moves_in_place_and_leaves_nothing_to_reopen(cx: &mut TestAppContext) {
        setup(cx);
        let (host, cx) = cx.add_window_view(|_, cx| Host(cx.new(Workspace::new)));
        let ws = host.read_with(cx, |h, _| h.0.clone());
        let built = std::rc::Rc::new(std::cell::Cell::new(0));
        register_file_opener(cx, built.clone());
        cx.update(|window, cx| {
            ws.update(cx, |ws, cx| {
                ws.open_project(PathBuf::from("/tmp/gitviber-project-move"), window, cx);
                for path in ["a", "b", "c"] {
                    ws.open(file_tab(path), false, window, cx);
                }
                // The inactive one waits to be shown; the active one is built now.
                assert!(ws.retarget_tab("file::a", file_tab("a2"), window, cx));
                assert!(ws.retarget_tab("file::c", file_tab("c2"), window, cx));
                let keys: Vec<_> = ws.tabs().tabs().iter().map(|t| t.key.to_string()).collect();
                assert_eq!(keys, ["file::a2", "file::b", "file::c2"]);
                assert_eq!(ws.tabs().active_key().unwrap(), "file::c2");
                assert!(ws.tabs().get("file::a2").unwrap().item.handle().is_none());
                // Onto an open one: they're one tab.
                assert!(ws.retarget_tab("file::c2", file_tab("b"), window, cx));
                assert_eq!(ws.tabs().len(), 2);
                assert_eq!(ws.tabs().active_key().unwrap(), "file::b");
                assert!(!ws.can_reopen_tab());
                assert!(!ws.retarget_tab("file::gone", file_tab("x"), window, cx));
            })
        });
        assert_eq!(built.get(), 4);
    }

    #[gpui_kit::test]
    fn only_the_latest_shown_tabs_keep_their_views(cx: &mut TestAppContext) {
        setup(cx);
        let (host, cx) = cx.add_window_view(|_, cx| Host(cx.new(Workspace::new)));
        let ws = host.read_with(cx, |h, _| h.0.clone());
        register_file_opener(cx, std::rc::Rc::new(std::cell::Cell::new(0)));
        let first = cx.update(|window, cx| {
            ws.update(cx, |ws, cx| {
                ws.open_project(PathBuf::from("/tmp/gitviber-project-live"), window, cx);
                ws.open(file_tab("f0"), false, window, cx);
                let first = ws.tabs().active().unwrap().item.view().unwrap().downgrade();
                for i in 1..=LIVE_TABS {
                    ws.open(file_tab(&format!("f{i}")), false, window, cx);
                }
                let built = ws
                    .tabs()
                    .tabs()
                    .iter()
                    .filter(|t| t.item.handle().is_some())
                    .count();
                assert_eq!(built, LIVE_TABS);
                assert!(ws.tabs().get("file::f0").unwrap().item.handle().is_none());
                first
            })
        });
        cx.run_until_parked();
        assert!(first.upgrade().is_none());
    }

    #[gpui_kit::test]
    fn tabs_made_active_by_a_close_count_as_shown(cx: &mut TestAppContext) {
        setup(cx);
        let (host, cx) = cx.add_window_view(|_, cx| Host(cx.new(Workspace::new)));
        let ws = host.read_with(cx, |h, _| h.0.clone());
        register_file_opener(cx, std::rc::Rc::new(std::cell::Cell::new(0)));
        cx.update(|window, cx| {
            ws.update(cx, |ws, cx| {
                ws.open_project(PathBuf::from("/tmp/gitviber-project-handover"), window, cx);
                for i in 0..(LIVE_TABS + 6) {
                    ws.open(file_tab(&format!("h{i}")), false, window, cx);
                }
                ws.go_to_tab(0, window, cx);
                // Each close hands over to a neighbour that the view then draws.
                for _ in 0..(LIVE_TABS + 4) {
                    ws.close_active_tab(cx);
                    assert!(ws.active_view(window, cx).is_some());
                    let built = ws
                        .tabs()
                        .tabs()
                        .iter()
                        .filter(|t| t.item.handle().is_some())
                        .count();
                    assert!(built <= LIVE_TABS, "{built} views built");
                }
            })
        });
    }

    /// An agent's page out of the strip.
    struct AgentPage;
    impl Render for AgentPage {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
        }
    }
    impl TabItem for AgentPage {
        fn tab_title(&self, _: &App) -> SharedString {
            "agent".into()
        }
        fn hidden(&self, _: &App) -> bool {
            true
        }
    }

    #[gpui_kit::test]
    fn a_close_skips_hidden_tabs_and_a_popup_hands_back_to_its_opener(cx: &mut TestAppContext) {
        setup(cx);
        let (host, cx) = cx.add_window_view(|_, cx| Host(cx.new(Workspace::new)));
        let ws = host.read_with(cx, |h, _| h.0.clone());
        register_file_opener(cx, std::rc::Rc::new(std::cell::Cell::new(0)));
        let key = |path: &str| SharedString::from(selection_key(&file_tab(path)));
        cx.update(|window, cx| {
            ws.update(cx, |ws, cx| {
                ws.open_project(PathBuf::from("/tmp/gitviber-project-hidden"), window, cx);
                ws.open(file_tab("a"), false, window, cx);
                ws.open(file_tab("b"), false, window, cx);
                let page = cx.new(|_| AgentPage);
                ws.add_tab("agent", page, None, cx);
                ws.close_active_tab(cx);
                assert_eq!(
                    ws.tabs().active_key(),
                    Some(&key("a")),
                    "past the agent's page"
                );
                ws.open(file_tab("c"), false, window, cx);
                ws.open_beside(file_tab("p"), key("a"), window, cx);
                let order: Vec<_> = ws.tabs().tabs().iter().map(|t| t.key.clone()).collect();
                assert_eq!(order, [key("a"), key("p"), "agent".into(), key("c")]);
                ws.close_active_tab(cx);
                assert_eq!(
                    ws.tabs().active_key(),
                    Some(&key("a")),
                    "back to its opener"
                );
                ws.close_tabs(&[key("a"), key("c")], cx);
                assert!(
                    ws.tabs().active_key().is_none(),
                    "only the agent's page is left"
                );
            })
        });
    }

    fn git_repo(name: &str) -> Option<PathBuf> {
        let dir = crate::test_support::temp_dir(&format!("ws-{name}"));
        let ok = std::process::Command::new("git")
            .args(["init", "-q"])
            .arg(&dir)
            .status()
            .ok()?
            .success();
        ok.then(|| dir.canonicalize().unwrap_or(dir))
    }

    #[gpui_kit::test]
    fn a_failed_switch_keeps_the_worktree_and_its_tabs(cx: &mut TestAppContext) {
        let (Some(a), Some(b)) = (git_repo("a"), git_repo("b")) else {
            return;
        };
        setup(cx);
        cx.update(crate::install_core);
        let (host, cx) = cx.add_window_view(|_, cx| Host(cx.new(Workspace::new)));
        let ws = host.read_with(cx, |h, _| h.0.clone());
        register_file_opener(cx, std::rc::Rc::new(std::cell::Cell::new(0)));
        cx.update(|window, cx| ws.update(cx, |ws, cx| ws.open_project(a.clone(), window, cx)));
        cx.run_until_parked();
        cx.update(|window, cx| {
            ws.update(cx, |ws, cx| {
                assert!(ws.project().unwrap().read(cx).is_open());
                ws.open(file_tab("kept.rs"), false, window, cx);
                ws.switch_worktree(PathBuf::from("/nonexistent/gitviber-worktree"), cx);
            })
        });
        cx.run_until_parked();
        let a_key = a.to_string_lossy().into_owned();
        cx.update(|_, cx| {
            let project = ws.read(cx).project().unwrap().read(cx);
            assert_eq!(project.root(), a.as_path());
            assert!(project.is_open());
            assert_eq!(ws.read(cx).tabs().len(), 1);
        });
        // A switch that works saves the tabs under the worktree they were in.
        cx.update(|_, cx| ws.update(cx, |ws, cx| ws.switch_worktree(b.clone(), cx)));
        cx.run_until_parked();
        cx.update(|_, cx| {
            assert_eq!(ws.read(cx).project().unwrap().read(cx).root(), b.as_path());
            assert!(ws.read(cx).tabs().is_empty());
            let saved = UiState::get(cx).workspace(&a_key).unwrap();
            assert_eq!(saved.tabs.len(), 1);
            assert!(
                UiState::get(cx)
                    .workspace("/nonexistent/gitviber-worktree")
                    .is_none()
            );
        });
    }

    struct Edited;
    impl Render for Edited {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
        }
    }
    impl TabItem for Edited {
        fn tab_title(&self, _: &App) -> SharedString {
            "edited".into()
        }
        fn is_dirty(&self, _: &App) -> bool {
            true
        }
    }

    #[gpui_kit::test]
    fn a_switch_guard_keeps_the_repo_where_it_is(cx: &mut TestAppContext) {
        setup(cx);
        let busy = std::rc::Rc::new(std::cell::Cell::new(true));
        let held = busy.clone();
        cx.update(|cx| {
            set_switch_guard(
                cx,
                Rc::new(move |_, _| held.get().then(|| "Wait for Push to finish.".into())),
            )
        });
        let (host, cx) = cx.add_window_view(|_, cx| Host(cx.new(Workspace::new)));
        let ws = host.read_with(cx, |h, _| h.0.clone());
        let a = PathBuf::from("/tmp/gitviber-project-guard-a");
        let b = PathBuf::from("/tmp/gitviber-project-guard-b");
        let root = |ws: &Entity<Workspace>, cx: &mut VisualTestContext| {
            cx.update(|_, cx| {
                ws.read(cx)
                    .project()
                    .map(|p| p.read(cx).root().to_path_buf())
            })
        };
        // Nothing open yet: nothing to keep.
        cx.update(|window, cx| ws.update(cx, |ws, cx| ws.open_project(a.clone(), window, cx)));
        assert_eq!(root(&ws, cx), Some(a.clone()));
        cx.update(|window, cx| {
            ws.update(cx, |ws, cx| {
                ws.open_project(b.clone(), window, cx);
                ws.switch_worktree(b.clone(), cx);
                assert_eq!(ws.take_toasts().len(), 2);
            })
        });
        assert_eq!(root(&ws, cx), Some(a.clone()));
        busy.set(false);
        cx.update(|window, cx| {
            ws.update(cx, |ws, cx| {
                ws.switch_worktree(b.clone(), cx);
                ws.open_project(b.clone(), window, cx);
                assert!(ws.take_toasts().is_empty());
            })
        });
        assert_eq!(root(&ws, cx), Some(b));
    }

    #[gpui_kit::test]
    fn tabs_with_unsaved_edits_wait_for_the_guard(cx: &mut TestAppContext) {
        setup(cx);
        let asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::<SharedString>::new()));
        let seen = asked.clone();
        cx.update(|cx| {
            set_close_guard(
                cx,
                Rc::new(move |keys, _, _| seen.borrow_mut().extend(keys)),
            )
        });
        let (host, cx) = cx.add_window_view(|_, cx| Host(cx.new(Workspace::new)));
        let ws = host.read_with(cx, |h, _| h.0.clone());
        cx.update(|window, cx| {
            ws.update(cx, |ws, cx| {
                ws.open_project(PathBuf::from("/tmp/gitviber-project-dirty"), window, cx);
                ws.open_tab("edited", false, |_, cx| cx.new(|_| Edited), window, cx);
                ws.open_tab("plain", false, |_, cx| cx.new(|_| Doc), window, cx);
                ws.close_group(TabGroup::All, cx);
                assert_eq!(ws.tabs().len(), 1);
                assert_eq!(ws.dirty_tabs(cx), vec![SharedString::from("edited")]);
            })
        });
        cx.run_until_parked();
        assert_eq!(*asked.borrow(), vec![SharedString::from("edited")]);
        cx.update(|_, cx| ws.update(cx, |ws, cx| ws.force_close_tabs(&["edited".into()], cx)));
        assert!(cx.update(|_, cx| ws.read(cx).tabs().is_empty()));
    }

    struct Kept;
    impl Render for Kept {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
        }
    }
    impl TabItem for Kept {
        fn tab_title(&self, _: &App) -> SharedString {
            "kept".into()
        }
        fn is_dirty(&self, _: &App) -> bool {
            true
        }
        fn keeps_edits(&self, _: &App) -> bool {
            true
        }
    }

    type Waiting =
        Rc<std::cell::RefCell<Vec<(Vec<SharedString>, Box<dyn FnOnce(bool, &mut App)>)>>>;

    #[gpui_kit::test]
    fn a_switch_waits_for_unsaved_edits_that_would_be_lost(cx: &mut TestAppContext) {
        setup(cx);
        let waiting: Waiting = Rc::default();
        let asks = waiting.clone();
        cx.update(|cx| {
            set_settle_edits(
                cx,
                Rc::new(move |keys, _, then, _| asks.borrow_mut().push((keys, then))),
            )
        });
        let (host, cx) = cx.add_window_view(|_, cx| Host(cx.new(Workspace::new)));
        let ws = host.read_with(cx, |h, _| h.0.clone());
        let a = PathBuf::from("/tmp/gitviber-project-settle-a");
        let b = PathBuf::from("/tmp/gitviber-project-settle-b");
        let c = PathBuf::from("/tmp/gitviber-project-settle-c");
        let root = |ws: &Entity<Workspace>, cx: &mut VisualTestContext| {
            cx.update(|_, cx| {
                ws.read(cx)
                    .project()
                    .map(|p| p.read(cx).root().to_path_buf())
            })
        };
        cx.update(|window, cx| {
            ws.update(cx, |ws, cx| {
                ws.open_project(a.clone(), window, cx);
                // Edits the tab keeps itself don't stop anything.
                ws.open_tab("kept", false, |_, cx| cx.new(|_| Kept), window, cx);
                ws.open_project(b.clone(), window, cx);
            })
        });
        cx.run_until_parked();
        assert_eq!(root(&ws, cx), Some(b.clone()));
        assert!(waiting.borrow().is_empty());
        // Ones that would go with the tab wait for the ask; Cancel (never running it) stays.
        cx.update(|window, cx| {
            ws.update(cx, |ws, cx| {
                ws.open_tab("edited", false, |_, cx| cx.new(|_| Edited), window, cx);
                assert_eq!(ws.unkept_dirty_tabs(cx), vec![SharedString::from("edited")]);
                ws.open_project(c.clone(), window, cx);
                ws.switch_worktree(c.clone(), cx);
            })
        });
        cx.run_until_parked();
        assert_eq!(root(&ws, cx), Some(b.clone()));
        let asked: Vec<_> = waiting.borrow_mut().drain(..).collect();
        assert_eq!(asked.len(), 2);
        assert_eq!(asked[0].0, vec![SharedString::from("edited")]);
        // Kept (Cancel): it stays; settled (saved or dropped): the switch goes on.
        let mut asked = asked.into_iter();
        let (_, first) = asked.next().unwrap();
        let (_, second) = asked.next().unwrap();
        cx.update(|_, cx| second(false, cx));
        cx.run_until_parked();
        assert_eq!(root(&ws, cx), Some(b.clone()));
        cx.update(|_, cx| first(true, cx));
        cx.run_until_parked();
        assert_eq!(root(&ws, cx), Some(c));
    }
}
