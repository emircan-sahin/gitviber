//! The terminal panel: a set of tabs per worktree, each tab one pane or a split of them, with the
//! toolbar, numbered pane headers, zoom, find and the keys around them.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gitviber_core::Core;
use gpui::{
    AnyElement, AnyView, App, AppContext as _, Bounds, Context, DispatchPhase, Entity,
    EventEmitter, FocusHandle, Focusable, InteractiveElement as _, IntoElement, MouseButton,
    MouseMoveEvent, MouseUpEvent, ParentElement as _, PathPromptOptions, Pixels, Point,
    PromptLevel, Render, SharedString, StatefulInteractiveElement as _, StyleRefinement,
    Styled as _, Subscription, Task, WeakEntity, Window, canvas, div, prelude::FluentBuilder as _,
    px,
};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::{Disableable as _, Icon, Sizable as _, h_flex, v_flex};

use crate::config::{CursorStyle, OptionAsMeta, TerminalConfig, TerminalTheme, ansi_colors};
use crate::find::FindOptions;
use crate::layout::{self, Dir, Direction, Layout, PaneId, Rect};
use crate::linkhost::LinkSite;
use crate::marks::CommandEnd;
use crate::pane::{FindResult, LinkCtx, PaneEvent, TerminalPane, new_pane};
use crate::{CloseFind, FindNext, FindPrev, PANEL_CONTEXT};
use gitviber_ui::actions::{editor, tab, terminal};
use gitviber_ui::settings::{OptionKey, TerminalCursor};
use gitviber_ui::workspace::{
    PanelContext, PanelSpec, Project, ProjectEvent, Slot, Workspace, WorkspaceEvent,
};
use gitviber_ui::{ActiveAppTheme as _, AppTheme, Settings};

mod agents_menu;
mod attention;
mod resume;
mod saved;

pub use attention::{agents_in, paste_to_agent, worktree_agent};
pub(crate) use saved::import_legacy;

/// A worktree the panel can open a terminal in, as the app lists them.
#[derive(Clone, Debug, PartialEq)]
pub struct WorktreeInfo {
    pub path: PathBuf,
    pub branch: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PanelEvent {
    /// The open worktree's last terminal closed: the app hides the panel.
    Empty,
    /// The panel covers the workspace, or is back in its place.
    MaximizeChanged(bool),
    /// The toolbar's hide button.
    Hide,
    /// ⌘= / ⌘− / ⌘0 or a pinch sized the font: the app keeps it as `terminalFontSize`.
    FontSize(f32),
    CommandEnded {
        pane: PaneId,
        end: CommandEnd,
    },
    Bell {
        pane: PaneId,
    },
}

/// A tab: one or more panes, split right and down.
struct Group {
    id: u64,
    /// The user's name for the tab, over the folder's.
    name: Option<String>,
    /// In the layout's tree order.
    panes: Vec<PaneId>,
    layout: Layout,
    focused: PaneId,
}

#[derive(Default)]
struct TabSet {
    groups: Vec<Group>,
    active: Option<u64>,
}

struct PaneSlot {
    view: Entity<TerminalPane>,
    /// The user's name for it, over the program's title in its header.
    name: Option<String>,
    /// It needs the user: it rang, sent a notification, or its agent finished or asked, while
    /// not looked at.
    rang: bool,
    /// That was told to the OS: once until it's looked at.
    told: bool,
    /// The coding agent running in it, until the shell prompts again.
    agent: Option<crate::agent::PaneAgent>,
    /// When its agent was first seen or last changed state (Unix seconds).
    since: f64,
    /// Its history's file in the session save, and whether it's been written.
    history_file: Option<String>,
    history_saved: bool,
    /// When its history was last serialized (ms since `epoch`).
    serialized_at: u64,
    _events: Subscription,
}

#[derive(Clone, Copy, PartialEq)]
enum RenameTarget {
    Pane(PaneId),
    Tab(u64),
}

struct Rename {
    target: RenameTarget,
    input: Entity<InputState>,
    _events: Subscription,
}

struct FindBar {
    input: Entity<InputState>,
    options: FindOptions,
    result: Result<FindResult, String>,
    pane: Option<PaneId>,
    _events: Subscription,
}

struct DividerDrag {
    group: u64,
    path: Vec<usize>,
    index: usize,
    dir: Dir,
    start: Point<Pixels>,
    sizes: Vec<f64>,
    /// The split's room for its children, dividers left out.
    free: f32,
}

/// Each split's bounds as last laid out, for its dividers' drags.
type SplitBounds = Rc<RefCell<HashMap<(u64, Vec<usize>), Bounds<Pixels>>>>;

const HEADER_HEIGHT: f32 = 24.;
const TOOLBAR_HEIGHT: f32 = 36.;
/// A pane's least room as a divider is dragged.
const MIN_WIDTH: f32 = 160.;
const MIN_HEIGHT: f32 = 80.;

pub struct TerminalPanel {
    core: Arc<Core>,
    config: TerminalConfig,
    theme: TerminalTheme,
    root: PathBuf,
    sets: HashMap<PathBuf, TabSet>,
    panes: HashMap<PaneId, PaneSlot>,
    next_id: u64,
    maximized: bool,
    zoomed: bool,
    worktrees: Vec<WorktreeInfo>,
    find: Option<FindBar>,
    rename: Option<Rename>,
    flash: Option<(PaneId, Task<()>)>,
    /// A link being followed that asked git or GitHub first.
    follow: Option<Task<()>>,
    link_cache: crate::linkhost::SharedCache,
    link_ctx: Option<Rc<LinkCtx>>,
    /// Agent lookups in flight.
    agent_tasks: Vec<Task<()>>,
    /// The session save's next round.
    round: Option<Task<()>>,
    epoch: std::time::Instant,
    /// The worktrees whose saved terminals this panel took up, and so saves.
    restored_here: std::collections::HashSet<PathBuf>,
    divider: Option<DividerDrag>,
    split_bounds: SplitBounds,
    killing: bool,
    focus: FocusHandle,
    /// Set when the panel lives in a workspace: its project's worktree, Settings and theme.
    bound: Option<Bound>,
}

struct Bound {
    workspace: WeakEntity<Workspace>,
    _project: Option<Subscription>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<PanelEvent> for TerminalPanel {}

impl gitviber_ui::workspace::Panel for TerminalPanel {
    fn panel_focus(&self, cx: &App) -> Option<FocusHandle> {
        self.active_pane().map(|p| p.read(cx).focus_handle(cx))
    }
}

/// The workspaces' panels, weakly: refs asks how many terminals run in a folder.
#[derive(Default)]
struct LivePanels(Vec<WeakEntity<TerminalPanel>>);

impl gpui::Global for LivePanels {}

/// The terminal in every workspace's bottom slot, kept across project switches.
pub(crate) fn register(cx: &mut App) {
    gitviber_refs::hooks::set_terminals_in(
        cx,
        Rc::new(|dir: &str, cx: &App| {
            let Some(live) = cx.try_global::<LivePanels>() else {
                return 0;
            };
            let dir = Path::new(dir);
            live.0
                .iter()
                .filter_map(|p| p.upgrade())
                .map(|p| p.read(cx).running_in(dir, cx))
                .sum()
        }),
    );
    gitviber_refs::hooks::set_agents_in(
        cx,
        Rc::new(|dir: &str, cx: &App| agents_in(Path::new(dir), cx)),
    );
    agents_menu::register(cx);
    // From anywhere in a window: the open worktree's conversations.
    cx.on_action(|_: &terminal::ResumeConversation, cx| {
        let Some(handle) = cx.active_window() else {
            return;
        };
        cx.defer(move |cx| {
            let _ = handle.update(cx, |_, window, cx| {
                let Some(ws) = Workspace::for_window(window, cx) else {
                    return;
                };
                let panel = ws.update(cx, |ws, cx| ws.panel("terminal", window, cx));
                if let Some(p) = panel.and_then(|p| p.view().downcast::<TerminalPanel>().ok()) {
                    p.update(cx, |p, cx| p.ask_conversation(p.root.clone(), window, cx));
                }
            });
        });
    });
    gitviber_ui::workspace::register_panel(
        cx,
        PanelSpec::new("terminal", Slot::Bottom, "Terminal", |ctx, window, cx| {
            cx.new(|cx| TerminalPanel::for_workspace(ctx, window, cx))
        })
        .per_window(),
    );
}

/// The terminal's settings from the app's: fonts resolved by the theme, the rest as stored.
pub fn config_from(s: &Settings, t: &AppTheme) -> TerminalConfig {
    TerminalConfig {
        font_family: t.fonts.terminal.clone(),
        font_size: f32::from(t.fonts.terminal_size),
        font_weight: t.fonts.code_weight.0,
        line_height: s.terminal_line_height,
        cursor: match s.terminal_cursor {
            TerminalCursor::Block => CursorStyle::Block,
            TerminalCursor::Bar => CursorStyle::Bar,
            TerminalCursor::Underline => CursorStyle::Underline,
        },
        cursor_blink: s.terminal_cursor_blink,
        scrollback: s.terminal_scrollback as usize,
        option_as_meta: match s.option_as_meta {
            OptionKey::Off => OptionAsMeta::Off,
            OptionKey::Left => OptionAsMeta::Left,
            OptionKey::Both => OptionAsMeta::Both,
        },
        shell_integration: s.shell_integration,
        inactive_dim: s.terminal_inactive_dim as f32,
    }
}

/// The terminal's colors from the app's theme: its surfaces and accent, the theme's ANSI set.
pub fn theme_from(t: &AppTheme) -> TerminalTheme {
    TerminalTheme {
        background: t.terminal_background(),
        foreground: t.foreground,
        cursor: t.primary,
        cursor_text: t.background,
        selection: t.primary.opacity(0x55 as f32 / 255.),
        ansi: ansi_colors(t.id.as_str(), t.dark),
        panel: t.panel,
        elevated: t.elevated,
        hover: t.hover,
        active: t.active,
        border: t.border,
        border_strong: t.border_strong,
        muted_foreground: t.muted_foreground,
        subtle: t.subtle,
        primary: t.primary,
        primary_foreground: t.primary_foreground,
        destructive: t.destructive,
        find_match: t.find_match,
        find_current: t.find_current,
        minimum_contrast: 4.5,
    }
}

impl Focusable for TerminalPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

fn folder_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

impl TerminalPanel {
    /// The panel for the worktree at `root`; no terminal starts until one is opened.
    pub fn new(
        core: Arc<Core>,
        root: PathBuf,
        config: TerminalConfig,
        theme: TerminalTheme,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        TerminalPanel {
            core,
            config,
            theme,
            root,
            sets: HashMap::new(),
            panes: HashMap::new(),
            next_id: 1,
            maximized: false,
            zoomed: false,
            worktrees: Vec::new(),
            find: None,
            rename: None,
            flash: None,
            follow: None,
            link_cache: Default::default(),
            link_ctx: None,
            agent_tasks: Vec::new(),
            round: None,
            epoch: std::time::Instant::now(),
            restored_here: Default::default(),
            divider: None,
            split_bounds: Rc::default(),
            killing: false,
            focus: cx.focus_handle(),
            bound: None,
        }
    }

