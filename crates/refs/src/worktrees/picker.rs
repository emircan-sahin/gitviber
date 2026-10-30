//! `git worktree list` as a switcher in the top bar. Always shown, even with only the main
//! worktree, so the feature is found at all. Rows lead with the branch, the name people know a
//! worktree by; the folder comes second. Each one's changes are counted as the picker opens,
//! a few at a time, never before.

use std::collections::HashMap;

use gitviber_core::api;
use gitviber_ui::keymap::worktree as wt_actions;
use gitviber_ui::memory::Live;
use gitviber_ui::widgets::{
    Ends, StatusDot, list_step, now_unix, relative_time, repaint_next_minute,
};
use gitviber_ui::workspace::PanelContext;
use gitviber_ui::{ActiveAppTheme, Project};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenuItem};
use gpui_kit::component::popover::Popover;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::{
    Anchor, AnyElement, AnyWindowHandle, App, AppContext as _, ClipboardItem, Context, Entity,
    FocusHandle, FontWeight, Hsla, InteractiveElement as _, IntoElement, KeyDownEvent,
    ParentElement as _, Render, SharedString, StatefulInteractiveElement as _, Styled as _,
    Subscription, Task, Window, div, prelude::FluentBuilder as _, px, rems,
};

use super::{
    Cleanable, HUES, MergedPull, State, Worktree, WorktreeEvent, WorktreeEvents, cleanable,
    hue_color, hue_name, list as wt_list, main_back_offer, short_path, worktree_hues,
    worktree_name,
};
use crate::hooks::{self, BranchPull, PullState};
use crate::names::{folder_name, plural};
use crate::ops;
use crate::repo::{RepoEvent, RepoState};
use crate::requests::{self, Picker};
use crate::ui::{chip_on, key_hints};

/// The top bar's worktree chip, kept in the bar's element state per project.
pub fn worktree_chip(ctx: &PanelContext, window: &mut Window, cx: &mut App) -> Option<AnyElement> {
    let project = ctx.project.clone()?;
    let id = project.entity_id();
    let chip = window.use_keyed_state(
        ("refs-worktree-chip", id.as_u64() as usize),
        cx,
        |window, cx| WorktreeChip::new(&project, window, cx),
    );
    Some(chip.into_any_element())
}

pub struct WorktreeChip {
    state: Entity<RepoState>,
    window: AnyWindowHandle,
    picker: Option<Entity<WorktreePicker>>,
    _subs: Vec<Subscription>,
    _live: Live,
}

impl WorktreeChip {
    fn new(project: &Entity<Project>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let state = RepoState::get(project, cx);
        let subs = vec![
            cx.observe(&state, |_, _, cx| cx.notify()),
            cx.observe_global::<crate::store::Saved>(|_, cx| cx.notify()),
            cx.observe_global::<hooks::AgentLooks>(|_, cx| cx.notify()),
        ];
        WorktreeChip {
            state,
            window: window.window_handle(),
            picker: None,
            _subs: subs,
            _live: Live::new("WorktreeChip"),
        }
    }

    fn set_open(&mut self, open: bool, window: &mut Window, cx: &mut Context<Self>) {
        if open == self.picker.is_some() {
            return;
        }
        self.picker = open.then(|| {
            let state = self.state.clone();
            let chip = cx.weak_entity();
            cx.new(|cx| WorktreePicker::new(state, Some(chip), window, cx))
        });
        cx.notify();
    }

    pub(crate) fn close(&mut self, cx: &mut Context<Self>) {
        self.picker = None;
        cx.notify();
    }
}

