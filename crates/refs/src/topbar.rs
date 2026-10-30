//! The top bar's items: the project, branch, worktree and pull request chips, and the git
//! actions on the right: what's running (with Cancel), undo and redo with their history, Fetch,
//! Pull and Push (or Publish) with their menus. Each keeps its state in the bar's element state
//! per project, so it goes with the project.

use std::collections::HashSet;
use std::time::Duration;

use gitviber_core::{api, git};
use gitviber_ui::keymap::{file, shortcut_label};
use gitviber_ui::memory::Live;
use gitviber_ui::widgets::{Ends, list_step};
use gitviber_ui::workspace::{BarItem, BarSlot, PanelContext, register_bar_item};
use gitviber_ui::{ActiveAppTheme, Project, Settings, UiState, Workspace};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _, DropdownButton};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::popover::Popover;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Disableable as _, Icon, Sizable as _};
use gpui_kit::{
    Anchor, AnyElement, App, AppContext as _, Context, Entity, FocusHandle, FontWeight, Global,
    InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement as _, Render, ScrollHandle,
    SharedString, StatefulInteractiveElement as _, Styled as _, Subscription, Task, Window, div,
    prelude::FluentBuilder as _, px, rems,
};

use crate::names::folder_name;
use crate::net::{NetActivity, NetOp, progress_text};
use crate::ops;
use crate::remote_tags;
use crate::repo::RepoState;
use crate::worktrees::{list as wt_list, worktree_chip};

pub fn register(cx: &mut App) {
    cx.on_action(|_: &file::SwitchProject, cx| switch_project(cx));
    register_bar_item(
        cx,
        BarItem::new("project", BarSlot::TopChips, project_chip).order(0),
    );
    register_bar_item(
        cx,
        BarItem::new("branch", BarSlot::TopChips, crate::branches::branch_chip).order(1),
    );
    register_bar_item(
        cx,
        BarItem::new("worktree", BarSlot::TopChips, worktree_chip).order(2),
    );
    register_bar_item(
        cx,
        BarItem::new("pull-request", BarSlot::TopChips, pull_request_chip).order(3),
    );
    for (id, part, order) in [
        ("activity", Part::Activity, 0),
        ("undo", Part::Undo, 1),
        ("refresh", Part::Fetch, 2),
        ("pull", Part::Pull, 3),
        ("push", Part::Push, 4),
    ] {
        register_bar_item(
            cx,
            BarItem::new(id, BarSlot::TopActions, move |ctx, window, cx| {
                top_part(part, ctx, window, cx)
            })
            .order(order),
        );
    }
}

// MARK: Chips

/// The pull request of the linked worktree's branch, when GitHub's crate knows it.
fn pull_request_chip(ctx: &PanelContext, _: &mut Window, cx: &mut App) -> Option<AnyElement> {
    let project = ctx.project.clone()?;
    let state = RepoState::get(&project, cx);
    let s = state.read(cx);
    let current = wt_list(&s.worktrees)
        .into_iter()
        .find(|w| w.current)
        .filter(|w| !w.main)?;
    let pull = crate::hooks::pull_of(
        current.branch.as_deref()?,
        current.head.as_deref(),
        false,
        cx,
    )?;
    Some(crate::worktrees::picker_pull_chip(&pull, false, cx))
}

/// Each window's project chip as last drawn, for Switch Project to open; a closed window's or an
/// old project's goes with it (weak).
#[derive(Default)]
struct ProjectChips(Vec<(gpui_kit::AnyWindowHandle, gpui_kit::WeakEntity<ProjectChip>)>);
impl Global for ProjectChips {}