    /// The panel for a workspace: the Core, Settings and theme are the app's, and the tab set
    /// follows the project's worktree.
    pub fn for_workspace(ctx: &PanelContext, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let root = ctx
            .project
            .as_ref()
            .map(|p| p.read(cx).root().to_path_buf())
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
            .unwrap_or_default();
        let (config, theme) = (
            config_from(Settings::get(cx), cx.app_theme()),
            theme_from(cx.app_theme()),
        );
        let mut this = Self::new(gitviber_ui::core(cx), root, config, theme, window, cx);
        let mut subscriptions = vec![
            cx.observe_global::<Settings>(|this, cx| this.sync_look(cx)),
            cx.observe_global::<AppTheme>(|this, cx| this.sync_look(cx)),
        ];
        let mut open = false;
        if let Some(ws) = ctx.workspace.upgrade() {
            open = ws.read(cx).is_open(Slot::Bottom);
            this.maximized = ws.read(cx).terminal_maximized();
            subscriptions.push(cx.subscribe_in(
                &ws,
                window,
                |this, ws, event: &WorkspaceEvent, window, cx| {
                    this.on_workspace(ws, event, window, cx)
                },
            ));
        }
        let worktrees = gitviber_refs::worktrees::WorktreeEvents::global(cx);
        subscriptions.push(
            cx.subscribe_in(&worktrees, window, |this, _, event, window, cx| {
                // The window it was asked in.
                let here = window.window_handle();
                if cx.active_window().is_none_or(|w| w == here) {
                    this.on_worktree(event, window, cx);
                }
            }),
        );
        let me = cx.entity().downgrade();
        let live = cx.default_global::<LivePanels>();
        live.0.retain(|p| p.upgrade().is_some());
        live.0.push(me);
        if gitviber_ui::has_core(cx) {
            let events = gitviber_ui::CoreEvents::global(cx);
            subscriptions.push(cx.subscribe_in(&events, window, |this, _, e, window, cx| {
                this.on_core_event(e, window, cx)
            }));
        }
        subscriptions.push(cx.on_app_quit(|this, cx| {
            this.save_at_quit(cx);
            async {}
        }));
        let project = ctx.project.as_ref().map(|p| this.watch(p, window, cx));
        this.bound = Some(Bound {
            workspace: ctx.workspace.clone(),
            _project: project,
            _subscriptions: subscriptions,
        });
        let root = this.root.clone();
        this.restore(&root, window, cx);
        // Shown for the first time: with a terminal in it, as ⌘J opens one.
        if open {
            cx.defer_in(window, |this, window, cx| {
                if !this.has_terminals() {
                    this.new_terminal(window, cx);
                }
            });
        }
        this
    }

    fn watch(
        &mut self,
        project: &Entity<Project>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Subscription {
        cx.subscribe_in(
            project,
            window,
            |this, project, event: &ProjectEvent, window, cx| match event {
                ProjectEvent::Switched { to, .. } => this.set_worktree(to.clone(), window, cx),
                ProjectEvent::Opened => {
                    let root = project.read(cx).root().to_path_buf();
                    this.set_worktree(root, window, cx);
                }
                ProjectEvent::RepoChanged(_) => this.update_links(cx),
                _ => {}
            },
        )
    }

    fn on_workspace(
        &mut self,
        ws: &Entity<Workspace>,
        event: &WorkspaceEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            WorkspaceEvent::ProjectChanged => {
                let project = ws.read(cx).project().cloned();
                if let Some(p) = &project {
                    let root = p.read(cx).root().to_path_buf();
                    self.set_worktree(root, window, cx);
                }
                let sub = project.as_ref().map(|p| self.watch(p, window, cx));
                if let Some(b) = self.bound.as_mut() {
                    b._project = sub;
                }
            }
            WorkspaceEvent::LayoutChanged => {
                let (open, maximized) = {
                    let ws = ws.read(cx);
                    (ws.is_open(Slot::Bottom), ws.terminal_maximized())
                };
                self.maximized = maximized;
                if open && !self.has_terminals() {
                    self.new_terminal(window, cx);
                }
                cx.notify();
            }
            WorkspaceEvent::TabsChanged => {}
        }
    }

    fn on_worktree(
        &mut self,
        event: &gitviber_refs::worktrees::WorktreeEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use gitviber_refs::worktrees::WorktreeEvent;
        let (path, run) = match event {
            WorktreeEvent::Created { command: None, .. } => return,
            WorktreeEvent::Created {
                path,
                command: Some(c),
            } => (path, Some(format!("{c}\r"))),
            WorktreeEvent::OpenTerminal { path } => (path, None),
            WorktreeEvent::ResumeConversation { path } => {
                return self.ask_conversation(PathBuf::from(path), window, cx);
            }
        };
        let path = PathBuf::from(path);
        self.open_terminal(path.clone(), run, window, cx);
        if let Some(ws) = self.workspace() {
            ws.update(cx, |ws, cx| ws.set_open(Slot::Bottom, true, cx));
        }
        self.focus_active(window, cx);
    }

    /// Terminals whose shell runs in `dir` or below it, which removing it would end.
    fn running_in(&self, dir: &Path, cx: &App) -> usize {
        self.panes
            .values()
            .filter(|s| {
                let p = s.view.read(cx);
                !p.has_exited() && p.cwd().starts_with(dir)
            })
            .count()
    }

    fn sync_look(&mut self, cx: &mut Context<Self>) {
        let (config, theme) = (
            config_from(Settings::get(cx), cx.app_theme()),
            theme_from(cx.app_theme()),
        );
        self.set_config(config, cx);
        self.set_theme(theme, cx);
    }

    fn workspace(&self) -> Option<Entity<Workspace>> {
        self.bound.as_ref()?.workspace.upgrade()
    }

    /// What links resolve against: the open worktree, the repo's others, its state on disk, and
    /// origin's GitHub page.
    fn link_site(&self, cx: &App) -> LinkSite {
        let project = self
            .workspace()
            .and_then(|ws| ws.read(cx).project().cloned());
        let repo = project
            .as_ref()
            .and_then(|p| gitviber_refs::RepoState::existing(p, cx));
        let mut worktrees: Vec<String> = self
            .worktrees
            .iter()
            .map(|w| w.path.to_string_lossy().into_owned())
            .collect();
        let mut repos = vec![];
        if let Some(repo) = &repo {
            let repo = repo.read(cx);
            worktrees.extend(repo.worktrees.iter().map(|w| w.path.clone()));
            repos.extend(
                repo.status
                    .as_ref()
                    .and_then(|s| s.web_url.clone().flatten()),
            );
        }
        LinkSite {
            root: self.root.to_string_lossy().into_owned(),
            worktrees: Arc::new(worktrees),
            revision: project.map_or(0, |p| p.read(cx).revision()),
            repos,
        }
    }

    fn update_links(&mut self, cx: &mut Context<Self>) {
        let site = self.link_site(cx);
        if self.link_ctx.as_ref().is_some_and(|c| c.site == site) {
            return;
        }
        let ctx = Rc::new(LinkCtx {
            site,
            cache: self.link_cache.clone(),
        });
        self.link_ctx = Some(ctx.clone());
        for slot in self.panes.values() {
            slot.view
                .update(cx, |p, cx| p.set_links(Some(ctx.clone()), cx));
        }
    }

    /// Shows `root`'s tabs; the other worktrees' keep running behind.
    pub fn set_worktree(&mut self, root: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if root == self.root {
            return;
        }
        self.restore(&root, window, cx);
        self.root = root;
        self.update_links(cx);
        self.zoomed = false;
        self.close_find_bar(cx);
        // Another worktree starts with its code in view.
        self.set_maximized(false, cx);
        cx.notify();
    }

    pub fn worktree(&self) -> &Path {
        &self.root
    }

    /// The worktrees offered in the toolbar's menu.
    pub fn set_worktrees(&mut self, worktrees: Vec<WorktreeInfo>, cx: &mut Context<Self>) {
        self.worktrees = worktrees;
        cx.notify();
    }

