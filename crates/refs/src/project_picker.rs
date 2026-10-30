//! The top bar's project chip and its picker: the saved projects as the user arranged them
//! (gitviber_ui::projects): Pinned, folders, the rest by last opened or by hand. A filter on
//! top, ↑/↓ Home/End PgUp/PgDn ↵ and ⇧F10 on the rows, drags to reorder or file, and a right-click menu for the
//! pin, the color and the folder.

use gitviber_ui::keymap::{file, shortcut_label};
use gitviber_ui::memory::Live;
use gitviber_ui::projects::{
    DropOn, Line, Place, ProjectOrg, ProjectSort, SWATCHES, Section, home_relative, project_name,
};
use gitviber_ui::widgets::{Ends, list_step, on_list_keys, project_tile, swatch_tile};
use gitviber_ui::workspace::PanelContext;
use gitviber_ui::{ActiveAppTheme, Project, UiState, Workspace};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::popover::Popover;
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::{
    Anchor, AnyElement, AnyWindowHandle, App, AppContext as _, Context, Entity, Focusable as _,
    FontWeight, Global, InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement as _,
    Render, ScrollHandle, SharedString, StatefulInteractiveElement as _, Styled as _, Subscription,
    WeakEntity, Window, div, prelude::FluentBuilder as _, px, rems,
};

use crate::ui::{is_menu_key, key_hints, open_row_menu};

/// Each window's project chip as last drawn, for Switch Project to open; a closed window's or an
/// old project's goes with it (weak).
#[derive(Default)]
struct ProjectChips(Vec<(AnyWindowHandle, WeakEntity<ProjectChip>)>);
impl Global for ProjectChips {}

pub fn project_chip(ctx: &PanelContext, window: &mut Window, cx: &mut App) -> Option<AnyElement> {
    let project = ctx.project.clone()?;
    let id = project.entity_id().as_u64() as usize;
    let chip = window.use_keyed_state(("refs-project-chip", id), cx, |_, cx| {
        ProjectChip::new(&project, cx)
    });
    let handle = window.window_handle();
    let chips = cx.default_global::<ProjectChips>();
    chips
        .0
        .retain(|(w, c)| *w != handle && c.upgrade().is_some());
    chips.0.push((handle, chip.downgrade()));
    Some(chip.into_any_element())
}

/// File → Switch Project: the window's project chip opens its list.
pub fn switch_project(cx: &mut App) {
    let Some(handle) = cx.active_window() else {
        return;
    };
    if let Some(chip) = ProjectChip::of_window(handle, cx) {
        chip.update(cx, |c, cx| {
            c.asked = true;
            cx.notify();
        });
    }
}

pub struct ProjectChip {
    project: Entity<Project>,
    picker: Option<Entity<ProjectPicker>>,
    /// Switch Project asked for the list: the next frame opens it, having a window to build it in.
    asked: bool,
    _subs: Vec<Subscription>,
    _live: Live,
}

impl ProjectChip {
    fn new(project: &Entity<Project>, cx: &mut Context<Self>) -> Self {
        let subs = vec![
            cx.observe(project, |_, _, cx| cx.notify()),
            // The tile's color.
            cx.observe_global::<UiState>(|_, cx| cx.notify()),
        ];
        ProjectChip {
            project: project.clone(),
            picker: None,
            asked: false,
            _subs: subs,
            _live: Live::new("ProjectChip"),
        }
    }

    /// The chip `window` drew last.
    pub fn of_window(window: AnyWindowHandle, cx: &App) -> Option<Entity<Self>> {
        cx.try_global::<ProjectChips>()?
            .0
            .iter()
            .find(|(w, _)| *w == window)?
            .1
            .upgrade()
    }

    pub fn is_open(&self) -> bool {
        self.picker.is_some()
    }

    /// The row the keys are on, counting folders and projects but not headers, while the
    /// filter has the keys.
    pub fn key_row(&self, window: &Window, cx: &App) -> Option<usize> {
        let p = self.picker.as_ref()?.read(cx);
        if !p.query.focus_handle(cx).is_focused(window) {
            return None;
        }
        let lines = p.lines(cx);
        let at = p.hot_line(&lines)?;
        Some(lines[..at].iter().filter(|l| l.is_row()).count())
    }