fn project_chip(ctx: &PanelContext, window: &mut Window, cx: &mut App) -> Option<AnyElement> {
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
fn switch_project(cx: &mut App) {
    let Some(handle) = cx.active_window() else {
        return;
    };
    if let Some(chip) = ProjectChip::of_window(handle, cx) {
        chip.update(cx, |c, cx| c.set_open(true, cx));
    }
}

pub struct ProjectChip {
    project: Entity<Project>,
    open: bool,
    /// The list's: it takes the keys as it opens.
    focus: FocusHandle,
    /// The row the keys are on: the projects, then Open and Clone.
    index: Option<usize>,
    scroll: ScrollHandle,
    _subs: Vec<Subscription>,
    _live: Live,
}

/// The list's rows past the projects: Open repository…, Clone repository….
const PROJECT_ACTIONS: usize = 2;

impl ProjectChip {
    fn new(project: &Entity<Project>, cx: &mut Context<Self>) -> Self {
        let subs = vec![cx.observe(project, |_, _, cx| cx.notify())];
        ProjectChip {
            project: project.clone(),
            open: false,
            focus: cx.focus_handle(),
            index: None,
            scroll: ScrollHandle::new(),
            _subs: subs,
            _live: Live::new("ProjectChip"),
        }
    }

    /// The chip `window` drew last.
    pub fn of_window(window: gpui_kit::AnyWindowHandle, cx: &App) -> Option<Entity<Self>> {
        cx.try_global::<ProjectChips>()?
            .0
            .iter()
            .find(|(w, _)| *w == window)?
            .1
            .upgrade()
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// The row the keys are on, while the list has them.
    pub fn key_row(&self, window: &Window) -> Option<usize> {
        self.index.filter(|_| self.focus.is_focused(window))
    }

    /// Opening, the keys start on the latest project besides this one.
    fn set_open(&mut self, open: bool, cx: &mut Context<Self>) {
        if open && !self.open {
            let main = self.main_path(cx);
            let recent = &UiState::get(cx).recent_projects;
            self.index = Some(recent.iter().position(|p| *p != main).unwrap_or(0));
        }
        self.open = open;
        cx.notify();
    }

    /// The project is its main worktree, even inside a linked one.
    fn main_path(&self, cx: &App) -> String {
        let p = self.project.read(cx);
        p.main_worktree()
            .map(|m| m.to_string_lossy().into_owned())
            .unwrap_or_else(|| p.root().to_string_lossy().into_owned())
    }

    /// ↑/↓, Home/End, PgUp/PgDn move through the rows, ↵ picks one; Esc is the popover's.
    fn on_key(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let n = UiState::get(cx).recent_projects.len() + PROJECT_ACTIONS;
        let k = &e.keystroke;
        if let Some(to) = list_step(k, self.index, n, Ends::Wrap, false) {
            self.index = Some(to);
            self.scroll.scroll_to_item(to);
        } else if k.key == "enter" && !k.modifiers.modified() {
            if let Some(i) = self.index {
                self.pick(i, window, cx);
            }
        } else {
            return;
        }
        cx.stop_propagation();
        cx.notify();
    }

    /// Row `i`: a project opens in this window; past them, Open or Clone repository….
    fn pick(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.set_open(false, cx);
        let recent = &UiState::get(cx).recent_projects;
        if let Some(path) = recent.get(i).cloned() {
            if let Some(ws) = Workspace::for_window(window, cx) {
                ws.update(cx, |ws, cx| ws.open_project(path.into(), window, cx));
            }
            return;
        }
        let act: Box<dyn gpui_kit::Action> = match i - recent.len() {
            0 => Box::new(file::OpenRepo),
            _ => Box::new(file::CloneRepo),
        };
        window.dispatch_action(act, cx);
    }
}

/// The project's tile: its first letter on a tint.
fn tile(name: &str, cx: &App) -> AnyElement {
    let t = cx.app_theme();
    div()
        .size(px(18.))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .rounded(px(4.))
        .bg(t.renamed_fill)
        .text_color(t.on_status)
        .text_size(rems(0.6875))
        .font_weight(FontWeight::BOLD)
        .child(
            name.chars()
                .next()
                .unwrap_or('?')
                .to_uppercase()
                .collect::<String>(),
        )
        .into_any_element()
}

impl Render for ProjectChip {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.app_theme().clone();
        let main = self.main_path(cx);
        let name = folder_name(&main).to_string();
        let recent = UiState::get(cx).recent_projects.clone();
        let open_key = shortcut_label("file.openRepo", cx);
        let trigger = Button::new("project-chip").ghost().small().child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .max_w(px(256.))
                .child(tile(&name, cx))
                .child(
                    div()
                        .truncate()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_size(rems(0.78125))
                        .child(name.clone()),
                )
                .child(
                    Icon::new(IconName::ChevronsUpDown)
                        .xsmall()
                        .text_color(t.subtle),
                ),
        );
        let weak = cx.weak_entity();
        let main_c = main.clone();
        let (focus, index, scroll) = (self.focus.clone(), self.index, self.scroll.clone());
        let on_key = std::rc::Rc::new(cx.listener(Self::on_key));
        Popover::new("project-picker")
            .anchor(Anchor::TopLeft)
            .open(self.open)
            .track_focus(&focus)
            .on_open_change(cx.listener(|this, open: &bool, _, cx| this.set_open(*open, cx)))
            .trigger(trigger)
            .content(move |_, _, cx| {
                let t = cx.app_theme().clone();
                let close = weak.clone();
                // The pointer moves the keys' row too, so one row shows lit.
                let hot = |i: usize, d: gpui_kit::Stateful<gpui_kit::Div>| {
                    let weak = close.clone();
                    d.when(index == Some(i), |d| d.bg(t.hover))
                        .on_mouse_move(move |_, _, cx| {
                            let _ = weak.update(cx, |c, cx| {
                                if c.index != Some(i) {
                                    c.index = Some(i);
                                    cx.notify();
                                }
                            });
                        })
                };
                let rows = recent.iter().enumerate().map(|(i, path)| {
                    let (path, weak) = (path.clone(), close.clone());
                    let current = path == main_c;
                    let name = folder_name(&path).to_string();
                    div()
                        .id(("project-row", i))
                        .flex()
                        .items_center()
                        .gap_2()
                        .h(px(36.))
                        .px_2()
                        .rounded(px(3.))
                        .cursor_pointer()
                        .hover(|s| s.bg(t.hover))
                        .child(tile(&name, cx))
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
                                        .truncate()
                                        .text_size(rems(0.65625))
                                        .text_color(t.subtle)
                                        .child(path.clone()),
                                ),
                        )
                        .when(current, |d| d.child(Icon::new(IconName::Check).small()))
                        .on_click(move |_, window, cx| {
                            let _ = weak.update(cx, |c, cx| {
                                c.open = false;
                                cx.notify();
                            });
                            if let Some(ws) = Workspace::for_window(window, cx) {
                                ws.update(cx, |ws, cx| {
                                    ws.open_project(path.clone().into(), window, cx)
                                });
                            }
                        })
                        .map(|d| hot(i, d))
                });
                let action =
                    |i: usize,
                     id: &'static str,
                     icon: IconName,
                     label: &'static str,
                     key: Option<SharedString>,
                     act: Box<dyn gpui_kit::Action>,
                     cx: &App| {
                        let (weak, act) = (close.clone(), act);
                        let t = cx.app_theme().clone();
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
                            .children(key.map(|k| {
                                div().text_size(rems(0.6875)).text_color(t.subtle).child(k)
                            }))
                            .on_click(move |_, window, cx| {
                                let _ = weak.update(cx, |c, cx| {
                                    c.open = false;
                                    cx.notify();
                                });
                                window.dispatch_action(act.boxed_clone(), cx);
                            })
                            .map(|d| hot(i, d))
                    };
                let projects = recent.len();
                div()
                    .track_focus(&focus)
                    .on_key_down({
                        let on_key = on_key.clone();
                        move |e, window, cx| on_key(e, window, cx)
                    })
                    .w(px(320.))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .px_3()
                            .pt_2p5()
                            .pb_1()
                            .text_size(rems(0.65625))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(t.subtle)
                            .child("PROJECTS"),
                    )
                    .child(
                        div()
                            .id("project-list")
                            .track_scroll(&scroll)
                            .max_h(px(360.))
                            .overflow_y_scroll()
                            .p_1()
                            .children(rows),
                    )
                    .child(
                        div()
                            .border_t_1()
                            .border_color(t.border)
                            .p_1()
                            .child(action(
                                projects,
                                "project-open",
                                IconName::Plus,
                                "Open repository…",
                                open_key.clone().map(Into::into),
                                Box::new(file::OpenRepo),
                                cx,
                            ))
                            .child(action(
                                projects + 1,
                                "project-clone",
                                IconName::FolderDown,
                                "Clone repository…",
                                None,
                                Box::new(file::CloneRepo),
                                cx,
                            )),
                    )
            })
    }
}