    pub fn set_config(&mut self, config: TerminalConfig, cx: &mut Context<Self>) {
        if config == self.config {
            return;
        }
        self.config = config.clone();
        for slot in self.panes.values() {
            slot.view
                .update(cx, |p, cx| p.set_config(config.clone(), cx));
        }
        self.apply_dims(cx);
        cx.notify();
    }

    pub fn set_theme(&mut self, theme: TerminalTheme, cx: &mut Context<Self>) {
        if theme == self.theme {
            return;
        }
        self.theme = theme.clone();
        for slot in self.panes.values() {
            slot.view.update(cx, |p, cx| p.set_theme(theme.clone(), cx));
        }
        cx.notify();
    }

    pub fn is_maximized(&self) -> bool {
        self.maximized
    }

    /// Whether the open worktree has a terminal.
    pub fn has_terminals(&self) -> bool {
        self.sets
            .get(&self.root)
            .is_some_and(|s| !s.groups.is_empty())
    }

    /// Terminals started in `dir` or below it, in any worktree's set.
    pub fn terminals_in(&self, dir: &Path, cx: &App) -> usize {
        self.panes
            .values()
            .filter(|s| s.view.read(cx).cwd().starts_with(dir))
            .count()
    }

    /// Every pane, for what reads them (links, agents).
    pub fn panes(&self) -> impl Iterator<Item = &Entity<TerminalPane>> {
        self.panes.values().map(|s| &s.view)
    }

    fn set(&self) -> Option<&TabSet> {
        self.sets.get(&self.root)
    }

    fn set_mut(&mut self) -> &mut TabSet {
        self.sets.entry(self.root.clone()).or_default()
    }

    fn active_group(&self) -> Option<&Group> {
        let set = self.set()?;
        set.groups.iter().find(|g| Some(g.id) == set.active)
    }

    /// The focused pane of the open tab.
    pub fn active_pane(&self) -> Option<Entity<TerminalPane>> {
        let g = self.active_group()?;
        self.panes.get(&g.focused).map(|s| s.view.clone())
    }

    fn make_pane(
        &mut self,
        cwd: PathBuf,
        folder: Option<PathBuf>,
        run: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> PaneId {
        let id = self.next_id;
        self.next_id += 1;
        let view = new_pane(
            id,
            self.core.clone(),
            cwd,
            folder,
            run,
            self.config.clone(),
            self.theme.clone(),
            window,
            cx,
        );
        let events = cx.subscribe_in(
            &view,
            window,
            move |this, _, event: &PaneEvent, window, cx| this.on_pane_event(id, event, window, cx),
        );
        self.update_links(cx);
        let links = self.link_ctx.clone();
        view.update(cx, |p, cx| p.set_links(links, cx));
        self.panes.insert(
            id,
            PaneSlot {
                view,
                name: None,
                rang: false,
                told: false,
                agent: None,
                since: 0.,
                history_file: None,
                history_saved: false,
                serialized_at: 0,
                _events: events,
            },
        );
        self.save_soon(cx);
        id
    }

    /// A new tab in `cwd` (any folder), shown and focused; `run` is typed at its first prompt.
    pub fn open_terminal(
        &mut self,
        cwd: PathBuf,
        run: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let id = self.make_pane(cwd, None, run, window, cx);
        let gid = self.next_id;
        self.next_id += 1;
        let set = self.set_mut();
        set.groups.push(Group {
            id: gid,
            name: None,
            panes: vec![id],
            layout: Layout::Pane(id),
            focused: id,
        });
        set.active = Some(gid);
        self.apply_dims(cx);
        self.focus_active(window, cx);
        cx.notify();
    }

    pub fn new_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let root = self.root.clone();
        self.open_terminal(root, None, window, cx);
    }

    /// Asks for a folder and opens a terminal there, the window staying on this worktree.
    pub fn choose_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("New Terminal".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = picked.await
                && let Some(dir) = paths.into_iter().next()
            {
                let _ = this.update_in(cx, |this, window, cx| {
                    this.open_terminal(dir, None, window, cx)
                });
            }
        })
        .detach();
    }

    /// A pane right of or below `at` (the focused one), where its shell is now, in the same worktree.
    pub fn split(
        &mut self,
        dir: Dir,
        at: Option<PaneId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.split_running(dir, at, None, window, cx);
    }

    /// `split`, with `run` typed at the new shell's first prompt.
    pub fn split_running(
        &mut self,
        dir: Dir,
        at: Option<PaneId>,
        run: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(g) = self.active_group() else { return };
        let from = at.unwrap_or(g.focused);
        let Some(slot) = self.panes.get(&from) else {
            return;
        };
        let (cwd, folder) = {
            let p = slot.view.read(cx);
            let now = p
                .session
                .cwd()
                .map(PathBuf::from)
                .unwrap_or_else(|| p.dir().to_path_buf());
            (p.cwd().to_path_buf(), Some(now))
        };
        let id = self.make_pane(cwd, folder, run, window, cx);
        let set = self.set_mut();
        let Some(g) = set.groups.iter_mut().find(|g| g.panes.contains(&from)) else {
            return;
        };
        let i = g.panes.iter().position(|&p| p == from).unwrap_or(0);
        g.panes.insert(i + 1, id);
        g.layout = layout::split_pane(&g.layout, from, id, dir);
        g.focused = id;
        set.active = Some(g.id);
        // A zoomed pane's split shows the two side by side.
        self.zoomed = false;
        self.apply_dims(cx);
        self.focus_active(window, cx);
        cx.notify();
    }

    /// Closes `id`: its shell goes, and the memory with it.
    fn close_pane(&mut self, id: PaneId, window: &mut Window, cx: &mut Context<Self>) {
        let Some(slot) = self.panes.remove(&id) else {
            return;
        };
        slot.view.update(cx, |p, _| p.session.close());
        drop(slot);
        Self::release_memory_soon(cx);
        if self.find.as_ref().is_some_and(|f| f.pane == Some(id)) {
            self.close_find_bar(cx);
        }
        let mut emptied = Vec::new();
        for (root, set) in self.sets.iter_mut() {
            let Some(gi) = set.groups.iter().position(|g| g.panes.contains(&id)) else {
                continue;
            };
            let g = &mut set.groups[gi];
            let i = g.panes.iter().position(|&p| p == id).unwrap_or(0);
            g.panes.retain(|&p| p != id);
            match layout::remove_pane(&g.layout, id) {
                Some(l) => {
                    g.layout = l;
                    if g.focused == id {
                        g.focused = g.panes[i.min(g.panes.len() - 1)];
                    }
                }
                None => {
                    set.groups.remove(gi);
                    if set
                        .active
                        .is_some_and(|a| !set.groups.iter().any(|g| g.id == a))
                    {
                        set.active = set
                            .groups
                            .get(gi.min(set.groups.len().saturating_sub(1)))
                            .map(|g| g.id);
                    }
                    if set.groups.is_empty() {
                        emptied.push(root.clone());
                    }
                }
            }
        }
        self.sets.retain(|_, s| !s.groups.is_empty());
        self.apply_dims(cx);
        attention::agents_changed(cx);
        self.save_soon(cx);
        if emptied.contains(&self.root) {
            self.set_maximized(false, cx);
            if let Some(ws) = self.workspace() {
                ws.update(cx, |ws, cx| ws.set_open(Slot::Bottom, false, cx));
                ws.update(cx, |ws, cx| ws.focus_center(window, cx));
            }
            cx.emit(PanelEvent::Empty);
        } else {
            self.focus_active(window, cx);
        }
        cx.notify();
    }