    /// The project is its main worktree, even inside a linked one.
    fn main(&self, cx: &App) -> String {
        let p = self.project.read(cx);
        p.main_worktree()
            .map(|m| m.to_string_lossy().into_owned())
            .unwrap_or_else(|| p.root().to_string_lossy().into_owned())
    }

    fn set_open(&mut self, open: bool, window: &mut Window, cx: &mut Context<Self>) {
        if open == self.picker.is_some() {
            return;
        }
        self.picker = open.then(|| {
            let (main, chip) = (self.main(cx), cx.weak_entity());
            cx.new(|cx| ProjectPicker::new(main, chip, window, cx))
        });
        cx.notify();
    }

    fn close(&mut self, cx: &mut Context<Self>) {
        self.picker = None;
        cx.notify();
    }
}

impl Render for ProjectChip {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if std::mem::take(&mut self.asked) {
            self.set_open(true, window, cx);
        }
        let t = cx.app_theme().clone();
        let main = self.main(cx);
        let name = project_name(&main).to_string();
        let trigger = Button::new("project-chip").ghost().small().child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .max_w(px(256.))
                .child(project_tile(&main, 18., cx))
                .child(
                    div()
                        .truncate()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_size(rems(0.78125))
                        .child(name),
                )
                .child(
                    Icon::new(IconName::ChevronsUpDown)
                        .xsmall()
                        .text_color(t.subtle),
                ),
        );
        let picker = self.picker.clone();
        let focus = picker.as_ref().map(|p| p.read(cx).query.focus_handle(cx));
        let mut popover = Popover::new("project-picker")
            .anchor(Anchor::TopLeft)
            .open(picker.is_some())
            .on_open_change(cx.listener(|this, open: &bool, window, cx| {
                this.set_open(*open, window, cx);
                // A click opened the popover, which took focus before the picker existed: the
                // filter takes it now, so the keys work at once.
                if let Some(p) = &this.picker {
                    p.read(cx).query.focus_handle(cx).focus(window, cx);
                }
            }))
            .trigger(trigger)
            .content(move |_, _, _| {
                picker
                    .clone()
                    .map(|p| p.into_any_element())
                    .unwrap_or_else(|| div().into_any_element())
            });
        if let Some(f) = &focus {
            popover = popover.track_focus(f);
        }
        popover
    }
}

/// The row the keys act on, by what it shows, so a change of the list keeps it.
#[derive(Clone, Debug, PartialEq)]
enum Hot {
    Folder(u64),
    Project(String),
}

impl Hot {
    fn of(line: &Line) -> Option<Hot> {
        match line {
            Line::Folder { id, .. } => Some(Hot::Folder(*id)),
            Line::Project { path, .. } => Some(Hot::Project(path.clone())),
            Line::Header(_) => None,
        }
    }
}

#[derive(Clone)]
struct DraggedProject {
    path: String,
}

impl Render for DraggedProject {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.app_theme();
        div()
            .flex()
            .items_center()
            .gap_2()
            .px_2()
            .py_1()
            .rounded(px(4.))
            .bg(t.elevated)
            .border_1()
            .border_color(t.border_strong)
            .text_size(rems(0.75))
            .child(project_tile(&self.path, 16., cx))
            .child(project_name(&self.path).to_string())
    }
}

/// Changes the arrangement and stores it.
fn arrange(cx: &mut App, f: impl FnOnce(&mut ProjectOrg, &[String])) {
    UiState::update(cx, |s| {
        let recent = s.recent_projects.clone();
        f(&mut s.project_org, &recent)
    });
}

const LIST_MAX: f32 = 400.;

pub struct ProjectPicker {
    /// The window's project, by its main folder.
    main: String,
    chip: WeakEntity<ProjectChip>,
    query: Entity<InputState>,
    hot: Option<Hot>,
    /// A folder being named, in its row.
    naming: Option<(u64, Entity<InputState>, Subscription)>,
    scroll: ScrollHandle,
    home: Option<String>,
    _subs: Vec<Subscription>,
    _live: Live,
}

