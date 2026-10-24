//! The Explorer panel: the working tree folder by folder, git's status on each row, ignored files
//! faint, and the file actions (new, rename, delete, reveal, copy path, compare two files) on its
//! menu. Rows are a virtual list: only those on screen are built.

pub mod tree;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use gitviber_core::{Core, api};
use gitviber_ui::ActiveAppTheme as _;
use gitviber_ui::keymap::explorer;
use gitviber_ui::memory::Live;
use gitviber_ui::widgets::{ChangeKind, FileIcon};
use gitviber_ui::workspace::{
    Panel, PanelContext, Project, ProjectEvent, Toast, Workspace, WorkspaceEvent,
};
use gpui_kit::component::Sizable as _;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenu, PopupMenuItem};
use gpui_kit::{
    App, AppContext as _, ClickEvent, ClipboardItem, Context, Entity, FocusHandle,
    InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement as _, PromptLevel, Render,
    ScrollStrategy, SharedString, StatefulInteractiveElement as _, Styled as _, Subscription, Task,
    UniformListScrollHandle, WeakEntity, Window, div, prelude::FluentBuilder as _, px,
    uniform_list,
};

use crate::backend;
use crate::{OpenOptions, open_file};
use tree::{LazyTree, Listed, Node, Row, TreeKey, dirname, tree_key};

pub const PANEL_ID: &str = "explorer";
const ROW_HEIGHT: f32 = 24.;
const INDENT: f32 = 12.;

/// Show History: the History panel narrowed to a file's (or a folder's) commits. It lives in
/// another crate; until the app sets this, the item says so.
pub type HistoryHandler = Rc<dyn Fn(&str, bool, &mut Window, &mut App)>;

struct HistoryHook(HistoryHandler);
impl gpui_kit::Global for HistoryHook {}

pub fn set_history_handler(cx: &mut App, handler: HistoryHandler) {
    cx.set_global(HistoryHook(handler));
}

/// The filter (⌘F in the explorer): every file git lists and the ignored ones, matched by path.
struct Filter {
    input: Entity<InputState>,
    needle: String,
    files: Option<Arc<Vec<Node>>>,
    found: usize,
    capped: bool,
    _sub: Subscription,
    _load: Task<()>,
}

/// What a menu item does to the panel.
type MenuAction = Rc<dyn Fn(&mut ExplorerPanel, &mut Window, &mut Context<ExplorerPanel>)>;

fn menu_item(me: &WeakEntity<ExplorerPanel>, label: &str, f: MenuAction) -> PopupMenuItem {
    let me = me.clone();
    PopupMenuItem::new(label.to_string()).on_click(move |_, window, cx| {
        let f = f.clone();
        _ = me.update(cx, |this, cx| f(this, window, cx));
    })
}

enum Editing {
    Rename { node: Node },
    New { parent: SharedString, is_dir: bool },
}

struct Edit {
    what: Editing,
    input: Entity<InputState>,
    _sub: Subscription,
}

/// One line of the list: a tree row, or the name box of a new entry.
#[derive(Clone)]
enum Item {
    Row(usize),
    NewInput { depth: usize, is_dir: bool },
}

pub struct ExplorerPanel {
    workspace: WeakEntity<Workspace>,
    root: PathBuf,
    core: Arc<Core>,
    tree: LazyTree,
    rows: Vec<Row>,
    items: Vec<Item>,
    status: HashMap<SharedString, ChangeKind>,
    dirty_dirs: HashSet<SharedString>,
    /// The keyboard's row, apart from the open tab.
    selected: Option<SharedString>,
    /// Rows picked with ⌘/⇧ from an anchor.
    picked: HashSet<SharedString>,
    anchor: Option<SharedString>,
    /// Select for Compare's file.
    mark: Option<SharedString>,
    editing: Option<Edit>,
    /// A path asked to be shown whose folders are still being listed.
    revealing: Option<SharedString>,
    filter: Option<Filter>,
    /// origin's page on GitHub, for links to a file there.
    web_url: Option<String>,
    /// Tracked files with unstaged edits: Discard Changes applies to them.
    discardable: HashSet<SharedString>,
    /// A row's menu opened from the keyboard (⇧F10), where it shows.
    kb_menu: Option<(
        Entity<PopupMenu>,
        gpui_kit::Point<gpui_kit::Pixels>,
        Subscription,
    )>,
    /// The list's height when last drawn, for a page of PgUp/PgDn.
    list_height: Rc<std::cell::Cell<gpui_kit::Pixels>>,
    scroll: UniformListScrollHandle,
    focus: FocusHandle,
    listings: HashMap<SharedString, Task<()>>,
    _status: Task<()>,
    _work: Task<()>,
    _subs: Vec<Subscription>,
    _live: Live,
}

impl Panel for ExplorerPanel {
    fn panel_focus(&self, _: &App) -> Option<FocusHandle> {
        Some(self.focus.clone())
    }
}

impl ExplorerPanel {
    pub fn new(ctx: &PanelContext, _window: &mut Window, cx: &mut Context<Self>) -> Self {
        let root = ctx
            .project
            .as_ref()
            .map(|p| p.read(cx).root().to_path_buf())
            .unwrap_or_default();
        let mut subs = Vec::new();
        if let Some(project) = &ctx.project {
            subs.push(cx.subscribe(
                project,
                |this, project: Entity<Project>, e: &ProjectEvent, cx| match e {
                    ProjectEvent::RepoChanged(_) => this.refresh(cx),
                    ProjectEvent::Opened | ProjectEvent::Switched { .. } => {
                        this.root = project.read(cx).root().to_path_buf();
                        this.tree = LazyTree::new();
                        this.rebuild(cx);
                        this.refresh(cx);
                    }
                    ProjectEvent::Failed(_) => {}
                },
            ));
        }
        if let Some(ws) = ctx.workspace.upgrade() {
            subs.push(cx.subscribe(&ws, |_, _, e: &WorkspaceEvent, cx| {
                if matches!(e, WorkspaceEvent::TabsChanged) {
                    cx.notify();
                }
            }));
        }
        let mut panel = ExplorerPanel {
            workspace: ctx.workspace.clone(),
            root,
            core: backend::core(cx),
            tree: LazyTree::new(),
            rows: Vec::new(),
            items: Vec::new(),
            status: HashMap::new(),
            dirty_dirs: HashSet::new(),
            selected: None,
            picked: HashSet::new(),
            anchor: None,
            mark: None,
            editing: None,
            revealing: None,
            filter: None,
            web_url: None,
            discardable: HashSet::new(),
            kb_menu: None,
            list_height: Rc::new(std::cell::Cell::new(px(0.))),
            scroll: UniformListScrollHandle::new(),
            focus: cx.focus_handle(),
            listings: HashMap::new(),
            _status: Task::ready(()),
            _work: Task::ready(()),
            _subs: subs,
            _live: Live::new("ExplorerPanel"),
        };
        panel.refresh(cx);
        crate::quick_open::warm(&panel.root, cx);
        panel
    }