    /// Once the closed terminal's reader thread has let go of it (its next read at the latest).
    fn release_memory_soon(cx: &mut Context<Self>) {
        cx.spawn(async move |_, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(500))
                .await;
            crate::session::release_freed_memory();
        })
        .detach();
    }

    /// Kills `ids`, asking first when one of them runs a command (an agent, a dev server), never
    /// for a shell at its prompt.
    fn kill(
        &mut self,
        ids: Vec<PaneId>,
        what: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.killing || ids.is_empty() {
            return;
        }
        let busy = ids
            .iter()
            .filter(|id| {
                self.panes
                    .get(id)
                    .is_some_and(|s| s.view.read(cx).session.busy())
            })
            .count();
        if busy == 0 {
            for id in ids {
                self.close_pane(id, window, cx);
            }
            return;
        }
        self.killing = true;
        let commands = if busy == 1 {
            "1 command".to_string()
        } else {
            format!("{busy} commands")
        };
        let answer = window.prompt(
            PromptLevel::Warning,
            &format!("Kill {what}"),
            Some(&format!("Killing {what} stops {commands} still running.")),
            &["Kill", "Cancel"],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let kill = answer.await.ok() == Some(0);
            let _ = this.update_in(cx, |this, window, cx| {
                this.killing = false;
                if kill {
                    for id in ids {
                        this.close_pane(id, window, cx);
                    }
                }
            });
        })
        .detach();
    }

    pub fn close_focused(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(g) = self.active_group() {
            let id = g.focused;
            self.kill(vec![id], "this terminal", window, cx);
        }
    }

    fn close_group(&mut self, gid: u64, window: &mut Window, cx: &mut Context<Self>) {
        let ids = self
            .set()
            .and_then(|s| s.groups.iter().find(|g| g.id == gid))
            .map(|g| g.panes.clone())
            .unwrap_or_default();
        self.kill(ids, "this terminal", window, cx);
    }

    /// Kills the open worktree's tab at `index`, asking first as ⌘W does.
    pub fn close_tab_at(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(gid) = self.set().and_then(|s| s.groups.get(index)).map(|g| g.id) {
            self.close_group(gid, window, cx);
        }
    }

    fn close_other_groups(&mut self, gid: u64, window: &mut Window, cx: &mut Context<Self>) {
        let ids = self
            .set()
            .map(|s| {
                s.groups
                    .iter()
                    .filter(|g| g.id != gid)
                    .flat_map(|g| g.panes.clone())
                    .collect()
            })
            .unwrap_or_default();
        self.kill(ids, "the other terminals", window, cx);
    }

    /// Ends every terminal, in every worktree: the workspace closed.
    pub fn close_all(&mut self, cx: &mut Context<Self>) {
        for (_, slot) in self.panes.drain() {
            slot.view.update(cx, |p, _| p.session.close());
        }
        Self::release_memory_soon(cx);
        self.sets.clear();
        self.find = None;
        self.rename = None;
        cx.notify();
    }

    fn on_pane_event(
        &mut self,
        id: PaneId,
        event: &PaneEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            PaneEvent::Exited { quick: false } => self.close_pane(id, window, cx),
            PaneEvent::Focused => {
                self.update_links(cx);
                self.looked_at(id, cx);
                self.focus_pane(id, cx);
            }
            PaneEvent::Bell => {
                self.needs_you(id, None, None, cx);
                cx.emit(PanelEvent::Bell { pane: id });
            }
            PaneEvent::Attention(note) => self.needs_you(id, Some(note.clone()), None, cx),
            PaneEvent::CommandEnded(end) => {
                self.command_ended(id, end, window, cx);
                cx.emit(PanelEvent::CommandEnded {
                    pane: id,
                    end: end.clone(),
                });
            }
            PaneEvent::Output => self.save_soon(cx),
            PaneEvent::Prompt => {
                self.agent_prompted(id, cx);
                cx.notify();
            }
            PaneEvent::FontStep(dir) => self.step_font(*dir, cx),
            PaneEvent::OpenLink(target) => {
                if let Some(ws) = self.workspace() {
                    self.follow =
                        crate::follow::follow(target.clone(), &ws.downgrade(), window, cx);
                }
            }
            PaneEvent::Copied(text) => gitviber_ui::workspace::Toast::success(text.clone())
                .title("Copied")
                .show(window, cx),
            PaneEvent::FindChanged => {
                if let Some(f) = self.find.as_mut().filter(|f| f.pane == Some(id)) {
                    if let Some(slot) = self.panes.get(&id) {
                        f.result = Ok(slot.view.read(cx).find_result());
                    }
                    cx.notify();
                }
            }
            PaneEvent::TitleChanged | PaneEvent::Exited { quick: true } => cx.notify(),
        }
    }

    /// Makes `id` its tab's focused pane, and its tab the open one.
    fn focus_pane(&mut self, id: PaneId, cx: &mut Context<Self>) {
        if let Some(slot) = self.panes.get_mut(&id) {
            slot.rang = false;
        }
        let Some(set) = self
            .sets
            .values_mut()
            .find(|s| s.groups.iter().any(|g| g.panes.contains(&id)))
        else {
            return;
        };
        let Some(g) = set.groups.iter_mut().find(|g| g.panes.contains(&id)) else {
            return;
        };
        let changed = g.focused != id || set.active != Some(g.id);
        g.focused = id;
        set.active = Some(g.id);
        if changed {
            self.apply_dims(cx);
            cx.notify();
        }
    }

    /// The panes of a split other than the focused one fade by the setting.
    fn apply_dims(&self, cx: &mut Context<Self>) {
        let dim = self.config.inactive_dim / 100.;
        for set in self.sets.values() {
            for g in &set.groups {
                for &id in &g.panes {
                    let d = if g.panes.len() > 1 && id != g.focused {
                        dim
                    } else {
                        0.
                    };
                    if let Some(slot) = self.panes.get(&id) {
                        slot.view.update(cx, |p, cx| {
                            if p.dim != d {
                                p.dim = d;
                                cx.notify();
                            }
                        });
                    }
                }
            }
        }
    }

    /// Keys to the open tab's focused pane.
    pub fn focus_active(&self, window: &mut Window, cx: &mut App) {
        match self.active_pane() {
            Some(p) => {
                let focus = p.read(cx).focus_handle(cx);
                window.focus(&focus, cx);
            }
            None => window.focus(&self.focus, cx),
        }
    }

    fn activate_group(&mut self, gid: u64, window: &mut Window, cx: &mut Context<Self>) {
        self.set_mut().active = Some(gid);
        self.apply_dims(cx);
        self.focus_active(window, cx);
        cx.notify();
    }

    fn step_group(&mut self, dir: i32, window: &mut Window, cx: &mut Context<Self>) {
        let Some(set) = self.set() else { return };
        let n = set.groups.len();
        if n < 2 {
            return;
        }
        let at = set
            .groups
            .iter()
            .position(|g| Some(g.id) == set.active)
            .unwrap_or(0);
        let gid = set.groups[(at as i64 + dir as i64).rem_euclid(n as i64) as usize].id;
        self.activate_group(gid, window, cx);
    }

    fn go_group(&mut self, index: Option<usize>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(set) = self.set() else { return };
        let g = match index {
            Some(i) => set.groups.get(i),
            None => set.groups.last(),
        };
        if let Some(gid) = g.map(|g| g.id) {
            self.activate_group(gid, window, cx);
        }
    }

    /// The pane the keys moved to shows its number a moment.
    fn show_switch(&mut self, id: PaneId, cx: &mut Context<Self>) {
        let task = cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(700))
                .await;
            let _ = this.update(cx, |this, cx| {
                this.flash = None;
                cx.notify();
            });
        });
        self.flash = Some((id, task));
    }

    fn step_pane(&mut self, dir: i32, window: &mut Window, cx: &mut Context<Self>) {
        let Some(g) = self.active_group() else { return };
        let Some(id) = layout::step_in(&layout::pane_order(&g.layout), g.focused, dir) else {
            return;
        };
        self.focus_pane(id, cx);
        self.focus_active(window, cx);
        self.show_switch(id, cx);
    }

    fn focus_toward(&mut self, dir: Direction, window: &mut Window, cx: &mut Context<Self>) {
        let Some(g) = self.active_group() else { return };
        // As drawn: a pane's least size can outweigh its share.
        let rects: Vec<(PaneId, Rect)> = g
            .panes
            .iter()
            .filter_map(|&id| {
                let m = self.panes.get(&id)?.view.read(cx).metrics?;
                let b = m.bounds;
                Some((
                    id,
                    Rect {
                        x: f32::from(b.origin.x) as f64,
                        y: f32::from(b.origin.y) as f64,
                        width: f32::from(b.size.width) as f64,
                        height: f32::from(b.size.height) as f64,
                    },
                ))
            })
            .collect();
        let Some(id) = layout::neighbor(&rects, g.focused, dir) else {
            return;
        };
        self.focus_pane(id, cx);
        self.focus_active(window, cx);
        self.show_switch(id, cx);
    }

    fn set_maximized(&mut self, on: bool, cx: &mut Context<Self>) {
        if let Some(ws) = self.workspace() {
            ws.update(cx, |ws, cx| {
                if ws.terminal_maximized() != on {
                    ws.set_terminal_maximized(on, cx);
                }
            });
        }
        if self.maximized != on {
            self.maximized = on;
            cx.emit(PanelEvent::MaximizeChanged(on));
            cx.notify();
        }
    }

    pub fn toggle_maximize(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.set_maximized(!self.maximized, cx);
        self.focus_active(window, cx);
    }

    /// The open tab's focused pane alone, or all of them again.
    pub fn toggle_zoom(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.zoomed = !self.zoomed;
        self.focus_active(window, cx);
        cx.notify();
    }

    fn step_font(&mut self, dir: i32, cx: &mut Context<Self>) {
        let size = self.config.stepped_font(dir);
        if self.bound.is_some() {
            // The theme resolves it from Settings; sync_look brings it back here.
            Settings::update(cx, |s| s.terminal_font_size = size);
            cx.emit(PanelEvent::FontSize(size));
            return;
        }
        if size != self.config.font_size {
            let config = TerminalConfig {
                font_size: size,
                ..self.config.clone()
            };
            self.set_config(config, cx);
            cx.emit(PanelEvent::FontSize(size));
        }
    }

    fn equalize(&mut self, gid: u64, path: Option<Vec<usize>>, cx: &mut Context<Self>) {
        let set = self.set_mut();
        if let Some(g) = set.groups.iter_mut().find(|g| g.id == gid) {
            g.layout = match &path {
                Some(p) => layout::equalize(&g.layout, p, false),
                None => layout::equalize(&g.layout, &[], true),
            };
        }
        cx.notify();
    }

    fn start_rename(&mut self, target: RenameTarget, window: &mut Window, cx: &mut Context<Self>) {
        let current = match target {
            RenameTarget::Pane(id) => self.pane_title(id, cx),
            RenameTarget::Tab(gid) => self.group_title(gid, cx),
        };
        let input = cx.new(|cx| InputState::new(window, cx).default_value(current));
        let events = cx.subscribe_in(
            &input,
            window,
            move |this, input, event: &InputEvent, window, cx| match event {
                InputEvent::PressEnter { .. } | InputEvent::Blur => {
                    let name = input.read(cx).value().trim().to_string();
                    this.finish_rename(Some(name), window, cx);
                }
                _ => {}
            },
        );
        input.update(cx, |i, cx| {
            i.focus(window, cx);
            i.select_all(window, cx);
        });
        self.rename = Some(Rename {
            target,
            input,
            _events: events,
        });
        cx.notify();
    }

    /// Names the pane or tab; an empty name gives back the program's title or folder's.
    fn finish_rename(&mut self, name: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(rename) = self.rename.take() else {
            return;
        };
        if let Some(name) = name {
            let shown = match rename.target {
                RenameTarget::Pane(id) => self.pane_title(id, cx),
                RenameTarget::Tab(gid) => self.group_title(gid, cx),
            };
            // Left as it was, the shown title isn't a name the user gave.
            if name != shown {
                let name = (!name.is_empty()).then_some(name);
                match rename.target {
                    RenameTarget::Pane(id) => {
                        if let Some(slot) = self.panes.get_mut(&id) {
                            slot.name = name;
                        }
                    }
                    RenameTarget::Tab(gid) => {
                        if let Some(g) = self.set_mut().groups.iter_mut().find(|g| g.id == gid) {
                            g.name = name;
                        }
                    }
                }
            }
        }
        self.focus_active(window, cx);
        cx.notify();
    }

    /// What a pane's header calls it: the user's name, else the program's title, else its folder.
    fn pane_title(&self, id: PaneId, cx: &App) -> String {
        let Some(slot) = self.panes.get(&id) else {
            return String::new();
        };
        if let Some(n) = &slot.name {
            return n.clone();
        }
        let p = slot.view.read(cx);
        p.title()
            .map(String::from)
            .unwrap_or_else(|| folder_name(p.dir()))
    }

    fn group_title(&self, gid: u64, cx: &App) -> String {
        let Some(g) = self
            .set()
            .and_then(|s| s.groups.iter().find(|g| g.id == gid))
        else {
            return String::new();
        };
        if let Some(n) = &g.name {
            return n.clone();
        }
        let cwd = self
            .panes
            .get(&g.panes[0])
            .map(|s| s.view.read(cx).cwd().to_path_buf())
            .unwrap_or_default();
        folder_name(&cwd)
    }

    fn open_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.find.is_none() {
            let input = cx.new(|cx| InputState::new(window, cx).placeholder("Find"));
            let events =
                cx.subscribe_in(
                    &input,
                    window,
                    |this, _, event: &InputEvent, _, cx| match event {
                        InputEvent::Change => this.search(0, cx),
                        InputEvent::PressEnter { shift, .. } => {
                            this.search(if *shift { -1 } else { 1 }, cx)
                        }
                        _ => {}
                    },
                );
            self.find = Some(FindBar {
                input,
                options: FindOptions::default(),
                result: Ok(FindResult::default()),
                pane: None,
                _events: events,
            });
        }
        if let Some(f) = &self.find {
            f.input.update(cx, |i, cx| {
                i.focus(window, cx);
                i.select_all(window, cx);
            });
        }
        self.search(0, cx);
        cx.notify();
    }

    /// Opens find with `query` in it, searched: ⌘F with a selection, or the app's own Find.
    pub fn find_text(&mut self, query: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.open_find(window, cx);
        if let Some(f) = &self.find {
            let query = query.to_string();
            f.input.update(cx, |i, cx| i.set_value(query, window, cx));
        }
        self.search(0, cx);
    }

    /// Find in the open tab's focused pane: `step` 0 as the query is typed, ±1 for next/previous.
    fn search(&mut self, step: i32, cx: &mut Context<Self>) {
        let Some(pane) = self.active_pane() else {
            return;
        };
        let id = pane.read(cx).id();
        let Some(f) = self.find.as_mut() else { return };
        // Another pane: the last one's marks go.
        if let Some(old) = f.pane.filter(|&p| p != id).and_then(|p| self.panes.get(&p)) {
            old.view.update(cx, |p, cx| p.clear_find(cx));
        }
        f.pane = Some(id);
        let query = f.input.read(cx).value().to_string();
        let options = f.options;
        f.result = pane.update(cx, |p, cx| p.find(&query, options, step, cx));
        cx.notify();
    }

    fn close_find_bar(&mut self, cx: &mut Context<Self>) {
        if let Some(f) = self.find.take()
            && let Some(slot) = f.pane.and_then(|p| self.panes.get(&p))
        {
            slot.view.update(cx, |p, cx| p.clear_find(cx));
        }
        cx.notify();
    }

    fn toggle_find_option(&mut self, which: u8, cx: &mut Context<Self>) {
        if let Some(f) = self.find.as_mut() {
            match which {
                0 => f.options.match_case = !f.options.match_case,
                1 => f.options.whole_word = !f.options.whole_word,
                _ => f.options.regex = !f.options.regex,
            }
        }
        self.search(0, cx);
    }

    fn start_divider_drag(&mut self, gid: u64, path: Vec<usize>, index: usize, pos: Point<Pixels>) {
        let Some(g) = self
            .set()
            .and_then(|s| s.groups.iter().find(|g| g.id == gid))
        else {
            return;
        };
        let mut node = &g.layout;
        for &i in &path {
            match node {
                Layout::Split(s) => node = &s.children[i],
                Layout::Pane(_) => return,
            }
        }
        let Layout::Split(s) = node else { return };
        let Some(bounds) = self
            .split_bounds
            .borrow()
            .get(&(gid, path.clone()))
            .copied()
        else {
            return;
        };
        let extent = match s.dir {
            Dir::Row => f32::from(bounds.size.width),
            Dir::Col => f32::from(bounds.size.height),
        };
        let free = extent - (s.children.len() as f32 - 1.);
        self.divider = Some(DividerDrag {
            group: gid,
            path,
            index,
            dir: s.dir,
            start: pos,
            sizes: s.sizes.clone(),
            free,
        });
    }

    fn drag_divider(&mut self, pos: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(d) = self.divider.as_ref() else {
            return;
        };
        let (delta, min) = match d.dir {
            Dir::Row => (f32::from(pos.x - d.start.x), MIN_WIDTH),
            Dir::Col => (f32::from(pos.y - d.start.y), MIN_HEIGHT),
        };
        if d.free <= 0. {
            return;
        }
        let pct = |v: f32| (v / d.free * 100.) as f64;
        let mut sizes = d.sizes.clone();
        let (a, b) = (sizes[d.index], sizes[d.index + 1]);
        let least = pct(min).min((a + b) / 2.);
        let moved = pct(delta).clamp(least - a, b - least);
        sizes[d.index] = a + moved;
        sizes[d.index + 1] = b - moved;
        let (gid, path) = (d.group, d.path.clone());
        if let Some(g) = self.set_mut().groups.iter_mut().find(|g| g.id == gid) {
            g.layout = layout::resize(&g.layout, &path, &sizes);
        }
        cx.notify();
    }

    fn icon_button(id: &'static str, icon: IconName, tip: impl Into<SharedString>) -> Button {
        Button::new(id)
            .ghost()
            .xsmall()
            .icon(Icon::new(icon))
            .tooltip(tip)
    }

    fn render_toolbar(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let t = self.theme.clone();
        let set_groups: Vec<(u64, usize)> = self
            .set()
            .map(|s| s.groups.iter().map(|g| (g.id, g.panes.len())).collect())
            .unwrap_or_default();
        let active = self.set().and_then(|s| s.active);
        let alone = set_groups.len() == 1;
        let mut tabs = h_flex()
            .id("terminal-tabs")
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_x_scroll();
        for (gid, _) in &set_groups {
            tabs = tabs.child(self.render_tab(*gid, Some(*gid) == active, alone, window, cx));
        }
        let group = self.active_group().map(|g| {
            (
                g.id,
                g.panes.len(),
                g.focused,
                layout::pane_order(&g.layout),
            )
        });
        let split = group.as_ref().is_some_and(|g| g.1 > 1);
        let zoomed = self.zoomed && split;
        let has = group.is_some();
        let root_name = folder_name(&self.root);
        let weak = cx.entity().downgrade();
        let worktrees: Vec<WorktreeInfo> = self
            .worktrees
            .iter()
            .filter(|w| w.path != self.root)
            .cloned()
            .collect();
        let pager = zoomed.then(|| {
            let (_, n, focused, order) = group.clone().unwrap_or((0, 0, 0, vec![]));
            let at = order
                .iter()
                .position(|&p| p == focused)
                .map_or(0, |i| i + 1);
            let dots = order.iter().map(|&id| {
                let big = id == focused;
                div()
                    .id(("dot", id))
                    .size(px(14.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_sm()
                    .hover(|s| s.bg(t.active))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.focus_pane(id, cx);
                        this.focus_active(window, cx);
                        this.show_switch(id, cx);
                    }))
                    .child(
                        div()
                            .rounded_full()
                            .bg(if big { t.muted_foreground } else { t.subtle })
                            .size(px(if big { 6. } else { 4. })),
                    )
            });
            h_flex()
                .items_center()
                .child(
                    Self::icon_button("prev-pane", IconName::ChevronLeft, "Previous pane")
                        .on_click(cx.listener(|this, _, w, cx| this.step_pane(-1, w, cx))),
                )
                .child(
                    div()
                        .px_0p5()
                        .text_size(px(11.))
                        .text_color(t.muted_foreground)
                        .child(format!("{at} / {n}")),
                )
                .child(
                    Self::icon_button("next-pane", IconName::ChevronRight, "Next pane")
                        .on_click(cx.listener(|this, _, w, cx| this.step_pane(1, w, cx))),
                )
                .children(dots)
        });
        h_flex()
            .h(px(TOOLBAR_HEIGHT))
            .flex_none()
            .items_stretch()
            .border_b_1()
            .border_color(t.border)
            .bg(t.panel)
            .child(tabs)
            .child(
                h_flex()
                    .flex_none()
                    .items_center()
                    .gap_0p5()
                    .px_1p5()
                    .child(
                        Self::icon_button(
                            "new-terminal",
                            IconName::Plus,
                            format!("New terminal in {root_name}"),
                        )
                        .on_click(cx.listener(|this, _, w, cx| this.new_terminal(w, cx))),
                    )
                    .child(
                        Self::icon_button(
                            "new-elsewhere",
                            IconName::ChevronDown,
                            "New terminal in another folder",
                        )
                        .dropdown_menu(move |menu, _, _| {
                            let mut menu = menu.min_w(px(260.));
                            if !worktrees.is_empty() {
                                menu = menu.label("New terminal in worktree");
                                for w in &worktrees {
                                    let (weak, path) = (weak.clone(), w.path.clone());
                                    let label = match &w.branch {
                                        Some(b) => format!("{}  {b}", folder_name(&w.path)),
                                        None => folder_name(&w.path),
                                    };
                                    menu = menu.item(PopupMenuItem::new(label).on_click(
                                        move |_, window, cx| {
                                            let _ = weak.update(cx, |this, cx| {
                                                this.open_terminal(path.clone(), None, window, cx)
                                            });
                                        },
                                    ));
                                }
                                menu = menu.separator();
                            }
                            let weak = weak.clone();
                            menu.item(PopupMenuItem::new("Choose Folder…").on_click(
                                move |_, window, cx| {
                                    let _ =
                                        weak.update(cx, |this, cx| this.choose_folder(window, cx));
                                },
                            ))
                        }),
                    )
                    .child(
                        Self::icon_button("split-right", IconName::Columns2, "Split right")
                            .disabled(!has)
                            .on_click(
                                cx.listener(|this, _, w, cx| this.split(Dir::Row, None, w, cx)),
                            ),
                    )
                    .child(
                        Self::icon_button("split-down", IconName::Rows2, "Split down")
                            .disabled(!has)
                            .on_click(
                                cx.listener(|this, _, w, cx| this.split(Dir::Col, None, w, cx)),
                            ),
                    )
                    .child(
                        Self::icon_button("kill", IconName::Trash, "Kill terminal")
                            .disabled(!has)
                            .on_click(cx.listener(|this, _, w, cx| this.close_focused(w, cx))),
                    )
                    .child(div().mx_0p5().h(px(16.)).w(px(1.)).bg(t.border_strong))
                    .children(pager)
                    .when(split, |d| {
                        d.child(
                            Self::icon_button(
                                "zoom",
                                if zoomed {
                                    IconName::ZoomOut
                                } else {
                                    IconName::ZoomIn
                                },
                                if zoomed {
                                    "Show all panes"
                                } else {
                                    "Zoom pane"
                                },
                            )
                            .on_click(cx.listener(|this, _, w, cx| this.toggle_zoom(w, cx))),
                        )
                    })
                    .child(
                        Self::icon_button(
                            "maximize",
                            if self.maximized {
                                IconName::Minimize2
                            } else {
                                IconName::Maximize2
                            },
                            if self.maximized {
                                "Exit maximized terminal"
                            } else {
                                "Maximize terminal"
                            },
                        )
                        .on_click(cx.listener(|this, _, w, cx| this.toggle_maximize(w, cx))),
                    )
                    .child(
                        Self::icon_button("hide", IconName::ChevronDown, "Hide terminal").on_click(
                            cx.listener(|this, _, window, cx| {
                                if let Some(ws) = this.workspace() {
                                    ws.update(cx, |ws, cx| {
                                        ws.set_open(Slot::Bottom, false, cx);
                                        ws.focus_center(window, cx);
                                    });
                                }
                                cx.emit(PanelEvent::Hide);
                            }),
                        ),
                    ),
            )
            .into_any_element()
    }

    fn render_tab(
        &self,
        gid: u64,
        active: bool,
        alone: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = &self.theme;
        let Some(g) = self
            .set()
            .and_then(|s| s.groups.iter().find(|g| g.id == gid))
        else {
            return div().into_any_element();
        };
        let renaming = self
            .rename
            .as_ref()
            .filter(|r| r.target == RenameTarget::Tab(gid))
            .map(|r| r.input.clone());
        let title = self.group_title(gid, cx);
        let cwd = self
            .panes
            .get(&g.panes[0])
            .map(|s| s.view.read(cx).cwd().to_path_buf())
            .unwrap_or_default();
        let branch = self
            .worktrees
            .iter()
            .find(|w| w.path == cwd)
            .and_then(|w| w.branch.clone());
        let here = cwd == self.root;
        let order = layout::pane_order(&g.layout);
        let look = crate::agent::most_urgent(g.panes.iter().map(|id| self.look_of_pane(*id)));
        let look_tip = look.map(|l| self.look_label(&g.panes, l));
        let focused = g.focused;
        // A split's panes by number, each in its agent's look; else the tab's one dot.
        let dots = if order.len() > 1 {
            Some(
                h_flex()
                    .gap_1()
                    .items_center()
                    .children(order.iter().map(|&id| {
                        let big = id == focused;
                        match self.look_of_pane(id) {
                            Some(l) => attention::look_dot(l, cx),
                            None => div()
                                .rounded_full()
                                .bg(if big { t.muted_foreground } else { t.subtle })
                                .size(px(if big { 6. } else { 4. }))
                                .into_any_element(),
                        }
                    }))
                    .into_any_element(),
            )
        } else {
            look.map(|l| attention::look_dot(l, cx))
        };
        let dots = dots.map(|d| {
            div()
                .id(("tab-look", gid))
                .flex()
                .items_center()
                .child(d)
                .when_some(look_tip, |d, tip| {
                    d.tooltip(move |window, cx| {
                        gpui_kit::component::tooltip::Tooltip::new(tip.clone()).build(window, cx)
                    })
                })
        });
        let weak = cx.entity().downgrade();
        let _ = window;
        div()
            .id(("terminal-tab", gid))
            .group("terminal-tab")
            .relative()
            .flex()
            .flex_none()
            .max_w(px(256.))
            .items_center()
            .gap_1p5()
            .pl_3()
            .pr_1p5()
            .border_r_1()
            .border_color(t.border)
            .text_size(px(12.))
            .cursor_pointer()
            .when(active, |d| d.bg(t.background).text_color(t.foreground))
            .when(!active, |d| {
                d.bg(t.panel)
                    .text_color(t.muted_foreground)
                    .hover(|s| s.bg(t.hover))
            })
            .when(active, |d| {
                d.child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .right_0()
                        .h(px(2.))
                        .bg(t.primary),
                )
            })
            .on_click(cx.listener(move |this, e: &gpui::ClickEvent, window, cx| {
                if e.click_count() >= 2 {
                    this.start_rename(RenameTarget::Tab(gid), window, cx);
                } else {
                    this.activate_group(gid, window, cx);
                }
            }))
            .on_mouse_down(
                MouseButton::Middle,
                cx.listener(move |this, _, window, cx| this.close_group(gid, window, cx)),
            )
            .child(
                Icon::new(IconName::SquareTerminal)
                    .size(px(14.))
                    .text_color(if here { t.primary } else { t.subtle }),
            )
            .child(match renaming {
                Some(input) => div()
                    .w(px(140.))
                    .child(Input::new(&input).xsmall())
                    .into_any_element(),
                None => div().truncate().child(title).into_any_element(),
            })
            .when_some(branch, |d, b| {
                d.child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_size(px(10.5))
                        .text_color(t.subtle)
                        .child(b),
                )
            })
            .children(dots)
            .child(
                div()
                    .id(("close-tab", gid))
                    .size(px(20.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_sm()
                    .text_color(t.subtle)
                    .hover(|s| s.bg(t.active).text_color(t.foreground))
                    .when(!active, |d| {
                        d.invisible().group_hover("terminal-tab", |s| s.visible())
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.close_group(gid, window, cx);
                    }))
                    .child(Icon::new(IconName::X).size(px(12.))),
            )
            .context_menu(move |menu, _, _| {
                let item = |label: &'static str,
                            f: fn(
                    &mut TerminalPanel,
                    u64,
                    &mut Window,
                    &mut Context<TerminalPanel>,
                )| {
                    let weak = weak.clone();
                    PopupMenuItem::new(label).on_click(move |_, window, cx| {
                        let _ = weak.update(cx, |this, cx| f(this, gid, window, cx));
                    })
                };
                menu.item(item("Rename…", |this, gid, w, cx| {
                    this.start_rename(RenameTarget::Tab(gid), w, cx)
                }))
                .separator()
                .item(item("Split Right", |this, gid, w, cx| {
                    this.activate_group(gid, w, cx);
                    this.split(Dir::Row, None, w, cx);
                }))
                .item(item("Split Down", |this, gid, w, cx| {
                    this.activate_group(gid, w, cx);
                    this.split(Dir::Col, None, w, cx);
                }))
                .item(item("Clear", |this, gid, w, cx| {
                    this.activate_group(gid, w, cx);
                    if let Some(p) = this.active_pane() {
                        p.update(cx, |p, cx| p.clear(cx));
                    }
                }))
                .separator()
                .item(item("Kill Terminal", |this, gid, w, cx| {
                    this.close_group(gid, w, cx)
                }))
                .when(!alone, |m| {
                    m.item(item("Kill Others", |this, gid, w, cx| {
                        this.close_other_groups(gid, w, cx)
                    }))
                })
            })
            .into_any_element()
    }

    fn render_node(
        &self,
        gid: u64,
        node: &Layout,
        path: Vec<usize>,
        split: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = &self.theme;
        match node {
            Layout::Pane(id) => {
                let Some(slot) = self.panes.get(id) else {
                    return div().into_any_element();
                };
                let view =
                    AnyView::from(slot.view.clone()).cached(StyleRefinement::default().size_full());
                let header = (split && !path.is_empty()).then(|| self.render_header(gid, *id, cx));
                let flash = self.flash.as_ref().filter(|(f, _)| f == id).map(|_| {
                    let g = self.active_group();
                    let order = g.map(|g| layout::pane_order(&g.layout)).unwrap_or_default();
                    let n = order.iter().position(|p| p == id).map_or(0, |i| i + 1);
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            div()
                                .rounded_full()
                                .border_1()
                                .border_color(t.border_strong)
                                .bg(t.elevated)
                                .px_3()
                                .py_1()
                                .text_size(px(13.))
                                .text_color(t.foreground)
                                .child(format!("{n} / {}", order.len())),
                        )
                });
                v_flex()
                    .size_full()
                    .relative()
                    .children(header)
                    .child(div().flex_1().min_h_0().child(view))
                    .children(flash)
                    .into_any_element()
            }
            Layout::Split(s) => {
                let row = s.dir == Dir::Row;
                let store = self.split_bounds.clone();
                let key = (gid, path.clone());
                let mut el = div()
                    .flex()
                    .size_full()
                    .relative()
                    .when(row, |d| d.flex_row())
                    .when(!row, |d| d.flex_col());
                el = el.child(
                    canvas(
                        move |bounds, _, _| {
                            store.borrow_mut().insert(key.clone(), bounds);
                        },
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .size_full(),
                );
                for (i, (child, size)) in s.children.iter().zip(&s.sizes).enumerate() {
                    if i > 0 {
                        let (p, index) = (path.clone(), i - 1);
                        let hit = div()
                            .id(gpui::ElementId::Name(
                                format!("divider-{gid}-{p:?}-{index}").into(),
                            ))
                            .absolute()
                            .when(row, |d| {
                                d.top_0()
                                    .bottom_0()
                                    .left(px(-3.))
                                    .w(px(7.))
                                    .cursor_col_resize()
                            })
                            .when(!row, |d| {
                                d.left_0()
                                    .right_0()
                                    .top(px(-3.))
                                    .h(px(7.))
                                    .cursor_row_resize()
                            })
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, e: &gpui::MouseDownEvent, _, cx| {
                                    cx.stop_propagation();
                                    if e.click_count >= 2 {
                                        this.divider = None;
                                        this.equalize(gid, Some(p.clone()), cx);
                                    } else {
                                        this.start_divider_drag(gid, p.clone(), index, e.position);
                                    }
                                }),
                            );
                        el = el.child(
                            div()
                                .flex_none()
                                .relative()
                                .bg(t.foreground.opacity(0.2))
                                .when(row, |d| d.w(px(1.)).h_full())
                                .when(!row, |d| d.h(px(1.)).w_full())
                                .child(hit),
                        );
                    }
                    let mut sub = path.clone();
                    sub.push(i);
                    el = el.child(
                        div()
                            .flex_grow(*size as f32)
                            .flex_shrink(1.)
                            .flex_basis(px(0.))
                            .min_w_0()
                            .min_h_0()
                            .overflow_hidden()
                            .child(self.render_node(gid, child, sub, split, cx)),
                    );
                }
                el.into_any_element()
            }
        }
    }

    /// A split pane's title bar: its number, its name or what runs there, and its own split and
    /// kill.
    fn render_header(&self, gid: u64, id: PaneId, cx: &mut Context<Self>) -> AnyElement {
        let t = &self.theme;
        let g = self
            .set()
            .and_then(|s| s.groups.iter().find(|g| g.id == gid));
        let order = g.map(|g| layout::pane_order(&g.layout)).unwrap_or_default();
        let n = order.iter().position(|&p| p == id).map_or(0, |i| i + 1);
        let focused = g.is_some_and(|g| g.focused == id);
        let renaming = self
            .rename
            .as_ref()
            .filter(|r| r.target == RenameTarget::Pane(id))
            .map(|r| r.input.clone());
        let title = self.pane_title(id, cx);
        let action = |name: &'static str, icon: IconName, tip: &'static str| {
            Button::new(gpui::ElementId::Name(format!("{name}-{id}").into()))
                .ghost()
                .xsmall()
                .icon(Icon::new(icon))
                .tooltip(tip)
        };
        h_flex()
            .id(("pane-header", id))
            .group("pane-header")
            .relative()
            .h(px(HEADER_HEIGHT))
            .flex_none()
            .items_center()
            .gap_1p5()
            .pl_2p5()
            .pr_1()
            .border_b_1()
            .border_color(t.border)
            .bg(t.panel)
            .text_size(px(11.5))
            .when(focused, |d| {
                d.child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .right_0()
                        .h(px(2.))
                        .bg(t.primary),
                )
            })
            .on_click(cx.listener(move |this, e: &gpui::ClickEvent, window, cx| {
                if e.click_count() >= 2 {
                    this.start_rename(RenameTarget::Pane(id), window, cx);
                } else {
                    this.focus_pane(id, cx);
                    this.focus_active(window, cx);
                }
            }))
            .child(
                div()
                    .flex_none()
                    .rounded_sm()
                    .px_1()
                    .text_size(px(10.))
                    .when(focused, |d| {
                        d.bg(t.primary).text_color(t.primary_foreground)
                    })
                    .when(!focused, |d| {
                        d.bg(t.elevated).text_color(t.muted_foreground)
                    })
                    .child(n.to_string()),
            )
            .child(match renaming {
                Some(input) => div()
                    .w(px(180.))
                    .child(Input::new(&input).xsmall())
                    .into_any_element(),
                None => div()
                    .min_w_0()
                    .truncate()
                    .text_color(if focused {
                        t.foreground
                    } else {
                        t.muted_foreground
                    })
                    .child(title)
                    .into_any_element(),
            })
            .children(self.look_of_pane(id).map(|l| attention::look_dot(l, cx)))
            .child(
                h_flex()
                    .ml_auto()
                    .flex_none()
                    .items_center()
                    .when(!focused, |d| {
                        d.invisible().group_hover("pane-header", |s| s.visible())
                    })
                    .child(
                        action("split-right", IconName::Columns2, "Split right").on_click(
                            cx.listener(move |this, _, w, cx| {
                                this.split(Dir::Row, Some(id), w, cx)
                            }),
                        ),
                    )
                    .child(
                        action("split-down", IconName::Rows2, "Split down").on_click(
                            cx.listener(move |this, _, w, cx| {
                                this.split(Dir::Col, Some(id), w, cx)
                            }),
                        ),
                    )
                    .child(
                        action("kill-pane", IconName::X, "Kill pane").on_click(cx.listener(
                            move |this, _, w, cx| this.kill(vec![id], "this terminal", w, cx),
                        )),
                    ),
            )
            .into_any_element()
    }

    fn render_find(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let f = self.find.as_ref()?;
        let t = &self.theme;
        let count = match &f.result {
            Err(e) => e.clone(),
            Ok(r) if f.input.read(cx).value().is_empty() => {
                let _ = r;
                String::new()
            }
            Ok(r) if r.total == 0 => "No results".into(),
            Ok(r) if r.more => format!("{} of {}+", r.index, r.total),
            Ok(r) => format!("{} of {}", r.index, r.total),
        };
        let toggle = |id: &'static str, icon: IconName, tip: &'static str, on: bool, which: u8| {
            Button::new(id)
                .ghost()
                .xsmall()
                .icon(Icon::new(icon))
                .tooltip(tip)
                .toggled(on)
                .on_click(cx.listener(move |this, _, _, cx| this.toggle_find_option(which, cx)))
        };
        Some(
            h_flex()
                .key_context("TerminalFind")
                .absolute()
                .top(px(8.))
                .right(px(20.))
                .gap_1()
                .p_1()
                .items_center()
                .rounded_md()
                .border_1()
                .border_color(t.border_strong)
                .bg(t.elevated)
                .on_action(cx.listener(|this, _: &CloseFind, window, cx| {
                    this.close_find_bar(cx);
                    this.focus_active(window, cx);
                }))
                .on_action(cx.listener(|this, _: &FindPrev, _, cx| this.search(-1, cx)))
                .on_action(cx.listener(|this, _: &FindNext, _, cx| this.search(1, cx)))
                .child(div().w(px(200.)).child(Input::new(&f.input).xsmall()))
                .child(toggle(
                    "find-case",
                    IconName::CaseSensitive,
                    "Match case",
                    f.options.match_case,
                    0,
                ))
                .child(toggle(
                    "find-word",
                    IconName::WholeWord,
                    "Match whole word",
                    f.options.whole_word,
                    1,
                ))
                .child(toggle(
                    "find-regex",
                    IconName::Regex,
                    "Use regular expression",
                    f.options.regex,
                    2,
                ))
                .child(
                    div()
                        .min_w(px(56.))
                        .text_size(px(11.))
                        .text_color(if f.result.is_err() {
                            t.destructive
                        } else {
                            t.muted_foreground
                        })
                        .child(count),
                )
                .child(
                    Self::icon_button("find-prev", IconName::ArrowUp, "Previous match")
                        .on_click(cx.listener(|this, _, _, cx| this.search(-1, cx))),
                )
                .child(
                    Self::icon_button("find-next", IconName::ArrowDown, "Next match")
                        .on_click(cx.listener(|this, _, _, cx| this.search(1, cx))),
                )
                .child(
                    Self::icon_button("find-close", IconName::X, "Close").on_click(cx.listener(
                        |this, _, window, cx| {
                            this.close_find_bar(cx);
                            this.focus_active(window, cx);
                        },
                    )),
                )
                .into_any_element(),
        )
    }
}

