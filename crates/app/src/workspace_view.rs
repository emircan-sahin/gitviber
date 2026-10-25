//! The main window: top bar, the git panel on the left, the center's tabs over the terminal,
//! the files panel on the right, the status bar. It draws the window's `Workspace` and handles
//! the commands that move around it.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gitviber_ui::actions::{diff, editor, file, repo, review, tab, terminal, view, workbench};
use gitviber_ui::keymap::ROOT_CONTEXT;
use gitviber_ui::memory::{self, Live};
use gitviber_ui::state::PanelSizes;
use gitviber_ui::widgets::{CountBadge, Pressable as _};
use gitviber_ui::workspace::{
    ChangesOf, PanelContext, ProjectEvent, Registry, Selection, Slot, TabGroup, Toast, Workspace,
    WorkspaceEvent,
};
use gitviber_ui::{ActiveAppTheme, Settings, UiState, theme, windows};
use gpui_kit::component::InteractiveElementExt as _;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::ButtonVariants as _;
use gpui_kit::component::resizable::HandleEdge;
use gpui_kit::component::resize_handle;
use gpui_kit::{
    AnyElement, AnyWindowHandle, App, AppContext as _, Axis, Context, DragMoveEvent, Empty, Entity,
    ExternalPaths, FocusHandle, Global, InteractiveElement as _, IntoElement, KeyDownEvent,
    ModifiersChangedEvent, MouseButton, ParentElement as _, PathPromptOptions, Render,
    ScrollHandle, StatefulInteractiveElement as _, Styled as _, Subscription, Task, WeakEntity,
    Window, div, prelude::*, px, size,
};

use crate::layout::{self, LEFT, RIGHT};
use crate::menus::{ClearRecent, OpenRecent};
use crate::open_in_button::OpenInButton;
use crate::open_repo::{self, Found, Request};
use crate::overlay::ShortcutOverlay;
use crate::palette::{Mode, Palette};
use crate::status_bar::{STATUS_BAR_HEIGHT, status_bar};
use crate::tab_strip::{StripKeys, tab_strip};
use crate::top_bar::top_bar;
use crate::wake::Wake;
use crate::welcome;

pub const DEFAULT_SIZE: (f32, f32) = (1480., 920.);
pub const MIN_SIZE: (f32, f32) = (900., 560.);
/// A sidebar, for what shows while the pointer is over it.
const SIDE_GROUP: &str = "side-panel";

/// The drags of the panels' edges, drawn as nothing: the panel itself follows the pointer. A
/// sidebar's keeps the width it had when the drag began, which it reopens at after a drag closed it.
#[derive(Clone)]
struct ResizeLeft {
    from: Option<f32>,
}
#[derive(Clone)]
struct ResizeRight {
    from: Option<f32>,
}
#[derive(Clone)]
struct ResizeBottom;

macro_rules! drag_preview {
    ($($t:ty),*) => {
        $(impl Render for $t {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                Empty
            }
        })*
    };
}
drag_preview!(ResizeLeft, ResizeRight, ResizeBottom);

pub struct WorkspaceView {
    workspace: Entity<Workspace>,
    overlay: Entity<ShortcutOverlay>,
    open_in: Entity<OpenInButton>,
    focus: FocusHandle,
    strip: StripKeys,
    /// The sidebars' header tabs, which scroll sideways when the panel is narrow: a double click
    /// on the divider widens it to show them.
    header_tabs: [ScrollHandle; 2],
    /// As the user left them; what a window too small for them draws is `drawn_sides`.
    sizes: PanelSizes,
    /// The sidebars' widths in the last frame (None: not drawn), for the drags and double
    /// clicks, which never take room from the other side.
    drawn_sides: (Option<f32>, Option<f32>),
    /// A frame was drawn: GPUI can tell which commands apply only from one.
    drawn: bool,
    /// The window's title as last set.
    title: String,
    /// ⌘F sent on to the active tab: set while it goes, so it can't come back here for good.
    find_resent: Rc<Cell<Resent>>,
    /// Repaints the memory readout now and then, while it shows.
    _memory_tick: Option<Task<()>>,
    _open_recent: Option<Task<()>>,
    _project: Option<Subscription>,
    /// Whether the open worktree is still there, while that's being looked at.
    _gone: Option<Task<()>>,
    /// The terminal's most urgent news, for its button's dot while it's hidden.
    terminal_news: Option<gitviber_ui::widgets::Look>,
    /// Reads the repo again when the window comes back to a change the watcher missed.
    wake: Entity<Wake>,
    _live: Live,
    _subscriptions: Vec<Subscription>,
}

/// The main window and its workspace, for the other windows and the app's own commands
/// (Settings reads its project, a quit asks there, `gitviber <path>` opens there).
struct MainWindow {
    window: AnyWindowHandle,
    workspace: WeakEntity<Workspace>,
}

impl Global for MainWindow {}

pub fn main_workspace(cx: &App) -> Option<Entity<Workspace>> {
    cx.try_global::<MainWindow>()?.workspace.upgrade()
}

pub fn main_window(cx: &App) -> Option<AnyWindowHandle> {
    let main = cx.try_global::<MainWindow>()?;
    main.workspace.upgrade()?;
    Some(main.window)
}

/// Opens the main window on `project`, or on none.
pub fn open_main_window(project: Option<PathBuf>, cx: &mut App) {
    let min = size(px(MIN_SIZE.0), px(MIN_SIZE.1));
    let bounds = windows::restore_bounds(UiState::get(cx).window.as_ref(), min, cx);
    let options = windows::options(
        bounds,
        size(px(DEFAULT_SIZE.0), px(DEFAULT_SIZE.1)),
        min,
        cx,
    );
    let opened = gpui_kit::open_window(options, cx, move |window, cx| {
        main_view(project, window, cx)
    });
    if let Err(err) = opened {
        eprintln!("gitviber: couldn't open the window: {err}");
        cx.quit();
    }
}

/// The main window's view: its workspace, with `project` opened when there is one.
pub(crate) fn main_view(
    project: Option<PathBuf>,
    window: &mut Window,
    cx: &mut App,
) -> Entity<WorkspaceView> {
    let workspace = cx.new(Workspace::new);
    Workspace::register_window(&workspace, window, cx);
    cx.set_global(MainWindow {
        window: window.window_handle(),
        workspace: workspace.downgrade(),
    });
    // Closing it is quitting, which asks first while something runs.
    window.on_window_should_close(cx, |_, cx| {
        crate::quit::request(cx);
        false
    });
    if let Some(root) = project {
        workspace.update(cx, |ws, cx| ws.open_project(root, window, cx));
    }
    cx.new(|cx| WorkspaceView::new(workspace, window, cx))
}