    /// Lists every open folder again and reads git's status.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        if self.filter.is_some() {
            self.load_filter_files(cx);
        }
        let open: Vec<SharedString> = self.tree.expanded().cloned().collect();
        for path in open {
            self.load(path, cx);
        }
        let core = self.core.clone();
        self._status = cx.spawn(async move |this, cx| {
            let status = cx
                .background_spawn(async move { api::changes::status(&core) })
                .await;
            let Ok(status) = status else { return };
            _ = this.update(cx, |this, cx| {
                this.web_url = status.web_url.clone().flatten();
                // Same rule as Changes: only tracked working-tree edits can be restored.
                this.discardable = status
                    .unstaged
                    .iter()
                    .filter(|f| f.status != "?")
                    .map(|f| SharedString::from(f.path.clone()))
                    .collect();
                this.status.clear();
                this.dirty_dirs.clear();
                for f in status
                    .staged
                    .iter()
                    .chain(&status.unstaged)
                    .chain(&status.conflicted)
                {
                    let kind = ChangeKind::from_letter(f.status.chars().next().unwrap_or('M'));
                    let path: SharedString = f.path.clone().into();
                    let mut dir = dirname(&path);
                    while !dir.is_empty() {
                        self::insert_dir(&mut this.dirty_dirs, dir);
                        dir = dirname(dir);
                    }
                    this.status.insert(path, kind);
                }
                cx.notify();
            });
        });
    }

    fn load(&mut self, path: SharedString, cx: &mut Context<Self>) {
        let id = self.tree.begin(&path);
        let core = self.core.clone();
        let rel = path.to_string();
        let key = path.clone();
        let task = cx.spawn(async move |this, cx| {
            let listed = cx
                .background_spawn(async move { api::files::list_dir(&core, rel) })
                .await;
            _ = this.update(cx, |this, cx| {
                this.listings.remove(&path);
                let listed = listed.map(|v| v.into_iter().map(Node::from).collect());
                match this.tree.listed(&path, id, listed) {
                    Listed::Changed | Listed::Closed => this.rebuild(cx),
                    Listed::RootFailed(e) => {
                        this.toast(Toast::error(e).title("Could not list repository"), cx)
                    }
                    Listed::Same | Listed::Stale => {}
                }
            });
        });
        self.listings.insert(key, task);
    }

    fn rebuild(&mut self, cx: &mut Context<Self>) {
        self.rows = match &mut self.filter {
            Some(f) if !f.needle.is_empty() => match &f.files {
                Some(files) => {
                    let (rows, found, capped) = tree::matching_rows(files, &f.needle);
                    (f.found, f.capped) = (found, capped);
                    rows
                }
                // The last tree until the files are listed.
                None => self.tree.rows(),
            },
            _ => self.tree.rows(),
        };
        self.items = items(&self.rows, self.editing.as_ref().map(|e| &e.what));
        // The edited entry (or the folder a new one goes into) went away.
        if let Some(edit) = &self.editing {
            let gone = match &edit.what {
                Editing::Rename { node } => !self.rows.iter().any(|r| r.node.path == node.path),
                Editing::New { parent, .. } => {
                    !parent.is_empty() && !self.rows.iter().any(|r| r.node.path == *parent)
                }
            };
            if gone {
                self.editing = None;
                self.items = items(&self.rows, None);
            }
        }
        self.picked
            .retain(|p| self.rows.iter().any(|r| r.node.path == *p));
        if let Some(path) = self.revealing.clone()
            && let Some(ix) = self.index_of(&path)
        {
            self.scroll.scroll_to_item(ix, ScrollStrategy::Center);
            self.revealing = None;
        }
        cx.notify();
    }

    #[cfg(test)]
    pub(crate) fn row_paths(&self) -> Vec<String> {
        self.rows.iter().map(|r| r.node.path.to_string()).collect()
    }

    #[cfg(test)]
    pub(crate) fn create_for_test(
        &mut self,
        parent: &str,
        name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.start_editing(
            Editing::New {
                parent: parent.to_string().into(),
                is_dir: false,
            },
            window,
            cx,
        );
        self.finish_editing(Some(name.to_string()), window, cx);
    }

    #[cfg(test)]
    pub(crate) fn rename_for_test(
        &mut self,
        path: &str,
        name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let node = self
            .rows
            .iter()
            .find(|r| r.node.path.as_ref() == path)
            .map(|r| r.node.clone())
            .expect("a row");
        self.start_editing(Editing::Rename { node }, window, cx);
        self.finish_editing(Some(name.to_string()), window, cx);
    }

    /// Opens the filter (⌘F here) and focuses it.
    fn open_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(f) = &self.filter {
            f.input.update(cx, |s, cx| {
                s.focus(window, cx);
                s.select_all(window, cx);
            });
            return;
        }
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Filter files"));
        input.update(cx, |s, cx| s.focus(window, cx));
        let sub = cx.subscribe(&input, |this, input, e: &InputEvent, cx| {
            if matches!(e, InputEvent::Change) {
                let needle = input.read(cx).value().trim().to_string();
                if let Some(f) = &mut this.filter {
                    f.needle = needle;
                }
                this.picked.clear();
                this.rebuild(cx);
            }
        });
        self.filter = Some(Filter {
            input,
            needle: String::new(),
            files: None,
            found: 0,
            capped: false,
            _sub: sub,
            _load: Task::ready(()),
        });
        self.load_filter_files(cx);
        cx.notify();
    }

    /// Leaving the filter, the file picked in it stays picked, shown in its folders.
    fn close_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.filter.take().is_none() {
            return;
        }
        self.picked.clear();
        if let Some(selected) = self.selected.clone() {
            for dir in self.tree.open_to(&selected, false) {
                self.load(dir, cx);
            }
            self.revealing = Some(selected);
        }
        self.focus.focus(window, cx);
        self.rebuild(cx);
    }

    fn load_filter_files(&mut self, cx: &mut Context<Self>) {
        let core = self.core.clone();
        let task = cx.spawn(async move |this, cx| {
            let files = cx
                .background_spawn(async move {
                    let listed = api::files::list_files(&core)?;
                    // A failure to list the ignored ones leaves the filter at what git lists.
                    let ignored = api::files::list_ignored(&core).unwrap_or_default();
                    let name = |p: &str| p.rsplit('/').next().unwrap_or(p).to_string();
                    let mut files: Vec<Node> = listed
                        .into_iter()
                        .map(|path| Node {
                            name: name(&path).into(),
                            path: path.into(),
                            is_dir: false,
                            ignored: false,
                        })
                        .collect();
                    files.extend(ignored.into_iter().map(Node::from));
                    Ok::<_, String>(files)
                })
                .await;
            _ = this.update(cx, |this, cx| match files {
                Ok(files) => {
                    if let Some(f) = &mut this.filter {
                        f.files = Some(Arc::new(files));
                    }
                    this.rebuild(cx);
                }
                Err(e) => this.toast(Toast::error(e).title("Could not list files"), cx),
            });
        });
        if let Some(f) = &mut self.filter {
            f._load = task;
        }
    }

    fn filtering(&self) -> bool {
        self.filter.as_ref().is_some_and(|f| !f.needle.is_empty())
    }

    /// A row's menu from the keyboard, under the row.
    fn open_keyboard_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let node = self
            .selected
            .as_ref()
            .and_then(|s| self.row_index(s))
            .map(|i| self.rows[i].node.clone());
        let at = self
            .selected
            .as_ref()
            .and_then(|s| self.index_of(s))
            .unwrap_or(0);
        let offset = self.scroll.0.borrow().base_handle.offset();
        let origin = self.scroll.0.borrow().base_handle.bounds().origin;
        let position = gpui_kit::point(
            origin.x + px(24.),
            origin.y + offset.y + px(ROW_HEIGHT * (at as f32 + 1.)),
        );
        let me = cx.weak_entity();
        window.defer(cx, move |window, cx| {
            let menu = PopupMenu::build(window, cx, |menu, window, pcx| match me.upgrade() {
                Some(this) => Self::build_menu(&this, node.clone(), menu, window, pcx),
                None => menu,
            });
            gpui_kit::Focusable::focus_handle(menu.read(cx), cx).focus(window, cx);
            let dismiss = cx.subscribe(&menu, {
                let me = me.clone();
                move |_, _: &gpui_kit::DismissEvent, cx| {
                    _ = me.update(cx, |this, cx| {
                        this.kb_menu = None;
                        cx.notify();
                    });
                }
            });
            _ = me.update(cx, |this, cx| {
                this.kb_menu = Some((menu, position, dismiss));
                cx.notify();
            });
        });
    }

    fn index_of(&self, path: &str) -> Option<usize> {
        self.items.iter().position(
            |item| matches!(item, Item::Row(i) if self.rows[*i].node.path.as_ref() == path),
        )
    }

    fn row_index(&self, path: &str) -> Option<usize> {
        self.rows.iter().position(|r| r.node.path.as_ref() == path)
    }

    fn toast(&self, toast: Toast, cx: &mut App) {
        _ = self.workspace.update(cx, |ws, cx| ws.toast(toast, cx));
    }

    /// Opens the folders down to `path`, selects it and, with `focus`, moves the keyboard here.
    pub fn reveal(&mut self, path: &str, focus: bool, window: &mut Window, cx: &mut Context<Self>) {
        for dir in self.tree.open_to(path, false) {
            self.load(dir, cx);
        }
        let path: SharedString = path.to_string().into();
        self.selected = Some(path.clone());
        self.picked.clear();
        match self.index_of(&path) {
            Some(ix) => self.scroll.scroll_to_item(ix, ScrollStrategy::Center),
            None => self.revealing = Some(path),
        }
        if focus {
            self.focus.focus(window, cx);
        }
        cx.notify();
    }

    pub fn collapse_all(&mut self, cx: &mut Context<Self>) {
        self.tree.collapse_all();
        self.picked.clear();
        self.selected = self
            .selected
            .as_ref()
            .map(|s| s.split('/').next().unwrap_or("").to_string().into());
        self.rebuild(cx);
    }

    fn set_open(&mut self, path: &str, open: bool, cx: &mut Context<Self>) {
        if self.tree.set_open(path, open) {
            self.load(path.to_string().into(), cx);
        }
        self.rebuild(cx);
    }

    fn activate(&mut self, node: &Node, pin: bool, window: &mut Window, cx: &mut Context<Self>) {
        // The filter holds its folders open around their matches.
        if node.is_dir && self.filtering() {
            return;
        }
        if node.is_dir {
            let open = !self.tree.is_open(&node.path);
            self.set_open(&node.path, open, cx);
        } else {
            open_file(
                &self.workspace,
                &node.path,
                OpenOptions {
                    preview: !pin,
                    ..Default::default()
                },
                window,
                cx,
            );
        }
    }

    fn click(&mut self, ix: usize, e: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(node) = self.rows.get(ix).map(|r| r.node.clone()) else {
            return;
        };
        let m = e.modifiers();
        self.focus.focus(window, cx);
        if m.secondary() {
            if self.picked.is_empty()
                && let Some(s) = self.selected.clone()
            {
                self.picked.insert(s);
            }
            if !self.picked.remove(&node.path) {
                self.picked.insert(node.path.clone());
            }
            self.anchor = Some(node.path.clone());
        } else if m.shift {
            let anchor = self
                .anchor
                .clone()
                .filter(|a| self.row_index(a).is_some())
                .or(self.selected.clone())
                .unwrap_or(node.path.clone());
            self.picked = self.range(&anchor, &node.path);
            self.anchor = Some(anchor);
        } else {
            self.picked.clear();
            self.anchor = Some(node.path.clone());
            self.activate(&node, e.click_count() >= 2, window, cx);
        }
        self.selected = Some(node.path);
        cx.notify();
    }

    /// The paths from `from` to `to` in the order they show.
    fn range(&self, from: &str, to: &str) -> HashSet<SharedString> {
        match (self.row_index(from), self.row_index(to)) {
            (Some(i), Some(j)) => self.rows[i.min(j)..=i.max(j)]
                .iter()
                .map(|r| r.node.path.clone())
                .collect(),
            _ => HashSet::from([SharedString::from(to.to_string())]),
        }
    }

    fn on_key(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.editing.is_some() {
            return;
        }
        let ks = &e.keystroke;
        if ks.modifiers.alt || ks.modifiers.platform || ks.modifiers.control {
            return;
        }
        if ks.key == "f10" && ks.modifiers.shift {
            cx.stop_propagation();
            return self.open_keyboard_menu(window, cx);
        }
        let cur = self.selected.as_ref().and_then(|s| self.row_index(s));
        if matches!(ks.key.as_str(), "pageup" | "pagedown") && !self.rows.is_empty() {
            cx.stop_propagation();
            let page = ((f32::from(self.list_height.get()) / ROW_HEIGHT) as isize - 1).max(1);
            let at = cur.map_or(0, |c| c as isize);
            let to = if ks.key == "pageup" {
                at - page
            } else {
                at + page
            };
            let to = to.clamp(0, self.rows.len() as isize - 1) as usize;
            let path = self.rows[to].node.path.clone();
            self.picked.clear();
            self.selected = Some(path.clone());
            if let Some(ix) = self.index_of(&path) {
                self.scroll.scroll_to_item(ix, ScrollStrategy::Nearest);
            }
            cx.notify();
            return;
        }
        if ks.key == "escape" && !self.picked.is_empty() {
            self.picked.clear();
            cx.notify();
            cx.stop_propagation();
            return;
        }
        let tree = &self.tree;
        let Some(k) = tree_key(&ks.key, &self.rows, cur, |p| tree.is_open(p)) else {
            return;
        };
        cx.stop_propagation();
        match k {
            TreeKey::Move(to) => {
                if self.rows.is_empty() {
                    return;
                }
                let to = to.clamp(0, self.rows.len() as isize - 1) as usize;
                let path = self.rows[to].node.path.clone();
                if ks.modifiers.shift {
                    let anchor = self
                        .anchor
                        .clone()
                        .or(self.selected.clone())
                        .unwrap_or(path.clone());
                    self.picked = self.range(&anchor, &path);
                    self.anchor = Some(anchor);
                } else {
                    self.picked.clear();
                }
                self.selected = Some(path.clone());
                if let Some(ix) = self.index_of(&path) {
                    self.scroll.scroll_to_item(ix, ScrollStrategy::Nearest);
                }
                cx.notify();
            }
            TreeKey::Open(path) => self.set_open(&path, true, cx),
            TreeKey::Close(path) => {
                self.picked.clear();
                self.set_open(&path, false, cx);
            }
            TreeKey::Select(path) => {
                self.selected = Some(path);
                cx.notify();
            }
            TreeKey::Activate {
                node,
                pin,
                focus_code,
            } => {
                self.activate(&node, pin, window, cx);
                if focus_code {
                    _ = self
                        .workspace
                        .update(cx, |ws, cx| ws.focus_center(window, cx));
                }
            }
        }
    }

    /// The menu's or the keys' targets: the picked rows when `node` is one of them.
    fn targets(&self, node: Option<&Node>) -> Vec<Node> {
        match node {
            Some(n) if self.picked.contains(&n.path) && self.picked.len() > 1 => self
                .rows
                .iter()
                .filter(|r| self.picked.contains(&r.node.path))
                .map(|r| r.node.clone())
                .collect(),
            Some(n) => vec![n.clone()],
            None => vec![],
        }
    }

    // MARK: Editing

    fn start_editing(&mut self, what: Editing, window: &mut Window, cx: &mut Context<Self>) {
        let initial = match &what {
            Editing::Rename { node } => node.name.to_string(),
            Editing::New { .. } => String::new(),
        };
        if let Editing::New { parent, .. } = &what
            && !parent.is_empty()
            && !self.tree.is_open(parent)
        {
            let parent = parent.clone();
            self.set_open(&parent, true, cx);
        }
        let stem = match &what {
            Editing::Rename { node } if !node.is_dir => initial
                .rfind('.')
                .filter(|i| *i > 0)
                .unwrap_or(initial.len()),
            _ => initial.len(),
        };
        let input = cx.new(|cx| InputState::new(window, cx).default_value(initial));
        input.update(cx, |s, cx| {
            s.set_selected_range(0..stem, cx);
            s.focus(window, cx);
        });
        let sub = cx.subscribe_in(
            &input,
            window,
            |this, input, e: &InputEvent, window, cx| match e {
                InputEvent::PressEnter { .. } => {
                    let name = input.read(cx).value().to_string();
                    this.finish_editing(Some(name), window, cx);
                }
                InputEvent::Blur => {
                    let name = input.read(cx).value().to_string();
                    this.finish_editing(Some(name), window, cx);
                }
                _ => {}
            },
        );
        self.editing = Some(Edit {
            what,
            input,
            _sub: sub,
        });
        self.items = items(&self.rows, self.editing.as_ref().map(|e| &e.what));
        cx.notify();
    }

    fn cancel_editing(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.editing.take().is_some() {
            self.items = items(&self.rows, None);
            self.focus.focus(window, cx);
            cx.notify();
        }
    }

    fn finish_editing(
        &mut self,
        name: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(edit) = self.editing.take() else {
            return;
        };
        self.items = items(&self.rows, None);
        self.focus.focus(window, cx);
        cx.notify();
        let name = name.unwrap_or_default().trim().to_string();
        if name.is_empty()
            || matches!(&edit.what, Editing::Rename { node } if node.name.as_ref() == name)
        {
            return;
        }
        // A new entry's name may start with folders ("src/a.ts"), made along with it; a rename's may not.
        let parts: Vec<&str> = match &edit.what {
            Editing::New { .. } => name.split('/').collect(),
            Editing::Rename { .. } => vec![name.as_str()],
        };
        if parts
            .iter()
            .any(|p| p.is_empty() || *p == "." || *p == ".." || p.contains('/'))
        {
            self.toast(
                Toast::error(format!("\"{name}\" is not a valid file or folder name."))
                    .title("Invalid name"),
                cx,
            );
            return;
        }
        let dir: SharedString = match &edit.what {
            Editing::Rename { node } => dirname(&node.path).to_string().into(),
            Editing::New { parent, .. } => parent.clone(),
        };
        let path = if dir.is_empty() {
            name.clone()
        } else {
            format!("{dir}/{name}")
        };
        let core = self.core.clone();
        let what = edit.what;
        let deep = parts.len() > 1;
        self._work = cx.spawn_in(window, async move |this, cx| {
            let (from, is_new_file) = match &what {
                Editing::Rename { node } => (Some(node.path.to_string()), false),
                Editing::New { is_dir, .. } => (None, !*is_dir),
            };
            let target = path.clone();
            let is_dir = matches!(what, Editing::New { is_dir: true, .. });
            let from_bg = from.clone();
            let result = cx
                .background_spawn(async move {
                    match from_bg {
                        Some(from) => api::files::rename_path(&core, from, target),
                        None if is_dir => api::files::create_dir(&core, target),
                        None => api::files::create_file(&core, target),
                    }
                })
                .await;
            _ = this.update_in(cx, |this, window, cx| {
                if let Err(e) = result {
                    let title = match (&from, is_dir) {
                        (Some(_), _) => "Rename failed",
                        (None, true) => "Could not create folder",
                        (None, false) => "Could not create file",
                    };
                    this.toast(Toast::error(e).title(title), cx);
                    return;
                }
                if let Some(from) = &from {
                    for p in this.tree.renamed(from, &path) {
                        this.load(p, cx);
                    }
                    crate::path_moved(&this.workspace, from, Some(&path), window, cx);
                } else if deep {
                    for d in this.tree.open_to(&path, false) {
                        this.load(d, cx);
                    }
                }
                if is_new_file {
                    open_file(
                        &this.workspace,
                        &path,
                        OpenOptions {
                            preview: false,
                            ..Default::default()
                        },
                        window,
                        cx,
                    );
                }
                this.selected = Some(path.clone().into());
                this.revealing = Some(path.clone().into());
                this.load(dir.clone(), cx);
            });
        });
    }

    // MARK: Actions

    fn delete(&mut self, list: Vec<Node>, window: &mut Window, cx: &mut Context<Self>) {
        // A folder's entries go with it.
        let inside = |p: &str, d: &str| p == d || p.starts_with(&format!("{d}/"));
        let top: Vec<Node> = list
            .iter()
            .filter(|e| {
                !list
                    .iter()
                    .any(|d| d.path != e.path && d.is_dir && inside(&e.path, &d.path))
            })
            .cloned()
            .collect();
        let Some(one) = top.first().cloned() else {
            return;
        };
        let many = top.len() > 1;
        let message = if many {
            format!(
                "Move {} items to the Trash?{}",
                top.len(),
                if top.iter().any(|e| e.is_dir) {
                    " Everything inside the folders goes too."
                } else {
                    ""
                }
            )
        } else {
            format!(
                "Move \"{}\" to the Trash?{}",
                one.path,
                if one.is_dir {
                    " Everything inside it goes too."
                } else {
                    ""
                }
            )
        };
        let answer = window.prompt(
            PromptLevel::Warning,
            &message,
            None,
            &["Move to Trash", "Cancel"],
            cx,
        );
        let core = self.core.clone();
        self._work = cx.spawn_in(window, async move |this, cx| {
            if answer.await != Ok(0) {
                return;
            }
            let paths: Vec<String> = top.iter().map(|e| e.path.to_string()).collect();
            let done = cx
                .background_spawn(async move {
                    let mut gone = Vec::new();
                    for p in paths {
                        match api::files::trash_path(&core, p.clone()) {
                            Ok(()) => gone.push(p),
                            Err(e) => return (gone, Some(e)),
                        }
                    }
                    (gone, None)
                })
                .await;
            _ = this.update_in(cx, |this, window, cx| {
                let (gone, err) = done;
                if let Some(e) = err {
                    this.toast(Toast::error(e).title("Could not move to Trash"), cx);
                }
                for p in &gone {
                    crate::path_moved(&this.workspace, p, None, window, cx);
                }
                // Land on the next row outside what went.
                let kept = |r: &Row| !gone.iter().any(|g| inside(&r.node.path, g));
                let at = this.rows.iter().position(|r| !kept(r)).unwrap_or(0);
                let next = this.rows[at..]
                    .iter()
                    .find(|r| kept(r))
                    .or_else(|| this.rows[..at].iter().rev().find(|r| kept(r)));
                this.selected = next.map(|r| r.node.path.clone());
                this.picked.clear();
                let dirs: HashSet<String> = gone.iter().map(|p| dirname(p).to_string()).collect();
                for d in dirs {
                    this.load(d.into(), cx);
                }
            });
        });
    }

    fn copy_paths(&self, targets: &[Node], absolute: bool, cx: &mut App) {
        let text = if targets.is_empty() {
            self.root.to_string_lossy().into_owned()
        } else {
            targets
                .iter()
                .map(|n| {
                    if absolute {
                        self.root
                            .join(n.path.as_ref())
                            .to_string_lossy()
                            .into_owned()
                    } else {
                        n.path.to_string()
                    }
                })
                .collect::<Vec<_>>()
                .join("\n")
        };
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        let what = match (targets.len() > 1, absolute) {
            (true, true) => format!("{} paths copied", targets.len()),
            (true, false) => format!("{} relative paths copied", targets.len()),
            (false, true) => "Path copied".into(),
            (false, false) => "Relative path copied".into(),
        };
        self.toast(Toast::success(what), cx);
    }

    fn reveal_in_finder(&self, path: &str, cx: &mut Context<Self>) {
        let core = self.core.clone();
        let path = path.to_string();
        cx.background_spawn(async move {
            _ = api::files::reveal_path(&core, path);
        })
        .detach();
    }

    fn show_history(&self, path: &str, file: bool, window: &mut Window, cx: &mut App) {
        match cx.try_global::<HistoryHook>().map(|h| h.0.clone()) {
            Some(show) => show(path, file, window, cx),
            None => self.toast(Toast::info("History comes with the history view."), cx),
        }
    }

    /// The file's (or folder's) page on GitHub at the last commit of this branch there: copied, or
    /// opened.
    fn github_link(&mut self, path: &str, is_dir: bool, open: bool, cx: &mut Context<Self>) {
        let Some(web) = self.web_url.clone() else {
            return;
        };
        let (core, path) = (self.core.clone(), path.to_string());
        self._work_link_spawn(cx, async move |this, cx| {
            let link = cx.background_spawn({
                let path = path.clone();
                async move { gitviber_core::api::github::github_permalink(&core, path, None) }
            });
            let link = link.await;
            _ = this.update(cx, |this, cx| match link {
                Ok(p) => {
                    let url = permalink_url(&web, &p.sha, &path, p.tree || is_dir, None);
                    if open {
                        cx.open_url(&url);
                    } else {
                        cx.write_to_clipboard(ClipboardItem::new_string(url));
                        this.toast(Toast::success("Link copied"), cx);
                    }
                }
                Err(e) => this.toast(Toast::info(e).title("Not on GitHub yet"), cx),
            });
        });
    }

    #[cfg(target_os = "macos")]
    fn copy_files(&mut self, paths: Vec<String>, cx: &mut Context<Self>) {
        let core = self.core.clone();
        let n = paths.len();
        self._work_link_spawn(cx, async move |this, cx| {
            let copied = cx
                .background_spawn(async move { gitviber_core::api::app::copy_files(&core, paths) })
                .await;
            _ = this.update(cx, |this, cx| match copied {
                Ok(()) => this.toast(
                    Toast::success(if n == 1 {
                        "File copied".to_string()
                    } else {
                        format!("{n} files copied")
                    }),
                    cx,
                ),
                Err(e) => this.toast(Toast::error(e).title("Could not copy"), cx),
            });
        });
    }

    /// Discard Changes, asked first: the current versions go to the Trash, and Undo puts them back.
    fn discard(&mut self, paths: Vec<SharedString>, window: &mut Window, cx: &mut Context<Self>) {
        let what = if paths.len() > 1 {
            format!("{} files", paths.len())
        } else {
            paths[0].to_string()
        };
        let detail = if paths.len() > 1 {
            "Their current versions are moved to the Trash."
        } else {
            "Its current version is moved to the Trash."
        };
        let answer = window.prompt(
            PromptLevel::Warning,
            &format!("Discard changes to {what}?"),
            Some(detail),
            &["Discard", "Cancel"],
            cx,
        );
        let core = self.core.clone();
        self._work = cx.spawn_in(window, async move |this, cx| {
            if answer.await != Ok(0) {
                return;
            }
            let list: Vec<String> = paths.iter().map(|p| p.to_string()).collect();
            let undo_core = core.clone();
            let done = cx
                .background_spawn(async move {
                    let before = gitviber_core::api::journal::journal_last(&core)
                        .ok()
                        .flatten();
                    gitviber_core::api::changes::discard(&core, list)?;
                    let after = gitviber_core::api::journal::journal_last(&core)
                        .ok()
                        .flatten();
                    Ok::<_, String>(after.filter(|a| Some(*a) != before))
                })
                .await;
            _ = this.update(cx, |this, cx| match done {
                Ok(entry) => {
                    let mut toast =
                        Toast::success(format!("Discarded {what}")).title(if paths.len() > 1 {
                            "The old versions are in the Trash."
                        } else {
                            "The old version is in the Trash."
                        });
                    if let Some(id) = entry {
                        toast.action = Some(gitviber_ui::workspace::ToastAction {
                            label: "Undo".into(),
                            run: Rc::new(move |_, cx| {
                                let core = undo_core.clone();
                                cx.background_spawn(async move {
                                    _ = gitviber_core::api::journal::undo(&core, Some(id));
                                })
                                .detach();
                            }),
                        });
                    }
                    this.toast(toast, cx);
                }
                Err(e) => this.toast(Toast::error(e).title("Discard failed"), cx),
            });
        });
    }

    fn _work_link_spawn(
        &mut self,
        cx: &mut Context<Self>,
        f: impl AsyncFnOnce(WeakEntity<Self>, &mut gpui_kit::AsyncApp) + 'static,
    ) {
        self._work = cx.spawn(f);
    }

    fn compare(&self, left: &str, right: &str, window: &mut Window, cx: &mut App) {
        crate::compare_files(&self.workspace, left, right, window, cx);
    }

    /// A row's menu (`node`), or the empty space's (None), built in the menu's own context:
    /// Open in… makes a submenu of it.
    fn build_menu(
        this: &Entity<Self>,
        node: Option<Node>,
        menu: PopupMenu,
        window: &mut Window,
        cx: &mut Context<PopupMenu>,
    ) -> PopupMenu {
        let (menu, open_in) = this.update(cx, |this, cx| this.menu(node.clone(), menu, cx));
        let menu = match open_in {
            Some(path) => gitviber_ui::open_in::menu_items(
                menu,
                move |_| gitviber_ui::open_in::OpenTarget::file(path.clone()),
                window,
                cx,
            ),
            None => menu,
        };
        this.update(cx, |this, cx| this.menu_rest(node, menu, cx))
    }

    /// The menu down to Reveal in Finder, and the path Open in… opens next (one target only).
    fn menu(
        &mut self,
        node: Option<Node>,
        menu: PopupMenu,
        cx: &mut Context<Self>,
    ) -> (PopupMenu, Option<String>) {
        if let Some(n) = &node {
            self.selected = Some(n.path.clone());
            if !self.picked.contains(&n.path) {
                self.picked.clear();
            }
        } else {
            self.picked.clear();
        }
        let targets = self.targets(node.as_ref());
        let multi = targets.len() > 1;
        let files: Vec<Node> = targets.iter().filter(|n| !n.is_dir).cloned().collect();
        let me = cx.weak_entity();
        let item = |label: &str, f: MenuAction| menu_item(&me, label, f);
        let mut menu = menu;
        if let Some(n) = node.clone().filter(|n| !n.is_dir && !multi) {
            let open = n.clone();
            menu = menu
                .item(item(
                    "Open",
                    Rc::new(move |this, w, cx| this.activate(&open, true, w, cx)),
                ))
                .separator();
            if self.mark.as_ref() == Some(&n.path) {
                menu = menu.item(item(
                    "Clear Compare Selection",
                    Rc::new(|this, _, cx| {
                        this.mark = None;
                        cx.notify();
                    }),
                ));
            } else {
                let path = n.path.clone();
                menu = menu.item(item(
                    "Select for Compare",
                    Rc::new(move |this, _, cx| {
                        this.mark = Some(path.clone());
                        cx.notify();
                    }),
                ));
            }
            if let Some(marked) = self.mark.clone().filter(|m| *m != n.path) {
                let name = marked.rsplit('/').next().unwrap_or(&marked).to_string();
                let right = n.path.clone();
                menu = menu.item(item(
                    &format!("Compare with {name}"),
                    Rc::new(move |this, w, cx| this.compare(&marked, &right, w, cx)),
                ));
            }
            menu = menu.separator();
        }
        if multi && targets.len() == 2 && files.len() == 2 {
            let (a, b) = (files[0].path.clone(), files[1].path.clone());
            menu = menu
                .item(item(
                    "Compare Selected",
                    Rc::new(move |this, w, cx| this.compare(&a, &b, w, cx)),
                ))
                .separator();
        }
        let dir: Option<SharedString> = match &node {
            None => Some("".into()),
            Some(n) if n.is_dir && !multi => Some(n.path.clone()),
            _ => None,
        };
        if let Some(dir) = dir {
            let d2 = dir.clone();
            menu = menu
                .item(item(
                    "New File…",
                    Rc::new(move |this, w, cx| {
                        this.start_editing(
                            Editing::New {
                                parent: dir.clone(),
                                is_dir: false,
                            },
                            w,
                            cx,
                        )
                    }),
                ))
                .item(item(
                    "New Folder…",
                    Rc::new(move |this, w, cx| {
                        this.start_editing(
                            Editing::New {
                                parent: d2.clone(),
                                is_dir: true,
                            },
                            w,
                            cx,
                        )
                    }),
                ))
                .separator();
        }
        // Untracked or ignored: git has no history of it, and GitHub no copy.
        let off_git = node
            .as_ref()
            .is_some_and(|n| n.ignored || self.status.get(&n.path) == Some(&ChangeKind::Untracked));
        if let Some(n) = node.clone().filter(|_| !multi) {
            let (path, file) = (n.path.to_string(), !n.is_dir);
            menu = menu
                .item(
                    item(
                        "Show History",
                        Rc::new(move |this, w, cx| this.show_history(&path, file, w, cx)),
                    )
                    .disabled(off_git),
                )
                .separator();
        }
        if multi {
            return (menu, None);
        }
        let path = node
            .as_ref()
            .map(|n| n.path.to_string())
            .unwrap_or_default();
        let reveal = path.clone();
        menu = menu.item(item(
            REVEAL_LABEL,
            Rc::new(move |this, _, cx| this.reveal_in_finder(&reveal, cx)),
        ));
        (menu, Some(path))
    }

    /// The menu from Copy GitHub Link on.
    fn menu_rest(
        &mut self,
        node: Option<Node>,
        menu: PopupMenu,
        cx: &mut Context<Self>,
    ) -> PopupMenu {
        let targets = self.targets(node.as_ref());
        let multi = targets.len() > 1;
        let files: Vec<Node> = targets.iter().filter(|n| !n.is_dir).cloned().collect();
        let me = cx.weak_entity();
        let item = |label: &str, f: MenuAction| menu_item(&me, label, f);
        let mut menu = menu;
        let off_git = node
            .as_ref()
            .is_some_and(|n| n.ignored || self.status.get(&n.path) == Some(&ChangeKind::Untracked));
        if !multi {
            let path = node
                .as_ref()
                .map(|n| n.path.to_string())
                .unwrap_or_default();
            if self.web_url.is_some() {
                let (p1, p2) = (path.clone(), path.clone());
                let is_dir = node.as_ref().is_some_and(|n| n.is_dir) || node.is_none();
                menu = menu
                    .item(
                        item(
                            "Copy GitHub Link",
                            Rc::new(move |this, _, cx| this.github_link(&p1, is_dir, false, cx)),
                        )
                        .disabled(off_git),
                    )
                    .item(
                        item(
                            "Open on GitHub",
                            Rc::new(move |this, _, cx| this.github_link(&p2, is_dir, true, cx)),
                        )
                        .disabled(off_git),
                    );
            }
        }
        #[cfg(target_os = "macos")]
        if !files.is_empty() {
            let abs: Vec<String> = files
                .iter()
                .map(|n| {
                    self.root
                        .join(n.path.as_ref())
                        .to_string_lossy()
                        .into_owned()
                })
                .collect();
            let label = if files.len() == 1 {
                format!("Copy “{}”", files[0].name)
            } else {
                format!("Copy {} Files", files.len())
            };
            menu = menu.item(item(
                &label,
                Rc::new(move |this, _, cx| this.copy_files(abs.clone(), cx)),
            ));
        }
        let t1 = targets.clone();
        menu = menu.item(item(
            if multi { "Copy Paths" } else { "Copy Path" },
            Rc::new(move |this, _, cx| this.copy_paths(&t1, true, cx)),
        ));
        if node.is_some() {
            let t2 = targets.clone();
            menu = menu
                .item(item(
                    if multi {
                        "Copy Relative Paths"
                    } else {
                        "Copy Relative Path"
                    },
                    Rc::new(move |this, _, cx| this.copy_paths(&t2, false, cx)),
                ))
                .separator();
            if let Some(n) = node.clone().filter(|_| !multi) {
                menu = menu.item(item(
                    "Rename…",
                    Rc::new(move |this, w, cx| {
                        this.start_editing(Editing::Rename { node: n.clone() }, w, cx)
                    }),
                ));
            }
            let discard: Vec<SharedString> = targets
                .iter()
                .filter(|n| self.discardable.contains(&n.path))
                .map(|n| n.path.clone())
                .collect();
            if !discard.is_empty() {
                let label = if discard.len() > 1 {
                    format!("Discard Changes to {} Files", discard.len())
                } else {
                    "Discard Changes".to_string()
                };
                menu = menu.item(item(
                    &label,
                    Rc::new(move |this, w, cx| this.discard(discard.clone(), w, cx)),
                ));
            }
            let t3 = targets.clone();
            let label = if multi {
                format!("Delete {} Items", targets.len())
            } else {
                "Delete".to_string()
            };
            menu = menu.item(item(
                &label,
                Rc::new(move |this, w, cx| this.delete(t3.clone(), w, cx)),
            ));
        }
        cx.notify();
        menu
    }

    fn render_row(
        &self,
        ix: usize,
        active: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let _ = window;
        let t = cx.app_theme();
        let item = self.items[ix].clone();
        let (depth, node) = match &item {
            Item::Row(i) => (self.rows[*i].depth, Some(self.rows[*i].node.clone())),
            Item::NewInput { depth, .. } => (*depth, None),
        };
        let pad = 8. + depth as f32 * INDENT;
        let guides = (0..depth).map(|i| {
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .w(px(1.))
                .left(px(14. + i as f32 * INDENT))
                .bg(t.border)
        });
        let base = div()
            .id(("explorer-row", ix))
            .relative()
            .flex()
            .items_center()
            .gap(px(6.))
            .h(px(ROW_HEIGHT))
            .pl(px(pad))
            .pr_2()
            .text_size(px(12.))
            .children(guides);
        let Some(node) = node else {
            let Item::NewInput { is_dir, .. } = item else {
                unreachable!()
            };
            let input = self.editing.as_ref().map(|e| e.input.clone());
            return base
                .child(div().w(px(12.)).flex_none())
                .child(if is_dir {
                    FileIcon::folder("", false)
                } else {
                    FileIcon::file("")
                })
                .children(input.map(|i| {
                    div()
                        .flex_1()
                        .child(Input::new(&i).xsmall().appearance(false))
                }))
                .into_any_element();
        };
        let open = node.is_dir && self.tree.is_open(&node.path);
        let status = self.status.get(&node.path).copied();
        let tone = if node.ignored {
            Some(t.subtle)
        } else {
            status.map(|s| s.color(t))
        };
        let is_active = !node.is_dir && active == Some(node.path.as_ref());
        let picked = self.picked.contains(&node.path);
        let selected = self.selected.as_ref() == Some(&node.path);
        let focused = self.focus.is_focused(window);
        let renaming = matches!(&self.editing, Some(Edit { what: Editing::Rename { node: n }, .. }) if n.path == node.path);
        let row_ix = match item {
            Item::Row(i) => i,
            _ => 0,
        };
        let menu_node = node.clone();
        let me = cx.weak_entity();
        base.cursor_pointer()
            .when(is_active || picked, |d| d.bg(t.primary.opacity(0.15)))
            .when(!(is_active || picked), |d| d.hover(|s| s.bg(t.hover)))
            .when(selected && focused, |d| {
                d.bg(t.hover)
                    .border_1()
                    .border_color(t.primary.opacity(0.7))
            })
            .when(is_active, |d| {
                d.child(
                    div()
                        .absolute()
                        .left_0()
                        .top_0()
                        .bottom_0()
                        .w(px(2.))
                        .bg(t.primary),
                )
            })
            .child(if node.is_dir {
                gpui_kit::component::Icon::new(if open {
                    gpui_kit::assets::IconName::ChevronDown
                } else {
                    gpui_kit::assets::IconName::ChevronRight
                })
                .size(px(12.))
                .text_color(t.subtle)
                .into_any_element()
            } else {
                div().w(px(12.)).flex_none().into_any_element()
            })
            .child(
                div()
                    .flex_none()
                    .when(node.ignored, |d| d.opacity(0.4))
                    .child(if node.is_dir {
                        FileIcon::folder(&node.name, open)
                    } else {
                        FileIcon::file(&node.path)
                    }),
            )
            .map(|d| {
                if renaming {
                    let input = self.editing.as_ref().map(|e| e.input.clone());
                    d.children(input.map(|i| {
                        div()
                            .flex_1()
                            .child(Input::new(&i).xsmall().appearance(false))
                    }))
                } else {
                    d.child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(tone.unwrap_or(t.foreground.opacity(0.85)))
                            .child(node.name.clone()),
                    )
                    .when_some(status, |d, s| {
                        d.child(
                            div()
                                .flex_none()
                                .font_family(t.fonts.code.clone())
                                .text_size(px(10.5))
                                .font_weight(gpui_kit::FontWeight::BOLD)
                                .text_color(tone.unwrap_or(t.foreground))
                                .child(s.letter()),
                        )
                    })
                    .when(
                        status.is_none() && node.is_dir && self.dirty_dirs.contains(&node.path),
                        |d| {
                            d.child(
                                div()
                                    .flex_none()
                                    .size(px(6.))
                                    .rounded_full()
                                    .bg(if node.ignored { t.subtle } else { t.modified }),
                            )
                        },
                    )
                }
            })
            .on_click(cx.listener(move |this, e: &ClickEvent, window, cx| {
                this.click(row_ix, e, window, cx)
            }))
            .context_menu(move |menu, window, cx| match me.upgrade() {
                Some(this) => Self::build_menu(&this, Some(menu_node.clone()), menu, window, cx),
                None => menu,
            })
            .into_any_element()
    }
}