impl Render for TerminalPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme.clone();
        let toolbar = self.render_toolbar(window, cx);
        let group = self
            .active_group()
            .map(|g| (g.id, g.layout.clone(), g.focused, g.panes.len()));
        self.split_bounds.borrow_mut().clear();
        let body: AnyElement = match group {
            Some((gid, layout, focused, n)) => {
                let zoomed = self.zoomed && n > 1;
                let node = if zoomed {
                    Layout::Pane(focused)
                } else {
                    layout
                };
                self.render_node(gid, &node, vec![], n > 1 && !zoomed, cx)
            }
            None => v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .gap_2()
                .text_size(px(12.))
                .text_color(t.muted_foreground)
                .child("No terminal in this worktree")
                .child(
                    Button::new("empty-new")
                        .small()
                        .label("New Terminal")
                        .on_click(cx.listener(|this, _, w, cx| this.new_terminal(w, cx))),
                )
                .into_any_element(),
        };
        let find = self.render_find(cx);
        let dragging = self.divider.is_some().then(|| {
            let weak = cx.entity().downgrade();
            canvas(
                |_, _, _| {},
                move |_, _, window, _| {
                    let moving = weak.clone();
                    window.on_mouse_event(move |e: &MouseMoveEvent, phase, _, cx| {
                        if phase == DispatchPhase::Capture {
                            let _ = moving.update(cx, |this, cx| this.drag_divider(e.position, cx));
                        }
                    });
                    let up = weak.clone();
                    window.on_mouse_event(move |_: &MouseUpEvent, phase, window, cx| {
                        if phase == DispatchPhase::Capture {
                            let _ = up.update(cx, |this, cx| {
                                this.divider = None;
                                this.focus_active(window, cx);
                                cx.notify();
                            });
                        }
                    });
                },
            )
            .absolute()
            .size_full()
        });
        v_flex()
            .id("terminal-panel")
            .track_focus(&self.focus)
            .key_context(PANEL_CONTEXT)
            .size_full()
            .bg(t.background)
            .on_action(cx.listener(|this, _: &terminal::New, w, cx| this.new_terminal(w, cx)))
            .on_action(
                cx.listener(|this, _: &terminal::Split, w, cx| this.split(Dir::Row, None, w, cx)),
            )
            .on_action(
                cx.listener(|this, _: &terminal::SplitDown, w, cx| {
                    this.split(Dir::Col, None, w, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &terminal::Close, w, cx| this.close_focused(w, cx)))
            .on_action(cx.listener(|this, _: &terminal::PrevPane, w, cx| this.step_pane(-1, w, cx)))
            .on_action(cx.listener(|this, _: &terminal::NextPane, w, cx| this.step_pane(1, w, cx)))
            .on_action(cx.listener(|this, _: &terminal::FocusLeft, w, cx| {
                this.focus_toward(Direction::Left, w, cx)
            }))
            .on_action(cx.listener(|this, _: &terminal::FocusRight, w, cx| {
                this.focus_toward(Direction::Right, w, cx)
            }))
            .on_action(cx.listener(|this, _: &terminal::FocusUp, w, cx| {
                this.focus_toward(Direction::Up, w, cx)
            }))
            .on_action(cx.listener(|this, _: &terminal::FocusDown, w, cx| {
                this.focus_toward(Direction::Down, w, cx)
            }))
            .on_action(
                cx.listener(|this, _: &terminal::ToggleMaximize, w, cx| {
                    this.toggle_maximize(w, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &terminal::ZoomPane, w, cx| {
                if this.active_group().is_some_and(|g| g.panes.len() > 1) {
                    this.toggle_zoom(w, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &terminal::RenamePane, w, cx| {
                // Only a pane with a header on screen: a split, not zoomed.
                if let Some(g) = this
                    .active_group()
                    .filter(|g| g.panes.len() > 1 && !this.zoomed)
                {
                    let id = g.focused;
                    this.start_rename(RenameTarget::Pane(id), w, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &terminal::EqualizePanes, _, cx| {
                if let Some(gid) = this.active_group().map(|g| g.id) {
                    this.equalize(gid, None, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &terminal::FontZoomIn, _, cx| this.step_font(1, cx)))
            .on_action(cx.listener(|this, _: &terminal::FontZoomOut, _, cx| this.step_font(-1, cx)))
            .on_action(
                cx.listener(|this, _: &terminal::FontZoomReset, _, cx| this.step_font(0, cx)),
            )
            .on_action(cx.listener(|this, _: &editor::Find, w, cx| this.open_find(w, cx)))
            .on_action(cx.listener(|this, _: &FindNext, _, cx| this.search(1, cx)))
            .on_action(cx.listener(|this, _: &FindPrev, _, cx| this.search(-1, cx)))
            .on_action(cx.listener(|this, _: &tab::Next, w, cx| this.step_group(1, w, cx)))
            .on_action(cx.listener(|this, _: &tab::Prev, w, cx| this.step_group(-1, w, cx)))
            .on_action(cx.listener(|this, _: &tab::Goto1, w, cx| this.go_group(Some(0), w, cx)))
            .on_action(cx.listener(|this, _: &tab::Goto2, w, cx| this.go_group(Some(1), w, cx)))
            .on_action(cx.listener(|this, _: &tab::Goto3, w, cx| this.go_group(Some(2), w, cx)))
            .on_action(cx.listener(|this, _: &tab::Goto4, w, cx| this.go_group(Some(3), w, cx)))
            .on_action(cx.listener(|this, _: &tab::Goto5, w, cx| this.go_group(Some(4), w, cx)))
            .on_action(cx.listener(|this, _: &tab::Goto6, w, cx| this.go_group(Some(5), w, cx)))
            .on_action(cx.listener(|this, _: &tab::Goto7, w, cx| this.go_group(Some(6), w, cx)))
            .on_action(cx.listener(|this, _: &tab::Goto8, w, cx| this.go_group(Some(7), w, cx)))
            .on_action(cx.listener(|this, _: &tab::Last, w, cx| this.go_group(None, w, cx)))
            .child(toolbar)
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(body)
                    .children(find),
            )
            .children(dragging)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;

    /// Terminals opened and closed leave no entity behind: no task, subscription or callback
    /// holds a pane, so its terminal and scrollback go with it.
    #[gpui::test]
    fn closing_terminals_gives_back_their_entities(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::init(cx);
        });
        let core = Arc::new(Core::new(Arc::new(|_| {})));
        let dir = std::env::temp_dir();
        let before = cx.update(|cx| cx.leak_detector_snapshot());
        {
            let (panel, cx) = cx.add_window_view(|window, cx| {
                TerminalPanel::new(
                    core.clone(),
                    dir.clone(),
                    TerminalConfig::default(),
                    TerminalTheme::dark(),
                    window,
                    cx,
                )
            });
            for round in 0..3 {
                for _ in 0..3 {
                    panel.update_in(cx, |p, window, cx| {
                        p.open_terminal(dir.clone(), None, window, cx)
                    });
                }
                panel.update_in(cx, |p, window, cx| p.split(Dir::Row, None, window, cx));
                cx.update(|window, cx| window.draw(cx).clear(cx));
                cx.run_until_parked();
                // Links looked up under a held ⌘, and labels asked for: their tasks go with the panes.
                panel.update_in(cx, |p, window, cx| {
                    for v in p.panes().cloned().collect::<Vec<_>>() {
                        v.update(cx, |v, cx| {
                            v.session
                                .emu
                                .lock()
                                .feed(b"see src/lib.rs:3 and https://example.com\r\n");
                            v.hover_links(Some(gpui::point(px(40.), px(12.))), true, cx);
                            v.toggle_hints(window, cx);
                        });
                    }
                });
                cx.run_until_parked();
                // What refs asks before removing a worktree: the folder's terminals, none above it.
                assert_eq!(panel.read_with(cx, |p, cx| p.running_in(&dir, cx)), 4);
                assert_eq!(
                    panel.read_with(cx, |p, cx| p.running_in(&dir.join("elsewhere"), cx)),
                    0
                );
                let terms: Vec<_> = panel.read_with(cx, |p, cx| {
                    p.panes()
                        .map(|v| Arc::downgrade(&v.read(cx).session.emu))
                        .collect()
                });
                assert_eq!(terms.len(), 4);
                panel.update_in(cx, |p, window, cx| {
                    if round == 0 {
                        while p.has_terminals() {
                            p.close_tab_at(0, window, cx);
                        }
                    } else {
                        p.close_all(cx);
                    }
                });
                cx.update(|window, cx| window.draw(cx).clear(cx));
                cx.run_until_parked();
                assert_eq!(panel.read_with(cx, |p, _| p.panes.len()), 0);
                assert_eq!(panel.read_with(cx, |p, cx| p.running_in(&dir, cx)), 0);
                // The terminals, scrollback and all, went with their panes: a reader thread holds
                // one only for the read it's parsing.
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
                while terms.iter().any(|t| t.strong_count() > 0)
                    && std::time::Instant::now() < deadline
                {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                assert!(terms.iter().all(|t| t.strong_count() == 0));
            }
            cx.update(|window, _| window.remove_window());
            cx.run_until_parked();
        }
        cx.update(|cx| cx.assert_no_new_leaks(&before));
    }
}