impl ProjectPicker {
    fn new(
        main: String,
        chip: WeakEntity<ProjectChip>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let query = cx.new(|cx| InputState::new(window, cx).placeholder("Filter projects…"));
        let subs = vec![
            cx.subscribe_in(
                &query,
                window,
                |this: &mut Self, _, e: &InputEvent, window, cx| match e {
                    // A search starts on its first match.
                    InputEvent::Change => {
                        this.hot = this.lines(cx).iter().find_map(Hot::of);
                        cx.notify();
                    }
                    InputEvent::PressEnter { .. } => this.choose(window, cx),
                    _ => {}
                },
            ),
            cx.observe_global::<UiState>(|_, cx| cx.notify()),
        ];
        ProjectPicker {
            hot: Some(Hot::Project(main.clone())),
            main,
            chip,
            query,
            naming: None,
            scroll: ScrollHandle::new(),
            home: std::env::var("HOME").ok().filter(|h| !h.is_empty()),
            _subs: subs,
            _live: Live::new("ProjectPicker"),
        }
    }

    fn typed(&self, cx: &App) -> String {
        self.query.read(cx).value().to_string()
    }

    fn lines(&self, cx: &App) -> Vec<Line> {
        let s = UiState::get(cx);
        s.project_org
            .layout(&s.recent_projects, &self.typed(cx), false)
    }

    /// The keys' row as a line of `lines`: the one moved to, else the first row.
    fn hot_line(&self, lines: &[Line]) -> Option<usize> {
        let first = lines.iter().position(Line::is_row);
        self.hot
            .as_ref()
            .and_then(|h| lines.iter().position(|l| Hot::of(l).as_ref() == Some(h)))
            .or(first)
    }

    fn close(&mut self, cx: &mut Context<Self>) {
        let _ = self.chip.update(cx, |c, cx| c.close(cx));
    }

    fn open(&mut self, path: String, window: &mut Window, cx: &mut Context<Self>) {
        self.close(cx);
        if path == self.main {
            return;
        }
        if let Some(ws) = Workspace::for_window(window, cx) {
            ws.update(cx, |ws, cx| ws.open_project(path.into(), window, cx));
        }
    }

    /// ↵: opens the project, or folds the folder.
    fn choose(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let lines = self.lines(cx);
        match self.hot_line(&lines).map(|i| &lines[i]) {
            Some(Line::Project { path, .. }) => self.open(path.clone(), window, cx),
            Some(Line::Folder { id, .. }) => {
                let id = *id;
                arrange(cx, |o, _| o.toggle_folder(id));
            }
            _ => {}
        }
    }

    fn on_key(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &e.keystroke;
        if self.naming.is_some() {
            // The name field has the keys; Escape leaves the name as it was.
            if k.key == "escape" {
                self.finish_naming(false, window, cx);
                cx.stop_propagation();
            }
            return;
        }
        let lines = self.lines(cx);
        let at = self.hot_line(&lines);
        if is_menu_key(k) {
            if let Some(mut row) = at.and_then(|i| self.scroll.bounds_for_item(i)) {
                row.origin += self.scroll.offset();
                open_row_menu(row, window, cx);
            }
            return cx.stop_propagation();
        }
        let m = k.modifiers;
        if m.platform || m.control || m.alt || m.shift {
            return;
        }
        let rows: Vec<usize> = (0..lines.len()).filter(|i| lines[*i].is_row()).collect();
        let n = rows.len();
        let pos = at.and_then(|a| rows.iter().position(|r| *r == a));
        let typed = self.typed(cx);
        let searching = !typed.trim().is_empty();
        let step = list_step(k, pos, n, Ends::Wrap, !typed.is_empty());
        let to = match k.key.as_str() {
            _ if step.is_some() => step.map(|p| rows[p]),
            // With nothing typed the caret has nowhere to go: ←/→ fold and unfold.
            "left" | "right" if !searching => {
                let open = k.key == "right";
                match at.map(|i| &lines[i]) {
                    Some(Line::Folder { id, open: was, .. }) if *was != open => {
                        let id = *id;
                        arrange(cx, |o, _| o.toggle_folder(id));
                        None
                    }
                    // ← on a filed project goes up to its folder.
                    Some(Line::Project {
                        place: Place::Folder(id),
                        ..
                    }) if !open => lines
                        .iter()
                        .position(|l| matches!(l, Line::Folder { id: f, .. } if f == id)),
                    _ => return,
                }
            }
            "escape" => {
                self.close(cx);
                None
            }
            "enter" => {
                self.choose(window, cx);
                None
            }
            _ => return,
        };
        if let Some(to) = to {
            self.hot = Hot::of(&lines[to]);
            self.scroll.scroll_to_item(to);
        }
        cx.stop_propagation();
        cx.notify();
    }

