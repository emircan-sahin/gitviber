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
    PromptLevel, Render, ScrollHandle, SharedString, StatefulInteractiveElement as _,
    StyleRefinement, Styled as _, Subscription, Task, WeakEntity, Window, canvas, div,
    prelude::FluentBuilder as _, px,
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
use gitviber_ui::widgets::Toggle as _;
use gitviber_ui::workspace::{
    PanelContext, PanelSpec, Project, ProjectEvent, Slot, Workspace, WorkspaceEvent,
};
use gitviber_ui::{ActiveAppTheme as _, AppTheme, Settings};

mod agents_menu;
mod attention;
mod reorder;
mod resume;
mod saved;
mod tab_dots;

pub use attention::{agents_in, calling, paste_to_agent, stopped_by_quit, worktree_agent};
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

/// A new pane or tab id. One count for every window's panel: the Agents menu, notifications and
/// element ids find a pane by its id alone, so two windows' "pane 1" would be mixed up.
fn fresh_id() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

pub struct TerminalPanel {
    core: Arc<Core>,
    config: TerminalConfig,
    theme: TerminalTheme,
    root: PathBuf,
    sets: HashMap<PathBuf, TabSet>,
    panes: HashMap<PaneId, PaneSlot>,
    maximized: bool,
    zoomed: bool,
    worktrees: Vec<WorktreeInfo>,
    find: Option<FindBar>,
    rename: Option<Rename>,
    flash: Option<(PaneId, Task<()>)>,
    /// A link being followed that asked git or GitHub first.
    follow: Option<Task<()>>,
    /// What find had when it closed, for its next open.
    last_find: (String, FindOptions),
    link_cache: crate::linkhost::SharedCache,
    link_ctx: Option<Rc<LinkCtx>>,
    /// Agent lookups in flight.
    agent_tasks: Vec<Task<()>>,
    /// The session save's next round.
    round: Option<Task<()>>,
    epoch: std::time::Instant,
    /// The worktrees whose saved terminals this panel took up, and so saves.
    restored_here: std::collections::HashSet<PathBuf>,
    /// Last run's terminals offered and not taken up yet, by worktree.
    offers: HashMap<PathBuf, saved::SavedSet>,
    divider: Option<DividerDrag>,
    split_bounds: SplitBounds,
    killing: bool,
    focus: FocusHandle,
    /// The tab strip, reached by keyboard only (a click goes into the terminal).
    strip_focus: FocusHandle,
    strip_scroll: ScrollHandle,
    /// The open tab as last drawn: a newly opened one is scrolled into view.
    strip_shown: Option<u64>,
    /// Branches of tabs outside the repo's worktrees, by tab, read once as each first shows:
    /// nothing watches that folder, so a checkout there shows on a new tab.
    outside_branches: HashMap<u64, Option<String>>,
    branch_reads: HashMap<u64, Task<()>>,
    /// Set when the panel lives in a workspace: its project's worktree, Settings and theme.
    bound: Option<Bound>,
    /// The home folder it started in while no project was open, until the first one opens.
    home: Option<PathBuf>,
}

struct Bound {
    workspace: WeakEntity<Workspace>,
    _project: Vec<Subscription>,
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

/// Every window's terminals saved for the next run, their agents as last known: the app calls it
/// as it quits, before ending the shells (which would leave no agent to save).
pub fn save_before_quit(cx: &mut App) {
    let Some(live) = cx.try_global::<LivePanels>() else {
        return;
    };
    for panel in live.0.clone().iter().filter_map(|p| p.upgrade()) {
        panel.update(cx, |p, cx| p.save_at_quit(cx));
    }
}

/// A split's pane in its tab's tooltip, by the number its header shows: "2 · api — Claude Code
/// working".
fn pane_line(n: usize, title: &str, look: Option<&str>) -> String {
    match look {
        Some(look) => format!("{n} · {title} — {look}"),
        None => format!("{n} · {title}"),
    }
}

/// A tab's tooltip, and what a screen reader says for it: its name, where it runs (and what, for
/// one pane), then a split's panes a line each, or the one dot's state. Read aloud, the branch
/// is in it and "; pane" goes between panes so one doesn't run into the next.
fn tab_label(
    name: Option<&str>,
    cwd: &str,
    program: Option<&str>,
    branch: Option<&str>,
    lines: &[String],
    look: Option<&str>,
) -> (String, String) {
    let split = !lines.is_empty();
    let where_ = match program.filter(|_| !split) {
        Some(p) => format!("{cwd} · {p}"),
        None => cwd.to_string(),
    };
    let head = match name {
        Some(n) => format!("{n} · {where_}"),
        None => where_,
    };
    let tip = match (split, look) {
        (true, _) => std::iter::once(head.clone())
            .chain(lines.iter().cloned())
            .collect::<Vec<_>>()
            .join("\n"),
        (false, Some(look)) => format!("{head} · {look}"),
        (false, None) => head.clone(),
    };
    let count = split.then(|| format!("{} panes", lines.len()));
    let mut aria = [
        Some(head),
        branch.map(String::from),
        count.or(look.map(String::from)),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" · ");
    for l in lines {
        aria.push_str(&format!("; pane {l}"));
    }
    (tip, aria)
}

/// The key that flips a find option, Monaco's: ⌥⌘C / ⌥⌘W / ⌥⌘R on macOS, Alt+C / Alt+W / Alt+R
/// elsewhere (0 case, 1 whole word, 2 regex).
fn find_option_key(key: &str, m: &gpui::Modifiers, mac: bool) -> Option<u8> {
    if !m.alt || m.shift || m.control || m.platform != mac {
        return None;
    }
    match key {
        "c" => Some(0),
        "w" => Some(1),
        "r" => Some(2),
        _ => None,
    }
}

/// Runs `f` on the terminal panel of the window in front once the key dispatch is over (the
/// window is busy during it); `show`: the panel opened for it.
fn in_front(
    cx: &mut App,
    show: bool,
    f: impl FnOnce(&mut TerminalPanel, &mut Window, &mut Context<TerminalPanel>) + 'static,
) {
    let Some(handle) = cx.active_window() else {
        return;
    };
    cx.defer(move |cx| {
        let _ = handle.update(cx, |_, window, cx| {
            let Some(ws) = Workspace::for_window(window, cx) else {
                return;
            };
            if show {
                ws.update(cx, |ws, cx| ws.set_open(Slot::Bottom, true, cx));
            }
            // Built outside the workspace's update: the panel reads the workspace as it's made.
            let panel = Workspace::ensure_panel(&ws, "terminal", window, cx);
            if let Some(p) = panel.and_then(|p| p.view().downcast::<TerminalPanel>().ok()) {
                p.update(cx, |p, cx| f(p, window, cx));
            }
        });
    });
}

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
    // Global as they were: they work whatever has the keys, the panel shown for them.
    cx.on_action(|_: &terminal::ResumeConversation, cx| {
        in_front(cx, false, |p, window, cx| {
            p.ask_conversation(p.root.clone(), window, cx)
        })
    });
    cx.on_action(|_: &terminal::New, cx| {
        in_front(cx, true, |p, window, cx| p.new_terminal(window, cx))
    });
    cx.on_action(|_: &terminal::EqualizePanes, cx| {
        in_front(cx, true, |p, _, cx| {
            if let Some(gid) = p.active_group().map(|g| g.id) {
                p.equalize(gid, None, cx);
            }
        })
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
        screen_reader: s.terminal_screen_reader,
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
            maximized: false,
            zoomed: false,
            worktrees: Vec::new(),
            find: None,
            rename: None,
            flash: None,
            follow: None,
            last_find: Default::default(),
            link_cache: Default::default(),
            link_ctx: None,
            agent_tasks: Vec::new(),
            round: None,
            epoch: std::time::Instant::now(),
            restored_here: Default::default(),
            offers: HashMap::new(),
            divider: None,
            split_bounds: Rc::default(),
            killing: false,
            focus: cx.focus_handle(),
            strip_focus: cx.focus_handle().tab_stop(true),
            strip_scroll: ScrollHandle::new(),
            strip_shown: None,
            outside_branches: HashMap::new(),
            branch_reads: HashMap::new(),
            bound: None,
            home: None,
        }
    }