impl WorkspaceView {
    fn new(workspace: Entity<Workspace>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut subs = theme::attach(window, cx);
        subs.push(cx.observe(&workspace, |_, _, cx| cx.notify()));
        subs.push(cx.observe_global::<Settings>(|_, cx| cx.notify()));
        subs.push(cx.observe_window_bounds(window, |_, window, cx| {
            let geometry = windows::geometry(window.window_bounds());
            UiState::update_later(cx, |s| s.window = Some(geometry));
        }));
        // ⌘Tab leaves without a keyup: a held overlay goes too. Back in front, a folder moved
        // meanwhile shows in the projects list, and a change the watcher missed in the repo.
        subs.push(cx.observe_window_activation(window, |this, window, cx| {
            if window.is_window_active() {
                crate::projects::mark_stale(cx);
                let project = this.workspace.read(cx).project().cloned();
                this.wake.update(cx, |w, cx| w.back(project, cx));
            } else {
                this.wake.update(cx, |w, cx| w.left(cx));
                this.overlay.update(cx, |o, cx| o.hide(cx));
            }
        }));
        // Set by the terminal whenever an agent's look changes; drawn again only when news did.
        subs.push(
            cx.observe_global::<gitviber_refs::hooks::AgentLooks>(|this, cx| {
                let news = gitviber_terminal::calling(cx);
                if news != this.terminal_news {
                    this.terminal_news = news;
                    cx.notify();
                }
            }),
        );
        subs.push(cx.subscribe_in(
            &workspace,
            window,
            |this, _, e: &WorkspaceEvent, window, cx| {
                if *e == WorkspaceEvent::ProjectChanged {
                    this.follow_project(window, cx);
                }
            },
        ));
        let overlay = cx.new(|_| ShortcutOverlay::new(workspace.clone()));
        let open_in = cx.new(|_| OpenInButton::new(workspace.clone()));
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        let memory_tick = memory::readout_enabled().then(|| {
            cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor()
                        .timer(std::time::Duration::from_secs(2))
                        .await;
                    if this.update(cx, |_, cx| cx.notify()).is_err() {
                        break;
                    }
                }
            })
        });
        let mut this = Self {
            workspace,
            overlay,
            open_in,
            focus,
            strip: StripKeys::new(cx),
            header_tabs: [ScrollHandle::new(), ScrollHandle::new()],
            sizes: UiState::get(cx).panels.clone(),
            drawn_sides: (None, None),
            drawn: false,
            title: String::new(),
            find_resent: Rc::default(),
            _memory_tick: memory_tick,
            _open_recent: None,
            _project: None,
            _gone: None,
            terminal_news: None,
            wake: cx.new(|_| Wake::new()),
            _live: Live::new("WorkspaceView"),
            _subscriptions: subs,
        };
        // The launch's project opened before this view was there to hear of it.
        this.follow_project(window, cx);
        this
    }

    /// The open project's events: one that opened asks for a git identity when there is none;
    /// a change on disk may be its worktree going away.
    fn follow_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let project = self.workspace.read(cx).project().cloned();
        self._gone = None;
        self.terminal_news = gitviber_terminal::calling(cx);
        self._project = project.map(|p| {
            cx.subscribe_in(
                &p,
                window,
                |this, _, e: &ProjectEvent, window, cx| match e {
                    ProjectEvent::Opened => crate::dialogs::identity::check(window, cx),
                    ProjectEvent::RepoChanged(_) => this.check_gone(window, cx),
                    _ => {}
                },
            )
        });
    }

    /// The open worktree was deleted from outside (an agent done with it, `git worktree remove`):
    /// the first of its main folder, a project it was in, the other projects that's still there
    /// takes its place, as at launch. Off the UI thread: a gone network mount takes seconds.
    fn check_gone(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self._gone.is_some() {
            return;
        }
        let Some(project) = self.workspace.read(cx).project().cloned() else {
            return;
        };
        let (root, main) = {
            let p = project.read(cx);
            (
                p.root().to_path_buf(),
                p.main_worktree().map(Path::to_path_buf),
            )
        };
        let candidates = fallback_order(&UiState::get(cx).recent_projects, &root, main.as_deref());
        let found = cx.background_spawn(async move {
            if root.is_dir() {
                return None;
            }
            let next = candidates
                .into_iter()
                .find_map(|p| match open_repo::look(&p) {
                    Found::Repo(top) => Some(top),
                    _ => None,
                });
            Some((root, next))
        });
        self._gone = Some(cx.spawn_in(window, async move |this, cx| {
            let found = found.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this._gone = None;
                let Some((gone, next)) = found else { return };
                // Too late: another repo is open, or on its way.
                let ws = this.workspace.clone();
                if ws.read(cx).project().map(|p| p.read(cx).root()) != Some(gone.as_path()) {
                    return;
                }
                let name = crate::projects::folder_name(&gone.to_string_lossy()).to_string();
                let toast = match next {
                    Some(next) => {
                        let opened =
                            crate::projects::folder_name(&next.to_string_lossy()).to_string();
                        ws.update(cx, |ws, cx| ws.open_project(next, window, cx));
                        Toast::info(format!("Opened {opened} in its place."))
                            .title(format!("{name} no longer exists"))
                    }
                    None => {
                        ws.update(cx, |ws, cx| ws.close_project(cx));
                        Toast::info(format!("{name} no longer exists"))
                    }
                };
                ws.update(cx, |ws, cx| ws.toast(toast, cx));
            });
        }));
    }

    fn context(&self, cx: &App) -> PanelContext {
        PanelContext {
            workspace: self.workspace.downgrade(),
            project: self.workspace.read(cx).project().cloned(),
        }
    }

    fn save_sizes(&self, cx: &mut App) {
        let sizes = self.sizes.clone();
        UiState::update_later(cx, move |s| {
            s.panels.left.size = sizes.left.size;
            s.panels.right.size = sizes.right.size;
            s.panels.bottom.size = sizes.bottom.size;
        });
    }

    /// A sidebar's divider double-clicked: as wide as its header needs, at least its base.
    fn fit_side(&mut self, slot: Slot, drawn: f32, total: f32, cx: &mut Context<Self>) {
        let (limits, at, other) = match slot {
            Slot::Left => (LEFT, 0, self.drawn_sides.1),
            _ => (RIGHT, 1, self.drawn_sides.0),
        };
        let tabs = &self.header_tabs[at];
        let n = Registry::get(cx).panels(slot).len();
        let (Some(first), Some(last)) = (
            tabs.bounds_for_item(0),
            n.checked_sub(1).and_then(|i| tabs.bounds_for_item(i)),
        ) else {
            return;
        };
        let spare = f32::from(tabs.bounds().size.width - (last.right() - first.left()));
        let size = Some(layout::fit_header(
            limits,
            drawn,
            spare,
            total,
            other.unwrap_or(0.),
        ));
        match slot {
            Slot::Left => self.sizes.left.size = size,
            _ => self.sizes.right.size = size,
        }
        self.save_sizes(cx);
        cx.notify();
    }

    /// A sidebar's edge dragged to `want` in a body `total` wide: it follows within its limits,
    /// never into the other side's room. Pulled under half its min it closes, keeping the width
    /// the drag began at (`from`); dragged back out in the same drag, it opens again.
    fn drag_side(
        &mut self,
        slot: Slot,
        want: f32,
        total: f32,
        from: Option<f32>,
        cx: &mut Context<Self>,
    ) {
        let (limits, other, saved) = match slot {
            Slot::Left => (LEFT, self.drawn_sides.1, &mut self.sizes.left.size),
            _ => (RIGHT, self.drawn_sides.0, &mut self.sizes.right.size),
        };
        let size = layout::dragged(want, limits, total, other.unwrap_or(0.));
        *saved = size.or(from);
        self.save_sizes(cx);
        self.workspace
            .update(cx, |ws, cx| ws.set_open(slot, size.is_some(), cx));
        cx.notify();
    }

    fn open_folder(&mut self, _: &file::OpenRepo, _: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Open".into()),
        });
        cx.spawn(async move |_, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            if let Some(folder) = paths.into_iter().next() {
                cx.update(|cx| open_repo::open(Request::folder(folder), cx));
            }
        })
        .detach();
    }

    fn open_recent(&mut self, action: &OpenRecent, _: &mut Window, cx: &mut Context<Self>) {
        let folder = PathBuf::from(action.path.as_ref());
        let workspace = self.workspace.downgrade();
        // Off the UI thread: a gone network mount can take seconds to say so.
        self._open_recent = Some(cx.spawn(async move |_, cx| {
            let at = folder.clone();
            let found = cx
                .background_spawn(async move { open_repo::look(&at) })
                .await;
            cx.update(|cx| {
                if found != Found::Gone {
                    return open_repo::act(found, Request::folder(folder), cx);
                }
                let path = folder.to_string_lossy().into_owned();
                UiState::update(cx, |s| s.recent_projects.retain(|p| *p != path));
                if let Some(workspace) = workspace.upgrade() {
                    workspace.update(cx, |ws, cx| {
                        ws.toast(
                            Toast::warning(format!("{path} is gone; it's off the list.")),
                            cx,
                        )
                    });
                }
            });
        }));
    }

    fn show_left(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.workspace.update(cx, |ws, cx| {
            ws.show_panel(id, cx);
            ws.focus_slot(Slot::Left, window, cx);
        });
    }

    fn toggle_setting(cx: &mut App, f: fn(&mut Settings)) {
        Settings::update(cx, f);
    }

    fn side_header(&self, slot: Slot, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let specs = Registry::get(cx).panels(slot);
        let active = self.workspace.read(cx).active_panel(slot, cx);
        let t = cx.app_theme().clone();
        let tabs = specs.into_iter().map(|spec| {
            let is_active = active == Some(spec.id);
            let badge = self
                .workspace
                .read(cx)
                .built_panel(spec.id)
                .and_then(|p| p.badge(cx));
            let ws = self.workspace.clone();
            let id = spec.id;
            div()
                .id(spec.id)
                .pressable(cx)
                .flex()
                .items_center()
                .gap(px(6.))
                .h(px(26.))
                .px_2()
                .rounded(px(4.))
                .cursor_pointer()
                .text_size(gpui_kit::rems(0.8125))
                .text_color(if is_active {
                    t.foreground
                } else {
                    t.muted_foreground
                })
                .when(is_active, |d| d.bg(t.active))
                .when(!is_active, |d| {
                    d.hover(|s| s.bg(t.hover).text_color(t.foreground))
                })
                .child(spec.title.clone())
                .when_some(badge, |d, badge| {
                    d.child(CountBadge::new(badge).active(is_active))
                })
                .on_click(move |_, window, cx| {
                    ws.update(cx, |ws, cx| {
                        ws.show_panel(id, cx);
                        ws.focus_slot(slot, window, cx);
                    })
                })
        });
        let (close_icon, close_title, close) = match slot {
            Slot::Left => (
                gpui_kit::assets::IconName::PanelLeftClose,
                "Hide Git Panel",
                Slot::Left,
            ),
            _ => (
                gpui_kit::assets::IconName::PanelRightClose,
                "Hide Explorer",
                Slot::Right,
            ),
        };
        let ws = self.workspace.clone();
        let _ = window;
        div()
            .flex()
            .flex_none()
            .items_center()
            .justify_between()
            .h(px(40.))
            .px_2()
            .border_b_1()
            .border_color(t.border)
            .child(
                div()
                    .id(("header-tabs", slot as usize))
                    .flex()
                    .flex_1()
                    .items_center()
                    .gap_1()
                    .min_w_0()
                    .overflow_x_scroll()
                    .track_scroll(&self.header_tabs[usize::from(slot != Slot::Left)])
                    .children(tabs),
            )
            .child(
                gpui_kit::component::button::Button::new(("hide-slot", slot as usize))
                    .ghost()
                    .xsmall()
                    .icon(close_icon)
                    .tooltip(close_title)
                    .on_click(move |_, window, cx| hide_slot(&ws, close, window, cx)),
            )
            .into_any_element()
    }

    /// The explorer's Filter (⌘F in the tree) and Collapse folders, over the tree's top right
    /// while the pointer is over the panel: in the header they squeezed its tabs.
    fn explorer_buttons(&self, cx: &App) -> AnyElement {
        use gitviber_files::explorer::{ExplorerPanel, PANEL_ID};
        use gpui_kit::component::button::Button;
        let (filter_ws, collapse_ws) = (self.workspace.downgrade(), self.workspace.downgrade());
        let panel = |ws: &WeakEntity<Workspace>, cx: &App| {
            ws.upgrade()
                .and_then(|ws| ws.read(cx).built_panel(PANEL_ID).cloned())
        };
        let find = gitviber_ui::keymap::shortcut_label("editor.find", cx);
        let t = cx.app_theme();
        div()
            .absolute()
            .top_1()
            .right_2()
            .flex()
            .items_center()
            .gap_px()
            .p_px()
            .rounded(px(4.))
            .border_1()
            .border_color(t.border)
            .bg(t.chrome(t.sidebar))
            .invisible()
            .group_hover(SIDE_GROUP, |s| s.visible())
            // Over the tree's rows: a click here is the buttons' alone. Not occluded, which would
            // end the panel's hover and hide them.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                Button::new("explorer-filter")
                    .ghost()
                    .xsmall()
                    .icon(gpui_kit::assets::IconName::Search)
                    .tooltip(match find {
                        Some(key) => format!("Filter files ({key})"),
                        None => "Filter files".into(),
                    })
                    .on_click(move |_, window, cx| {
                        if let Some(f) = panel(&filter_ws, cx).and_then(|p| p.focus_handle(cx)) {
                            f.focus(window, cx);
                            f.dispatch_action(&editor::Find, window, cx);
                        }
                    }),
            )
            .child(
                Button::new("explorer-collapse")
                    .ghost()
                    .xsmall()
                    .icon(gpui_kit::assets::IconName::ChevronsDownUp)
                    .tooltip("Collapse folders")
                    .on_click(move |_, _, cx| {
                        let explorer = panel(&collapse_ws, cx)
                            .and_then(|p| p.view().downcast::<ExplorerPanel>().ok());
                        if let Some(e) = explorer {
                            e.update(cx, |e, cx| e.collapse_all(cx));
                        }
                    }),
            )
            .into_any_element()
    }

    fn side(
        &self,
        slot: Slot,
        width: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let header = self.side_header(slot, window, cx);
        let active = self.workspace.read(cx).active_panel(slot, cx);
        let explorer = active == Some(gitviber_files::explorer::PANEL_ID);
        let panel = active.and_then(|id| Workspace::ensure_panel(&self.workspace, id, window, cx));
        reachable(panel.as_ref().and_then(|p| p.focus_handle(cx)));
        let t = cx.app_theme().clone();
        let focus = self.workspace.read(cx).slot_focus(slot).clone();
        let edge = if slot == Slot::Left {
            HandleEdge::Trailing
        } else {
            HandleEdge::Leading
        };
        let view = cx.entity().downgrade();
        let fit = move |_: &gpui_kit::ClickEvent, window: &mut Window, cx: &mut App| {
            let total = f32::from(window.viewport_size().width);
            let _ = view.update(cx, |this, cx| this.fit_side(slot, width, total, cx));
        };
        let handle = match slot {
            Slot::Left => resize_handle::<ResizeLeft, ResizeLeft>("resize-left", Axis::Horizontal)
                .inside(edge)
                .on_double_click(fit)
                .on_drag(
                    ResizeLeft {
                        from: self.sizes.left.size,
                    },
                    |d, _, _, cx| cx.new(|_| d.clone()),
                )
                .into_any_element(),
            _ => resize_handle::<ResizeRight, ResizeRight>("resize-right", Axis::Horizontal)
                .inside(edge)
                .on_double_click(fit)
                .on_drag(
                    ResizeRight {
                        from: self.sizes.right.size,
                    },
                    |d, _, _, cx| cx.new(|_| d.clone()),
                )
                .into_any_element(),
        };
        div()
            .id(if slot == Slot::Left {
                "left-panel"
            } else {
                "right-panel"
            })
            .debug_selector(move || format!("{slot:?}-panel"))
            .track_focus(&focus)
            .group(SIDE_GROUP)
            .relative()
            .flex()
            .flex_col()
            .flex_none()
            .w(px(width))
            .h_full()
            .bg(t.chrome(t.sidebar))
            .when(
                slot == Slot::Left && Settings::get(cx).sidebar_projects,
                |d| {
                    let project = self.workspace.read(cx).project().cloned();
                    let (main, away) = project
                        .and_then(|p| {
                            let p_ = p.read(cx);
                            let main = p_.main_worktree()?.to_string_lossy().into_owned();
                            let root = p_.root().to_string_lossy().into_owned();
                            let worktrees = gitviber_refs::RepoState::existing(&p, cx)
                                .map(|s| s.read(cx).worktrees.clone())
                                .unwrap_or_default();
                            let away = crate::projects::away_of(&root, &main, &worktrees);
                            Some((Some(main), away))
                        })
                        .unwrap_or_default();
                    // About five rows show, the rest scroll.
                    d.child(
                        div()
                            .flex_none()
                            .p_1()
                            .border_b_1()
                            .border_color(t.border)
                            .child(crate::projects::list(
                                "sidebar-projects-list",
                                main,
                                away,
                                true,
                                Some(140.),
                                window,
                                cx,
                            )),
                    )
                },
            )
            .when(slot == Slot::Left, |d| d.border_r_1())
            .when(slot == Slot::Right, |d| d.border_l_1())
            .border_color(t.border)
            .child(header)
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .children(panel.map(|p| p.view()))
                    .when(explorer, |d| d.child(self.explorer_buttons(cx))),
            )
            .child(handle)
            .children(focus_line(&focus, window, cx))
            .into_any_element()
    }

    fn bottom(
        &self,
        height: Option<f32>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let panel = self
            .workspace
            .read(cx)
            .active_panel(Slot::Bottom, cx)
            .and_then(|id| Workspace::ensure_panel(&self.workspace, id, window, cx));
        reachable(panel.as_ref().and_then(|p| p.focus_handle(cx)));
        let t = cx.app_theme().clone();
        let focus = self.workspace.read(cx).slot_focus(Slot::Bottom).clone();
        div()
            .id("bottom-panel")
            .debug_selector(|| "Bottom-panel".into())
            .key_context("BottomPanel")
            .track_focus(&focus)
            .relative()
            .flex()
            .flex_col()
            .w_full()
            .map(|d| match height {
                Some(h) => d.flex_none().h(px(h)).border_t_1().border_color(t.border),
                None => d.flex_1().min_h_0(),
            })
            .bg(t.terminal_background())
            .child(div().flex_1().min_h_0().children(panel.map(|p| p.view())))
            .children(focus_line(&focus, window, cx))
            .when(height.is_some(), |d| {
                // A double click puts it back to its share of the column (the next frame sizes it).
                let view = cx.entity().downgrade();
                d.child(
                    resize_handle::<ResizeBottom, ResizeBottom>("resize-bottom", Axis::Vertical)
                        .inside(HandleEdge::Leading)
                        .on_double_click(move |_, _, cx| {
                            let _ = view.update(cx, |this, cx| {
                                this.sizes.bottom.size = None;
                                this.save_sizes(cx);
                                cx.notify();
                            });
                        })
                        .on_drag(ResizeBottom, |d, _, _, cx| cx.new(|_| d.clone())),
                )
            })
            .into_any_element()
    }

    fn center(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let view = self
            .workspace
            .update(cx, |ws, cx| ws.active_view(window, cx));
        let ws = self.workspace.read(cx);
        reachable(ws.tabs().active().and_then(|t| t.item.focus_handle(cx)));
        let t = cx.app_theme().clone();
        let focus = ws.center_focus().clone();
        let has_tabs = !ws.tabs().is_empty();
        let content = match view {
            Some(view) => view.into_any_element(),
            None if has_tabs => div().into_any_element(),
            None => welcome::empty_center(cx),
        };
        let line = focus_line(&focus, window, cx);
        div()
            .id("center")
            .debug_selector(|| "center".into())
            .key_context("CodeArea")
            .track_focus(&focus)
            .relative()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .bg(t.code(t.background))
            .when(has_tabs, |d| {
                d.child(tab_strip(&self.workspace, &mut self.strip, window, cx))
            })
            .child(div().flex_1().min_h_0().child(content))
            .children(line)
            .into_any_element()
    }
}