    /// The folder `id`'s name in a field in its row; ↵ or leaving it keeps what was typed.
    fn start_naming(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let name = UiState::get(cx)
            .project_org
            .folders
            .iter()
            .find(|f| f.id == id)
            .map(|f| f.name.clone())
            .unwrap_or_default();
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Folder name")
                .default_value(name)
        });
        let sub = cx.subscribe_in(
            &input,
            window,
            |this: &mut Self, _, e: &InputEvent, window, cx| match e {
                InputEvent::PressEnter { .. } | InputEvent::Blur => {
                    this.finish_naming(true, window, cx)
                }
                _ => {}
            },
        );
        input.read(cx).focus_handle(cx).focus(window, cx);
        self.hot = Some(Hot::Folder(id));
        self.naming = Some((id, input, sub));
        cx.notify();
    }

    fn finish_naming(&mut self, keep: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some((id, input, _)) = self.naming.take() else {
            return;
        };
        if keep {
            let name = input.read(cx).value().to_string();
            arrange(cx, |o, _| o.rename_folder(id, &name));
        }
        self.query.read(cx).focus_handle(cx).focus(window, cx);
        cx.notify();
    }

    /// A new folder, named in its row; `with` goes into it.
    fn new_folder(&mut self, with: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        let mut id = 0;
        arrange(cx, |o, _| {
            id = o.new_folder("New Folder");
            if let Some(p) = &with {
                o.move_to(p, Place::Folder(id));
            }
        });
        // A filter could hide the new folder.
        self.query.update(cx, |q, cx| q.set_value("", window, cx));
        self.start_naming(id, window, cx);
    }

    fn header(&self, section: Section, cx: &Context<Self>) -> AnyElement {
        let t = cx.app_theme();
        let (label, place) = match section {
            Section::Pinned => ("PINNED", Place::Pinned),
            Section::Projects => ("PROJECTS", Place::Loose),
        };
        div()
            .id(label)
            .flex()
            .items_end()
            .h(px(24.))
            .px_2()
            .pb_1()
            .text_size(rems(0.65625))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(t.subtle)
            .child(label)
            .drag_over::<DraggedProject>(|s, _, _, cx| s.bg(cx.app_theme().active))
            .on_drop(move |d: &DraggedProject, _, cx| {
                let path = d.path.clone();
                arrange(cx, |o, recent| {
                    o.drop_on(&path, DropOn::Into(place), recent)
                });
            })
            .into_any_element()
    }

    fn folder(
        &self,
        id: u64,
        name: &str,
        open: bool,
        count: usize,
        hot: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = cx.app_theme().clone();
        let naming = self
            .naming
            .as_ref()
            .filter(|(n, _, _)| *n == id)
            .map(|(_, input, _)| input.clone());
        let weak = cx.weak_entity();
        let fg = if hot {
            t.primary_foreground
        } else {
            t.foreground
        };
        let dim = if hot {
            t.primary_foreground.opacity(0.8)
        } else {
            t.subtle
        };
        div()
            .id(("project-folder", id as usize))
            .flex()
            .items_center()
            .gap_1p5()
            .h(px(28.))
            .px_2()
            .rounded(px(3.))
            .text_size(rems(0.75))
            .font_weight(FontWeight::MEDIUM)
            .text_color(fg)
            .when(hot && naming.is_none(), |d| d.bg(t.primary))
            .cursor_pointer()
            .child(
                Icon::new(if open {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                })
                .xsmall()
                .text_color(dim),
            )
            .child(
                Icon::new(if open {
                    IconName::FolderOpen
                } else {
                    IconName::Folder
                })
                .small()
                .text_color(dim),
            )
            .map(|d| match naming {
                Some(input) => d.child(div().flex_1().child(Input::new(&input).xsmall())),
                None => d
                    .child(div().flex_1().min_w_0().truncate().child(name.to_string()))
                    .child(
                        div()
                            .text_size(rems(0.65625))
                            .text_color(dim)
                            .child(count.to_string()),
                    ),
            })
            .on_mouse_move(cx.listener(move |this, _, _, cx| {
                if this.naming.is_none() && this.hot != Some(Hot::Folder(id)) {
                    this.hot = Some(Hot::Folder(id));
                    cx.notify();
                }
            }))
            .on_click(cx.listener(move |this, _, _, cx| {
                if this.naming.is_none() {
                    this.hot = Some(Hot::Folder(id));
                    arrange(cx, |o, _| o.toggle_folder(id));
                }
            }))
            .drag_over::<DraggedProject>(|s, _, _, cx| s.bg(cx.app_theme().active))
            .on_drop(move |d: &DraggedProject, _, cx| {
                let path = d.path.clone();
                arrange(cx, |o, recent| {
                    o.drop_on(&path, DropOn::Into(Place::Folder(id)), recent)
                });
            })
            .context_menu(move |menu, _, _| {
                let (rename, add) = (weak.clone(), weak.clone());
                menu.item(
                    PopupMenuItem::new("Rename…")
                        .icon(IconName::Pencil)
                        .on_click(move |_, window, cx| {
                            let _ = rename.update(cx, |p, cx| p.start_naming(id, window, cx));
                        }),
                )
                .item(
                    PopupMenuItem::new(if open { "Collapse" } else { "Expand" })
                        .on_click(move |_, _, cx| arrange(cx, |o, _| o.toggle_folder(id))),
                )
                .item(
                    PopupMenuItem::new("Delete Folder")
                        .icon(IconName::Trash)
                        .on_click(move |_, _, cx| arrange(cx, |o, _| o.delete_folder(id))),
                )
                .separator()
                .item(
                    PopupMenuItem::new("New Folder…")
                        .icon(IconName::FolderPlus)
                        .on_click(move |_, window, cx| {
                            let _ = add.update(cx, |p, cx| p.new_folder(None, window, cx));
                        }),
                )
            })
            .into_any_element()
    }

    fn project(&self, path: &str, place: Place, hot: bool, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.app_theme().clone();
        let current = path == self.main;
        let name = project_name(path).to_string();
        let detail = home_relative(path, self.home.as_deref());
        let dim = if hot {
            t.primary_foreground.opacity(0.8)
        } else {
            t.subtle
        };
        let org = &UiState::get(cx).project_org;
        let color = org.colors.get(path).cloned();
        let folders: Vec<(u64, String)> =
            org.folders.iter().map(|f| (f.id, f.name.clone())).collect();
        let (hover, click, drop_to) = (path.to_string(), path.to_string(), path.to_string());
        let path = path.to_string();
        let weak = cx.weak_entity();
        div()
            .id(SharedString::from(format!("project-row-{path}")))
            .flex()
            .items_center()
            .gap_2()
            .h(px(36.))
            .px_2()
            .when(matches!(place, Place::Folder(_)), |d| d.pl(px(26.)))
            .rounded(px(3.))
            .cursor_pointer()
            .when(hot, |d| d.bg(t.primary).text_color(t.primary_foreground))
            .child(project_tile(&path, 18., cx))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .truncate()
                            .text_size(rems(0.75))
                            .when(current, |d| d.font_weight(FontWeight::SEMIBOLD))
                            .child(name),
                    )
                    .child(
                        div()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis_middle()
                            .text_size(rems(0.65625))
                            .text_color(dim)
                            .child(detail),
                    ),
            )
            .when(place == Place::Pinned, |d| {
                d.child(Icon::new(IconName::Pin).xsmall().text_color(dim))
            })
            .when(current, |d| d.child(Icon::new(IconName::Check).small()))
            .on_mouse_move(cx.listener(move |this, _, _, cx| {
                let h = Some(Hot::Project(hover.clone()));
                if this.naming.is_none() && this.hot != h {
                    this.hot = h;
                    cx.notify();
                }
            }))
            .on_click(cx.listener(move |this, _, window, cx| this.open(click.clone(), window, cx)))
            .on_drag(DraggedProject { path: path.clone() }, |d, _, _, cx| {
                cx.new(|_| d.clone())
            })
            .drag_over::<DraggedProject>(|s, _, _, cx| s.bg(cx.app_theme().active))
            .on_drop(move |d: &DraggedProject, _, cx| {
                let (path, before) = (d.path.clone(), drop_to.clone());
                arrange(cx, |o, recent| {
                    o.drop_on(&path, DropOn::Before(before), recent)
                });
            })
            .context_menu(move |menu, window, cx| {
                project_menu(
                    menu,
                    &path,
                    place,
                    current,
                    color.as_deref(),
                    &folders,
                    &weak,
                    window,
                    cx,
                )
            })
            .into_any_element()
    }
}

