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
    FocusHandle, Global, InteractiveElement as _, IntoElement, KeyDownEvent, ModifiersChangedEvent,
    MouseButton, ParentElement as _, PathPromptOptions, Render, ScrollHandle,
    StatefulInteractiveElement as _, Styled as _, Subscription, Task, WeakEntity, Window, div,
    prelude::*, px, size,
};

use crate::layout::{self, LEFT, RIGHT};
use crate::menus::{ClearRecent, OpenRecent};
use crate::open_in_button::OpenInButton;
use crate::overlay::ShortcutOverlay;
use crate::palette::{Mode, Palette};
use crate::status_bar::{STATUS_BAR_HEIGHT, status_bar};
use crate::tab_strip::{StripKeys, tab_strip};
use crate::top_bar::top_bar;
use crate::welcome;

pub const DEFAULT_SIZE: (f32, f32) = (1480., 920.);
pub const MIN_SIZE: (f32, f32) = (900., 560.);
/// A sidebar, for what shows while the pointer is over it.
const SIDE_GROUP: &str = "side-panel";

/// The drags of the panels' edges, drawn as nothing: the panel itself follows the pointer.
#[derive(Clone)]
struct ResizeLeft;
#[derive(Clone)]
struct ResizeRight;
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
    sizes: PanelSizes,
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
        // meanwhile shows in the projects list.
        subs.push(cx.observe_window_activation(window, |this, window, cx| {
            if window.is_window_active() {
                crate::projects::mark_stale(cx);
            } else {
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
            drawn: false,
            title: String::new(),
            find_resent: Rc::default(),
            _memory_tick: memory_tick,
            _open_recent: None,
            _project: None,
            _gone: None,
            terminal_news: None,
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
            let next = candidates.into_iter().find(|p| p.is_dir());
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
        let (limits, at) = match slot {
            Slot::Left => (LEFT, 0),
            _ => (RIGHT, 1),
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
        let size = Some(layout::fit_header(limits, drawn, spare, total));
        match slot {
            Slot::Left => self.sizes.left.size = size,
            _ => self.sizes.right.size = size,
        }
        self.save_sizes(cx);
        cx.notify();
    }

    fn open_folder(&mut self, _: &file::OpenRepo, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Open".into()),
        });
        let workspace = self.workspace.clone();
        let handle = window.window_handle();
        cx.spawn(async move |_, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            let Some(root) = paths.into_iter().next() else {
                return;
            };
            let _ = handle.update(cx, |_, window, cx| {
                workspace.update(cx, |ws, cx| ws.open_project(root, window, cx));
            });
        })
        .detach();
    }

    fn open_recent(&mut self, action: &OpenRecent, window: &mut Window, cx: &mut Context<Self>) {
        let root = PathBuf::from(action.path.as_ref());
        let workspace = self.workspace.downgrade();
        let handle = window.window_handle();
        // Off the UI thread: a gone network mount can take seconds to say so.
        self._open_recent = Some(cx.spawn(async move |_, cx| {
            let check = root.clone();
            let there = cx.background_spawn(async move { check.is_dir() }).await;
            let _ = handle.update(cx, |_, window, cx| {
                let Some(workspace) = workspace.upgrade() else {
                    return;
                };
                if there {
                    workspace.update(cx, |ws, cx| ws.open_project(root, window, cx));
                } else {
                    let path = root.to_string_lossy().into_owned();
                    UiState::update(cx, |s| s.recent_projects.retain(|p| *p != path));
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
                    .on_click(move |_, _, cx| {
                        ws.update(cx, |ws, cx| ws.set_open(close, false, cx))
                    }),
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
                .on_drag(ResizeLeft, |d, _, _, cx| cx.new(|_| d.clone()))
                .into_any_element(),
            _ => resize_handle::<ResizeRight, ResizeRight>("resize-right", Axis::Horizontal)
                .inside(edge)
                .on_double_click(fit)
                .on_drag(ResizeRight, |d, _, _, cx| cx.new(|_| d.clone()))
                .into_any_element(),
        };
        div()
            .id(if slot == Slot::Left {
                "left-panel"
            } else {
                "right-panel"
            })
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
                // A double click puts it back to its share of the column.
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
        let body: AnyElement = if !ws_has_project {
            match crate::dialogs::needs_git::missing(cx) {
                Some(info) => crate::dialogs::needs_git::screen(info, cx),
                None => welcome::no_project(window, cx),
            }
        } else if maximized {
            self.bottom(None, window, cx)
        } else {
            let total = f32::from(viewport.width);
            let (left_w, right_w) = layout::fit_sides(
                total,
                left_open.then_some(self.sizes.left.size),
                right_open.then_some(self.sizes.right.size),
            );
            let column =
                f32::from(viewport.height) - f32::from(windows::TOP_BAR_HEIGHT) - STATUS_BAR_HEIGHT;
            let bottom_h = bottom_open.then(|| layout::fit_bottom(column, self.sizes.bottom.size));
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
                    let total = f32::from(e.bounds.size.width);
                    let want = f32::from(e.event.position.x - e.bounds.left());
                    match layout::dragged(want, LEFT, total) {
                        Some(w) => this.sizes.left.size = Some(w),
                        None => this
                            .workspace
                            .update(cx, |ws, cx| ws.set_open(Slot::Left, false, cx)),
                    }
                    this.save_sizes(cx);
                    cx.notify();
                }))
                .on_drag_move(cx.listener(|this, e: &DragMoveEvent<ResizeRight>, _, cx| {
                    let total = f32::from(e.bounds.size.width);
                    let want = f32::from(e.bounds.right() - e.event.position.x);
                    match layout::dragged(want, RIGHT, total) {
                        Some(w) => this.sizes.right.size = Some(w),
                        None => this
                            .workspace
                            .update(cx, |ws, cx| ws.set_open(Slot::Right, false, cx)),
                    }
                    this.save_sizes(cx);
                    cx.notify();
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
                                    Some(layout::fit_bottom(total, Some(want)));
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
            .on_action(cx.listener(|this, _: &view::ToggleGitPanel, _, cx| {
                this.workspace
                    .update(cx, |ws, cx| ws.toggle(Slot::Left, cx))
            }))
            .on_action(cx.listener(|this, _: &view::ToggleExplorer, _, cx| {
                this.workspace
                    .update(cx, |ws, cx| ws.toggle(Slot::Right, cx))
            }))
            .on_action(cx.listener(|this, _: &terminal::Toggle, window, cx| {
                this.workspace.update(cx, |ws, cx| {
                    if ws.is_open(Slot::Bottom) {
                        ws.set_open(Slot::Bottom, false, cx);
                    } else {
                        ws.focus_slot(Slot::Bottom, window, cx);
                    }
                })
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
}