// MARK: Actions

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Part {
    Activity,
    Undo,
    Fetch,
    Pull,
    Push,
}

fn top_part(
    part: Part,
    ctx: &PanelContext,
    window: &mut Window,
    cx: &mut App,
) -> Option<AnyElement> {
    let project = ctx.project.clone()?;
    let id = project.entity_id().as_u64() as usize;
    let entity = window.use_keyed_state(
        (SharedString::from(format!("refs-top-{part:?}")), id),
        cx,
        |_, cx| TopPart::new(part, &project, cx),
    );
    Some(entity.into_any_element())
}

pub struct TopPart {
    part: Part,
    state: Entity<RepoState>,
    /// The background fetch, on the Fetch button's part: one per project.
    _fetch: Option<Task<()>>,
    _subs: Vec<Subscription>,
    _live: Live,
}

const FIRST_CHECK: Duration = Duration::from_secs(5);
const CHECK_EVERY: Duration = Duration::from_secs(60);

impl TopPart {
    fn new(part: Part, project: &Entity<Project>, cx: &mut Context<Self>) -> Self {
        let state = RepoState::get(project, cx);
        let mut subs = vec![cx.observe(&state, |_, _, cx| cx.notify())];
        if part == Part::Activity {
            let net = NetActivity::global(cx);
            subs.push(cx.observe(&net, |_, _, cx| cx.notify()));
        }
        if part == Part::Fetch {
            subs.push(cx.observe_global::<Settings>(|this, cx| this.arm_fetch(cx)));
        }
        let mut this = TopPart {
            part,
            state,
            _fetch: None,
            _subs: subs,
            _live: Live::new("TopPart"),
        };
        if part == Part::Fetch {
            this.arm_fetch(cx);
        }
        this
    }

