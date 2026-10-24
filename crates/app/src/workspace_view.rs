//! The main window: top bar, the git panel on the left, the center's tabs over the terminal,
//! the files panel on the right, the status bar. It draws the window's `Workspace` and handles
//! the commands that move around it.

use std::path::PathBuf;

use gitviber_ui::actions::{diff, editor, file, repo, tab, terminal, view, workbench};
use gitviber_ui::keymap::ROOT_CONTEXT;
use gitviber_ui::memory::{self, Live};
use gitviber_ui::state::PanelSizes;
use gitviber_ui::widgets::CountBadge;
use gitviber_ui::workspace::{PanelContext, Registry, Slot, TabGroup, Toast, Workspace};
use gitviber_ui::{ActiveAppTheme, Settings, UiState, theme, windows};
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::ButtonVariants as _;
use gpui_kit::component::resizable::HandleEdge;
use gpui_kit::component::resize_handle;
use gpui_kit::{
    AnyElement, App, AppContext as _, Axis, Context, DragMoveEvent, Empty, Entity, FocusHandle,
    InteractiveElement as _, IntoElement, ParentElement as _, PathPromptOptions, Render,
    StatefulInteractiveElement as _, Styled as _, Subscription, Task, Window, div, prelude::*, px,
    size,
};

use crate::layout::{self, LEFT, RIGHT};
use crate::menus::{ClearRecent, OpenRecent};
use crate::palette::{Mode, Palette};
use crate::status_bar::{STATUS_BAR_HEIGHT, status_bar};
use crate::tab_strip::tab_strip;
use crate::top_bar::top_bar;
use crate::welcome;

pub const DEFAULT_SIZE: (f32, f32) = (1480., 920.);
pub const MIN_SIZE: (f32, f32) = (900., 560.);

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
    focus: FocusHandle,
    sizes: PanelSizes,
    /// A frame was drawn: GPUI can tell which commands apply only from one.
    drawn: bool,
    /// Repaints the memory readout now and then, while it shows.
    _memory_tick: Option<Task<()>>,
    _open_recent: Option<Task<()>>,
    _live: Live,
    _subscriptions: Vec<Subscription>,
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
        Self {
            workspace,
            focus,
            sizes: UiState::get(cx).panels.clone(),
            drawn: false,
            _memory_tick: memory_tick,
            _open_recent: None,
            _live: Live::new("WorkspaceView"),
            _subscriptions: subs,
        }
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
                    .flex()
                    .items_center()
                    .gap_1()
                    .min_w_0()
                    .overflow_hidden()
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

    fn side(
        &self,
        slot: Slot,
        width: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let header = self.side_header(slot, window, cx);
        let panel = self
            .workspace
            .read(cx)
            .active_panel(slot, cx)
            .and_then(|id| Workspace::ensure_panel(&self.workspace, id, window, cx));
        let t = cx.app_theme().clone();
        let focus = self.workspace.read(cx).slot_focus(slot).clone();
        let edge = if slot == Slot::Left {
            HandleEdge::Trailing
        } else {
            HandleEdge::Leading
        };
        let handle = match slot {
            Slot::Left => resize_handle::<ResizeLeft, ResizeLeft>("resize-left", Axis::Horizontal)
                .inside(edge)
                .on_drag(ResizeLeft, |d, _, _, cx| cx.new(|_| d.clone()))
                .into_any_element(),
            _ => resize_handle::<ResizeRight, ResizeRight>("resize-right", Axis::Horizontal)
                .inside(edge)
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
            .relative()
            .flex()
            .flex_col()
            .flex_none()
            .w(px(width))
            .h_full()
            .bg(t.chrome(t.sidebar))
            .when(slot == Slot::Left, |d| d.border_r_1())
            .when(slot == Slot::Right, |d| d.border_l_1())
            .border_color(t.border)
            .child(header)
            .child(div().flex_1().min_h_0().children(panel.map(|p| p.view())))
            .child(handle)
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
            .when(height.is_some(), |d| {
                d.child(
                    resize_handle::<ResizeBottom, ResizeBottom>("resize-bottom", Axis::Vertical)
                        .inside(HandleEdge::Leading)
                        .on_drag(ResizeBottom, |d, _, _, cx| cx.new(|_| d.clone())),
                )
            })
            .into_any_element()
    }

    fn center(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let view = self
            .workspace
            .update(cx, |ws, cx| ws.active_view(window, cx));
        let ws = self.workspace.read(cx);
        let t = cx.app_theme().clone();
        let focus = ws.center_focus().clone();
        let has_tabs = !ws.tabs().is_empty();
        let content = match view {
            Some(view) => view.into_any_element(),
            None if has_tabs => div().into_any_element(),
            None => welcome::empty_center(cx),
        };
        div()
            .id("center")
            .key_context("CodeArea")
            .track_focus(&focus)
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .bg(t.code(t.background))
            .when(has_tabs, |d| {
                d.child(tab_strip(&self.workspace, window, cx))
            })
            .child(div().flex_1().min_h_0().child(content))
            .into_any_element()
    }
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
        let t = cx.app_theme().clone();
        let top = top_bar(&ctx, self.drawn, window, cx);
        self.drawn = true;
        let status = status_bar(&ctx, window, cx);
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
            welcome::no_project(cx)
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
            .on_action(
                cx.listener(|this, _: &workbench::ShowCommands, window, cx| {
                    Palette::open(Mode::Commands, &this.workspace, window, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &workbench::QuickOpen, window, cx| {
                Palette::open(Mode::Files, &this.workspace, window, cx)
            }))
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

    /// The tab commands, while there are tabs to act on.
    fn tab_actions(
        &self,
        d: gpui_kit::Stateful<gpui_kit::Div>,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Stateful<gpui_kit::Div> {
        let (has_tabs, can_reopen) = {
            let ws = self.workspace.read(cx);
            (!ws.tabs().is_empty(), ws.can_reopen_tab())
        };
        let goto = |d: gpui_kit::Stateful<gpui_kit::Div>, cx: &mut Context<Self>| {
            macro_rules! go {
                ($d:expr, $($a:ident => $i:expr),*) => {
                    $d$(.on_action(cx.listener(|this, _: &tab::$a, w, cx| this.workspace.update(cx, |ws, cx| ws.go_to_tab($i, w, cx)))))*
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
                .on_action(cx.listener(|this, _: &tab::Last, w, cx| {
                    this.workspace.update(cx, |ws, cx| ws.go_to_last_tab(w, cx))
                }))
                .on_action(cx.listener(|this, _: &tab::Next, w, cx| {
                    this.workspace.update(cx, |ws, cx| ws.step_tab(1, w, cx))
                }))
                .on_action(cx.listener(|this, _: &tab::Prev, w, cx| {
                    this.workspace.update(cx, |ws, cx| ws.step_tab(-1, w, cx))
                }))
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