impl Render for WorktreeChip {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if requests::take(Picker::Worktrees, self.window, cx) {
            self.set_open(true, window, cx);
        }
        let t = cx.app_theme().clone();
        let s = self.state.read(cx);
        let all = wt_list(&s.worktrees);
        let current = all.iter().find(|w| w.current).cloned();
        let linked = current.as_ref().is_some_and(|w| !w.main);
        let extra = all.iter().filter(|w| !w.main).count();
        let picked = crate::store::colors(cx);
        let hues = worktree_hues(
            &all.iter()
                .map(|w| (w.path.clone(), w.main))
                .collect::<Vec<_>>(),
            &picked,
        );
        let hue = current
            .as_ref()
            .and_then(|w| hues.get(&w.path).copied().flatten());
        let tint = |a: f32| hue.map(|h| hue_color(h, t.tint_l, t.tint_c, a));
        let label: AnyElement = match current.as_ref().filter(|_| linked) {
            Some(w) => {
                let (branch, folder) = worktree_name(w);
                div()
                    .flex()
                    .items_baseline()
                    .gap_1p5()
                    .min_w_0()
                    .child(
                        div()
                            .truncate()
                            .font_family(t.fonts.code.clone())
                            .text_size(rems(0.75))
                            .child(branch),
                    )
                    .children(folder.map(|f| {
                        div()
                            .truncate()
                            .font_family(t.fonts.code.clone())
                            .text_size(rems(0.65625))
                            .text_color(t.muted_foreground)
                            .child(f)
                    }))
                    .into_any_element()
            }
            None => div()
                .flex()
                .text_size(rems(0.75))
                .child("Worktrees")
                .when(extra > 0, |d| {
                    d.child(
                        div()
                            .text_color(t.muted_foreground)
                            .child(format!(" ({})", all.len())),
                    )
                })
                .into_any_element(),
        };
        let tip = match current.as_ref().filter(|_| linked) {
            Some(w) => format!(
                "{} in worktree {} ({}) · switch worktree",
                worktree_name(w).0,
                folder_name(&w.path),
                w.path
            ),
            None if extra == 0 => "Worktrees".to_string(),
            None => format!("{} · switch worktree", plural(all.len(), "worktree")),
        };
        // Another worktree's agent asked or finished unseen: the chip says so, as the Dock does.
        let away = hooks::most_urgent(
            all.iter()
                .filter(|w| !w.current)
                .map(|w| hooks::agents_in(&w.path, cx)),
        );
        let away = hooks::is_news(away).then_some(away).flatten();
        let open = self.picker.is_some();
        let picker = self.picker.clone();
        let focus = picker.as_ref().map(|p| p.read(cx).focus.clone());
        let mut trigger =
            Button::new("worktree-chip")
                .ghost()
                .small()
                .tooltip(tip)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1p5()
                        .max_w(px(224.))
                        .child(Icon::new(IconName::FolderGit2).small().text_color(
                            tint(1.).unwrap_or(if linked { t.primary } else { t.subtle }),
                        ))
                        .child(label)
                        .child(
                            Icon::new(IconName::ChevronsUpDown)
                                .xsmall()
                                .text_color(t.subtle),
                        )
                        .children(away.map(StatusDot)),
                );
        if let Some(bg) = tint(0.16).or(linked.then(|| t.primary.opacity(0.1))) {
            trigger = trigger.bg(bg);
        }
        let mut popover = Popover::new("worktree-picker")
            .anchor(Anchor::TopLeft)
            .open(open)
            .on_open_change(
                cx.listener(|this, open: &bool, window, cx| this.set_open(*open, window, cx)),
            )
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
        div().flex().flex_none().items_center().child(popover)
    }
}

/// A branch's PR: its state, number and checks; opens it in the app.
/// A pull request's state as its icon.
fn pull_icon(state: PullState) -> IconName {
    match state {
        PullState::Open => IconName::GitPullRequest,
        PullState::Draft => IconName::GitPullRequestDraft,
        PullState::Closed => IconName::GitPullRequestClosed,
        PullState::Merged => IconName::GitMerge,
    }
}

pub fn pull_chip(p: &BranchPull, hot: bool, cx: &App) -> AnyElement {
    let t = cx.app_theme();
    let (icon, color) = match p.state {
        PullState::Open => (IconName::GitPullRequest, t.added),
        PullState::Draft => (IconName::GitPullRequestDraft, t.subtle),
        PullState::Closed => (IconName::GitPullRequestClosed, t.removed),
        PullState::Merged => (IconName::GitMerge, t.renamed),
    };
    let checks = p.checks.map(|c| match c {
        hooks::Checks::Pending => (IconName::CircleDot, t.modified),
        hooks::Checks::Passed => (IconName::CircleCheck, t.added),
        hooks::Checks::Failed => (IconName::CircleX, t.removed),
    });
    let url = p.url.clone();
    div()
        .id(SharedString::from(format!("pull-chip-{}", p.number)))
        .flex()
        .flex_none()
        .items_center()
        .gap_1()
        .h(px(20.))
        .px_1()
        .rounded(px(3.))
        .font_family(t.fonts.code.clone())
        .text_size(rems(0.65625))
        .text_color(if hot {
            t.primary_foreground
        } else {
            t.muted_foreground
        })
        .hover(|s| s.bg(t.hover))
        .cursor_pointer()
        .child(
            Icon::new(icon)
                .xsmall()
                .text_color(if hot { t.primary_foreground } else { color }),
        )
        .child(format!("#{}", p.number))
        .children(checks.map(|(i, c)| {
            Icon::new(i)
                .xsmall()
                .text_color(if hot { t.primary_foreground } else { c })
        }))
        .on_click(move |_, window, cx| {
            cx.stop_propagation();
            hooks::open_pull(&url, window, cx)
        })
        .into_any_element()
}