/// ⌘B, ⌥⌘B, ⌘J: a hidden panel shows with focus in it, a shown one hides.
fn toggle_slot(ws: &Entity<Workspace>, slot: Slot, window: &mut Window, cx: &mut App) {
    if ws.read(cx).is_open(slot) {
        hide_slot(ws, slot, window, cx);
    } else {
        ws.update(cx, |ws, cx| ws.focus_slot(slot, window, cx));
    }
}

/// Hides a panel; focus in it goes to the code view rather than out of every part.
fn hide_slot(ws: &Entity<Workspace>, slot: Slot, window: &mut Window, cx: &mut App) {
    ws.update(cx, |ws, cx| {
        let had = ws.slot_focus(slot).contains_focused(window, cx);
        ws.set_open(slot, false, cx);
        if had {
            ws.focus_center(window, cx);
        }
    });
}

/// Marks the part the keys go to: a thin accent along its top while focus is inside.
fn focus_line(focus: &FocusHandle, window: &Window, cx: &App) -> Option<AnyElement> {
    focus.contains_focused(window, cx).then(|| {
        div()
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .h(px(1.))
            .bg(cx.app_theme().primary)
            .into_any_element()
    })
}

impl Render for WorkspaceView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        for toast in self.workspace.update(cx, |ws, _| ws.take_toasts()) {
            toast.show(window, cx);
        }
        if window.focused(cx).is_none() {
            self.focus.focus(window, cx);
        }
        let ctx = self.context(cx);
        let title = ctx.project.as_ref().map_or_else(
            || window_title(None, None, None),
            |p| {
                let status = gitviber_refs::RepoState::existing(p, cx)
                    .and_then(|s| s.read(cx).status.clone());
                let status = status.as_deref();
                window_title(
                    Some(p.read(cx).root()),
                    status.and_then(|s| s.branch.as_deref()),
                    status.and_then(|s| s.head.as_deref()),
                )
            },
        );
        if title != self.title {
            window.set_window_title(&title);
            self.title = title;
        }
        let t = cx.app_theme().clone();
        let news = self
            .terminal_news
            .filter(|_| !self.workspace.read(cx).is_open(Slot::Bottom));
        let top = top_bar(&ctx, self.drawn, news, window, cx);
        self.drawn = true;
        let status = status_bar(&ctx, &self.open_in, window, cx);
        let viewport = window.viewport_size();
        let (ws_has_project, left_open, right_open, bottom_open, maximized) = {
            let ws = self.workspace.read(cx);
            (
                ws.project().is_some(),
                ws.is_open(Slot::Left),
                ws.is_open(Slot::Right),
                ws.is_open(Slot::Bottom),
                ws.terminal_maximized(),
            )
        };
        let column =
            f32::from(viewport.height) - f32::from(windows::TOP_BAR_HEIGHT) - STATUS_BAR_HEIGHT;
        // Its first height, or the one a double click on its edge asked for: from the column
        // now, kept in points from then on.
        if ws_has_project && bottom_open && self.sizes.bottom.size.is_none() {
            self.sizes.bottom.size = Some(layout::bottom_base(column));
            self.save_sizes(cx);
        }
        self.drawn_sides = (None, None);
        let body: AnyElement = if !ws_has_project {
            match crate::dialogs::needs_git::missing(cx) {
                Some(info) => crate::dialogs::needs_git::screen(info, cx),
                // Not before the launch said whether a project opens: it would flash first.
                None if crate::opened::launching(cx) => div()
                    .size_full()
                    .bg(t.code(t.background))
                    .into_any_element(),
                None => welcome::no_project(window, cx),
            }
        } else if maximized {
            self.bottom(None, window, cx)
        } else {
            let (left_w, right_w) = layout::fit_sides(
                f32::from(viewport.width),
                left_open.then_some(self.sizes.left.size),
                right_open.then_some(self.sizes.right.size),
            );
            self.drawn_sides = (left_w, right_w);
            let bottom_h = bottom_open
                .then(|| layout::fit_bottom(column, self.sizes.bottom.size.unwrap_or(0.)));
            let left = left_w.map(|w| self.side(Slot::Left, w, window, cx));
            let right = right_w.map(|w| self.side(Slot::Right, w, window, cx));
            let center = self.center(window, cx);
            let bottom = bottom_h.map(|h| self.bottom(Some(h), window, cx));
            div()
                .id("body")
                .flex()
                .flex_1()
                .min_h_0()
                .on_drag_move(cx.listener(|this, e: &DragMoveEvent<ResizeLeft>, _, cx| {
                    let want = f32::from(e.event.position.x - e.bounds.left());
                    let from = e.drag(cx).from;
                    this.drag_side(Slot::Left, want, f32::from(e.bounds.size.width), from, cx);
                }))
                .on_drag_move(cx.listener(|this, e: &DragMoveEvent<ResizeRight>, _, cx| {
                    let want = f32::from(e.bounds.right() - e.event.position.x);
                    let from = e.drag(cx).from;
                    this.drag_side(Slot::Right, want, f32::from(e.bounds.size.width), from, cx);
                }))
                .children(left)
                .child(
                    div()
                        .id("column")
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w_0()
                        .on_drag_move(cx.listener(
                            |this, e: &DragMoveEvent<ResizeBottom>, _, cx| {
                                let total = f32::from(e.bounds.size.height);
                                let want = f32::from(e.bounds.bottom() - e.event.position.y);
                                this.sizes.bottom.size =
                                    Some(layout::fit_bottom(total, want.round()));
                                this.save_sizes(cx);
                                cx.notify();
                            },
                        ))
                        .child(center)
                        .children(bottom),
                )
                .children(right)
                .into_any_element()
        };
        div()
            .id("workspace")
            .key_context(ROOT_CONTEXT)
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .flex_col()
            .text_color(t.foreground)
            .font_family(t.fonts.ui.clone())
            .font_weight(t.fonts.ui_weight)
            .text_size(t.ui_text())
            .on_modifiers_changed(cx.listener(|this, e: &ModifiersChangedEvent, window, cx| {
                this.overlay
                    .update(cx, |o, cx| o.modifiers_changed(e, window, cx))
            }))
            .capture_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| {
                if this.overlay.update(cx, |o, cx| o.key_down(e, cx)) {
                    cx.stop_propagation();
                }
            }))
            .capture_any_mouse_down(
                cx.listener(|this, _, _, cx| this.overlay.update(cx, |o, _| o.pointer())),
            )
            // Away from the terminal panes, which paste a drop's paths: a folder opens as a
            // project, a file in its repository, as `gitviber <path>` opens them.
            .on_drop(|paths: &ExternalPaths, _, cx| {
                let paths = paths.paths().iter();
                let paths = paths.map(|p| p.to_string_lossy().into_owned()).collect();
                if gitviber_ui::has_core(cx) {
                    gitviber_core::api::app::open_dropped(&gitviber_ui::core(cx), paths);
                }
            })
            .on_mouse_move(
                cx.listener(|this, _, _, cx| this.overlay.update(cx, |o, _| o.pointer())),
            )
            .on_action(cx.listener(|this, _: &workbench::ShortcutOverlay, _, cx| {
                this.overlay.update(cx, |o, cx| o.toggle(cx))
            }))
            .on_action(
                cx.listener(|this, _: &workbench::ShowCommands, window, cx| {
                    Palette::open(Mode::Commands, &this.workspace, window, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &workbench::QuickOpen, window, cx| {
                Palette::open(Mode::Files, &this.workspace, window, cx)
            }))
            .on_action({
                // Not a listener: sent on, it may come back here while this view is busy.
                let (ws, resent) = (self.workspace.downgrade(), self.find_resent.clone());
                move |_: &editor::Find, window, cx| find_in_code_view(&ws, &resent, window, cx)
            })
            .on_action(cx.listener(|this, _: &file::OpenIn, _, cx| {
                this.open_in.update(cx, |b, cx| b.run(cx))
            }))
            .on_action(cx.listener(|this, _: &terminal::NewInProject, window, cx| {
                Palette::open(Mode::Projects, &this.workspace, window, cx)
            }))
            .when(ws_has_project, |d| {
                d.on_action(cx.listener(|this, _: &workbench::OpenChange, window, cx| {
                    Palette::open(Mode::Changes, &this.workspace, window, cx)
                }))
            })
            .on_action(cx.listener(Self::open_folder))
            .on_action(cx.listener(Self::open_recent))
            .on_action(cx.listener(|_, _: &ClearRecent, _, cx| {
                UiState::update(cx, |s| s.recent_projects.clear())
            }))
            .on_action(cx.listener(|this, _: &repo::Refresh, _, cx| {
                if let Some(project) = this.workspace.read(cx).project().cloned() {
                    project.update(cx, |p, cx| p.repo_changed(cx));
                }
            }))
            .on_action(cx.listener(|_, _: &view::ZoomIn, _, cx| Settings::step_ui_scale(cx, 1)))
            .on_action(cx.listener(|_, _: &view::ZoomOut, _, cx| Settings::step_ui_scale(cx, -1)))
            .on_action(cx.listener(|_, _: &view::ZoomReset, _, cx| Settings::step_ui_scale(cx, 0)))
            .on_action(
                cx.listener(|_, _: &editor::FontZoomIn, _, cx| Settings::step_code_font(cx, 1)),
            )
            .on_action(
                cx.listener(|_, _: &editor::FontZoomOut, _, cx| Settings::step_code_font(cx, -1)),
            )
            .on_action(
                cx.listener(|_, _: &editor::FontZoomReset, _, cx| Settings::step_code_font(cx, 0)),
            )
            .on_action(cx.listener(|_, _: &diff::ToggleSplit, _, cx| {
                Self::toggle_setting(cx, |s| s.side_by_side = !s.side_by_side)
            }))
            .on_action(cx.listener(|_, _: &diff::ToggleCollapse, _, cx| {
                Self::toggle_setting(cx, |s| s.hide_unchanged = !s.hide_unchanged)
            }))
            .on_action(cx.listener(|_, _: &diff::ToggleWhitespace, _, cx| {
                Self::toggle_setting(cx, |s| s.ignore_whitespace = !s.ignore_whitespace)
            }))
            .on_action(cx.listener(|_, _: &editor::ToggleWrap, _, cx| {
                Self::toggle_setting(cx, |s| s.word_wrap = !s.word_wrap)
            }))
            .on_action(cx.listener(|_, _: &editor::ToggleBlame, _, cx| {
                Self::toggle_setting(cx, |s| s.blame = !s.blame)
            }))
            .when(ws_has_project, |d| {
                self.panel_actions(self.tab_actions(d, cx), cx)
            })
            .child(top)
            .child(body)
            .child(status)
            .child(self.overlay.clone())
    }
}