    /// Fetches the open repo quietly once its last fetch, by anyone (the app, a terminal, an
    /// agent), is older than the interval set in Settings; checked shortly after it opens, then
    /// every minute. A failed try also waits a full interval. Off in Settings: no timer at all.
    fn arm_fetch(&mut self, cx: &mut Context<Self>) {
        let minutes = Settings::get(cx).background_fetch;
        if minutes == 0 {
            self._fetch = None;
            return;
        }
        if self._fetch.is_some() {
            return;
        }
        let state = self.state.downgrade();
        self._fetch = Some(cx.spawn(async move |this, cx| {
            let mut last_try = 0u64;
            let mut wait = FIRST_CHECK;
            loop {
                cx.background_executor().timer(wait).await;
                wait = CHECK_EVERY;
                let Some((minutes, remotes, busy)) = cx.update(|cx| {
                    let s = state.upgrade()?;
                    let remotes = s
                        .read(cx)
                        .status
                        .as_ref()
                        .is_some_and(|st| !st.remotes.is_empty());
                    let busy = NetActivity::global(cx).read(cx).is_running()
                        || s.read(cx).busy().is_some();
                    Some((Settings::get(cx).background_fetch, remotes, busy))
                }) else {
                    break;
                };
                if minutes == 0 {
                    let _ = this.update(cx, |this, _| this._fetch = None);
                    break;
                }
                if !remotes || busy {
                    continue;
                }
                let core = cx.update(|cx| gitviber_ui::core(cx));
                let before = last_try;
                let interval = u64::from(minutes) * 60;
                let fetched = cx
                    .background_spawn(async move {
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map_or(0, |d| d.as_secs());
                        let last = api::sync::last_fetch(&core)
                            .ok()
                            .flatten()
                            .unwrap_or(0)
                            .max(last_try);
                        if now.saturating_sub(last) < interval {
                            return (None, now);
                        }
                        let op = NetOp::quiet();
                        (
                            Some(api::sync::fetch(&core, op.id.clone(), op.progress())),
                            now,
                        )
                    })
                    .await;
                match fetched {
                    (None, _) => {}
                    // It gave way to something the user started: not a failed try.
                    (Some(Err(e)), _) if e == gitviber_core::network::CANCELLED => {
                        last_try = before
                    }
                    (Some(_), now) => {
                        last_try = now;
                        let _ = state.update(cx, |s, cx| s.refresh(cx));
                    }
                }
            }
        }));
    }
}

impl Render for TopPart {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        match self.part {
            Part::Activity => activity(&self.state, cx),
            Part::Undo => then_divider(undo_controls(&self.state, window, cx), cx),
            Part::Fetch => {
                let state = self.state.clone();
                let busy = state.read(cx).busy().is_some();
                Button::new("fetch")
                    .ghost()
                    .small()
                    .icon(IconName::RefreshCw)
                    .tooltip(tip("Fetch", "git.fetch", cx))
                    .disabled(busy)
                    .on_click(move |_, window, cx| ops::fetch(&state, window, cx))
                    .into_any_element()
            }
            Part::Pull => pull_button(&self.state, cx),
            Part::Push => then_divider(push_button(&self.state, cx), cx),
        }
    }
}

/// A group's end: undo and redo | fetch, pull and push | what follows. The bar's own divider's
/// look, with the bar's gap kept between the two.
fn then_divider(group: AnyElement, cx: &App) -> AnyElement {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap_1()
        .child(group)
        .child(div().w(px(1.)).h(px(18.)).mx_1().bg(cx.app_theme().border))
        .into_any_element()
}