/// A project row's right-click menu.
#[allow(clippy::too_many_arguments)]
fn project_menu(
    menu: PopupMenu,
    path: &str,
    place: Place,
    current: bool,
    color: Option<&str>,
    folders: &[(u64, String)],
    picker: &WeakEntity<ProjectPicker>,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let p = path.to_string();
    let (open, pin, forget) = (picker.clone(), p.clone(), p.clone());
    let color = color.map(str::to_string);
    let (cp, fp, fw) = (p.clone(), p.clone(), picker.clone());
    let folders = folders.to_vec();
    menu.item(
        PopupMenuItem::new("Open")
            .icon(IconName::FolderOpen)
            .disabled(current)
            .on_click({
                let p = p.clone();
                move |_, window, cx| {
                    let _ = open.update(cx, |this, cx| this.open(p.clone(), window, cx));
                }
            }),
    )
    .separator()
    .item(
        PopupMenuItem::new(if place == Place::Pinned {
            "Unpin"
        } else {
            "Pin"
        })
        .icon(if place == Place::Pinned {
            IconName::PinOff
        } else {
            IconName::Pin
        })
        .on_click(move |_, _, cx| arrange(cx, |o, _| o.toggle_pin(&pin))),
    )
    .submenu_with_icon(
        Some(Icon::new(IconName::Palette)),
        "Color",
        window,
        cx,
        move |mut sub, _, _| {
            let auto = cp.clone();
            sub = sub
                .item(
                    PopupMenuItem::new("Automatic")
                        .checked(color.is_none())
                        .on_click(move |_, _, cx| {
                            arrange(cx, |o, recent| {
                                o.set_color(&auto, None);
                                o.assign_colors(recent);
                            })
                        }),
                )
                .separator();
            for s in &SWATCHES {
                let p = cp.clone();
                sub = sub.item(
                    PopupMenuItem::element(move |_, cx| {
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(swatch_tile(s, "", 14., cx))
                            .child(s.name)
                    })
                    .checked(color.as_deref() == Some(s.key))
                    .on_click(move |_, _, cx| arrange(cx, |o, _| o.set_color(&p, Some(s.key)))),
                );
            }
            sub
        },
    )
    .submenu_with_icon(
        Some(Icon::new(IconName::Folder)),
        "Move to Folder",
        window,
        cx,
        move |mut sub, _, _| {
            for (id, name) in &folders {
                let (id, p) = (*id, fp.clone());
                sub = sub.item(
                    PopupMenuItem::new(name.clone())
                        .checked(place == Place::Folder(id))
                        .on_click(move |_, _, cx| {
                            arrange(cx, |o, _| o.move_to(&p, Place::Folder(id)))
                        }),
                );
            }
            if !folders.is_empty() {
                sub = sub.separator();
            }
            let (p, w) = (fp.clone(), fw.clone());
            sub = sub.item(
                PopupMenuItem::new("New Folder…")
                    .icon(IconName::FolderPlus)
                    .on_click(move |_, window, cx| {
                        let _ =
                            w.update(cx, |this, cx| this.new_folder(Some(p.clone()), window, cx));
                    }),
            );
            if matches!(place, Place::Folder(_)) {
                let p = fp.clone();
                sub = sub.item(
                    PopupMenuItem::new("Remove from Folder")
                        .on_click(move |_, _, cx| arrange(cx, |o, _| o.move_to(&p, Place::Loose))),
                );
            }
            sub
        },
    )
    .separator()
    .item(
        PopupMenuItem::new("Remove from List")
            .icon(IconName::X)
            .disabled(current)
            .on_click(move |_, _, cx| UiState::update(cx, |s| s.forget_project(&forget))),
    )
}