pub struct WorktreePicker {
    state: Entity<RepoState>,
    chip: Option<gpui_kit::WeakEntity<WorktreeChip>>,
    list: Vec<Worktree>,
    states: HashMap<String, State>,
    index: Option<usize>,
    /// The list's: ↑/↓ keep the highlighted row in view, ⇧F10 finds it for its menu.
    scroll: gpui_kit::ScrollHandle,
    focus: FocusHandle,
    _read: Task<()>,
    _subs: Vec<Subscription>,
    _live: Live,
}

impl WorktreePicker {
    /// The picker's list; `chip`: the top bar's, which closes it.
    pub fn new(
        state: Entity<RepoState>,
        chip: Option<gpui_kit::WeakEntity<WorktreeChip>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let list = wt_list(&state.read(cx).worktrees);
        let index = list.iter().position(|w| w.current).or(Some(0));
        let core = gitviber_ui::core(cx);
        // Fresh on open: whether a lock's session still runs is only asked here.
        let read = cx.spawn(async move |this, cx| {
            let c = core.clone();
            let Ok(fresh) = cx
                .background_spawn(async move { api::worktrees::worktrees(&c) })
                .await
            else {
                return;
            };
            let fresh: Vec<Worktree> = fresh.iter().map(Worktree::from).collect();
            let todo: Vec<String> = fresh
                .iter()
                .filter(|w| !w.prunable && !w.bare)
                .map(|w| w.path.clone())
                .collect();
            if this
                .update(cx, |this, cx| {
                    this.list = fresh;
                    cx.notify();
                })
                .is_err()
            {
                return;
            }
            // A few at a time: a dozen agent worktrees shouldn't mean a dozen `git status` at once.
            for chunk in todo.chunks(3) {
                let reads: Vec<_> = chunk
                    .iter()
                    .map(|p| {
                        let (c, p) = (core.clone(), p.clone());
                        cx.background_spawn(async move {
                            (p.clone(), api::worktrees::worktree_state(&c, p, true))
                        })
                    })
                    .collect();
                for r in reads {
                    let (path, s) = r.await;
                    if let Ok(s) = s
                        && this
                            .update(cx, |this, cx| {
                                this.states.insert(path, State::from(s));
                                cx.notify();
                            })
                            .is_err()
                    {
                        return;
                    }
                }
            }
        });
        let subs = vec![
            cx.subscribe(&state, |this, state, e: &RepoEvent, cx| {
                if *e == RepoEvent::Changed {
                    this.list = wt_list(&state.read(cx).worktrees);
                    cx.notify();
                }
            }),
            cx.observe_global::<hooks::AgentLooks>(|_, cx| cx.notify()),
        ];
        WorktreePicker {
            state,
            chip,
            list,
            states: HashMap::new(),
            index,
            scroll: gpui_kit::ScrollHandle::new(),
            focus: cx.focus_handle(),
            _read: read,
            _subs: subs,
            _live: Live::new("WorktreePicker"),
        }
    }

    /// What opening reads, now, on this thread: for a demo's first frame. The UI never calls it.
    #[doc(hidden)]
    pub fn load_now(&mut self, cx: &mut Context<Self>) {
        let core = gitviber_ui::core(cx);
        if let Ok(fresh) = api::worktrees::worktrees(&core) {
            self.list = fresh.iter().map(Worktree::from).collect();
        }
        for w in self.list.iter().filter(|w| !w.prunable && !w.bare) {
            if let Ok(s) = api::worktrees::worktree_state(&core, w.path.clone(), true) {
                self.states.insert(w.path.clone(), State::from(s));
            }
        }
        cx.notify();
    }