/// Where a window whose worktree is gone goes, best first: the project's main folder, a project
/// the worktree was inside, the others in the list's order.
fn fallback_order(projects: &[String], gone: &Path, main: Option<&Path>) -> Vec<PathBuf> {
    let others = projects.iter().map(PathBuf::from).filter(|p| p != gone);
    let inside = others.clone().filter(|p| gone.starts_with(p));
    let mut out: Vec<PathBuf> = main
        .filter(|m| *m != gone)
        .map(Path::to_path_buf)
        .into_iter()
        .collect();
    for p in inside.chain(others) {
        if !out.contains(&p) {
            out.push(p);
        }
    }
    out
}

/// A part's own focus (a panel's list, the tab's view) is a tab stop, as the old app's lists were:
/// Tab reaches it among the buttons. Set where it's drawn, as a part may change its handle.
fn reachable(focus: Option<FocusHandle>) {
    if let Some(f) = focus.filter(|f| !f.tab_stop) {
        let _ = f.tab_stop(true);
    }
}

/// The main window's title: the title bar hides it, but Mission Control, the Window menu and
/// VoiceOver tell windows apart by it. A detached HEAD shows its commit.
fn window_title(root: Option<&Path>, branch: Option<&str>, head: Option<&str>) -> String {
    let Some(root) = root else {
        return "GitViber".into();
    };
    let root = root.to_string_lossy();
    let name = crate::projects::folder_name(&root);
    match branch.or_else(|| head.map(|h| h.get(..7).unwrap_or(h))) {
        Some(at) => format!("{name} — {at}"),
        None => name.to_string(),
    }
}