/// GitHub's page of `path` at `sha`: a file's blob (at `lines`, `#L10-L24`) or a folder's tree.
pub fn permalink_url(
    web: &str,
    sha: &str,
    path: &str,
    tree: bool,
    lines: Option<(u32, u32)>,
) -> String {
    let at: String = if path.is_empty() {
        String::new()
    } else {
        path.split('/')
            .map(|seg| format!("/{}", encode_segment(seg)))
            .collect()
    };
    let anchor = match lines {
        Some((a, b)) if a == b => format!("#L{a}"),
        Some((a, b)) => format!("#L{a}-L{b}"),
        None => String::new(),
    };
    format!(
        "{web}/{}/{sha}{at}{anchor}",
        if tree { "tree" } else { "blob" }
    )
}

/// A path segment percent-encoded as JavaScript's encodeURIComponent does.
fn encode_segment(seg: &str) -> String {
    seg.bytes()
        .map(|b| match b {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'!'
            | b'~'
            | b'*'
            | b'\''
            | b'('
            | b')' => (b as char).to_string(),
            b => format!("%{b:02X}"),
        })
        .collect()
}

fn insert_dir(set: &mut HashSet<SharedString>, dir: &str) {
    if !set.contains(dir) {
        set.insert(dir.to_string().into());
    }
}