fn tip(title: &str, command: &str, cx: &App) -> String {
    match shortcut_label(command, cx) {
        Some(k) => format!("{title}  {k}"),
        None => title.to_string(),
    }
}

/// What runs now: an action's label, or a network command's with git's progress and Cancel. At
/// the bar's right end, so starting or ending one never shifts the buttons around it.
fn activity(state: &Entity<RepoState>, cx: &mut App) -> AnyElement {
    let t = cx.app_theme().clone();
    let net = NetActivity::global(cx);
    let (label, progress, cancellable, running) = {
        let n = net.read(cx);
        match n.current() {
            Some(r) => (
                Some(r.label.clone()),
                r.progress.as_ref().map(progress_text),
                r.progress.as_ref().is_none_or(|p| p.cancellable),
                true,
            ),
            None => (state.read(cx).busy().cloned(), None, false, false),
        }
    };
    let Some(label) = label else {
        return div().into_any_element();
    };
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap_1p5()
        .mr_1p5()
        .max_w(px(260.))
        .text_size(rems(0.71875))
        .text_color(t.muted_foreground)
        .child(Icon::new(IconName::LoaderCircle).small())
        .child(
            div()
                .min_w_0()
                .truncate()
                .child(format!("{label}…"))
                .children(progress.map(|p| div().text_color(t.subtle).child(p))),
        )
        .when(running, |d| {
            let lower = label.to_lowercase();
            d.child(
                Button::new("net-cancel")
                    .ghost()
                    .xsmall()
                    .icon(IconName::X)
                    .tooltip(if cancellable {
                        format!("Cancel {lower}")
                    } else {
                        "Too late to cancel: git is updating your files".into()
                    })
                    .disabled(!cancellable)
                    .on_click(move |_, _, cx| NetActivity::global(cx).read(cx).cancel_current(cx)),
            )
        })
        .into_any_element()
}

fn undo_controls(state: &Entity<RepoState>, _: &mut Window, cx: &mut App) -> AnyElement {
    let s = state.read(cx);
    let off = s.busy().is_some();
    let journal = s.journal.clone();
    let (undos, redos) = journal.as_ref().map_or((vec![], vec![]), |j| {
        (
            j.undo
                .iter()
                .map(|e| (e.id, e.label.clone(), e.time, e.switch_to.clone()))
                .collect::<Vec<_>>(),
            j.redo
                .iter()
                .map(|e| (e.id, e.label.clone(), e.time, e.switch_to.clone()))
                .collect::<Vec<_>>(),
        )
    });
    let (undo_blocked, redo_blocked) = journal.as_ref().map_or((None, None), |j| {
        (j.undo_blocked.clone(), j.redo_blocked.clone())
    });
    let button = |forward: bool, cx: &App| {
        let list = if forward { &redos } else { &undos };
        let blocked = if forward {
            redo_blocked.clone()
        } else {
            undo_blocked.clone()
        };
        let verb = if forward { "Redo" } else { "Undo" };
        let next = list.first().cloned();
        let label = match (&next, &blocked) {
            (None, _) => format!("Nothing to {}", verb.to_lowercase()),
            (Some(e), Some(b)) => format!("Can't {} {}. {b}", verb.to_lowercase(), e.1),
            (Some(e), None) => format!(
                "{verb} {}{}",
                e.1,
                ops::switch_note(forward, e.3.as_deref())
            ),
        };
        let state = state.clone();
        Button::new(if forward { "redo" } else { "undo" })
            .ghost()
            .small()
            .icon(if forward {
                IconName::Redo2
            } else {
                IconName::Undo2
            })
            .tooltip(tip(
                &label,
                if forward { "git.redo" } else { "git.undo" },
                cx,
            ))
            .disabled(off || next.is_none() || blocked.is_some())
            .on_click(move |_, window, cx| {
                if let Some((id, _, _, to)) = next.clone() {
                    ops::travel(&state, forward, vec![(id, to)], window, cx);
                }
            })
    };
    let undo = button(false, cx);
    let redo = button(true, cx);
    let st = state.clone();
    let history = Button::new("undo-history")
        .ghost()
        .small()
        .icon(IconName::ChevronDown)
        .tooltip("Undo history")
        .disabled(off)
        .dropdown_menu(move |menu, _, _| {
            history_menu(
                menu,
                &st,
                &undos,
                &redos,
                undo_blocked.clone().or(redo_blocked.clone()),
            )
        });
    div()
        .flex()
        .flex_none()
        .items_center()
        .child(undo)
        .child(redo)
        .child(history)
        .into_any_element()
}