#[derive(Clone, Copy, Default, PartialEq)]
enum Resent {
    #[default]
    No,
    Sent,
    /// It came back: the tab has no find either.
    Back,
}

/// ⌘F that nothing under focus took searches the code view, as with focus in no panel: sent on
/// from the active tab, focus going back where it was when that tab has no find either.
fn find_in_code_view(
    ws: &WeakEntity<Workspace>,
    resent: &Cell<Resent>,
    window: &mut Window,
    cx: &mut App,
) {
    if resent.get() == Resent::Sent {
        resent.set(Resent::Back);
        return;
    }
    let Some(tab) = ws.upgrade().and_then(|ws| {
        let ws = ws.read(cx);
        // Maximized, the terminal hides the code view.
        if ws.terminal_maximized() {
            return None;
        }
        ws.tabs().active().and_then(|t| t.item.focus_handle(cx))
    }) else {
        return;
    };
    if tab.contains_focused(window, cx) {
        return;
    }
    let before = window.focused(cx);
    tab.focus(window, cx);
    resent.set(Resent::Sent);
    tab.dispatch_action(&editor::Find, window, cx);
    if resent.replace(Resent::No) == Resent::Back
        && let Some(before) = before
    {
        before.focus(window, cx);
    }
}

impl WorkspaceView {
    /// Showing, hiding and focusing the panels, while a project is open and they show.
    fn panel_actions(
        &self,
        d: gpui_kit::Stateful<gpui_kit::Div>,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Stateful<gpui_kit::Div> {
        d.on_action(cx.listener(|this, _: &view::Changes, w, cx| this.show_left("changes", w, cx)))
            .on_action(
                cx.listener(|this, _: &view::History, w, cx| this.show_left("history", w, cx)),
            )
            .on_action(cx.listener(|this, _: &view::Pulls, w, cx| this.show_left("pulls", w, cx)))
            .on_action(cx.listener(|this, _: &view::Issues, w, cx| this.show_left("issues", w, cx)))
            .on_action(cx.listener(|this, _: &view::ToggleGitPanel, w, cx| {
                toggle_slot(&this.workspace, Slot::Left, w, cx)
            }))
            .on_action(cx.listener(|this, _: &view::ToggleExplorer, w, cx| {
                toggle_slot(&this.workspace, Slot::Right, w, cx)
            }))
            .on_action(cx.listener(|this, _: &terminal::Toggle, w, cx| {
                toggle_slot(&this.workspace, Slot::Bottom, w, cx)
            }))
            .on_action(cx.listener(|this, _: &view::FocusGitPanel, w, cx| {
                this.workspace
                    .update(cx, |ws, cx| ws.focus_slot(Slot::Left, w, cx))
            }))
            .on_action(cx.listener(|this, _: &view::ShowExplorer, w, cx| {
                this.workspace
                    .update(cx, |ws, cx| ws.focus_slot(Slot::Right, w, cx))
            }))
            .on_action(cx.listener(|this, _: &view::FocusCode, w, cx| {
                this.workspace.update(cx, |ws, cx| ws.focus_center(w, cx))
            }))
            .on_action(cx.listener(|this, _: &view::FocusNextPanel, w, cx| {
                this.workspace
                    .update(cx, |ws, cx| ws.focus_next_part(1, w, cx))
            }))
            .on_action(cx.listener(|this, _: &view::FocusPrevPanel, w, cx| {
                this.workspace
                    .update(cx, |ws, cx| ws.focus_next_part(-1, w, cx))
            }))
    }

    /// ⌘1–⌘8 by the tabs the strip shows; None: the last of them.
    fn go_to_shown(&mut self, nth: Option<usize>, window: &mut Window, cx: &mut Context<Self>) {
        let shown = crate::tab_strip::visible_tabs(self.workspace.read(cx), cx);
        let target = match nth {
            Some(n) => shown.get(n).copied(),
            None => shown.last().copied(),
        };
        if let Some(i) = target {
            self.workspace
                .update(cx, |ws, cx| ws.go_to_tab(i, window, cx));
        }
    }

    fn step_shown(&mut self, dir: isize, window: &mut Window, cx: &mut Context<Self>) {
        let ws = self.workspace.read(cx);
        let shown = crate::tab_strip::visible_tabs(ws, cx);
        if let Some(i) = crate::tab_strip::stepped(&shown, ws.tabs().active_index(), dir) {
            self.workspace
                .update(cx, |ws, cx| ws.go_to_tab(i, window, cx));
        }
    }

    fn open_all(&mut self, all: Selection, window: &mut Window, cx: &mut Context<Self>) {
        self.workspace.update(cx, |ws, cx| {
            ws.open(all, false, window, cx);
            ws.focus_center(window, cx);
        });
    }

    /// The tab commands, while there are tabs to act on.
    fn tab_actions(
        &self,
        d: gpui_kit::Stateful<gpui_kit::Div>,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Stateful<gpui_kit::Div> {
        let (has_tabs, can_reopen, shown) = {
            let ws = self.workspace.read(cx);
            let shown = ws.tabs().active().and_then(|t| t.item.selection().cloned());
            (!ws.tabs().is_empty(), ws.can_reopen_tab(), shown)
        };
        // From a commit's file or a comparison's: every file of it in one scroll.
        let (of_commit, of_range) = match shown {
            Some(Selection::Commit { commit, url, .. }) => (
                Some(Selection::Changes {
                    of: ChangesOf::Commit { commit, url },
                }),
                None,
            ),
            Some(Selection::PrFile { range, .. }) => (
                None,
                Some(Selection::Changes {
                    of: ChangesOf::Range { range },
                }),
            ),
            _ => (None, None),
        };
        let goto = |d: gpui_kit::Stateful<gpui_kit::Div>, cx: &mut Context<Self>| {
            macro_rules! go {
                ($d:expr, $($a:ident => $i:expr),*) => {
                    $d$(.on_action(cx.listener(|this, _: &tab::$a, w, cx| this.go_to_shown(Some($i), w, cx))))*
                };
            }
            go!(d, Goto1 => 0, Goto2 => 1, Goto3 => 2, Goto4 => 3, Goto5 => 4, Goto6 => 5, Goto7 => 6, Goto8 => 7)
        };
        d.when(can_reopen, |d| {
            d.on_action(cx.listener(|this, _: &tab::ReopenClosed, w, cx| {
                this.workspace
                    .update(cx, |ws, cx| ws.reopen_closed_tab(w, cx))
            }))
        })
        .when_some(of_commit, |d, all| {
            d.on_action(cx.listener(move |this, _: &review::OpenAllCommit, w, cx| {
                this.open_all(all.clone(), w, cx)
            }))
        })
        .when_some(of_range, |d, all| {
            d.on_action(
                cx.listener(move |this, _: &review::OpenAllComparison, w, cx| {
                    this.open_all(all.clone(), w, cx)
                }),
            )
        })
        .when(has_tabs, |d| {
            goto(d, cx)
                .on_action(cx.listener(|this, _: &tab::Close, _, cx| {
                    this.workspace.update(cx, |ws, cx| ws.close_active_tab(cx))
                }))
                .on_action(cx.listener(|this, _: &tab::CloseOthers, _, cx| {
                    this.workspace
                        .update(cx, |ws, cx| ws.close_group(TabGroup::Others, cx))
                }))
                .on_action(cx.listener(|this, _: &tab::CloseLeft, _, cx| {
                    this.workspace
                        .update(cx, |ws, cx| ws.close_group(TabGroup::Left, cx))
                }))
                .on_action(cx.listener(|this, _: &tab::CloseRight, _, cx| {
                    this.workspace
                        .update(cx, |ws, cx| ws.close_group(TabGroup::Right, cx))
                }))
                .on_action(cx.listener(|this, _: &tab::CloseAll, _, cx| {
                    this.workspace
                        .update(cx, |ws, cx| ws.close_group(TabGroup::All, cx))
                }))
                .on_action(cx.listener(|this, _: &tab::Last, w, cx| this.go_to_shown(None, w, cx)))
                .on_action(cx.listener(|this, _: &tab::Next, w, cx| this.step_shown(1, w, cx)))
                .on_action(cx.listener(|this, _: &tab::Prev, w, cx| this.step_shown(-1, w, cx)))
                .on_action(cx.listener(|this, _: &tab::MoveLeft, _, cx| {
                    this.workspace
                        .update(cx, |ws, cx| ws.move_active_tab(-1, cx))
                }))
                .on_action(cx.listener(|this, _: &tab::MoveRight, _, cx| {
                    this.workspace
                        .update(cx, |ws, cx| ws.move_active_tab(1, cx))
                }))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use gitviber_ui::workspace::TabItem;
    use gpui_kit::{SharedString, TestAppContext};

    #[test]
    fn a_gone_worktree_falls_back_to_its_main_folder_then_a_project_around_it() {
        let projects: Vec<String> = ["/r/other", "/r/app", "/r/agents"].map(String::from).into();
        let gone = Path::new("/r/agents/fix-1");
        assert_eq!(
            fallback_order(&projects, gone, Some(Path::new("/r/app"))),
            ["/r/app", "/r/agents", "/r/other"].map(PathBuf::from)
        );
        assert_eq!(
            fallback_order(&projects, gone, None),
            ["/r/agents", "/r/other", "/r/app"].map(PathBuf::from)
        );
        // The main folder itself gone: the others.
        let main = Path::new("/r/app");
        assert_eq!(
            fallback_order(&projects, main, Some(main)),
            ["/r/other", "/r/agents"].map(PathBuf::from)
        );
    }

    #[test]
    fn the_title_names_the_folder_and_the_branch_or_its_commit() {
        assert_eq!(window_title(None, None, None), "GitViber");
        let root = Path::new("/Users/ada/code/demo-app");
        let head = Some("0123456789abcdef");
        assert_eq!(
            window_title(Some(root), Some("main"), head),
            "demo-app — main"
        );
        assert_eq!(window_title(Some(root), None, head), "demo-app — 0123456");
        assert_eq!(window_title(Some(root), None, None), "demo-app");
    }

    /// A tab whose view has a find of its own, or none.
    struct Tab {
        focus: FocusHandle,
        finds: bool,
        found: Rc<Cell<usize>>,
    }
    impl Render for Tab {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div().track_focus(&self.focus).when(self.finds, |d| {
                d.on_action(
                    cx.listener(|this, _: &editor::Find, _, _| {
                        this.found.set(this.found.get() + 1)
                    }),
                )
            })
        }
    }
    impl TabItem for Tab {
        fn tab_title(&self, _: &App) -> SharedString {
            "tab".into()
        }
        fn tab_focus(&self, _: &App) -> Option<FocusHandle> {
            Some(self.focus.clone())
        }
    }

    /// The root as the app's: the fallback, focused itself, the active tab under it.
    struct Host {
        ws: Entity<Workspace>,
        focus: FocusHandle,
        resent: Rc<Cell<Resent>>,
    }
    impl Render for Host {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let view = self.ws.update(cx, |ws, cx| ws.active_view(window, cx));
            let (ws, resent) = (self.ws.downgrade(), self.resent.clone());
            div()
                .track_focus(&self.focus)
                .on_action(move |_: &editor::Find, window, cx| {
                    find_in_code_view(&ws, &resent, window, cx)
                })
                .children(view)
        }
    }

    /// ⌘F with focus outside the tab: how many times the tab found, and whether focus is in it.
    fn find_with_focus_outside(finds: bool, cx: &mut TestAppContext) -> (usize, bool) {
        let dir = std::env::temp_dir().join(format!("gitviber-app-find-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // SAFETY: the tests that read the config dir all point it at a temporary folder.
        unsafe { std::env::set_var("GITVIBER_CONFIG_DIR", &dir) };
        cx.update(gitviber_ui::init);
        let found = Rc::new(Cell::new(0));
        let mut host = None;
        let window: AnyWindowHandle = cx
            .add_window(|window, cx| {
                let ws = cx.new(Workspace::new);
                Workspace::register_window(&ws, window, cx);
                let h = cx.new(|cx| Host {
                    ws,
                    focus: cx.focus_handle(),
                    resent: Rc::default(),
                });
                host = Some(h.clone());
                gpui_kit::base::Root::new(h, window, cx)
            })
            .into();
        let host = host.unwrap();
        let f = found.clone();
        let _ = window.update(cx, |_, window, cx| {
            let ws = host.read(cx).ws.clone();
            ws.update(cx, |ws, cx| {
                ws.open_tab(
                    "tab",
                    false,
                    move |_, cx| {
                        cx.new(|cx| Tab {
                            focus: cx.focus_handle(),
                            finds,
                            found: f,
                        })
                    },
                    window,
                    cx,
                )
            });
            let focus = host.read(cx).focus.clone();
            focus.focus(window, cx);
        });
        cx.run_until_parked();
        let _ = cx.update_window(window, |_, window, cx| window.draw(cx).clear(cx));
        let _ = window.update(cx, |_, window, cx| {
            window.dispatch_action(Box::new(editor::Find), cx)
        });
        cx.run_until_parked();
        let in_tab = window
            .update(cx, |_, window, cx| {
                let ws = host.read(cx).ws.read(cx);
                let tab = ws.tabs().active().and_then(|t| t.item.focus_handle(cx));
                tab.is_some_and(|t| t.is_focused(window))
            })
            .unwrap();
        (found.get(), in_tab)
    }

    /// The strip and the active tab's view, as the center draws them.
    struct StripHost {
        ws: Entity<Workspace>,
        focus: FocusHandle,
        strip: StripKeys,
    }
    impl Render for StripHost {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let view = self.ws.update(cx, |ws, cx| ws.active_view(window, cx));
            reachable(
                self.ws
                    .read(cx)
                    .tabs()
                    .active()
                    .and_then(|t| t.item.focus_handle(cx)),
            );
            div()
                .track_focus(&self.focus)
                .child(tab_strip(&self.ws, &mut self.strip, window, cx))
                .children(view)
        }
    }

    #[gpui_kit::test]
    fn tab_reaches_the_strip_whose_keys_step_keep_and_close(cx: &mut TestAppContext) {
        let dir = std::env::temp_dir().join(format!("gitviber-app-strip-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // SAFETY: the tests that read the config dir all point it at a temporary folder.
        unsafe { std::env::set_var("GITVIBER_CONFIG_DIR", &dir) };
        cx.update(gitviber_ui::init);
        let mut host = None;
        let window: AnyWindowHandle = cx
            .add_window(|window, cx| {
                let ws = cx.new(Workspace::new);
                Workspace::register_window(&ws, window, cx);
                let h = cx.new(|cx| StripHost {
                    ws,
                    focus: cx.focus_handle(),
                    strip: StripKeys::new(cx),
                });
                host = Some(h.clone());
                gpui_kit::base::Root::new(h, window, cx)
            })
            .into();
        let host = host.unwrap();
        let ws = host.read_with(cx, |h, _| h.ws.clone());
        let _ = window.update(cx, |_, window, cx| {
            for key in ["a", "b"] {
                ws.update(cx, |ws, cx| {
                    ws.open_tab(
                        key,
                        false,
                        |_, cx| {
                            cx.new(|cx| Tab {
                                focus: cx.focus_handle(),
                                finds: false,
                                found: Rc::default(),
                            })
                        },
                        window,
                        cx,
                    )
                });
            }
            host.read(cx).focus.clone().focus(window, cx);
        });
        let cx = &mut gpui_kit::VisualTestContext::from_window(window, cx);
        cx.run_until_parked();
        let strip_focused = |cx: &mut gpui_kit::VisualTestContext| {
            cx.update(|window, cx| host.read(cx).strip.focus.is_focused(window))
        };
        let active = |cx: &mut gpui_kit::VisualTestContext| {
            cx.update(|_, cx| ws.read(cx).tabs().active_key().map(|k| k.to_string()))
        };
        let in_tab = |cx: &mut gpui_kit::VisualTestContext| {
            cx.update(|window, cx| {
                let tab = ws
                    .read(cx)
                    .tabs()
                    .active()
                    .and_then(|t| t.item.focus_handle(cx));
                tab.is_some_and(|t| t.is_focused(window))
            })
        };
        cx.simulate_keystrokes("tab");
        assert!(strip_focused(cx));
        cx.simulate_keystrokes("left");
        assert_eq!(active(cx).as_deref(), Some("a"));
        assert!(strip_focused(cx), "the arrows keep focus in the strip");
        cx.simulate_keystrokes("tab");
        assert!(in_tab(cx), "the next stop is the tab's view");
        cx.simulate_keystrokes("shift-tab");
        assert!(strip_focused(cx));
        cx.simulate_keystrokes("end backspace");
        assert_eq!(active(cx).as_deref(), Some("a"));
        assert_eq!(cx.update(|_, cx| ws.read(cx).tabs().len()), 1);
        assert!(strip_focused(cx));
    }

    #[gpui_kit::test]
    fn find_nothing_took_goes_to_the_active_tab(cx: &mut TestAppContext) {
        assert_eq!(find_with_focus_outside(true, cx), (1, true));
    }

    #[gpui_kit::test]
    fn find_a_tab_has_none_of_comes_back_once_and_focus_stays(cx: &mut TestAppContext) {
        assert_eq!(find_with_focus_outside(false, cx), (0, false));
    }

    /// The main window as the app lays it out: panels shown, hidden and sized only by the user.
    mod layout {
        use gitviber_ui::state::{self, UiState};
        use gpui_kit::{
            Bounds, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, Pixels, TestAppContext,
            VisualTestContext, point,
        };

        use super::super::*;
        use super::Tab;
        use gitviber_ui::workspace::Part;

        const START: (f32, f32) = DEFAULT_SIZE;
        const L: usize = 0;
        const R: usize = 1;
        const B: usize = 2;

        /// A project folder (without core it's taken as open where it's asked) and a config dir
        /// of the test's own.
        fn setup(tag: &str, cx: &mut TestAppContext) -> PathBuf {
            let dir = std::env::temp_dir()
                .join(format!("gitviber-app-layout-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            let project = dir.join("project");
            std::fs::create_dir_all(&project).unwrap();
            // SAFETY: the tests that read the config dir all point it at a temporary folder.
            unsafe { std::env::set_var("GITVIBER_CONFIG_DIR", dir.join("config")) };
            cx.update(|cx| {
                gitviber_ui::init(cx);
                crate::placeholders::register(cx);
            });
            project
        }

        /// The main window as a launch opens it, `size` big.
        fn launch(
            project: Option<PathBuf>,
            size: (f32, f32),
            cx: &mut TestAppContext,
        ) -> VisualTestContext {
            let window: AnyWindowHandle = cx
                .add_window(|window, cx| {
                    let view = main_view(project, window, cx);
                    gpui_kit::base::Root::new(view, window, cx)
                })
                .into();
            let mut vcx = VisualTestContext::from_window(window, cx);
            resize(&mut vcx, size);
            vcx
        }

        /// A quit (what's saved is written out), then the next launch, which reads it back.
        fn relaunch(
            project: Option<PathBuf>,
            size: (f32, f32),
            cx: &mut VisualTestContext,
        ) -> VisualTestContext {
            cx.update(|window, cx| {
                state::flush(cx);
                window.remove_window();
            });
            cx.run_until_parked();
            let text = std::fs::read_to_string(state::state_path()).unwrap();
            let saved: UiState = serde_json::from_str(&text).unwrap();
            let app: &mut TestAppContext = cx;
            app.update(|cx| cx.set_global(saved));
            launch(project, size, app)
        }

        fn draw(cx: &mut VisualTestContext) {
            cx.run_until_parked();
            cx.update(|window, cx| window.draw(cx).clear(cx));
            cx.run_until_parked();
        }

        fn resize(cx: &mut VisualTestContext, (w, h): (f32, f32)) {
            cx.simulate_resize(size(px(w), px(h)));
            draw(cx);
        }

        fn keys(cx: &mut VisualTestContext, keys: &str) {
            cx.simulate_keystrokes(keys);
            draw(cx);
        }

        fn bounds(cx: &mut VisualTestContext, slot: usize) -> Option<Bounds<Pixels>> {
            cx.debug_bounds(["Left-panel", "Right-panel", "Bottom-panel"][slot])
        }

        /// The sidebars' widths and the terminal's height as drawn; None: not drawn.
        fn drawn(cx: &mut VisualTestContext) -> [Option<f32>; 3] {
            let w = |b: Option<Bounds<Pixels>>| b.map(|b| f32::from(b.size.width));
            [
                w(bounds(cx, L)),
                w(bounds(cx, R)),
                bounds(cx, B).map(|b| f32::from(b.size.height)),
            ]
        }

        fn workspace(cx: &mut VisualTestContext) -> Entity<Workspace> {
            cx.update(|window, cx| Workspace::for_window(window, cx).unwrap())
        }

        fn open(cx: &mut VisualTestContext) -> [bool; 3] {
            let ws = workspace(cx);
            ws.read_with(cx, |ws, _| {
                [Slot::Left, Slot::Right, Slot::Bottom].map(|s| ws.is_open(s))
            })
        }

        /// The terminal's column in a window `h` tall.
        fn column(h: f32) -> f32 {
            h - f32::from(windows::TOP_BAR_HEIGHT) - STATUS_BAR_HEIGHT
        }

        /// What a window can go through: bigger, full screen on a big display, the smallest
        /// window, a display of another scale, back.
        const RESIZES: [(f32, f32); 6] = [
            (1800., 1100.),
            (2560., 1440.),
            (3008., 1692.),
            MIN_SIZE,
            (1200., 700.),
            START,
        ];

        /// A drag of a panel's edge to each of `to`, released at the last.
        fn drag(cx: &mut VisualTestContext, slot: usize, to: &[(f32, f32)]) {
            let from = edge(cx, slot);
            let at = |(x, y): (f32, f32)| point(px(x), px(y));
            cx.simulate_mouse_down(at(from), MouseButton::Left, Modifiers::default());
            draw(cx);
            for p in to {
                cx.simulate_mouse_move(at(*p), MouseButton::Left, Modifiers::default());
                draw(cx);
            }
            let end = *to.last().unwrap_or(&from);
            cx.simulate_mouse_up(at(end), MouseButton::Left, Modifiers::default());
            draw(cx);
        }

        fn double_click(cx: &mut VisualTestContext, slot: usize) {
            let (x, y) = edge(cx, slot);
            for click_count in [1, 2] {
                cx.simulate_event(MouseDownEvent {
                    position: point(px(x), px(y)),
                    modifiers: Modifiers::default(),
                    button: MouseButton::Left,
                    click_count,
                    first_mouse: false,
                });
                cx.simulate_event(MouseUpEvent {
                    position: point(px(x), px(y)),
                    modifiers: Modifiers::default(),
                    button: MouseButton::Left,
                    click_count,
                });
                draw(cx);
            }
        }

        /// A point on a panel's edge: the sidebars' inner edges, the terminal's top.
        fn edge(cx: &mut VisualTestContext, slot: usize) -> (f32, f32) {
            let b = bounds(cx, slot).expect("drawn");
            let mid_y = f32::from(b.center().y);
            match slot {
                L => (f32::from(b.right()) - 2., mid_y),
                R => (f32::from(b.left()) + 2., mid_y),
                _ => (f32::from(b.center().x), f32::from(b.top()) + 2.),
            }
        }

        #[gpui_kit::test]
        fn the_git_panel_hidden_with_cmd_b_stays_hidden_as_the_window_grows(
            cx: &mut TestAppContext,
        ) {
            let project = setup("report", cx);
            let cx = &mut launch(Some(project), (1200., 800.), cx);
            assert_eq!(drawn(cx), [Some(320.), Some(260.), None]);
            keys(cx, "cmd-b");
            for size in [(1600., 1000.), (2400., 1350.), (3008., 1692.)] {
                resize(cx, size);
                assert_eq!(drawn(cx)[L], None, "shown again at {size:?}");
                assert!(!open(cx)[L]);
            }
            keys(cx, "cmd-b");
            assert_eq!(
                drawn(cx),
                [Some(320.), Some(260.), None],
                "back at its width"
            );
        }

        #[gpui_kit::test]
        fn a_hidden_panel_stays_hidden_through_any_resize_and_comes_back_at_its_size(
            cx: &mut TestAppContext,
        ) {
            let project = setup("resizes", cx);
            let cx = &mut launch(Some(project), START, cx);
            keys(cx, "cmd-j");
            let shown = drawn(cx);
            let bottom = layout::bottom_base(column(START.1));
            assert_eq!(shown, [Some(320.), Some(260.), Some(bottom)]);
            let toggles: [(&str, Box<dyn gpui_kit::Action>); 3] = [
                ("cmd-b", Box::new(view::ToggleGitPanel)),
                ("alt-cmd-b", Box::new(view::ToggleExplorer)),
                ("cmd-j", Box::new(terminal::Toggle)),
            ];
            for (slot, (key, menu)) in toggles.into_iter().enumerate() {
                // Hidden by its key, shown by its menu item.
                keys(cx, key);
                for size in RESIZES {
                    resize(cx, size);
                    assert_eq!(drawn(cx)[slot], None, "slot {slot} at {size:?}");
                    if size == (1200., 700.) {
                        cx.simulate_scale_factor_change(1.);
                        draw(cx);
                        assert_eq!(drawn(cx)[slot], None, "slot {slot} on a 1x display");
                        cx.simulate_scale_factor_change(2.);
                    }
                }
                cx.update(|window, cx| window.dispatch_action(menu, cx));
                draw(cx);
                assert_eq!(drawn(cx), shown, "slot {slot} came back as it was");
            }
        }

        #[gpui_kit::test]
        fn panels_keep_their_points_and_a_small_window_squeezes_the_code_view_first(
            cx: &mut TestAppContext,
        ) {
            let project = setup("points", cx);
            let cx = &mut launch(Some(project), START, cx);
            keys(cx, "cmd-j");
            // Sized by the user: 400, 300 and 500 tall.
            drag(cx, L, &[(390., 400.), (400., 400.)]);
            drag(cx, R, &[(1190., 400.), (1180., 400.)]);
            let top = 920. - STATUS_BAR_HEIGHT - 500.;
            drag(cx, B, &[(700., top + 10.), (700., top)]);
            let set = [Some(400.), Some(300.), Some(500.)];
            assert_eq!(drawn(cx), set);
            // Bigger: as set, the code view takes the room.
            for size in [(1800., 1100.), (3008., 1692.)] {
                resize(cx, size);
                assert_eq!(drawn(cx), set, "at {size:?}");
            }
            // Smaller: the code view gives way first...
            resize(cx, (1200., 920.));
            assert_eq!(drawn(cx), set);
            // ...then the right side, then the left, each down to its min; the terminal leaves
            // the code view its min.
            resize(cx, MIN_SIZE);
            let squeezed = [Some(340.), Some(200.), Some(column(MIN_SIZE.1) - 120.)];
            assert_eq!(drawn(cx), squeezed);
            let center = cx.debug_bounds("center").unwrap();
            assert_eq!(f32::from(center.size.width), 360.);
            // The room back, the sizes back.
            resize(cx, START);
            assert_eq!(drawn(cx), set);
            assert_eq!(open(cx), [true; 3]);
        }

        #[gpui_kit::test]
        fn drags_clamp_close_under_half_the_min_and_open_again_dragged_back(
            cx: &mut TestAppContext,
        ) {
            let project = setup("drags", cx);
            let cx = &mut launch(Some(project), START, cx);
            let y = 400.;
            drag(cx, L, &[(330., y), (500., y)]);
            assert_eq!(drawn(cx)[L], Some(500.));
            // A double click fits it to its header's tabs, from its base up.
            double_click(cx, L);
            let fit = drawn(cx)[L].unwrap();
            assert!((320. ..500.).contains(&fit), "{fit}");
            drag(cx, L, &[(330., y), (500., y)]);
            // Never past its share of the window, nor into the right side's room.
            drag(cx, L, &[(900., y), (1400., y)]);
            assert_eq!(drawn(cx)[L], Some(666.));
            assert_eq!(drawn(cx)[R], Some(260.));
            drag(cx, R, &[(1000., y), (100., y)]);
            assert_eq!(drawn(cx)[R], Some(START.0 - 360. - 666.));
            assert_eq!(drawn(cx)[L], Some(666.));
            // Under half its min it closes; back out in the same drag, it opens there.
            drag(cx, L, &[(400., y), (100., y), (300., y)]);
            assert_eq!(drawn(cx)[L], Some(300.));
            let (x, _) = edge(cx, L);
            cx.simulate_mouse_down(point(px(x), px(y)), MouseButton::Left, Modifiers::default());
            for to in [250., 100., 60.] {
                cx.simulate_mouse_move(
                    point(px(to), px(y)),
                    MouseButton::Left,
                    Modifiers::default(),
                );
                draw(cx);
            }
            assert_eq!(drawn(cx)[L], None, "closed mid-drag");
            cx.simulate_mouse_up(
                point(px(60.), px(y)),
                MouseButton::Left,
                Modifiers::default(),
            );
            draw(cx);
            assert!(!open(cx)[L]);
            // It opens again at the width the drag began with.
            keys(cx, "cmd-b");
            assert_eq!(drawn(cx)[L], Some(300.));
            // The terminal stops where the code view keeps its min, and a double click on its
            // edge puts it back to its share of the column.
            keys(cx, "cmd-j");
            drag(cx, B, &[(700., 300.), (700., 0.)]);
            assert_eq!(drawn(cx)[B], Some(column(START.1) - 120.));
            double_click(cx, B);
            assert_eq!(drawn(cx)[B], Some(layout::bottom_base(column(START.1))));
        }

        #[gpui_kit::test]
        fn a_relaunch_brings_back_the_panels_as_they_were_left(cx: &mut TestAppContext) {
            let project = setup("relaunch", cx);
            let cx = &mut launch(Some(project.clone()), START, cx);
            // The terminal shown and never sized: it comes back, at the height it had.
            keys(cx, "cmd-j");
            let first = drawn(cx);
            let cx = &mut relaunch(Some(project.clone()), START, cx);
            assert_eq!(drawn(cx), first);
            // Sized, one side hidden, the terminal hidden.
            drag(cx, L, &[(390., 400.), (410., 400.)]);
            drag(cx, R, &[(1190., 400.), (1170., 400.)]);
            keys(cx, "cmd-b cmd-j");
            assert_eq!(drawn(cx), [None, Some(310.), None]);
            let cx = &mut relaunch(Some(project.clone()), (1600., 1000.), cx);
            assert_eq!(drawn(cx), [None, Some(310.), None]);
            assert_eq!(open(cx), [false, true, false]);
            keys(cx, "cmd-b cmd-j");
            assert_eq!(drawn(cx), [Some(410.), Some(310.), first[B]]);
            // Relaunched in a window too small for them: squeezed, and saved as they were.
            let cx = &mut relaunch(Some(project), MIN_SIZE, cx);
            assert_eq!(drawn(cx), [Some(340.), Some(200.), first[B]]);
            resize(cx, START);
            assert_eq!(drawn(cx), [Some(410.), Some(310.), first[B]]);
        }

        #[gpui_kit::test]
        fn projects_tabs_and_the_welcome_screen_leave_the_panels_alone(cx: &mut TestAppContext) {
            let project = setup("projects", cx);
            let other = project.with_file_name("other");
            std::fs::create_dir_all(&other).unwrap();
            let cx = &mut launch(None, START, cx);
            assert_eq!(drawn(cx), [None, None, None], "the welcome screen");
            let ws = workspace(cx);
            cx.update(|window, cx| ws.update(cx, |ws, cx| ws.open_project(project, window, cx)));
            draw(cx);
            assert_eq!(drawn(cx), [Some(320.), Some(260.), None]);
            keys(cx, "alt-cmd-b");
            cx.update(|window, cx| {
                ws.update(cx, |ws, cx| {
                    ws.open_tab(
                        "tab",
                        false,
                        |_, cx| {
                            cx.new(|cx| Tab {
                                focus: cx.focus_handle(),
                                finds: false,
                                found: Rc::default(),
                            })
                        },
                        window,
                        cx,
                    );
                    ws.open_project(other, window, cx);
                })
            });
            draw(cx);
            assert_eq!(drawn(cx), [Some(320.), None, None]);
            cx.update(|_, cx| ws.update(cx, |ws, cx| ws.close_project(cx)));
            resize(cx, (2400., 1350.));
            assert_eq!(drawn(cx), [None, None, None]);
            assert_eq!(open(cx), [true, false, false]);
        }

        #[gpui_kit::test]
        fn a_maximized_terminal_stays_maximized_through_resizes(cx: &mut TestAppContext) {
            let project = setup("maximized", cx);
            let cx = &mut launch(Some(project), START, cx);
            keys(cx, "cmd-j");
            let height = drawn(cx)[B];
            let ws = workspace(cx);
            cx.update(|_, cx| ws.update(cx, |ws, cx| ws.set_terminal_maximized(true, cx)));
            for size in RESIZES {
                resize(cx, size);
                assert_eq!(drawn(cx), [None, None, Some(column(size.1))], "at {size:?}");
            }
            cx.update(|_, cx| ws.update(cx, |ws, cx| ws.set_terminal_maximized(false, cx)));
            draw(cx);
            assert_eq!(drawn(cx), [Some(320.), Some(260.), height]);
        }

        #[gpui_kit::test]
        fn a_toggle_shows_a_panel_with_focus_and_hides_it_leaving_focus_to_the_code_view(
            cx: &mut TestAppContext,
        ) {
            let project = setup("focus", cx);
            let cx = &mut launch(Some(project), START, cx);
            let ws = workspace(cx);
            let focused = |cx: &mut VisualTestContext| {
                cx.update(|window, cx| ws.read(cx).focused_part(window, cx))
            };
            // The terminal's view built, as a panel shown before is.
            cx.update(|_, cx| ws.update(cx, |ws, cx| ws.set_open(Slot::Bottom, true, cx)));
            draw(cx);
            keys(cx, "cmd-j");
            assert_eq!(drawn(cx)[B], None);
            keys(cx, "cmd-b");
            keys(cx, "cmd-b");
            assert_eq!(focused(cx), Some(Part::Slot(Slot::Left)));
            keys(cx, "cmd-b");
            assert_eq!(focused(cx), Some(Part::Center));
            keys(cx, "cmd-j");
            assert_eq!(focused(cx), Some(Part::Slot(Slot::Bottom)));
            keys(cx, "cmd-j");
            assert_eq!(focused(cx), Some(Part::Center));
        }
    }

    /// Bugs the old app fixed, checked on the real window.
    mod old_bugs {
        use gpui_kit::{FileDropEvent, Modifiers, TestAppContext, VisualTestContext, point};

        use super::super::*;

        /// The main window on `project`, drawn, with a config dir of the test's own.
        fn launch(
            tag: &str,
            project: Option<PathBuf>,
            cx: &mut TestAppContext,
        ) -> VisualTestContext {
            let dir =
                std::env::temp_dir().join(format!("gitviber-app-old-{tag}-{}", std::process::id()));
            // SAFETY: the tests that read the config dir all point it at a temporary folder.
            unsafe { std::env::set_var("GITVIBER_CONFIG_DIR", dir.join("config")) };
            cx.update(|cx| {
                gitviber_ui::init(cx);
                crate::placeholders::register(cx);
            });
            let window: AnyWindowHandle = cx
                .add_window(|window, cx| {
                    let view = main_view(project, window, cx);
                    gpui_kit::base::Root::new(view, window, cx)
                })
                .into();
            let vcx = VisualTestContext::from_window(window, cx);
            vcx.simulate_resize(size(px(DEFAULT_SIZE.0), px(DEFAULT_SIZE.1)));
            vcx.run_until_parked();
            vcx
        }

        /// #31: ⌘ let go in Mission Control (no keyup reaches the window, which stays key): the
        /// first move that says ⌘ is up closes the held overlay.
        #[gpui_kit::test]
        fn the_held_overlay_closes_once_an_event_says_cmd_is_up(cx: &mut TestAppContext) {
            let cx = &mut launch("overlay", None, cx);
            let shown = |cx: &mut VisualTestContext| cx.debug_bounds("shortcut-overlay").is_some();
            cx.simulate_modifiers_change(Modifiers::command());
            cx.executor()
                .advance_clock(std::time::Duration::from_secs(2));
            cx.run_until_parked();
            assert!(shown(cx), "held ⌘ shows it");
            let at = point(px(400.), px(300.));
            cx.simulate_mouse_move(at, None, Modifiers::command());
            cx.run_until_parked();
            assert!(shown(cx), "still held");
            cx.simulate_mouse_move(at, None, Modifiers::none());
            cx.run_until_parked();
            assert!(!shown(cx), "the move says ⌘ is up");
        }

        /// A folder dropped on the window away from the terminal goes where `gitviber <path>`
        /// goes, which opens it (opened.rs, open_repo.rs).
        #[gpui_kit::test]
        fn a_folder_dropped_on_the_window_is_opened_as_gitviber_path_is(cx: &mut TestAppContext) {
            let dir =
                std::env::temp_dir().join(format!("gitviber-app-drop-{}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            let dir = dir.canonicalize().unwrap();
            // No repo opens here: core's watcher thread would wake the test's executor.
            let core = cx.update(gitviber_ui::install_core);
            let cx = &mut launch("drop", None, cx);
            let at = point(px(700.), px(400.));
            cx.simulate_event(FileDropEvent::Entered {
                position: at,
                paths: ExternalPaths(vec![dir.clone()].into()),
            });
            cx.simulate_event(FileDropEvent::Submit { position: at });
            cx.run_until_parked();
            let taken = gitviber_core::api::app::take_opened(&core).unwrap();
            let folders: Vec<&str> = taken.open.iter().map(|t| t.folder.as_str()).collect();
            assert_eq!(folders, [dir.to_string_lossy()]);
        }

        /// The status bar's syntax theme opens Settings → Editor, where it's set.
        #[gpui_kit::test]
        fn the_syntax_item_opens_its_settings(cx: &mut TestAppContext) {
            let project =
                std::env::temp_dir().join(format!("gitviber-app-status-{}", std::process::id()));
            std::fs::create_dir_all(&project).unwrap();
            let cx = &mut launch("status", Some(project), cx);
            let item = cx
                .debug_bounds("status-syntax")
                .expect("drawn with a project");
            cx.simulate_click(item.center(), Modifiers::none());
            cx.run_until_parked();
            assert_eq!(cx.update(|_, cx| cx.windows().len()), 2);
        }
    }
}