    fn close(&mut self, cx: &mut Context<Self>) {
        if let Some(chip) = &self.chip {
            let _ = chip.update(cx, |c, cx| c.close(cx));
        }
    }

    fn hot(&self) -> Option<Worktree> {
        self.index.and_then(|i| self.list.get(i)).cloned()
    }

    fn usable(w: &Worktree) -> bool {
        !w.current && !w.prunable && !w.bare
    }

    fn can_merge(&self, w: &Worktree) -> bool {
        let into = self
            .list
            .iter()
            .find(|x| x.current)
            .and_then(|c| c.branch.clone());
        into.is_some()
            && w.branch.is_some()
            && !w.current
            && self.states.get(&w.path).is_some_and(|s| s.commits > 0)
    }

    fn act(&mut self, what: Act, w: Worktree, window: &mut Window, cx: &mut Context<Self>) {
        self.close(cx);
        let state = self.state.clone();
        match what {
            Act::Open => ops::open_worktree(w.path, window, cx),
            Act::Terminal => WorktreeEvents::emit(WorktreeEvent::OpenTerminal { path: w.path }, cx),
            Act::Resume => {
                WorktreeEvents::emit(WorktreeEvent::ResumeConversation { path: w.path }, cx)
            }
            Act::Merge => {
                if let Some(b) = w.branch {
                    ops::merge(
                        &state,
                        b,
                        gitviber_core::git::MergeKind::Ff,
                        false,
                        window,
                        cx,
                    )
                }
            }
            Act::Rename => super::open_rename(w, window, cx),
            Act::Lock if w.locked => ops::unlock_worktree(&state, w, window, cx),
            Act::Lock => super::open_lock(w, window, cx),
            Act::Remove => ops::remove_worktree(&state, w, None, window, cx),
            Act::Reveal => {
                let core = gitviber_ui::core(cx);
                let path = w.path.clone();
                cx.background_spawn(async move { api::files::reveal_path(&core, path) })
                    .detach();
            }
            Act::Copy => cx.write_to_clipboard(ClipboardItem::new_string(w.path)),
        }
    }

    fn on_key(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &e.keystroke;
        if crate::ui::is_menu_key(k) {
            if let Some(row) = self.index.and_then(|i| self.scroll.bounds_for_item(i)) {
                crate::ui::open_row_menu(row, window, cx);
            }
            cx.stop_propagation();
            return;
        }
        if k.modifiers.platform || k.modifiers.control || k.modifiers.alt {
            return;
        }
        if let Some(to) = list_step(k, self.index, self.list.len(), Ends::Wrap, false) {
            self.index = Some(to);
            self.scroll.scroll_to_item(to);
        } else {
            match k.key.as_str() {
                "enter" => {
                    if let Some(w) = self.hot().filter(Self::usable) {
                        self.act(Act::Open, w, window, cx);
                    }
                }
                "escape" => self.close(cx),
                _ => return,
            }
        }
        cx.stop_propagation();
        cx.notify();
    }