    /// The panel for a workspace: the Core, Settings and theme are the app's, and the tab set
    /// follows the project's worktree.
    pub fn for_workspace(ctx: &PanelContext, window: &mut Window, cx: &mut Context<Self>) -> Self {
        // A per-window panel is given no project: the workspace's is the one open now.
        let project = ctx.project.clone().or_else(|| {
            let ws = ctx.workspace.upgrade()?;
            ws.read(cx).project().cloned()
        });
        let root = project
            .as_ref()
            .map(|p| p.read(cx).root().to_path_buf())
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
            .unwrap_or_default();
        let (config, theme) = (
            config_from(Settings::get(cx), cx.app_theme()),
            theme_from(cx.app_theme()),
        );
        let mut this = Self::new(gitviber_ui::core(cx), root, config, theme, window, cx);
        this.home = project.is_none().then(|| this.root.clone());
        let mut subscriptions = vec![
            cx.observe_global::<Settings>(|this, cx| this.sync_look(cx)),
            cx.observe_global::<AppTheme>(|this, cx| this.sync_look(cx)),
            // A worktree's color picked: its tabs take it.
            cx.observe_global::<gitviber_refs::store::Saved>(|_, cx| cx.notify()),
            // Recent projects fill the new-terminal menu.
            cx.observe_global::<gitviber_ui::UiState>(|_, cx| cx.notify()),
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
        let project = project
            .as_ref()
            .map_or_else(Vec::new, |p| this.watch(p, window, cx));
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
    ) -> Vec<Subscription> {
        let repo = gitviber_refs::RepoState::get(project, cx);
        vec![
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
            ),
            // The repo's worktrees land after RepoChanged; tab labels, colors and the
            // worktree menu are drawn from them.
            cx.observe(&repo, |this, _, cx| {
                this.update_links(cx);
                cx.notify();
            }),
        ]
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
                    if let Some(home) = self.home.take() {
                        let open = ws.read(cx).is_open(Slot::Bottom);
                        self.leave_home(home, open, window, cx);
                    }
                }
                let sub = project
                    .as_ref()
                    .map_or_else(Vec::new, |p| self.watch(p, window, cx));
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