impl Render for ProjectPicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.app_theme().clone();
        let lines = self.lines(cx);
        let hot_at = self.hot_line(&lines);
        let sort = UiState::get(cx).project_org.sort;
        let searching = !self.typed(cx).trim().is_empty();
        let shown: Vec<AnyElement> = lines
            .iter()
            .enumerate()
            .map(|(i, line)| {
                let hot = hot_at == Some(i);
                match line {
                    Line::Header(s) => self.header(*s, cx),
                    Line::Folder {
                        id,
                        name,
                        open,
                        count,
                    } => self.folder(*id, name, *open, *count, hot, cx),
                    Line::Project { path, place } => self.project(path, *place, hot, cx),
                }
            })
            .collect();
        let empty = shown.is_empty();
        let weak = cx.weak_entity();
        let sort_button = Button::new("project-sort")
            .ghost()
            .xsmall()
            .icon(IconName::ArrowDownUp)
            .tooltip(match sort {
                ProjectSort::Recent => "Sorted by last opened",
                ProjectSort::Manual => "Sorted by hand",
            })
            .dropdown_menu(move |menu, _, _| {
                let item = |label: &'static str, to: ProjectSort| {
                    PopupMenuItem::new(label)
                        .checked(sort == to)
                        .on_click(move |_, _, cx| arrange(cx, |o, recent| o.set_sort(to, recent)))
                };
                menu.label("Sort")
                    .item(item("Last Opened", ProjectSort::Recent))
                    .item(item("Manual (drag to arrange)", ProjectSort::Manual))
            });
        let new_folder = Button::new("project-new-folder")
            .ghost()
            .xsmall()
            .icon(IconName::FolderPlus)
            .tooltip("New folder")
            .on_click(cx.listener(|this, _, window, cx| this.new_folder(None, window, cx)));
        let open_key = shortcut_label("file.openRepo", cx);
        let action = |id: &'static str,
                      icon: IconName,
                      label: &'static str,
                      key: Option<SharedString>,
                      act: Box<dyn gpui_kit::Action>| {
            let weak = weak.clone();
            div()
                .id(id)
                .flex()
                .items_center()
                .gap_2()
                .h(px(28.))
                .px_2()
                .rounded(px(3.))
                .cursor_pointer()
                .hover(|s| s.bg(t.hover))
                .text_size(rems(0.75))
                .child(Icon::new(icon).small().text_color(t.muted_foreground))
                .child(div().flex_1().child(label))
                .children(key.map(|k| div().text_size(rems(0.6875)).text_color(t.subtle).child(k)))
                .on_click(move |_, window, cx| {
                    let _ = weak.update(cx, |p, cx| p.close(cx));
                    window.dispatch_action(act.boxed_clone(), cx);
                })
        };
        on_list_keys(div(), cx.listener(Self::on_key))
            .key_context("ProjectPicker ListFilter")
            .capture_key_down(cx.listener(Self::on_key))
            .w(px(340.))
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .p_1()
                    .border_b_1()
                    .border_color(t.border)
                    .child(
                        div().flex_1().child(
                            Input::new(&self.query)
                                .appearance(false)
                                .prefix(Icon::new(IconName::Search).small().text_color(t.subtle)),
                        ),
                    )
                    .child(sort_button)
                    .child(new_folder),
            )
            .child(
                div()
                    .id("project-list")
                    .track_scroll(&self.scroll)
                    .max_h(px(LIST_MAX))
                    .overflow_y_scroll()
                    .p_1()
                    .flex()
                    .flex_col()
                    .children(shown)
                    .when(empty, |d| {
                        d.child(
                            div()
                                .px_2()
                                .py_3()
                                .text_center()
                                .text_size(rems(0.75))
                                .text_color(t.subtle)
                                .child(if searching {
                                    "No project matches"
                                } else {
                                    "No projects yet"
                                }),
                        )
                    }),
            )
            .child(
                div()
                    .border_t_1()
                    .border_color(t.border)
                    .p_1()
                    .child(action(
                        "project-open",
                        IconName::Plus,
                        "Open repository…",
                        open_key.map(Into::into),
                        Box::new(file::OpenRepo),
                    ))
                    .child(action(
                        "project-clone",
                        IconName::FolderDown,
                        "Clone repository…",
                        None,
                        Box::new(file::CloneRepo),
                    )),
            )
            .child(
                div()
                    .border_t_1()
                    .border_color(t.border)
                    .px_2p5()
                    .py_1p5()
                    .text_size(rems(0.65625))
                    .text_color(t.subtle)
                    .child(key_hints([
                        "↵ open".into(),
                        "drag to arrange".into(),
                        "right-click for more".into(),
                    ])),
            )
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::{TestAppContext, VisualTestContext, point, size};

    use super::*;

    struct Host;
    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
        }
    }

    /// ↑/↓ walk the rows past the headers, ←/→ fold a folder, and a filter starts on its match.
    #[gpui_kit::test]
    fn keys_walk_the_rows_and_fold_folders(cx: &mut TestAppContext) {
        gitviber_ui::test_support::config_dir("refs-project-picker");
        cx.update(|cx| {
            gitviber_ui::init(cx);
            UiState::update(cx, |s| {
                for p in ["/w/a", "/w/b", "/w/c"] {
                    s.add_recent_project(p);
                }
                s.project_org.toggle_pin("/w/a");
                let f = s.project_org.new_folder("Work");
                s.project_org.move_to("/w/b", Place::Folder(f));
            });
        });
        let (_, cx) = cx.add_window_view(|_, _| Host);
        let picker = cx.update(|window, cx| {
            cx.new(|cx| ProjectPicker::new("/w/c".into(), WeakEntity::new_invalid(), window, cx))
        });
        let draw = |cx: &mut VisualTestContext| {
            let picker = picker.clone();
            cx.draw(
                point(px(0.), px(0.)),
                size(px(400.), px(600.)),
                move |_, _| div().child(picker),
            );
        };
        draw(cx);
        let key = |k: &str, cx: &mut VisualTestContext| {
            let e = KeyDownEvent {
                keystroke: gpui_kit::Keystroke::parse(k).unwrap(),
                is_held: false,
                prefer_character_input: false,
            };
            picker.update_in(cx, |p, window, cx| p.on_key(&e, window, cx));
        };
        let hot = |cx: &mut VisualTestContext| picker.read_with(cx, |p, _| p.hot.clone());
        // Opens on the window's project, the last row.
        assert_eq!(hot(cx), Some(Hot::Project("/w/c".into())));
        key("down", cx);
        assert_eq!(hot(cx), Some(Hot::Project("/w/a".into())));
        key("down", cx);
        assert_eq!(hot(cx), Some(Hot::Folder(1)));
        key("left", cx);
        let folded = cx.update(|_, cx| UiState::get(cx).project_org.folders[0].collapsed);
        assert!(folded);
        key("down", cx);
        assert_eq!(hot(cx), Some(Hot::Project("/w/c".into())));
        key("up", cx);
        key("right", cx);
        key("down", cx);
        assert_eq!(hot(cx), Some(Hot::Project("/w/b".into())));
        // ← on a filed project goes up to its folder.
        key("left", cx);
        assert_eq!(hot(cx), Some(Hot::Folder(1)));
        picker.update_in(cx, |p, window, cx| {
            p.query.update(cx, |q, cx| q.set_value("b", window, cx))
        });
        draw(cx);
        assert_eq!(hot(cx), Some(Hot::Folder(1)));
        key("down", cx);
        assert_eq!(hot(cx), Some(Hot::Project("/w/b".into())));
    }
}