    fn row(
        &self,
        i: usize,
        w: &Worktree,
        main: &str,
        hues: &HashMap<String, Option<usize>>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = cx.app_theme().clone();
        let hot = self.index == Some(i);
        let usable = Self::usable(w);
        let hue = hues.get(&w.path).copied().flatten();
        let color = hue
            .filter(|_| !hot)
            .map(|h| hue_color(h, t.tint_l, t.tint_c, 1.));
        let (branch, _) = worktree_name(w);
        let look = hooks::agents_in(&w.path, cx);
        let state = self.states.get(&w.path).copied();
        let pull = w
            .branch
            .as_deref()
            .and_then(|b| hooks::pull_of(b, w.head.as_deref(), true, cx));
        let time = state.and_then(|s| s.updated.map(|u| u as i64)).or_else(|| {
            self.state
                .read(cx)
                .branches
                .iter()
                .find(|b| !b.remote && Some(&b.name) == w.branch.as_ref())
                .map(|b| b.timestamp)
        });
        let dim = if hot {
            t.primary_foreground.opacity(0.8)
        } else {
            t.subtle
        };
        let right: AnyElement = if hot && !w.bare {
            self.row_actions(w, cx)
        } else {
            let when = if w.prunable {
                "missing".to_string()
            } else if w.bare {
                String::new()
            } else if let Some(time) = time {
                relative_time(time, now_unix())
            } else if w.current {
                "current".into()
            } else {
                String::new()
            };
            div()
                .flex()
                .flex_col()
                .items_end()
                .max_w(px(144.))
                .text_size(rems(0.65625))
                .text_color(dim)
                .child(div().truncate().child(when))
                .children(state.map(|s| state_label(&s, hot, cx)))
                .into_any_element()
        };
        let w2 = w.clone();
        let menu_w = w.clone();
        let menu_pull = pull.clone();
        let menu_hue = hue;
        let weak = cx.weak_entity();
        let can_merge = self.can_merge(w);
        let into = self
            .list
            .iter()
            .find(|x| x.current)
            .and_then(|c| c.branch.clone())
            .unwrap_or_default();
        let path_text = if w.main {
            folder_name(&w.path).to_string()
        } else {
            short_path(&w.path, main)
        };
        div()
            .id(("worktree-row", i))
            .flex()
            .items_center()
            .gap_2()
            .h(px(36.))
            .px_2()
            .rounded(px(3.))
            .when(hot, |d| d.bg(t.primary).text_color(t.primary_foreground))
            .when(w.prunable || w.bare, |d| d.opacity(0.5))
            .when(usable, |d| d.cursor_pointer())
            .on_mouse_move(cx.listener(move |this, _, _, cx| {
                if this.index != Some(i) {
                    this.index = Some(i);
                    cx.notify();
                }
            }))
            .on_click(cx.listener(move |this, _, window, cx| {
                if Self::usable(&w2) {
                    this.act(Act::Open, w2.clone(), window, cx);
                }
            }))
            .child(
                Icon::new(if w.current {
                    IconName::Check
                } else {
                    IconName::GitBranch
                })
                .small()
                .text_color(color.unwrap_or(if hot {
                    t.primary_foreground
                } else {
                    t.muted_foreground
                })),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1p5()
                            .min_w_0()
                            .child(
                                div()
                                    .truncate()
                                    .font_family(t.fonts.code.clone())
                                    .text_size(rems(0.71875))
                                    .when(w.branch.is_none(), |d| d.opacity(0.7))
                                    .child(branch),
                            )
                            .when(w.main, |d| d.child(chip_on("main folder", false, hot, cx)))
                            .when(w.in_use, |d| d.child(chip_on("working", true, hot, cx)))
                            .when(w.locked && !w.in_use, |d| {
                                d.child(Icon::new(IconName::Lock).xsmall().text_color(dim))
                            })
                            .children(pull.map(|p| pull_chip(&p, hot, cx)))
                            .children(look.map(|l| {
                                let tip: SharedString =
                                    format!("A terminal here: {}", hooks::look_label(l)).into();
                                div()
                                    .id(("worktree-look", i))
                                    .when(hot, |d| d.text_color(t.primary_foreground))
                                    .child(StatusDot(l))
                                    .tooltip(move |w, cx| Tooltip::new(tip.clone()).build(w, cx))
                            })),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .min_w_0()
                            .text_size(rems(0.65625))
                            .text_color(dim)
                            .child(Icon::new(IconName::FolderGit2).xsmall())
                            .child(div().truncate().child(path_text)),
                    ),
            )
            .child(right)
            .context_menu(move |menu, _, _| {
                let w = menu_w.clone();
                let on_disk = !w.prunable && !w.bare;
                let item = |label: &str, icon: IconName, what: Act| {
                    let (weak, w) = (weak.clone(), w.clone());
                    PopupMenuItem::new(label.to_string())
                        .icon(Icon::new(icon))
                        .on_click(move |_, window, cx| {
                            let _ =
                                weak.update(cx, |this, cx| this.act(what, w.clone(), window, cx));
                        })
                };
                let mut menu = menu;
                if Self::usable(&w) {
                    menu = menu.item(item("Open here", IconName::FolderGit2, Act::Open));
                }
                if on_disk {
                    menu = menu
                        .item(item(
                            "Open a terminal here",
                            IconName::SquareTerminal,
                            Act::Terminal,
                        ))
                        .item(item(
                            "Resume a conversation…",
                            IconName::MessageCircleReply,
                            Act::Resume,
                        ));
                }
                if let Some(p) = &menu_pull {
                    let (weak, url) = (weak.clone(), p.url.clone());
                    menu = menu.item(
                        PopupMenuItem::new(format!("Open pull request #{}", p.number))
                            .icon(Icon::new(pull_icon(p.state)))
                            .on_click(move |_, window, cx| {
                                let _ = weak.update(cx, |this, cx| this.close(cx));
                                hooks::open_pull(&url, window, cx);
                            }),
                    );
                }
                if can_merge {
                    menu = menu.item(item(
                        &format!("Merge into {into}"),
                        IconName::GitMerge,
                        Act::Merge,
                    ));
                }
                if w.branch.is_some() && !w.prunable {
                    menu = menu.item(item("Rename…", IconName::Pencil, Act::Rename));
                }
                if !w.main && !w.bare {
                    menu = menu.item(item(
                        if w.locked { "Unlock" } else { "Lock…" },
                        if w.locked {
                            IconName::LockOpen
                        } else {
                            IconName::Lock
                        },
                        Act::Lock,
                    ));
                }
                if !w.bare {
                    let path = w.path.clone();
                    menu = menu.separator().label("Color");
                    for (h, (name, _)) in HUES.iter().enumerate() {
                        let path = path.clone();
                        let label = format!("{}{}", name[..1].to_uppercase(), &name[1..]);
                        menu = menu.item(
                            PopupMenuItem::new(label)
                                .checked(menu_hue == Some(h))
                                .on_click(move |_, _, cx| {
                                    crate::store::set_color(&path, Some(hue_name(h)), cx);
                                }),
                        );
                    }
                    menu = menu.item(
                        PopupMenuItem::new("None")
                            .checked(menu_hue.is_none())
                            .on_click(move |_, _, cx| {
                                crate::store::set_color(&path, Some("none"), cx)
                            }),
                    );
                }
                menu = menu.separator();
                if on_disk {
                    menu = menu.item(item(
                        if cfg!(target_os = "macos") {
                            "Reveal in Finder"
                        } else {
                            "Show in Folder"
                        },
                        IconName::FolderOpen,
                        Act::Reveal,
                    ));
                }
                menu = menu.item(item("Copy path", IconName::Copy, Act::Copy));
                if !w.main && !w.current {
                    menu = menu.separator().item(item(
                        if w.prunable {
                            "Prune"
                        } else {
                            "Remove worktree…"
                        },
                        if w.prunable {
                            IconName::Eraser
                        } else {
                            IconName::Trash
                        },
                        Act::Remove,
                    ));
                }
                menu
            })
            .into_any_element()
    }

    /// The hot row's buttons, which the mouse finds on the row.
    fn row_actions(&self, w: &Worktree, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.app_theme().clone();
        let mut row = div().flex().flex_none().gap_0p5();
        let button = |id: &'static str,
                      icon: IconName,
                      tip: String,
                      what: Act,
                      w: &Worktree,
                      cx: &mut Context<Self>| {
            let w = w.clone();
            Button::new(id)
                .ghost()
                .xsmall()
                .icon(Icon::new(icon).text_color(t.primary_foreground))
                .tooltip(tip)
                .on_click(cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    this.act(what, w.clone(), window, cx);
                }))
        };
        if !w.prunable && !w.bare {
            row = row.child(button(
                "wt-act-terminal",
                IconName::SquareTerminal,
                "Open a terminal here".into(),
                Act::Terminal,
                w,
                cx,
            ));
        }
        if self.can_merge(w) {
            let into = self
                .list
                .iter()
                .find(|x| x.current)
                .and_then(|c| c.branch.clone())
                .unwrap_or_default();
            let left = self.states.get(&w.path).map_or(0, |s| s.uncommitted);
            let tip = if left > 0 {
                format!(
                    "Merge into {into} · its {left} uncommitted {} behind",
                    if left == 1 {
                        "change stays"
                    } else {
                        "changes stay"
                    }
                )
            } else {
                format!("Merge into {into}")
            };
            row = row.child(button(
                "wt-act-merge",
                IconName::GitMerge,
                tip,
                Act::Merge,
                w,
                cx,
            ));
        }
        if w.branch.is_some() && !w.prunable {
            row = row.child(button(
                "wt-act-rename",
                IconName::Pencil,
                "Rename…".into(),
                Act::Rename,
                w,
                cx,
            ));
        }
        if !w.main && !w.bare {
            let tip = if w.locked {
                format!(
                    "Unlock{}",
                    w.lock_reason
                        .as_deref()
                        .map(|r| format!(" ({r})"))
                        .unwrap_or_default()
                )
            } else {
                "Lock: keep it from being pruned, moved or removed…".into()
            };
            row = row.child(button(
                "wt-act-lock",
                if w.locked {
                    IconName::LockOpen
                } else {
                    IconName::Lock
                },
                tip,
                Act::Lock,
                w,
                cx,
            ));
        }
        if !w.main && !w.current {
            let (icon, tip) = if w.prunable {
                (
                    IconName::Eraser,
                    "Prune: its folder is gone, drop it from the list",
                )
            } else {
                (IconName::Trash, "Remove worktree…")
            };
            row = row.child(button(
                "wt-act-remove",
                icon,
                tip.into(),
                Act::Remove,
                w,
                cx,
            ));
        }
        row.into_any_element()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Act {
    Open,
    Terminal,
    Resume,
    Merge,
    Rename,
    Lock,
    Remove,
    Reveal,
    Copy,
}