    /// The first project opened: the terminals started in the home folder before it (a launch
    /// draws one before its project is there) give way to one in the project; any typed into
    /// or running a command move over to the project's tabs instead.
    fn leave_home(
        &mut self,
        home: PathBuf,
        open: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if home == self.root {
            return;
        }
        if let Some(set) = self.sets.remove(&home) {
            let ids: Vec<PaneId> = set.groups.iter().flat_map(|g| g.panes.clone()).collect();
            let used = ids.iter().any(|id| {
                self.panes.get(id).is_some_and(|s| {
                    let p = s.view.read(cx);
                    p.was_used() || p.session.busy()
                })
            });
            if used {
                let here = self.set_mut();
                here.active = here.active.or(set.active);
                here.groups.extend(set.groups);
                self.apply_dims(cx);
            } else {
                for id in ids {
                    if let Some(slot) = self.panes.remove(&id) {
                        slot.view.update(cx, |p, _| p.session.close());
                    }
                }
                Self::release_memory_soon(cx);
            }
            self.save_soon(cx);
            cx.notify();
        }
        if open && !self.has_terminals() {
            self.new_terminal(window, cx);
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

    /// The repo's worktrees a terminal can open in, with their branches; empty until refs has
    /// listed them.
    fn repo_worktrees(&self, cx: &App) -> Vec<WorktreeInfo> {
        let mut out: Vec<WorktreeInfo> = self.worktrees.clone();
        if let Some(repo) = self
            .workspace()
            .and_then(|ws| ws.read(cx).project().cloned())
            .and_then(|p| gitviber_refs::RepoState::existing(&p, cx))
        {
            for w in gitviber_refs::worktrees::list(&repo.read(cx).worktrees) {
                let path = PathBuf::from(&w.path);
                if !w.bare && !w.prunable && !out.iter().any(|o| o.path == path) {
                    out.push(WorktreeInfo {
                        path,
                        branch: w.branch,
                    });
                }
            }
        }
        out
    }

    /// The repo's other worktrees, for the ▾ menu.
    fn other_worktrees(&self, cx: &App) -> Vec<WorktreeInfo> {
        let mut out = self.repo_worktrees(cx);
        out.retain(|w| w.path != self.root);
        out
    }

    /// Reads the branch of each shown tab outside `listed` once; forgets closed tabs'.
    fn read_outside_branches(&mut self, listed: &[WorktreeInfo], cx: &mut Context<Self>) {
        // Worktrees not listed yet: every tab would look outside.
        if listed.is_empty() {
            return;
        }
        let live: std::collections::HashSet<u64> = self
            .sets
            .values()
            .flat_map(|s| s.groups.iter().map(|g| g.id))
            .collect();
        self.outside_branches.retain(|id, _| live.contains(id));
        self.branch_reads.retain(|id, _| live.contains(id));
        let new: Vec<(u64, PathBuf)> = self
            .set()
            .map(|s| s.groups.iter())
            .into_iter()
            .flatten()
            .filter(|g| !self.outside_branches.contains_key(&g.id))
            .filter_map(|g| {
                Some((
                    g.id,
                    self.panes
                        .get(&g.panes[0])?
                        .view
                        .read(cx)
                        .cwd()
                        .to_path_buf(),
                ))
            })
            .filter(|(_, cwd)| !listed.iter().any(|w| &w.path == cwd))
            .collect();
        for (gid, cwd) in new {
            self.outside_branches.insert(gid, None);
            let core = self.core.clone();
            let path = cwd.to_string_lossy().into_owned();
            let read = cx.spawn(async move |this, cx| {
                let branch = cx
                    .background_spawn(async move {
                        gitviber_core::api::repo::folder_branch(&core, path)
                            .ok()
                            .flatten()
                    })
                    .await;
                let _ = this.update(cx, |this, cx| {
                    this.branch_reads.remove(&gid);
                    if let Some(slot) = this.outside_branches.get_mut(&gid)
                        && branch.is_some()
                    {
                        *slot = branch;
                        cx.notify();
                    }
                });
            });
            self.branch_reads.insert(gid, read);
        }
    }

    /// The saved projects other than this one: a terminal there leaves the window on this one.
    fn other_projects(&self, cx: &App) -> Vec<String> {
        let main = self
            .workspace()
            .and_then(|ws| ws.read(cx).project().cloned())
            .map(|p| {
                let p = p.read(cx);
                p.main_worktree().unwrap_or(p.root()).to_path_buf()
            })
            .unwrap_or_else(|| self.root.clone());
        cx.try_global::<gitviber_ui::UiState>()
            .map(|s| {
                s.recent_projects
                    .iter()
                    .filter(|p| Path::new(p) != main)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    /// A tab's color: its worktree's, as the picker shows it (picked, else its name's); a tab
    /// elsewhere only one picked for its folder.
    fn tab_hue(&self, cwd: &Path, cx: &mut Context<Self>) -> Option<gpui::Hsla> {
        use gitviber_refs::worktrees::{hue_color, worktree_hue, worktree_hues};
        let picked = gitviber_refs::store::colors(cx);
        let list: Vec<(String, bool)> = self
            .workspace()
            .and_then(|ws| ws.read(cx).project().cloned())
            .and_then(|p| gitviber_refs::RepoState::existing(&p, cx))
            .map(|r| {
                gitviber_refs::worktrees::list(&r.read(cx).worktrees)
                    .into_iter()
                    .map(|w| (w.path, w.main))
                    .collect()
            })
            .unwrap_or_default();
        let key = cwd.to_string_lossy().into_owned();
        let hue = match worktree_hues(&list, &picked).get(&key) {
            Some(h) => *h,
            None => worktree_hue(&key, true, &picked),
        };
        let t = cx.try_global::<AppTheme>()?;
        hue.map(|h| hue_color(h, t.tint_l, t.tint_c, 1.))
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
        let id = fresh_id();
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
        let gid = fresh_id();
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
        let had_focus = window.focused(cx).is_some();
        // What goes with the pane: keys in any of it move on once the close is done.
        let mut going = vec![slot.view.read(cx).focus_handle(cx)];
        slot.view.update(cx, |p, _| p.session.close());
        drop(slot);
        Self::release_memory_soon(cx);
        if let Some(f) = self.find.as_ref().filter(|f| f.pane == Some(id)) {
            going.push(f.input.focus_handle(cx));
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
        // A name being typed for the pane, or for its tab that went with it.
        let renamed_gone = self.rename.as_ref().is_some_and(|r| match r.target {
            RenameTarget::Pane(p) => p == id,
            RenameTarget::Tab(gid) => !self
                .sets
                .values()
                .any(|s| s.groups.iter().any(|g| g.id == gid)),
        });
        if renamed_gone && let Some(r) = self.rename.take() {
            going.push(r.input.focus_handle(cx));
        }
        self.apply_dims(cx);
        attention::agents_changed(cx);
        self.save_soon(cx);
        let last = emptied.contains(&self.root);
        if last {
            self.set_maximized(false, cx);
            if let Some(ws) = self.workspace() {
                ws.update(cx, |ws, cx| ws.set_open(Slot::Bottom, false, cx));
            }
            cx.emit(PanelEvent::Empty);
        }
        // Only keys the close took away move on, once a menu that killed it has put its focus
        // back: a shell exiting in the background leaves the commit box alone, ⌫ on the tabs
        // stays on them.
        cx.defer_in(window, move |this, window, cx| {
            let lost = match window.focused(cx) {
                None => had_focus,
                Some(_) => {
                    going.iter().any(|f| f.contains_focused(window, cx))
                        || (last && this.focus.contains_focused(window, cx))
                }
            };
            if !lost {
                return;
            }
            if last {
                if let Some(ws) = this.workspace() {
                    ws.update(cx, |ws, cx| ws.focus_center(window, cx));
                }
            } else {
                this.focus_active(window, cx);
            }
        });
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

    /// Keys to the tab strip (terminal.focusTabs): from a pane Tab is the shell's. Again from
    /// the strip, back into the terminal. Not `focus_back`: the chord is bound only inside the
    /// terminal, so the active pane is where focus came from.
    pub fn focus_strip(&self, window: &mut Window, cx: &mut App) {
        if self.strip_focus.is_focused(window) {
            return self.focus_active(window, cx);
        }
        if self.set().is_some_and(|s| !s.groups.is_empty()) {
            window.focus(&self.strip_focus, cx);
        }
    }

    /// Keys on the focused tab strip: ←/→ (Home/End) open another tab and stay on the strip, ↵,
    /// Space or Esc go into the terminal, ⌫ kills it (⌘W too, as tab.close). ⌥←/⌥→ are
    /// tab.moveLeft/moveRight (`move_group`).
    fn strip_key(&mut self, e: &gpui::KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(set) = self.set() else { return };
        // Its binding is the panes' own: on the strip, its chord (as bound now) goes back in.
        let again = cx
            .all_bindings_for_input(std::slice::from_ref(&e.keystroke))
            .iter()
            .any(|b| b.action().partial_eq(&terminal::FocusTabs));
        if again && self.strip_focus.is_focused(window) {
            self.focus_active(window, cx);
            return cx.stop_propagation();
        }
        // A tab's name field has its own keys (a space is typed).
        if !self.strip_focus.is_focused(window)
            || e.keystroke.modifiers.modified()
            || set.groups.is_empty()
        {
            return;
        }
        let ids: Vec<u64> = set.groups.iter().map(|g| g.id).collect();
        let at = ids
            .iter()
            .position(|id| Some(*id) == set.active)
            .unwrap_or(0);
        let last = ids.len() - 1;
        match e.keystroke.key.as_str() {
            "left" | "right" | "home" | "end" => {
                let to = match e.keystroke.key.as_str() {
                    "left" => at.saturating_sub(1),
                    "right" => (at + 1).min(last),
                    "home" => 0,
                    _ => last,
                };
                self.set_mut().active = Some(ids[to]);
                self.apply_dims(cx);
                cx.notify();
            }
            "enter" | "space" | "escape" => self.focus_active(window, cx),
            "backspace" | "delete" => self.close_group(ids[at], window, cx),
            _ => return,
        }
        cx.stop_propagation();
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
            move |this, input, event: &InputEvent, window, cx| {
                let enter = match event {
                    InputEvent::PressEnter { .. } => true,
                    // A click elsewhere keeps its focus. The window going to the back blurs it
                    // too: the field is still there to come back to.
                    InputEvent::Blur if window.is_window_active() => false,
                    _ => return,
                };
                let name = input.read(cx).value().trim().to_string();
                this.finish_rename(Some(name), cx);
                if enter {
                    this.focus_active(window, cx);
                }
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
    fn finish_rename(&mut self, name: Option<String>, cx: &mut Context<Self>) {
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
            // Opened again: the last query, its options and its matches come back.
            let (query, options) = self.last_find.clone();
            if !query.is_empty() {
                input.update(cx, |i, cx| i.set_value(query, window, cx));
            }
            self.find = Some(FindBar {
                input,
                options,
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
        if let Some(f) = self.find.take() {
            self.last_find = (f.input.read(cx).value().to_string(), f.options);
            if let Some(slot) = f.pane.and_then(|p| self.panes.get(&p)) {
                slot.view.update(cx, |p, cx| p.clear_find(cx));
            }
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
        let listed = self.repo_worktrees(cx);
        self.read_outside_branches(&listed, cx);
        let strip_focused = self.strip_focus.is_focused(window);
        if active != self.strip_shown {
            if let Some(at) = set_groups.iter().position(|(g, _)| Some(*g) == active) {
                self.strip_scroll.scroll_to_item(at);
            }
            self.strip_shown = active;
        }
        let mut tabs = h_flex()
            .id("terminal-tabs")
            .track_focus(&self.strip_focus)
            .track_scroll(&self.strip_scroll)
            .key_context("TabStrip")
            .role(gpui::Role::TabList)
            .aria_label("Terminals")
            // A click goes into the terminal (the tab's own handler), not to the strip.
            .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
            .on_key_down(cx.listener(Self::strip_key))
            .on_action(cx.listener(|this, _: &tab::MoveLeft, _, cx| this.move_group(-1, cx)))
            .on_action(cx.listener(|this, _: &tab::MoveRight, _, cx| this.move_group(1, cx)))
            .on_action(cx.listener(|this, _: &tab::Close, window, cx| {
                if let Some(gid) = this.set().and_then(|s| s.active) {
                    this.close_group(gid, window, cx);
                }
            }))
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_x_scroll();
        for (at, (gid, _)) in set_groups.iter().enumerate() {
            let active = Some(*gid) == active;
            let ring = active && strip_focused;
            let place = (at, set_groups.len());
            tabs = tabs.child(self.render_tab(*gid, place, active, ring, &listed, cx));
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
        let worktrees = self.other_worktrees(cx);
        let projects = self.other_projects(cx);
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
                    .cursor_pointer()
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
        // The strip's bottom line is drawn by everything but the open tab, which it would cut.
        let tabs = tabs.child(div().flex_1().h_full().border_b_1().border_color(t.border));
        h_flex()
            .h(px(TOOLBAR_HEIGHT))
            .flex_none()
            .items_stretch()
            .bg(t.panel)
            .child(tabs)
            .child(
                h_flex()
                    .flex_none()
                    .items_center()
                    .border_b_1()
                    .border_color(t.border)
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
                            "New terminal in another folder, or resume a conversation",
                        )
                        .dropdown_menu(move |menu, _, _| {
                            let resume = weak.clone();
                            let mut menu = menu
                                .min_w(px(260.))
                                .item(PopupMenuItem::new("Resume a Conversation…").on_click(
                                    move |_, window, cx| {
                                        let _ = resume.update(cx, |this, cx| {
                                            this.ask_conversation(this.root.clone(), window, cx)
                                        });
                                    },
                                ))
                                .separator();
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
                            if !projects.is_empty() {
                                menu = menu.label("New terminal in project");
                                for path in &projects {
                                    let (weak, at) = (weak.clone(), PathBuf::from(path));
                                    // Where it is, as a worktree shows its branch: same-named
                                    // projects tell apart.
                                    let parent = at.parent().map(folder_name).unwrap_or_default();
                                    let label = format!("{}  {parent}", folder_name(&at));
                                    menu = menu.item(PopupMenuItem::new(label).on_click(
                                        move |_, window, cx| {
                                            let _ = weak.update(cx, |this, cx| {
                                                this.open_terminal(at.clone(), None, window, cx)
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

    /// `place`: the tab's index among the strip's, and their count.
    fn render_tab(
        &self,
        gid: u64,
        (at, count): (usize, usize),
        active: bool,
        ring: bool,
        listed: &[WorktreeInfo],
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
        let branch = match listed.iter().find(|w| w.path == cwd) {
            Some(w) => w.branch.clone(),
            None => self.outside_branches.get(&gid).cloned().flatten(),
        };
        let here = cwd == self.root;
        let hue = self.tab_hue(&cwd, cx);
        let order = layout::pane_order(&g.layout);
        let look = crate::agent::most_urgent(g.panes.iter().map(|id| self.look_of_pane(*id)));
        let look_tip = look.map(|l| self.look_label(&g.panes, l));
        let lines: Vec<String> = if order.len() > 1 {
            order
                .iter()
                .enumerate()
                .map(|(i, &id)| {
                    let look = self.look_of_pane(id).map(|l| self.look_label(&[id], l));
                    pane_line(i + 1, &self.pane_title(id, cx), look.as_deref())
                })
                .collect()
        } else {
            vec![]
        };
        let program = self
            .panes
            .get(&g.panes[0])
            .and_then(|s| s.view.read(cx).title().map(String::from));
        let (tip, aria) = tab_label(
            g.name.as_deref(),
            &cwd.to_string_lossy(),
            program.as_deref(),
            branch.as_deref(),
            &lines,
            look_tip.as_deref(),
        );
        let focused = g.focused;
        // A split's panes by number, each in its agent's look; else the tab's one dot.
        let dots = if order.len() > 1 {
            let panes: Vec<_> = order
                .iter()
                .map(|&id| (self.look_of_pane(id), id == focused))
                .collect();
            Some(tab_dots::split_dots(gid, &panes, cx))
        } else {
            look.map(|l| attention::look_dot(l, cx))
        };
        let dots = dots.map(|d| div().flex().items_center().child(d));
        let weak = cx.entity().downgrade();
        let alone = count == 1;
        let dragged = reorder::DraggedTerminal::new(gid, at, title.clone());
        let line = t.primary;
        let (focus_ring, medium) = {
            let app = cx.app_theme();
            (app.ring, app.medium())
        };
        div()
            .id(("terminal-tab", gid))
            .role(gpui::Role::Tab)
            .aria_label(aria)
            .aria_selected(active)
            // The dot's state, where it runs, a split's panes a line each.
            .tooltip(move |window, cx| {
                gpui_kit::component::tooltip::Tooltip::new(tip.clone()).build(window, cx)
            })
            .group("terminal-tab")
            .relative()
            .flex()
            .flex_none()
            .h_full()
            .max_w(px(256.))
            .items_center()
            .gap_1p5()
            .pl_3()
            .pr_1p5()
            .border_r_1()
            .border_color(t.border)
            .text_size(px(12.))
            .cursor_pointer()
            // The open tab is the panes' own surface with no line under it, so it reads as part
            // of them; the others sit back on the strip, over its bottom line.
            .when(active, |d| d.bg(t.background).text_color(t.foreground))
            .when(!active, |d| {
                d.bg(t.panel)
                    .border_b_1()
                    .text_color(t.muted_foreground)
                    .hover(|s| s.bg(t.hover).text_color(t.foreground))
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
            // The strip has the keys: the open tab says so.
            .when(ring, |d| {
                d.child(
                    div()
                        .absolute()
                        .inset_0()
                        .border_2()
                        .border_color(focus_ring),
                )
            })
            // Its worktree's color, as the worktree picker shows it.
            .when_some(hue, |d, hue| {
                d.child(
                    div()
                        .absolute()
                        .top(px(8.))
                        .bottom(px(8.))
                        .left_0()
                        .w(px(2.))
                        .rounded_r_full()
                        .bg(hue),
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
            .when(!alone && renaming.is_none(), |d| {
                d.on_drag(dragged, |tab, _, _, cx| cx.new(|_| tab.clone()))
                    .drag_over::<reorder::DraggedTerminal>(move |s, tab, _, _| {
                        match reorder::drop_edge(tab.from, at) {
                            Some(right) => reorder::drop_line(s, right, line),
                            None => s,
                        }
                    })
                    .on_drop(
                        cx.listener(move |this, tab: &reorder::DraggedTerminal, _, cx| {
                            this.move_group_to(tab.gid, at, cx);
                        }),
                    )
            })
            .child(
                Icon::new(IconName::SquareTerminal)
                    .size(px(14.))
                    .text_color(hue.unwrap_or(if here { t.primary } else { t.subtle })),
            )
            .child(match renaming {
                Some(input) => Self::rename_field(&input, 140.),
                None => div()
                    .truncate()
                    .when(active, |d| d.font_weight(medium))
                    .child(title)
                    .into_any_element(),
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
                    .cursor_pointer()
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
                .when(!alone, |m| {
                    m.separator()
                        .item(
                            item("Move Left", |this, gid, _, cx| {
                                this.nudge_group(gid, -1, cx)
                            })
                            .disabled(at == 0),
                        )
                        .item(
                            item("Move Right", |this, gid, _, cx| {
                                this.nudge_group(gid, 1, cx)
                            })
                            .disabled(at + 1 == count),
                        )
                })
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

    /// The name being typed, in a tab or a pane's header: clicks in it (a double-click selects a
    /// word) aren't the tab's or the header's.
    fn rename_field(input: &Entity<InputState>, width: f32) -> AnyElement {
        div()
            .id("rename")
            .debug_selector(|| "terminal-rename".into())
            .w(px(width))
            .cursor_text()
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(Input::new(input).xsmall())
            .into_any_element()
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
        let weak = cx.entity().downgrade();
        // Not the header's click or double-click (a focus jump, a rename), and a double-click's
        // second click doesn't act again on what moved under it.
        let action =
            |name: &'static str,
             icon: IconName,
             tip: &'static str,
             run: fn(&mut Self, PaneId, &mut Window, &mut Context<Self>)| {
                let key = format!("{name}-{id}");
                let weak = weak.clone();
                Button::new(gpui::ElementId::Name(key.clone().into()))
                    .ghost()
                    .xsmall()
                    .icon(Icon::new(icon))
                    .tooltip(tip)
                    .debug_selector(move || key)
                    .on_click(move |e, window, cx| {
                        cx.stop_propagation();
                        if e.click_count() < 2 {
                            let _ = weak.update(cx, |this, cx| run(this, id, window, cx));
                        }
                    })
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
            .cursor_pointer()
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
                Some(input) => Self::rename_field(&input, 180.),
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
                    .child(action(
                        "split-right",
                        IconName::Columns2,
                        "Split right",
                        |this, id, w, cx| this.split(Dir::Row, Some(id), w, cx),
                    ))
                    .child(action(
                        "split-down",
                        IconName::Rows2,
                        "Split down",
                        |this, id, w, cx| this.split(Dir::Col, Some(id), w, cx),
                    ))
                    .child(action(
                        "kill-pane",
                        IconName::X,
                        "Kill pane",
                        |this, id, w, cx| this.kill(vec![id], "this terminal", w, cx),
                    )),
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
                .toggle(on, cx)
                .on_click(cx.listener(move |this, _, _, cx| this.toggle_find_option(which, cx)))
        };
        Some(
            h_flex()
                .key_context("TerminalFind")
                .on_key_down(cx.listener(|this, e: &gpui::KeyDownEvent, _, cx| {
                    let k = &e.keystroke;
                    if let Some(which) =
                        find_option_key(&k.key, &k.modifiers, cfg!(target_os = "macos"))
                    {
                        cx.stop_propagation();
                        this.toggle_find_option(which, cx);
                    }
                }))
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
            .on_action(cx.listener(|this, _: &terminal::FocusTabs, w, cx| this.focus_strip(w, cx)))
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

    #[test]
    fn find_options_flip_on_monacos_keys() {
        let m = |alt, platform, shift, control| gpui::Modifiers {
            alt,
            platform,
            shift,
            control,
            ..Default::default()
        };
        assert_eq!(
            find_option_key("c", &m(true, true, false, false), true),
            Some(0)
        );
        assert_eq!(
            find_option_key("w", &m(true, true, false, false), true),
            Some(1)
        );
        assert_eq!(
            find_option_key("r", &m(true, false, false, false), false),
            Some(2)
        );
        // ⌥C alone types ç on a Mac; ⌥⌘C elsewhere is the desktop's; Shift is another key.
        assert_eq!(
            find_option_key("c", &m(true, false, false, false), true),
            None
        );
        assert_eq!(
            find_option_key("c", &m(true, true, false, false), false),
            None
        );
        assert_eq!(
            find_option_key("c", &m(true, true, true, false), true),
            None
        );
        assert_eq!(
            find_option_key("x", &m(true, true, false, false), true),
            None
        );
    }
    #[test]
    fn a_tab_says_where_it_runs_and_a_split_each_pane() {
        let (tip, aria) = tab_label(
            None,
            "/w/a",
            Some("vim"),
            Some("main"),
            &[],
            Some("Claude Code working"),
        );
        assert_eq!(tip, "/w/a · vim · Claude Code working");
        assert_eq!(aria, "/w/a · vim · main · Claude Code working");
        let lines = [
            pane_line(1, "srv", None),
            pane_line(2, "claude", Some("Claude Code asks")),
        ];
        let (tip, aria) = tab_label(Some("api"), "/w/b", Some("ignored"), None, &lines, None);
        assert_eq!(tip, "api · /w/b\n1 · srv\n2 · claude — Claude Code asks");
        assert_eq!(
            aria,
            "api · /w/b · 2 panes; pane 1 · srv; pane 2 · claude — Claude Code asks"
        );
        assert_eq!(
            tab_label(None, "/", None, None, &[], None),
            ("/".into(), "/".into())
        );
    }

    use gpui::{TestAppContext, VisualTestContext};

    /// Drawn panes start shells, whose reader threads wake them as in the app: the test scheduler
    /// would fail the test on such a wake (one came on a loaded machine).
    fn real_shells(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        cx.executor().forbid_parking();
    }

    /// The terminal button's dot: a pane with news calls whatever folder it runs in.
    #[gpui::test]
    fn a_pane_anywhere_with_news_calls(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            gitviber_ui::test_support::install_theme(cx);
            crate::init(cx);
        });
        let core = Arc::new(Core::new(Arc::new(|_| {})));
        let dir = std::env::temp_dir();
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
        for _ in 0..2 {
            panel.update_in(cx, |p, window, cx| {
                p.open_terminal(dir.clone(), None, window, cx)
            });
        }
        // As a workspace's panel joins them.
        let weak = panel.downgrade();
        cx.update(|_, cx| cx.default_global::<LivePanels>().0.push(weak));
        let calling = |cx: &mut VisualTestContext| cx.update(|_, cx| super::calling(cx));
        assert_eq!(calling(cx), None);
        panel.update(cx, |p, _| {
            p.panes.values_mut().next().expect("a pane").rang = true;
        });
        assert_eq!(calling(cx), Some(gitviber_ui::widgets::Look::Unread));
        panel.update(cx, |p, cx| p.close_all(cx));
        assert_eq!(calling(cx), None);
        cx.update(|window, _| window.remove_window());
        cx.run_until_parked();
    }

    /// The focused strip: arrows open tabs and keep the keys, ⌥→ moves the open tab, ↵ goes in;
    /// a screen reader gets the pane's screen without trailing blanks.
    #[gpui::test]
    fn the_focused_tab_strip_takes_the_arrows(cx: &mut TestAppContext) {
        real_shells(cx);
        cx.update(|cx| {
            gpui_kit::init(cx);
            gitviber_ui::test_support::install_theme(cx);
            crate::init(cx);
        });
        let core = Arc::new(Core::new(Arc::new(|_| {})));
        let dir = std::env::temp_dir();
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
        for _ in 0..3 {
            panel.update_in(cx, |p, window, cx| {
                p.open_terminal(dir.clone(), None, window, cx)
            });
        }
        let groups = |cx: &mut VisualTestContext| {
            panel.read_with(cx, |p, _| {
                let s = p.set().expect("a set");
                (s.groups.iter().map(|g| g.id).collect::<Vec<_>>(), s.active)
            })
        };
        let (ids, active) = groups(cx);
        assert_eq!(active, Some(ids[2]));
        panel.update_in(cx, |p, window, cx| window.focus(&p.strip_focus, cx));
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.simulate_keystrokes("home");
        assert_eq!(groups(cx).1, Some(ids[0]));
        cx.simulate_keystrokes("right left right");
        assert_eq!(groups(cx).1, Some(ids[1]));
        cx.simulate_keystrokes("end right");
        assert_eq!(groups(cx).1, Some(ids[2]));
        assert!(cx.update(|window, cx| panel.read(cx).strip_focus.is_focused(window)));
        panel.update(cx, |p, cx| p.move_group(-1, cx));
        assert_eq!(groups(cx), (vec![ids[0], ids[2], ids[1]], Some(ids[2])));
        panel.update(cx, |p, cx| p.move_group(-1, cx));
        panel.update(cx, |p, cx| p.move_group(-1, cx));
        assert_eq!(groups(cx).0, vec![ids[2], ids[0], ids[1]]);
        // A drop and the tab's menu move any tab, not just the open one, which stays open.
        panel.update(cx, |p, cx| p.move_group_to(ids[1], 0, cx));
        assert_eq!(groups(cx), (vec![ids[1], ids[2], ids[0]], Some(ids[2])));
        panel.update(cx, |p, cx| p.nudge_group(ids[1], -1, cx));
        panel.update(cx, |p, cx| p.nudge_group(ids[0], 1, cx));
        assert_eq!(groups(cx).0, vec![ids[1], ids[2], ids[0]]);
        panel.update(cx, |p, cx| p.nudge_group(ids[1], 1, cx));
        panel.update(cx, |p, cx| p.move_group_to(ids[0], 1, cx));
        assert_eq!(groups(cx), (vec![ids[2], ids[0], ids[1]], Some(ids[2])));
        cx.simulate_keystrokes("enter");
        let pane = panel.read_with(cx, |p, _| p.active_pane()).expect("a pane");
        assert!(cx.update(|window, cx| pane.read(cx).focus_handle(cx).is_focused(window)));
        let text = pane.update(cx, |p, _| {
            p.session.emu.lock().feed(b"\x1b[2J\x1b[Hone  \r\ntwo\r\n");
            p.screen_text()
        });
        assert!(text.starts_with("one\ntwo"), "{text:?}");
        assert!(text.lines().all(|l| l.trim_end() == l));
        panel.update(cx, |p, cx| p.close_all(cx));
        cx.update(|window, _| window.remove_window());
        cx.run_until_parked();
    }

    /// Row 7: a tab opened past a narrow strip's edge scrolls into view, by as much as it needs; a
    /// redraw with the same tab open leaves the strip where the user scrolled it.
    #[gpui::test]
    fn the_open_tab_scrolls_into_view(cx: &mut TestAppContext) {
        real_shells(cx);
        cx.update(|cx| {
            gpui_kit::init(cx);
            gitviber_ui::test_support::install_theme(cx);
            crate::init(cx);
        });
        let core = Arc::new(Core::new(Arc::new(|_| {})));
        let dir = std::env::temp_dir();
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
        cx.simulate_resize(gpui::size(px(360.), px(300.)));
        for _ in 0..8 {
            panel.update_in(cx, |p, window, cx| {
                p.open_terminal(dir.clone(), None, window, cx)
            });
        }
        draw(cx);
        // The open tab's place in the strip as drawn, and the strip's.
        let shown = |cx: &mut VisualTestContext| {
            panel.read_with(cx, |p, _| {
                let s = p.set().expect("a set");
                let at = s
                    .groups
                    .iter()
                    .position(|g| Some(g.id) == s.active)
                    .unwrap();
                let mut tab = p.strip_scroll.bounds_for_item(at).expect("a tab");
                tab.origin += p.strip_scroll.offset();
                (tab, p.strip_scroll.bounds())
            })
        };
        let (tab, strip) = shown(cx);
        assert!(strip.size.width < tab.size.width * 8., "the tabs overflow");
        assert_eq!(tab.right(), strip.right());
        panel.update_in(cx, |p, window, cx| window.focus(&p.strip_focus, cx));
        draw(cx);
        cx.simulate_keystrokes("home");
        draw(cx);
        let (tab, strip) = shown(cx);
        assert_eq!(tab.left(), strip.left());
        let scrolled = gpui::point(px(-40.), px(0.));
        panel.update(cx, |p, _| p.strip_scroll.set_offset(scrolled));
        draw(cx);
        assert_eq!(
            panel.read_with(cx, |p, _| p.strip_scroll.offset()),
            scrolled
        );
        panel.update(cx, |p, cx| p.close_all(cx));
        cx.update(|window, _| window.remove_window());
        cx.run_until_parked();
    }

    struct Host;
    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
        }
    }

    /// A launch draws the terminal before its project opens: that first terminal, untouched, gives
    /// way to one in the project; one typed into moves over to the project's tabs. A project
    /// already open when the panel is built is where its first terminal starts.
    #[gpui::test]
    fn the_first_terminal_starts_in_the_project(cx: &mut TestAppContext) {
        let base = std::env::temp_dir().join(format!("gitviber-term-home-{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        // Plain folders open as projects under their real path ($TMPDIR is a link on macOS).
        let base = base.canonicalize().unwrap();
        let (home, a, b) = (base.join("home"), base.join("a"), base.join("b"));
        for d in [&home, &a, &b] {
            std::fs::create_dir_all(d).unwrap();
        }
        // SAFETY: nextest gives each test its own process; nothing else reads these meanwhile.
        unsafe {
            std::env::set_var("GITVIBER_CONFIG_DIR", base.join("config"));
            std::env::set_var("HOME", &home);
        }
        cx.update(|cx| {
            gitviber_ui::init(cx);
            gitviber_ui::install_core(cx);
            crate::init(cx);
        });
        let (_, cx) = cx.add_window_view(|_, _| Host);
        let panel_of = |ws: &Entity<Workspace>, cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                Workspace::ensure_panel(ws, "terminal", window, cx)
                    .and_then(|p| p.view().downcast::<TerminalPanel>().ok())
                    .expect("the terminal panel")
            })
        };
        let cwds = |panel: &Entity<TerminalPanel>, cx: &mut VisualTestContext| {
            panel.read_with(cx, |p, cx| {
                let mut all: Vec<PathBuf> = p.panes().map(|v| v.read(cx).cwd().into()).collect();
                all.sort();
                all
            })
        };
        let open = |ws: &Entity<Workspace>, root: &Path, cx: &mut VisualTestContext| {
            let root = root.to_path_buf();
            cx.update(|window, cx| ws.update(cx, |ws, cx| ws.open_project(root, window, cx)));
            cx.run_until_parked();
        };
        let workspace = |cx: &mut VisualTestContext| {
            let ws = cx.update(|_, cx| cx.new(Workspace::new));
            ws.update(cx, |ws, cx| ws.set_open(Slot::Bottom, true, cx));
            ws
        };

        // Shown before the project: its terminal starts at home, then moves to the project.
        let ws = workspace(cx);
        let panel = panel_of(&ws, cx);
        cx.run_until_parked();
        assert_eq!(cwds(&panel, cx), vec![home.clone()]);
        open(&ws, &a, cx);
        assert_eq!(panel.read_with(cx, |p, _| p.worktree().to_path_buf()), a);
        assert_eq!(cwds(&panel, cx), vec![a.clone()]);
        assert!(panel.read_with(cx, |p, _| !p.sets.contains_key(&home)));
        // Another project later is a switch: the first one's terminal stays where it is.
        open(&ws, &b, cx);
        assert_eq!(cwds(&panel, cx), vec![a.clone()]);
        panel.update(cx, |p, cx| p.close_all(cx));

        // Typed into before the project came: kept, among the project's tabs.
        let ws = workspace(cx);
        let panel = panel_of(&ws, cx);
        cx.run_until_parked();
        let first = panel
            .read_with(cx, |p, _| p.active_pane())
            .expect("a terminal");
        first.update_in(cx, |p, window, cx| p.paste_text("ls".into(), window, cx));
        open(&ws, &a, cx);
        assert_eq!(cwds(&panel, cx), vec![home.clone()]);
        assert_eq!(panel.read_with(cx, |p, _| p.active_pane()), Some(first));
        panel.update(cx, |p, cx| p.close_all(cx));

        // Built with the project open (⌘J later on): it starts there.
        let ws = workspace(cx);
        open(&ws, &b, cx);
        let panel = panel_of(&ws, cx);
        cx.run_until_parked();
        assert_eq!(cwds(&panel, cx), vec![b.clone()]);
        panel.update(cx, |p, cx| p.close_all(cx));
        cx.update(|window, _| window.remove_window());
        cx.run_until_parked();
        let _ = std::fs::remove_dir_all(&base);
    }

    /// ⌘T (the menu's and the palette's New Terminal) on a hidden panel that was never built:
    /// built outside the workspace's update (its constructor reads the workspace), shown, one
    /// new terminal, last run's offered beside it. Hidden again it adds one; emptied, ⌘J starts one.
    #[gpui::test]
    fn new_terminal_opens_a_hidden_or_unbuilt_panel(cx: &mut TestAppContext) {
        let base = std::env::temp_dir().join(format!("gitviber-term-new-{}", std::process::id()));
        let (home, config) = (base.join("home"), base.join("config"));
        for d in [&home, &config] {
            std::fs::create_dir_all(d).unwrap();
        }
        let saved = serde_json::json!({ home.to_string_lossy(): {
            "savedAt": 1.0, "active": 0, "groups": [{"focused": 0, "panes": [{"cwd": home}]}]
        }});
        std::fs::write(config.join("terminals.json"), saved.to_string()).unwrap();
        // SAFETY: nextest gives each test its own process; nothing else reads these meanwhile.
        unsafe {
            std::env::set_var("GITVIBER_CONFIG_DIR", &config);
            std::env::set_var("HOME", &home);
        }
        cx.update(|cx| {
            gitviber_ui::init(cx);
            gitviber_ui::install_core(cx);
            crate::init(cx);
        });
        let (_, cx) = cx.add_window_view(|_, _| Host);
        let ws = cx.update(|window, cx| {
            let ws = cx.new(Workspace::new);
            Workspace::register_window(&ws, window, cx);
            window.activate_window();
            ws
        });
        let shown = |cx: &mut VisualTestContext| ws.read_with(cx, |ws, _| ws.is_open(Slot::Bottom));
        let panel = |cx: &mut VisualTestContext| {
            ws.read_with(cx, |ws, _| ws.built_panel("terminal").cloned())
                .and_then(|p| p.view().downcast::<TerminalPanel>().ok())
        };
        let count = |p: &Entity<TerminalPanel>, cx: &mut VisualTestContext| {
            p.read_with(cx, |p, _| p.panes.len())
        };
        assert!(!shown(cx) && panel(cx).is_none());

        cx.dispatch_action(terminal::New);
        cx.run_until_parked();
        let p = panel(cx).expect("built by ⌘T");
        assert!(shown(cx));
        assert_eq!(count(&p, cx), 1);
        assert_eq!(ws.update(cx, |ws, _| ws.take_toasts()).len(), 1);
        p.update_in(cx, |p, window, cx| p.accept_restore(&home, window, cx));
        assert_eq!(count(&p, cx), 2);

        ws.update(cx, |ws, cx| ws.set_open(Slot::Bottom, false, cx));
        cx.dispatch_action(terminal::New);
        cx.run_until_parked();
        assert!(shown(cx));
        assert_eq!(count(&p, cx), 3);

        p.update_in(cx, |p, window, cx| {
            while p.has_terminals() {
                p.close_tab_at(0, window, cx);
            }
        });
        cx.run_until_parked();
        assert!(!shown(cx));
        ws.update(cx, |ws, cx| ws.toggle(Slot::Bottom, cx));
        cx.run_until_parked();
        assert_eq!(count(&p, cx), 1);

        p.update(cx, |p, cx| p.close_all(cx));
        cx.update(|window, _| window.remove_window());
        cx.run_until_parked();
        let _ = std::fs::remove_dir_all(&base);
    }

    /// The panel under a focusable stand-in for the commit box.
    struct Beside {
        panel: Entity<TerminalPanel>,
        elsewhere: FocusHandle,
    }

    impl Render for Beside {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let elsewhere = self.elsewhere.clone();
            v_flex()
                .key_context(gitviber_ui::keymap::ROOT_CONTEXT)
                .size_full()
                .child(
                    div()
                        .id("elsewhere")
                        .track_focus(&self.elsewhere)
                        .h(px(40.))
                        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                            window.focus(&elsewhere, cx)
                        }),
                )
                .child(div().flex_1().min_h_0().child(self.panel.clone()))
        }
    }

    fn beside(
        cx: &mut TestAppContext,
    ) -> (Entity<TerminalPanel>, FocusHandle, &mut VisualTestContext) {
        real_shells(cx);
        cx.update(|cx| {
            gpui_kit::init(cx);
            gitviber_ui::test_support::install_theme(cx);
            crate::init(cx);
        });
        let core = Arc::new(Core::new(Arc::new(|_| {})));
        let (host, cx) = cx.add_window_view(|window, cx| Beside {
            panel: cx.new(|cx| {
                TerminalPanel::new(
                    core,
                    std::env::temp_dir(),
                    TerminalConfig::default(),
                    TerminalTheme::dark(),
                    window,
                    cx,
                )
            }),
            elsewhere: cx.focus_handle(),
        });
        let (panel, elsewhere) = host.read_with(cx, |h, _| (h.panel.clone(), h.elsewhere.clone()));
        cx.update(|window, _| window.activate_window());
        (panel, elsewhere, cx)
    }

    fn draw(cx: &mut VisualTestContext) {
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
    }

    /// Row 66: a pane's Tab is the shell's, so terminal.focusTabs takes the keys to the strip.
    /// There ←/→ open tabs, ⌥← moves one, ⌘W closes it and the keys stay, Esc and ↵ go back in.
    #[gpui::test]
    fn focus_tabs_steps_out_to_the_strip_and_esc_goes_back(cx: &mut TestAppContext) {
        let (panel, _, cx) = beside(cx);
        let mac = cfg!(target_os = "macos");
        cx.update(|_, cx| {
            cx.bind_keys(gitviber_ui::keymap::key_bindings(&Default::default(), mac))
        });
        let dir = std::env::temp_dir();
        for _ in 0..3 {
            panel.update_in(cx, |p, window, cx| {
                p.open_terminal(dir.clone(), None, window, cx)
            });
        }
        draw(cx);
        let groups = |cx: &mut VisualTestContext| {
            panel.read_with(cx, |p, _| {
                let s = p.set().expect("a set");
                (s.groups.iter().map(|g| g.id).collect::<Vec<_>>(), s.active)
            })
        };
        let on_strip = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| panel.read(cx).strip_focus.is_focused(window))
        };
        let in_pane = |cx: &mut VisualTestContext| {
            let pane = panel.read_with(cx, |p, _| p.active_pane()).expect("a pane");
            cx.update(|window, cx| pane.read(cx).focus_handle(cx).is_focused(window))
        };
        // Unbound off macOS: there the palette runs it.
        let focus_tabs = |cx: &mut VisualTestContext| {
            if mac {
                cx.simulate_keystrokes("ctrl-cmd-t");
            } else {
                cx.dispatch_action(terminal::FocusTabs);
            }
            draw(cx);
        };
        let (ids, _) = groups(cx);
        assert!(in_pane(cx));
        focus_tabs(cx);
        assert!(on_strip(cx));
        cx.simulate_keystrokes("left");
        assert_eq!(groups(cx).1, Some(ids[1]));
        cx.simulate_keystrokes("alt-left");
        assert_eq!(groups(cx), (vec![ids[1], ids[0], ids[2]], Some(ids[1])));
        cx.simulate_keystrokes("escape");
        assert!(in_pane(cx));
        focus_tabs(cx);
        cx.simulate_keystrokes("secondary-w");
        draw(cx);
        assert_eq!(groups(cx).0, vec![ids[0], ids[2]]);
        assert!(on_strip(cx));
        cx.simulate_keystrokes("enter");
        assert!(in_pane(cx));
        focus_tabs(cx);
        assert!(on_strip(cx));
        focus_tabs(cx);
        assert!(in_pane(cx), "pressed again, the shortcut goes back in");
        panel.update(cx, |p, cx| p.close_all(cx));
        cx.update(|window, _| window.remove_window());
        cx.run_until_parked();
    }

    fn has_keys(handle: &FocusHandle, cx: &mut VisualTestContext) -> bool {
        cx.update(|window, _| handle.is_focused(window))
    }

    /// Only a close that takes the keys moves them: a shell exiting (or its worktree removed)
    /// while the user types elsewhere leaves them there; the focused pane's exit gives them to the
    /// open tab's pane, as does a menu's Kill that put its focus back on the pane it killed. The
    /// last one going leaves the panel empty and the keys where they were.
    #[gpui::test]
    fn a_close_moves_the_keys_only_when_it_took_them(cx: &mut TestAppContext) {
        let (panel, elsewhere, cx) = beside(cx);
        let dir = std::env::temp_dir();
        for _ in 0..4 {
            panel.update_in(cx, |p, window, cx| {
                p.open_terminal(dir.clone(), None, window, cx)
            });
        }
        let active = |cx: &mut VisualTestContext| {
            panel.read_with(cx, |p, _| p.active_pane()).expect("a pane")
        };
        let exit = |pane: &Entity<TerminalPane>, cx: &mut VisualTestContext| {
            pane.update(cx, |_, cx| cx.emit(PaneEvent::Exited { quick: false }));
            cx.run_until_parked();
            draw(cx);
        };
        let keys_of = |pane: &Entity<TerminalPane>, cx: &mut VisualTestContext| {
            let handle = pane.read_with(cx, |p, cx| p.focus_handle(cx));
            has_keys(&handle, cx)
        };
        let count = |cx: &mut VisualTestContext| panel.read_with(cx, |p, _| p.panes.len());

        cx.update(|window, cx| window.focus(&elsewhere, cx));
        draw(cx);
        let open = active(cx);
        exit(&open, cx);
        let background = panel.read_with(cx, |p, _| {
            let open = p.active_pane();
            p.panes().find(|v| Some(*v) != open.as_ref()).cloned()
        });
        exit(&background.expect("a tab in the back"), cx);
        assert_eq!(count(cx), 2);
        assert!(has_keys(&elsewhere, cx));

        let open = active(cx);
        open.update_in(cx, |p, window, cx| p.focus(window, cx));
        draw(cx);
        exit(&open, cx);
        assert_eq!(count(cx), 1);
        assert!(keys_of(&active(cx), cx));

        // Kill Terminal in a tab's menu: the menu has the keys, its dismiss gives them back to the
        // pane it killed, after the kill.
        let menu = cx.update(|_, cx| cx.focus_handle());
        panel.update_in(cx, |p, window, cx| {
            p.open_terminal(dir.clone(), None, window, cx)
        });
        draw(cx);
        let (victim, survivor) = panel.read_with(cx, |p, _| {
            let open = p.active_pane().expect("a pane");
            let other = p.panes().find(|v| **v != open).cloned().expect("another");
            (open, other)
        });
        panel.update_in(cx, |p, window, cx| {
            let handle = victim.read(cx).focus_handle(cx);
            window.focus(&menu, cx);
            p.close_pane(victim.read(cx).id(), window, cx);
            window.focus(&handle, cx);
        });
        cx.run_until_parked();
        draw(cx);
        assert!(keys_of(&survivor, cx));

        cx.update(|window, cx| window.focus(&elsewhere, cx));
        draw(cx);
        exit(&survivor, cx);
        assert_eq!(count(cx), 0);
        assert!(has_keys(&elsewhere, cx));
        cx.update(|window, _| window.remove_window());
        cx.run_until_parked();
    }

    /// A pane header's buttons are theirs alone: Split right splits once and leaves the keys in
    /// the new pane, and a double-click's second click on the button under it splits nothing
    /// more and names nothing. Clicks in a name being typed stay in it; a click elsewhere ends it
    /// and keeps the keys where it went, ↵ gives them back to the terminal.
    #[gpui::test]
    fn header_buttons_and_names_keep_their_clicks(cx: &mut TestAppContext) {
        let (panel, elsewhere, cx) = beside(cx);
        let dir = std::env::temp_dir();
        panel.update_in(cx, |p, window, cx| {
            p.open_terminal(dir.clone(), None, window, cx);
            p.split(Dir::Row, None, window, cx);
        });
        let order = |cx: &mut VisualTestContext| {
            panel.read_with(cx, |p, _| {
                layout::pane_order(&p.active_group().expect("a tab").layout)
            })
        };
        let first = order(cx)[0];
        let keys_in = |id: PaneId, cx: &mut VisualTestContext| {
            let handle = panel.read_with(cx, |p, cx| p.panes[&id].view.read(cx).focus_handle(cx));
            has_keys(&handle, cx)
        };
        let renaming = |cx: &mut VisualTestContext| panel.read_with(cx, |p, _| p.rename.is_some());
        let bounds = |selector: String, cx: &mut VisualTestContext| {
            cx.debug_bounds(Box::leak(selector.into_boxed_str()))
                .expect("on screen")
        };
        let click = |at: Point<Pixels>, count: usize, cx: &mut VisualTestContext| {
            let modifiers = gpui::Modifiers::none();
            cx.simulate_event(gpui::MouseDownEvent {
                position: at,
                modifiers,
                button: MouseButton::Left,
                click_count: count,
                first_mouse: false,
            });
            cx.simulate_event(gpui::MouseUpEvent {
                position: at,
                modifiers,
                button: MouseButton::Left,
                click_count: count,
            });
            draw(cx);
        };
        panel.update_in(cx, |p, window, cx| {
            p.focus_pane(first, cx);
            p.focus_active(window, cx);
        });
        draw(cx);

        let split = bounds(format!("split-right-{first}"), cx).center();
        click(split, 1, cx);
        let after = order(cx);
        assert_eq!(after.len(), 3);
        assert!(keys_in(after[1], cx), "the new pane has the keys");
        click(split, 2, cx);
        assert_eq!(order(cx).len(), 3);
        assert!(!renaming(cx));

        // A double-click on the header itself names its pane.
        let button = bounds(format!("split-right-{}", after[1]), cx);
        let title = gpui::point(button.left() - px(60.), button.center().y);
        click(title, 1, cx);
        click(title, 2, cx);
        assert!(renaming(cx));
        let field = bounds("terminal-rename".into(), cx).center();
        let input = |cx: &mut VisualTestContext| {
            panel.read_with(cx, |p, _| p.rename.as_ref().map(|r| r.input.clone()))
        };
        let naming = input(cx).expect("naming");
        for count in [1, 2] {
            click(field, count, cx);
            assert_eq!(input(cx).as_ref(), Some(&naming), "the same name field");
            assert!(has_keys(
                &naming.read_with(cx, |i, cx| i.focus_handle(cx)),
                cx
            ));
        }

        click(gpui::point(px(10.), px(20.)), 1, cx);
        assert!(!renaming(cx));
        assert!(has_keys(&elsewhere, cx), "the click's focus stays");

        panel.update_in(cx, |p, window, cx| {
            p.start_rename(RenameTarget::Pane(first), window, cx)
        });
        draw(cx);
        cx.simulate_keystrokes("enter");
        draw(cx);
        assert!(!renaming(cx));
        assert!(keys_in(
            panel.read_with(cx, |p, _| p.active_group().expect("a tab").focused),
            cx
        ));

        panel.update(cx, |p, cx| p.close_all(cx));
        cx.update(|window, _| window.remove_window());
        cx.run_until_parked();
    }

    /// Terminals opened and closed leave no entity behind: no task, subscription or callback
    /// holds a pane, so its terminal and scrollback go with it.
    #[gpui::test]
    fn closing_terminals_gives_back_their_entities(cx: &mut TestAppContext) {
        real_shells(cx);
        // /bin/sh in a home of its own: the owner's shell and rc files ran commands at startup,
        // which on a loaded machine were still running when the test closed the tab.
        let home = gitviber_ui::test_support::config_dir("term-close");
        // SAFETY: nextest gives each test its own process; no shell has started yet.
        unsafe {
            std::env::set_var("HOME", &home);
            std::env::set_var("SHELL", "/bin/sh");
        }
        cx.update(|cx| {
            gpui_kit::init(cx);
            gitviber_ui::test_support::install_theme(cx);
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
                if round == 0 {
                    // Tab by tab as ⌘W does; one whose shell is still busy starting asks first.
                    for _ in 0..20 {
                        if !panel.read_with(cx, |p, _| p.has_terminals()) {
                            break;
                        }
                        panel.update_in(cx, |p, window, cx| p.close_tab_at(0, window, cx));
                        if cx.has_pending_prompt() {
                            cx.simulate_prompt_answer("Kill");
                        }
                        cx.run_until_parked();
                    }
                } else {
                    panel.update(cx, |p, cx| p.close_all(cx));
                }
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