type Entry = (u64, String, u64, Option<String>);

/// Picking an entry undoes it and everything after it (or redoes up to it). Furthest redo on
/// top, so the list reads newest to oldest.
fn history_menu(
    mut menu: PopupMenu,
    state: &Entity<RepoState>,
    undos: &[Entry],
    redos: &[Entry],
    blocked: Option<String>,
) -> PopupMenu {
    menu = menu.min_w(px(320.)).label("Undo history");
    if undos.is_empty() && redos.is_empty() {
        return menu.label(format!("{} show up here, to undo and redo.", ops::UNDOABLE));
    }
    let now = gitviber_ui::widgets::now_unix();
    let item = |e: &Entry, forward: bool, steps: Vec<(u64, Option<String>)>, disabled: bool| {
        let state = state.downgrade();
        let when = gitviber_ui::widgets::relative_time(e.2 as i64, now);
        let text: SharedString = format!("{}  ·  {when}", e.1).into();
        let note: SharedString = entry_tip(forward, &e.1, &steps).into();
        let id = (
            if forward { "redo-entry" } else { "undo-entry" },
            e.0 as usize,
        );
        PopupMenuItem::element(move |_, _| {
            let note = note.clone();
            div()
                .id(id)
                .flex_1()
                .child(text.clone())
                .tooltip(move |window, cx| Tooltip::new(note.clone()).build(window, cx))
        })
        .icon(Icon::new(if forward {
            IconName::Redo2
        } else {
            IconName::Undo2
        }))
        .disabled(disabled)
        .on_click(move |_, window, cx| {
            if let Some(s) = state.upgrade() {
                ops::travel(&s, forward, steps.clone(), window, cx);
            }
        })
    };
    let steps = |list: &[Entry], upto: usize| {
        list[..=upto]
            .iter()
            .map(|e| (e.0, e.3.clone()))
            .collect::<Vec<_>>()
    };
    for (i, e) in redos.iter().enumerate().rev() {
        menu = menu.item(item(e, true, steps(redos, i), blocked.is_some()));
    }
    if !redos.is_empty() && !undos.is_empty() {
        menu = menu.label("— Now —");
    }
    for (i, e) in undos.iter().enumerate() {
        menu = menu.item(item(e, false, steps(undos, i), blocked.is_some()));
    }
    if let Some(b) = blocked {
        menu = menu.separator().label(b);
    }
    menu
}

/// A history entry's tooltip: how far picking it goes, and the branch it lands on if it switches.
fn entry_tip(forward: bool, label: &str, steps: &[(u64, Option<String>)]) -> String {
    let to = ops::landing(&steps.iter().map(|s| s.1.clone()).collect::<Vec<_>>());
    let what = if forward {
        format!("Redo up to {label}")
    } else {
        format!("Undo back to before {label}")
    };
    what + &ops::switch_note(forward, to.as_deref())
}

fn tracked(st: &git::RepoStatus) -> bool {
    st.upstream.is_some() && !st.upstream_gone
}

fn pull_button(state: &Entity<RepoState>, cx: &mut App) -> AnyElement {
    let t = cx.app_theme().clone();
    let s = state.read(cx);
    let busy = s.busy().is_some();
    let status = s.status.clone();
    // A fork's pull request checked out pulls from its refs/pull/<n>/head.
    let pullable = status
        .as_ref()
        .is_some_and(|st| tracked(st) || st.follows.is_some());
    let gone = status.as_ref().filter(|st| st.upstream_gone).map(|st| {
        format!(
            "{} isn't on the remote (deleted, or never pushed)",
            st.upstream.clone().unwrap_or_default()
        )
    });
    let behind = status.as_ref().map_or(0, |st| st.behind);
    let st1 = state.clone();
    let button = Button::new("pull-main")
        .small()
        .icon(IconName::ArrowDownToLine)
        .child(
            div()
                .flex()
                .items_center()
                .gap_1()
                .child("Pull")
                .when(behind > 0, |d| {
                    d.child(
                        div()
                            .font_family(t.fonts.code.clone())
                            .text_size(rems(0.65625))
                            .text_color(t.primary)
                            .child(behind.to_string()),
                    )
                }),
        )
        .tooltip(gone.unwrap_or_else(|| tip("Pull (fast-forward only)", "git.pull", cx)))
        .on_click(move |_, window, cx| ops::pull(&st1, git::PullMode::Ff, false, window, cx));
    let st2 = state.clone();
    DropdownButton::new("pull")
        .small()
        .disabled(busy || !pullable)
        .button(button)
        .dropdown_menu(move |menu, _, _| {
            let (a, b) = (st2.downgrade(), st2.downgrade());
            menu.label("When branches have diverged")
                .item(
                    PopupMenuItem::new("Pull with merge")
                        .icon(Icon::new(IconName::GitMerge))
                        .on_click(move |_, window, cx| {
                            if let Some(s) = a.upgrade() {
                                ops::pull(&s, git::PullMode::Merge, false, window, cx)
                            }
                        }),
                )
                .item(
                    PopupMenuItem::new("Pull with rebase")
                        .icon(Icon::new(IconName::GitPullRequestArrow))
                        .on_click(move |_, window, cx| {
                            if let Some(s) = b.upgrade() {
                                ops::pull(&s, git::PullMode::Rebase, false, window, cx)
                            }
                        }),
                )
        })
        .into_any_element()
}