/// The rows with a name box where a new entry goes: first in its folder.
fn items(rows: &[Row], editing: Option<&Editing>) -> Vec<Item> {
    let mut out: Vec<Item> = Vec::with_capacity(rows.len() + 1);
    let new = match editing {
        Some(Editing::New { parent, is_dir }) => Some((parent.clone(), *is_dir)),
        _ => None,
    };
    if let Some((parent, is_dir)) = &new
        && parent.is_empty()
    {
        out.push(Item::NewInput {
            depth: 0,
            is_dir: *is_dir,
        });
    }
    for (i, row) in rows.iter().enumerate() {
        out.push(Item::Row(i));
        if let Some((parent, is_dir)) = &new
            && row.node.is_dir
            && row.node.path == *parent
        {
            out.push(Item::NewInput {
                depth: row.depth + 1,
                is_dir: *is_dir,
            });
        }
    }
    out
}

const REVEAL_LABEL: &str = if cfg!(target_os = "macos") {
    "Reveal in Finder"
} else if cfg!(windows) {
    "Show in Explorer"
} else {
    "Show in Folder"
};

impl Render for ExplorerPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _ = window;
        let t = cx.app_theme();
        let active: Option<SharedString> = self.workspace.upgrade().and_then(|ws| {
            let key = ws.read(cx).tabs().active_key()?.clone();
            crate::path_of_key(&key).map(|p| SharedString::from(p.to_string()))
        });
        let count = self.items.len();
        let me = cx.weak_entity();
        div()
            .id("explorer")
            .key_context("Explorer")
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .flex_col()
            .text_color(t.foreground)
            .on_key_down(
                cx.listener(|this, e: &KeyDownEvent, window, cx| this.on_key(e, window, cx)),
            )
            .on_action(cx.listener(|this, _: &explorer::Rename, window, cx| {
                let node = this
                    .selected
                    .as_ref()
                    .and_then(|s| this.row_index(s))
                    .map(|i| this.rows[i].node.clone());
                if let Some(node) = node {
                    this.start_editing(Editing::Rename { node }, window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &explorer::Delete, window, cx| {
                let node = this
                    .selected
                    .as_ref()
                    .and_then(|s| this.row_index(s))
                    .map(|i| this.rows[i].node.clone());
                let targets = this.targets(node.as_ref());
                if !targets.is_empty() {
                    this.delete(targets, window, cx);
                }
            }))
            .on_action(
                cx.listener(|this, _: &gpui_kit::component::input::Escape, window, cx| {
                    this.cancel_editing(window, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &gitviber_ui::keymap::editor::Find, window, cx| {
                    this.open_filter(window, cx)
                }),
            )
            .children(self.filter.as_ref().map(|f| {
                let input = f.input.clone();
                div()
                    .id("explorer-filter")
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap_2()
                    .h(px(32.))
                    .px_2()
                    .border_b_1()
                    .border_color(t.border)
                    // Esc clears, then closes; ↓ or ↵ go to the rows.
                    .capture_key_down(cx.listener(|this, e: &KeyDownEvent, window, cx| {
                        let Some(f) = &this.filter else { return };
                        match e.keystroke.key.as_str() {
                            "escape" if f.needle.is_empty() => this.close_filter(window, cx),
                            "escape" => f
                                .input
                                .clone()
                                .update(cx, |s, cx| s.set_value("", window, cx)),
                            "down" | "enter" => this.focus.focus(window, cx),
                            _ => return,
                        }
                        cx.stop_propagation();
                    }))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(Input::new(&input).xsmall().appearance(false)),
                    )
            }))
            .when(
                self.filtering()
                    && self
                        .filter
                        .as_ref()
                        .is_some_and(|f| f.files.is_some() && f.found == 0),
                |d| {
                    d.child(
                        div()
                            .px_4()
                            .py_6()
                            .text_center()
                            .text_size(px(12.))
                            .text_color(t.subtle)
                            .child("No files match."),
                    )
                },
            )
            .child(
                uniform_list(
                    "explorer-rows",
                    count,
                    cx.processor(move |this, range: std::ops::Range<usize>, window, cx| {
                        range
                            .map(|ix| this.render_row(ix, active.as_deref(), window, cx))
                            .collect::<Vec<_>>()
                    }),
                )
                .track_scroll(&self.scroll)
                .flex_1()
                .py_1(),
            )
            .when(
                self.filtering() && self.filter.as_ref().is_some_and(|f| f.capped),
                |d| {
                    d.child(
                        div()
                            .px_4()
                            .py_2()
                            .text_center()
                            .text_size(px(11.))
                            .text_color(t.subtle)
                            .child(format!("Showing the first {} matches", tree::MAX_MATCHES)),
                    )
                },
            )
            // The list's height, for a page of PgUp/PgDn.
            .child({
                let height = self.list_height.clone();
                gpui_kit::canvas(
                    move |bounds, _, _| height.set(bounds.size.height),
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full()
            })
            .children(self.kb_menu.as_ref().map(|(menu, at, _)| {
                gpui_kit::deferred(
                    gpui_kit::anchored()
                        .position(*at)
                        .snap_to_window_with_margin(px(8.))
                        .child(menu.clone()),
                )
                .with_priority(1)
            }))
            // The space below the rows: the repo root's menu.
            .child(
                div()
                    .id("explorer-blank")
                    .h(px(48.))
                    .flex_none()
                    .context_menu(move |menu, window, cx| match me.upgrade() {
                        Some(this) => Self::build_menu(&this, None, menu, window, cx),
                        None => menu,
                    }),
            )
    }
}