/// Uncommitted files and unmerged commits side by side; "merged" or "no changes" only when neither.
fn state_label(s: &State, hot: bool, cx: &App) -> AnyElement {
    let t = cx.app_theme();
    let mut parts: Vec<(String, Hsla)> = vec![];
    if s.uncommitted > 0 {
        parts.push((plural(s.uncommitted as usize, "change"), t.removed));
    }
    if s.commits > 0 {
        parts.push((plural(s.commits as usize, "commit"), t.added));
    }
    if parts.is_empty() {
        parts.push(if s.merged {
            ("merged".into(), t.renamed)
        } else {
            ("no changes".into(), t.subtle)
        });
    }
    let mut row = div().flex().truncate();
    for (i, (text, tone)) in parts.into_iter().enumerate() {
        let text = if i > 0 { format!(" · {text}") } else { text };
        row = row.child(div().when(!hot, |d| d.text_color(tone)).child(text));
    }
    row.into_any_element()
}

impl Render for WorktreePicker {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The rows' "3h ago" stay current while it's open.
        repaint_next_minute(window, cx);
        let t = cx.app_theme().clone();
        let main = self
            .list
            .iter()
            .find(|w| w.main && !w.bare)
            .map(|w| w.path.clone())
            .unwrap_or_default();
        let picked = crate::store::colors(cx);
        let hues = worktree_hues(
            &self
                .list
                .iter()
                .map(|w| (w.path.clone(), w.main))
                .collect::<Vec<_>>(),
            &picked,
        );
        let branches = self.state.read(cx).branches.clone();
        let offer = main_back_offer(&self.list, &branches);
        let merged: Vec<Cleanable> = cleanable(
            &self.list,
            &self.states,
            |b| {
                let p = hooks::pull_of(b?, None, false, cx)?;
                Some(MergedPull {
                    number: p.number,
                    merged: p.state == PullState::Merged,
                    head_sha: p.head_sha,
                })
            },
            |p| hooks::terminals_in(p, cx),
        );
        let rows: Vec<AnyElement> = self
            .list
            .clone()
            .iter()
            .enumerate()
            .map(|(i, w)| self.row(i, w, &main, &hues, cx))
            .collect();
        let hot = self.hot();
        // ↑↓ goes without saying: left out, the hints of the open row fit on one line.
        let mut keys = vec!["↵ open here".to_string()];
        if let Some(h) = &hot {
            if !h.prunable && !h.bare {
                keys.push("T terminal".into());
            }
            if self.can_merge(h) {
                keys.push("M merge".into());
            }
            if h.branch.is_some() && !h.prunable {
                keys.push("F2 rename".into());
            }
            if !h.main && !h.current {
                let key = if h.prunable {
                    "⌫ prune"
                } else {
                    "⌫ remove"
                };
                keys.push(key.into());
            }
            keys.push("right-click for more".into());
        }
        let lock_note = hot.as_ref().filter(|h| h.locked).map(|h| {
            format!(
                "{}{}",
                if h.in_use { "In use" } else { "Locked" },
                h.lock_reason
                    .as_deref()
                    .map(|r| format!(": {r}"))
                    .unwrap_or_default()
            )
        });
        let footer_button =
            |id: &'static str,
             label: &'static str,
             cx: &mut Context<Self>,
             f: fn(&mut WorktreePicker, &mut Window, &mut Context<WorktreePicker>)| {
                div()
                    .id(id)
                    .flex_none()
                    .h(px(20.))
                    .flex()
                    .items_center()
                    .px_1p5()
                    .rounded(px(3.))
                    .cursor_pointer()
                    .hover(|s| s.bg(t.hover).text_color(t.foreground))
                    .child(label)
                    .on_click(cx.listener(move |this, _, window, cx| f(this, window, cx)))
            };
        div()
            .key_context("WorktreePicker")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .on_action(cx.listener(|this, _: &wt_actions::OpenTerminal, window, cx| {
                if let Some(w) = this.hot().filter(|w| !w.prunable && !w.bare) {
                    this.act(Act::Terminal, w, window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &wt_actions::Merge, window, cx| {
                if let Some(w) = this.hot().filter(|w| this.can_merge(w)) {
                    this.act(Act::Merge, w, window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &wt_actions::Rename, window, cx| {
                if let Some(w) = this.hot().filter(|w| w.branch.is_some() && !w.prunable) {
                    this.act(Act::Rename, w, window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &wt_actions::Remove, window, cx| {
                if let Some(w) = this.hot().filter(|w| !w.main && !w.current) {
                    this.act(Act::Remove, w, window, cx);
                }
            }))
            .w(px(384.))
            .flex()
            .flex_col()
            .text_size(rems(0.75))
            .child(
                div()
                    .id("worktree-list")
                    .max_h(px(360.))
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .p_1()
                    .child(div().px_2().pt_2().pb_1().text_size(rems(0.65625)).font_weight(FontWeight::SEMIBOLD).text_color(t.subtle).child("WORKTREES"))
                    .children(rows)
                    .when(self.list.len() == 1, |d| {
                        d.child(div().px_2().py_3().text_center().text_size(rems(0.71875)).text_color(t.subtle).child(
                            "No other worktrees yet. A worktree checks out another branch in its own folder, side by side with this one. Make one with New worktree below.",
                        ))
                    }),
            )
            .children(offer.map(|o| {
                let branch = o.branch.clone();
                div()
                    .id("main-back")
                    .border_t_1()
                    .border_color(t.border)
                    .px_2p5()
                    .py_2()
                    .text_size(rems(0.6875))
                    .text_color(t.muted_foreground)
                    .child(format!(
                        "{} is checked out in {}, so {} (on {}) can't switch to it. ",
                        o.branch,
                        folder_name(&o.holder.path),
                        folder_name(&o.main.path),
                        o.main.branch.clone().unwrap_or_default()
                    ))
                    .child(
                        div()
                            .id("main-back-go")
                            .text_color(t.primary)
                            .cursor_pointer()
                            .child(format!("Move {} back to {}…", o.branch, folder_name(&o.main.path)))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.close(cx);
                                ops::move_main_back(&this.state, branch.clone(), window, cx);
                            })),
                    )
            }))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .border_t_1()
                    .border_color(t.border)
                    .px_2p5()
                    .py_1p5()
                    .text_size(rems(0.65625))
                    .text_color(t.subtle)
                    // The buttons get a line of their own: beside them the key hints wrapped.
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap_1()
                            .when(!merged.is_empty(), |d| {
                                let list = merged.clone();
                                d.child(
                                    div()
                                        .id("wt-cleanup")
                                        .flex_none()
                                        .h(px(20.))
                                        .flex()
                                        .items_center()
                                        .px_1p5()
                                        .rounded(px(3.))
                                        .cursor_pointer()
                                        .hover(|s| s.bg(t.hover).text_color(t.foreground))
                                        .child("Clean up…")
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.close(cx);
                                            super::open_clean_up(list.clone(), window, cx);
                                        })),
                                )
                            })
                            .child(footer_button("wt-new", "New worktree…", cx, |this, window, cx| {
                                this.close(cx);
                                super::open_new_worktree(None, window, cx);
                            })),
                    )
                    .child(key_hints(keys))
                    .children(lock_note.map(|n| div().child(n))),
            )
    }
}