fn push_button(state: &Entity<RepoState>, cx: &mut App) -> AnyElement {
    let t = cx.app_theme().clone();
    let s = state.read(cx);
    let busy = s.busy().is_some();
    let Some(status) = s.status.clone() else {
        return Button::new("push-main")
            .small()
            .icon(IconName::ArrowUpFromLine)
            .label("Push")
            .disabled(true)
            .into_any_element();
    };
    if tracked(&status) {
        // Where the push lands, which a fork can set apart from where it pulls. Unknown until
        // the push target has the branch; then a push is due.
        let ahead = match &status.push {
            Some(p) => p.branch.as_ref().map(|_| p.ahead),
            None => Some(status.ahead),
        };
        let label = match status.push.as_ref() {
            Some(p) => match &p.branch {
                Some(b) => format!("Push to {b}"),
                None => format!("Push to {} (creates the branch there)", p.remote),
            },
            None => format!("Push to {}", status.upstream.clone().unwrap_or_default()),
        };
        let due = ahead != Some(0);
        let st1 = state.clone();
        let button = Button::new("push-main")
            .small()
            .icon(IconName::ArrowUpFromLine)
            .child(div().flex().items_center().gap_1().child("Push").when(
                ahead.is_some_and(|a| a > 0),
                |d| {
                    d.child(
                        div()
                            .font_family(t.fonts.code.clone())
                            .text_size(rems(0.65625))
                            .child(ahead.unwrap_or(0).to_string()),
                    )
                },
            ))
            .tooltip(tip(&label, "git.push", cx))
            .on_click(move |_, window, cx| ops::push(&st1, false, false, window, cx));
        let st2 = state.clone();
        let mut split = DropdownButton::new("push")
            .small()
            .disabled(busy)
            .button(button)
            .dropdown_menu(move |menu, window, cx| push_menu(menu, &st2, window, cx));
        if due {
            split = split.primary();
        }
        return split.into_any_element();
    }
    if let Some(f) = &status.follows {
        return Button::new("push-main")
            .small()
            .icon(IconName::ArrowUpFromLine)
            .label("Push")
            .tooltip(format!(
                "Pull request {f}'s branch is on its author's fork; push there"
            ))
            .disabled(true)
            .into_any_element();
    }
    publish_button(state, &status, busy)
}

/// Push with tags, and the local tags the remote lacks: which those are takes a network call,
/// so it is asked only as the menu opens.
fn push_menu(
    menu: PopupMenu,
    state: &Entity<RepoState>,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let weak = state.downgrade();
    let st = state.downgrade();
    let menu = menu
        .min_w(px(288.))
        .item(
            PopupMenuItem::new("Push with tags")
                .icon(Icon::new(IconName::CloudUpload))
                .on_click(move |_, window, cx| {
                    if let Some(s) = st.upgrade() {
                        ops::push(&s, true, false, window, cx);
                    }
                }),
        )
        .label("Also sends annotated tags on the pushed commits (--follow-tags).")
        .separator()
        .label("Local tags");
    let root = state.read(cx).root().clone();
    let there = remote_tags::lookup(&root, cx);
    let core = gitviber_ui::core(cx);
    let local = cx.background_spawn(async move { api::branches::tags(&core) });
    cx.spawn_in(window, async move |menu, cx| {
        let there = there.await;
        // Thousands of tags on either side: not on the UI thread.
        let read = cx
            .background_spawn(async move {
                let (remote, there) = there?;
                let there: HashSet<String> = there.into_iter().collect();
                let lacking: Vec<String> = local
                    .await?
                    .into_iter()
                    .filter(|t| !there.contains(t))
                    .collect();
                Ok::<_, String>((remote, lacking))
            })
            .await;
        let _ = menu.update_in(cx, |menu, window, cx| {
            let weak = weak.clone();
            menu.rebuild(window, cx, move |m, _, _| {
                let m = m.min_w(px(288.)).label("Local tags");
                match read {
                    Err(e) => m.label(format!("Couldn't compare tags with the remote: {e}")),
                    Ok((remote, lacking)) if lacking.is_empty() => {
                        m.label(format!("{remote} has every local tag."))
                    }
                    Ok((remote, lacking)) => {
                        let shown = lacking
                            .iter()
                            .take(3)
                            .cloned()
                            .collect::<Vec<_>>()
                            .join(", ")
                            + &if lacking.len() > 3 {
                                format!(" +{}", lacking.len() - 3)
                            } else {
                                String::new()
                            };
                        let n = lacking.len();
                        m.item(
                            PopupMenuItem::new(format!(
                                "Push {} {remote} lacks: {shown}",
                                if n == 1 {
                                    "1 tag".to_string()
                                } else {
                                    format!("{n} tags")
                                }
                            ))
                            .icon(Icon::new(IconName::Tag))
                            .on_click(move |_, window, cx| {
                                if let Some(s) = weak.upgrade() {
                                    ops::push_tags(&s, lacking.clone(), remote.clone(), window, cx);
                                }
                            }),
                        )
                    }
                }
            });
        });
    })
    .detach();
    menu.label("Checking the remote…")
}

/// Publishes to the preferred remote; with several remotes, any can be picked.
fn publish_button(state: &Entity<RepoState>, status: &git::RepoStatus, busy: bool) -> AnyElement {
    let remotes = status.remotes.clone();
    // Before the first commit there's nothing to push.
    let disabled = busy || status.branch.is_none() || status.head.is_none();
    if remotes.is_empty() {
        return Button::new("publish")
            .small()
            .icon(IconName::CloudUpload)
            .label("Publish")
            .tooltip(
                "This repository has no remote. Add one (git remote add origin <url>) to publish.",
            )
            .disabled(true)
            .into_any_element();
    }
    let preferred = status.publish.clone();
    let menu = {
        let (st, remotes, preferred) = (state.downgrade(), remotes.clone(), preferred.clone());
        move |mut menu: PopupMenu, _: &mut Window, _: &mut Context<PopupMenu>| {
            menu = menu.label("Publish to");
            for r in &remotes {
                let (st, r) = (st.clone(), r.clone());
                menu = menu.item(
                    PopupMenuItem::new(r.clone())
                        .icon(Icon::new(IconName::CloudUpload))
                        .checked(Some(&r) == preferred.as_ref())
                        .on_click(move |_, window, cx| {
                            if let Some(s) = st.upgrade() {
                                ops::publish(&s, r.clone(), window, cx);
                            }
                        }),
                );
            }
            menu
        }
    };
    match preferred {
        None => Button::new("publish")
            .small()
            .primary()
            .icon(IconName::CloudUpload)
            .label("Publish")
            .dropdown_caret(true)
            .tooltip("Choose a remote to push this branch to and track it")
            .disabled(disabled)
            .dropdown_menu(menu)
            .into_any_element(),
        Some(p) => {
            let st = state.clone();
            let remote = p.clone();
            let button = Button::new("publish-main")
                .small()
                .icon(IconName::CloudUpload)
                .label("Publish")
                .tooltip(format!("Push this branch to {p} and track it"))
                .on_click(move |_, window, cx| ops::publish(&st, remote.clone(), window, cx));
            if remotes.len() > 1 {
                DropdownButton::new("publish")
                    .small()
                    .primary()
                    .disabled(disabled)
                    .button(button)
                    .dropdown_menu(menu)
                    .into_any_element()
            } else {
                button.primary().disabled(disabled).into_any_element()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::entry_tip;

    #[test]
    fn a_history_entry_says_how_far_it_goes_and_where_it_lands() {
        let steps = vec![(2, Some("main".to_string())), (1, None)];
        assert_eq!(
            entry_tip(false, "Commit", &steps),
            "Undo back to before Commit: switches back to main"
        );
        assert_eq!(entry_tip(true, "Commit", &[(1, None)]), "Redo up to Commit");
    }
}
