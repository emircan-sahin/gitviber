//! What a pane's program wants of the user: its coding agent (core's agents.rs: looked up each
//! save round and after a prompt, then followed through its state file's events, never polled),
//! a bell or a notification escape, a long command ending out of sight. Each marks its pane, tab
//! and worktree until looked at, and tells the OS when the app is in the background and Settings
//! → Notifications allows it. The Dock badge counts the agents waiting.

use std::sync::OnceLock;

use gitviber_core::Event;
use gitviber_core::api::app as pty_api;
use gitviber_ui::workspace::Toast;
use gpui::{AnimationExt as _, Hsla};

use super::*;
use crate::agent::{self, AgentEntry, Look, Note, PaneAgent, next_agent, pane_look};

/// Which switch in Settings → Notifications a notification goes by.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum NotifyEvent {
    AgentDone,
    AgentWaiting,
    Terminal,
    LongCommand,
}

/// Pane ids start over each run: a click on an older run's notification, still in Notification
/// Center, would show some other pane.
fn run_token() -> &'static str {
    static RUN: OnceLock<String> = OnceLock::new();
    RUN.get_or_init(|| {
        gitviber_core::local_socket::random_hex(4)
            .unwrap_or_else(|| format!("{:x}", std::process::id()))
    })
}

pub(super) fn now_secs() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0., |d| d.as_secs_f64())
}

/// Tells the OS, if the user asked to be told of `event` and isn't looking at the app. True when
/// it was sent.
fn notify_if_away(
    event: NotifyEvent,
    title: String,
    body: String,
    target: String,
    cx: &mut App,
) -> bool {
    let wanted: fn(&Settings) -> bool = match event {
        NotifyEvent::AgentDone => |s| s.notify_agent_done,
        NotifyEvent::AgentWaiting => |s| s.notify_agent_waiting,
        NotifyEvent::Terminal => |s| s.notify_terminal,
        NotifyEvent::LongCommand => |s| s.notify_long_command,
    };
    gitviber_ui::notify_if_away(wanted, title, body, Some(target), cx)
}

/// What shows the agents (the top bar's button) observes; the Dock badge's count.
#[derive(Default)]
pub(super) struct AgentNews {
    waiting: usize,
    pending: bool,
}

impl gpui::Global for AgentNews {}

/// Every window's agents, the ones that need the user first.
pub(crate) fn all_agents(cx: &App) -> Vec<(WeakEntity<TerminalPanel>, AgentEntry)> {
    let Some(live) = cx.try_global::<LivePanels>() else {
        return vec![];
    };
    let mut out: Vec<_> = live
        .0
        .iter()
        .filter_map(|p| Some((p.clone(), p.upgrade()?)))
        .flat_map(|(weak, p)| {
            p.read(cx)
                .agent_list(cx)
                .into_iter()
                .map(move |e| (weak.clone(), e))
        })
        .collect();
    out.sort_by(|a, b| agent::by_urgency(&a.1, &b.1));
    out
}

/// After any pane's agent or mark changed: once the update that changed it is over, as every
/// window's panel is read, this one among them.
pub(super) fn agents_changed(cx: &mut App) {
    let news = cx.default_global::<AgentNews>();
    if news.pending {
        return;
    }
    news.pending = true;
    cx.defer(|cx| {
        let waiting = agent::agents_waiting(
            &all_agents(cx)
                .into_iter()
                .map(|(_, e)| e)
                .collect::<Vec<_>>(),
        );
        let news = cx.default_global::<AgentNews>();
        news.pending = false;
        if news.waiting != waiting {
            news.waiting = waiting;
            set_badge(waiting);
        }
        // Its observers hear of every change, the count's or not; so do the worktree chip's.
        gpui::BorrowAppContext::update_global::<AgentNews, _>(cx, |_, _| {});
        gitviber_refs::hooks::agents_changed(cx);
    });
}

/// The Dock badge: the agents waiting for the user; none clears it (macOS shows a 0 as "0").
#[cfg(target_os = "macos")]
fn set_badge(n: usize) {
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject};
    let label = std::ffi::CString::new(n.to_string()).unwrap_or_default();
    // SAFETY: plain AppKit messages on the main thread, where the UI runs; nil objects answer nil.
    unsafe {
        let (Some(app_class), Some(string_class)) =
            (AnyClass::get(c"NSApplication"), AnyClass::get(c"NSString"))
        else {
            return;
        };
        let app: *mut AnyObject = msg_send![app_class, sharedApplication];
        if app.is_null() {
            return;
        }
        let tile: *mut AnyObject = msg_send![app, dockTile];
        if tile.is_null() {
            return;
        }
        let text: *mut AnyObject = if n == 0 {
            std::ptr::null_mut()
        } else {
            msg_send![string_class, stringWithUTF8String: label.as_ptr()]
        };
        let _: () = msg_send![tile, setBadgeLabel: text];
    }
}

#[cfg(not(target_os = "macos"))]
fn set_badge(_: usize) {}

/// A worktree's look, for its chip: the most urgent of its panes' (any window's).
pub fn agents_in(dir: &Path, cx: &App) -> Option<gitviber_ui::widgets::Look> {
    let live = cx.try_global::<LivePanels>()?;
    let look = agent::most_urgent(live.0.iter().filter_map(|p| p.upgrade()).flat_map(|p| {
        let p = p.read(cx);
        let worktrees = p.known_worktrees(cx);
        p.panes
            .values()
            .filter(|s| agent::in_worktree(s.view.read(cx).cwd(), dir, &worktrees))
            .map(|s| pane_look(s.agent.as_ref(), s.rang))
            .collect::<Vec<_>>()
    }));
    look.map(ui_look)
}

/// For the terminal button's dot: the most urgent news of every pane in every window (one asked,
/// or news not looked at), whatever folder it runs in. None when no pane calls.
pub fn calling(cx: &App) -> Option<gitviber_ui::widgets::Look> {
    let live = cx.try_global::<LivePanels>()?;
    let look = agent::most_urgent(live.0.iter().filter_map(|p| p.upgrade()).map(|p| {
        let p = p.read(cx);
        agent::most_urgent(
            p.panes
                .values()
                .map(|s| pane_look(s.agent.as_ref(), s.rang)),
        )
    }));
    look.filter(|l| matches!(l, Look::Needs | Look::Unread))
        .map(ui_look)
}

pub(crate) fn ui_look(l: Look) -> gitviber_ui::widgets::Look {
    match l {
        Look::Needs => gitviber_ui::widgets::Look::Needs,
        Look::Unread => gitviber_ui::widgets::Look::Unread,
        Look::Working => gitviber_ui::widgets::Look::Working,
        Look::Done => gitviber_ui::widgets::Look::Done,
    }
}

/// The agent running in `worktree`'s terminals as last looked up: the open tab's first, each tab's
/// focused pane first. What text handed to it (a review's Ask Agent) does: `agent::handoff_to`.
pub fn worktree_agent(worktree: &Path, cx: &App) -> Option<PaneAgent> {
    let live = cx.try_global::<LivePanels>()?;
    live.0.iter().filter_map(|p| p.upgrade()).find_map(|p| {
        let p = p.read(cx);
        let id = p.agent_pane(worktree, cx)?;
        p.panes.get(&id)?.agent.clone()
    })
}

/// What quitting would stop in every window's terminals, as `agent::quit_stops` words it: an
/// agent mid-turn, a command running. Not an agent done with its turn, which the next run
/// resumes. Each pane's foreground job is asked of core off the UI thread.
pub fn stopped_by_quit(cx: &App) -> gpui::Task<Vec<String>> {
    let panes: Vec<(u32, Option<PaneAgent>)> = cx
        .try_global::<LivePanels>()
        .map(|live| {
            live.0
                .iter()
                .filter_map(|p| p.upgrade())
                .flat_map(|p| {
                    p.read(cx)
                        .panes
                        .values()
                        .filter_map(|s| Some((s.view.read(cx).pty_id()?, s.agent.clone())))
                        .collect::<Vec<_>>()
                })
                .collect()
        })
        .unwrap_or_default();
    if panes.is_empty() || !gitviber_ui::has_core(cx) {
        return gpui::Task::ready(Vec::new());
    }
    let core = gitviber_ui::core(cx);
    cx.background_spawn(async move {
        let busy: Vec<(bool, Option<&PaneAgent>)> = panes
            .iter()
            .map(|(id, a)| (core.ptys.busy(Some(&[*id])) > 0, a.as_ref()))
            .collect();
        agent::quit_stops(&busy)
    })
}

/// Pastes `text` into the pane an agent runs in, in `worktree`, and sends it: false when none does.
pub fn paste_to_agent(worktree: &Path, text: &str, window: &mut Window, cx: &mut App) -> bool {
    let Some(live) = cx.try_global::<LivePanels>() else {
        return false;
    };
    let Some((panel, id)) = live.0.iter().filter_map(|p| p.upgrade()).find_map(|p| {
        let id = p.read(cx).agent_pane(worktree, cx)?;
        Some((p, id))
    }) else {
        return false;
    };
    panel.update(cx, |p, cx| {
        if let Some(view) = p.panes.get(&id).map(|s| s.view.clone()) {
            view.update(cx, |v, cx| {
                v.paste_text(text.to_string(), window, cx);
                v.session.write("\r");
            });
        }
        p.reveal_pane(id, window, cx);
    });
    true
}

impl TerminalPanel {
    /// The panel's part of the agents list.
    pub(crate) fn agent_list(&self, cx: &App) -> Vec<AgentEntry> {
        let mut out = vec![];
        for set in self.sets.values() {
            for g in &set.groups {
                let order = layout::pane_order(&g.layout);
                let tab_cwd = self
                    .panes
                    .get(&g.panes[0])
                    .map(|s| s.view.read(cx).cwd().to_string_lossy().into_owned())
                    .unwrap_or_default();
                for (i, id) in order.iter().enumerate() {
                    let Some(slot) = self.panes.get(id) else {
                        continue;
                    };
                    let Some(a) = &slot.agent else { continue };
                    out.push(AgentEntry {
                        pane: *id,
                        name: a.name.clone(),
                        state: agent::shown_state(a.state),
                        unseen: slot.rang,
                        cwd: slot.view.read(cx).cwd().to_string_lossy().into_owned(),
                        since: slot.since,
                        tab: g.id,
                        at: i + 1,
                        of: order.len(),
                        pane_name: slot.name.clone(),
                        tab_name: g.name.clone(),
                        tab_cwd: tab_cwd.clone(),
                    });
                }
            }
        }
        out
    }

    /// The pane an agent runs in, in `worktree`'s set: the open tab's first, each tab's focused one
    /// first.
    fn agent_pane(&self, worktree: &Path, cx: &App) -> Option<PaneId> {
        let worktrees = self.known_worktrees(cx);
        // Every tab set: an agent's pane may have opened from another worktree's view.
        let mut groups: Vec<(&Group, bool)> = self
            .sets
            .iter()
            .flat_map(|(root, set)| {
                set.groups
                    .iter()
                    .map(move |g| (g, root == worktree && Some(g.id) == set.active))
            })
            .collect();
        groups.sort_by_key(|(_, open)| !open);
        groups.into_iter().find_map(|(g, _)| {
            std::iter::once(g.focused)
                .chain(g.panes.iter().copied())
                .find(|id| {
                    self.panes.get(id).is_some_and(|s| {
                        s.agent.is_some()
                            && agent::in_worktree(s.view.read(cx).cwd(), worktree, &worktrees)
                    })
                })
        })
    }

    /// The worktrees this panel knows of: the repo's, and the ones whose tabs it holds.
    pub(super) fn known_worktrees(&self, cx: &App) -> Vec<PathBuf> {
        let mut out: Vec<PathBuf> = self.worktrees.iter().map(|w| w.path.clone()).collect();
        out.extend(self.sets.keys().cloned());
        if let Some(repo) = self
            .workspace()
            .and_then(|ws| ws.read(cx).project().cloned())
            .and_then(|p| gitviber_refs::RepoState::existing(&p, cx))
        {
            out.extend(
                repo.read(cx)
                    .worktrees
                    .iter()
                    .map(|w| PathBuf::from(&w.path)),
            );
        }
        out
    }

    /// A pane's look, for its header's dot and its tab's.
    pub(super) fn look_of_pane(&self, id: PaneId) -> Option<Look> {
        let slot = self.panes.get(&id)?;
        pane_look(slot.agent.as_ref(), slot.rang)
    }

    /// What a dot says, for its tooltip: "Claude Code working".
    pub(super) fn look_label(&self, ids: &[PaneId], look: Look) -> String {
        let agent = ids
            .iter()
            .find(|id| self.look_of_pane(**id) == Some(look))
            .and_then(|id| self.panes.get(id)?.agent.as_ref())
            .map(|a| a.name.as_str());
        look.label(agent)
    }
}

/// A look as a dot: the app's, with a ring while an agent works, so a turn in progress reads at a
/// glance beside one that's done; the working one pulses.
pub(super) fn look_dot(look: Look, cx: &App) -> AnyElement {
    let dot = gitviber_ui::widgets::StatusDot(ui_look(look));
    if look != Look::Working {
        return dot.into_any_element();
    }
    let ring = cx.app_theme().modified.opacity(0.45);
    pulsing(
        look,
        div()
            .flex_none()
            .p(px(1.5))
            .rounded_full()
            .border_1()
            .border_color(ring)
            .child(dot)
            .into_any_element(),
    )
}

/// What an icon's badge shows of its agents' most urgent look: a finished, seen one is nothing to
/// tell, so there's no badge.
pub(super) fn badge_look(loudest: Option<Look>) -> Option<Look> {
    loudest.filter(|l| *l != Look::Done)
}

/// A badge on an icon's top-right corner, cut out of `under` (what the icon sits on) so it reads
/// apart from the glyph. The parent must be the icon's box, `relative`.
pub(super) fn corner_badge(look: Look, under: Hsla, inset: Pixels) -> AnyElement {
    div()
        .absolute()
        .top(inset)
        .right(inset)
        .p(px(1.5))
        .rounded_full()
        .bg(under)
        .child(pulsing(
            look,
            gitviber_ui::widgets::StatusDot(ui_look(look)).into_any_element(),
        ))
        .into_any_element()
}

/// The Agents menu's dot: in a highlighted row's `ink`, and a working one pulses.
pub(super) fn menu_dot(look: Look, ink: Option<Hsla>, cx: &App) -> AnyElement {
    let Some(ink) = ink else {
        return look_dot(look, cx);
    };
    let dot = div().flex_none().size(px(6.)).rounded_full();
    let dot = match look {
        Look::Done => dot.border_1().border_color(ink),
        Look::Needs => dot.bg(ink).shadow(vec![gpui::BoxShadow {
            color: ink.opacity(0.3),
            offset: gpui::point(px(0.), px(0.)),
            blur_radius: px(0.),
            spread_radius: px(2.),
            inset: false,
        }]),
        _ => dot.bg(ink),
    };
    let dot = if look == Look::Working {
        div()
            .flex_none()
            .p(px(1.5))
            .rounded_full()
            .border_1()
            .border_color(ink.opacity(0.45))
            .child(dot)
            .into_any_element()
    } else {
        dot.into_any_element()
    };
    pulsing(look, dot)
}

/// Only a working agent's dot animates: with none at work no animation is laid out, so nothing
/// asks for frames.
pub(super) fn pulses(look: Look) -> bool {
    look == Look::Working
}

/// A working dot fading to 35% and back over 1.6 s; held still under Reduce Motion (gpui's).
/// Synced: the dots share one phase, and their timers, armed in the same frame, wake one redraw.
/// 10 fps steps a 6px dot's opacity by under 13%; a covered window draws none (no display link).
fn pulsing(look: Look, dot: AnyElement) -> AnyElement {
    if !pulses(look) {
        return dot;
    }
    div()
        .flex_none()
        .child(dot)
        .with_animation(
            "agent-working",
            gpui::Animation::new(Duration::from_millis(1600))
                .repeat_synced()
                .with_max_fps(10.),
            |d, t| d.opacity(1. - 0.65 * (1. - (t * std::f32::consts::TAU).cos()) / 2.),
        )
        .into_any_element()
}

impl TerminalPanel {
    /// Marks `id` (and tells the OS, when away and `event`'s switch is on) unless it's looked at:
    /// once until it is, so an agent's escape and its state file saying the same are one mark,
    /// one notification. Without `event` it's the terminal's bell or escape, which only marks a
    /// pane whose agent reports its state: that one is told as finished or waiting.
    pub(super) fn needs_you(
        &mut self,
        id: PaneId,
        note: Option<Note>,
        event: Option<NotifyEvent>,
        cx: &mut Context<Self>,
    ) {
        let Some(slot) = self.panes.get(&id) else {
            return;
        };
        let pane = slot.view.read(cx);
        if slot.told || pane.looked_at() {
            return;
        }
        let program = pane.title().map(String::from);
        let cwd = pane.cwd().to_path_buf();
        let tab = self
            .sets
            .values()
            .flat_map(|s| &s.groups)
            .find(|g| g.panes.contains(&id))
            .and_then(|g| g.name.clone());
        let reports = slot.agent.as_ref().is_some_and(|a| a.state.is_some());
        if let Some(slot) = self.panes.get_mut(&id)
            && !slot.rang
        {
            slot.rang = true;
            cx.notify();
        }
        agents_changed(cx);
        if event.is_none() && reports {
            return;
        }
        let text = note
            .map(|n| {
                [n.title.unwrap_or_default(), n.body]
                    .into_iter()
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join(": ")
            })
            .filter(|t| !t.is_empty());
        let title = tab.unwrap_or_else(|| folder_name(&cwd));
        let body = text.or(program).unwrap_or_else(|| "Needs you".into());
        let target = format!("pane:{}:{id}", run_token());
        if notify_if_away(
            event.unwrap_or(NotifyEvent::Terminal),
            title,
            body,
            target,
            cx,
        ) && let Some(slot) = self.panes.get_mut(&id)
        {
            slot.told = true;
        }
    }

    /// The user is on `id` again: its mark goes, and its next news is told again.
    pub(super) fn looked_at(&mut self, id: PaneId, cx: &mut Context<Self>) {
        if let Some(slot) = self.panes.get_mut(&id) {
            slot.told = false;
            if slot.rang {
                slot.rang = false;
                cx.notify();
                agents_changed(cx);
            }
        }
    }

    /// A marked command that ran past the threshold and ended out of sight; an agent's pane has
    /// its own news.
    pub(super) fn command_ended(
        &mut self,
        id: PaneId,
        end: &CommandEnd,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let s = Settings::get(cx);
        if !s.notify_long_command || end.duration.as_secs_f64() <= s.long_command_seconds as f64 {
            return;
        }
        if self.panes.get(&id).is_none_or(|s| s.agent.is_some()) {
            return;
        }
        if window.is_window_active() && self.on_screen(id, cx) {
            return;
        }
        let body = crate::marks::end_text(end);
        self.needs_you(
            id,
            Some(Note { title: None, body }),
            Some(NotifyEvent::LongCommand),
            cx,
        );
    }

    /// In the panel as it shows now: its worktree's open tab, the panel open.
    fn on_screen(&self, id: PaneId, cx: &App) -> bool {
        let open = self
            .workspace()
            .is_none_or(|ws| ws.read(cx).is_open(Slot::Bottom));
        open && self
            .active_group()
            .is_some_and(|g| g.panes.contains(&id) && (!self.zoomed || g.focused == id))
    }

    /// Takes in what was read of `id`'s agent (None: none runs there now). `live`: its state
    /// file's change as it happened.
    pub(super) fn apply_agent(
        &mut self,
        id: PaneId,
        read: Option<PaneAgent>,
        live: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(slot) = self.panes.get(&id) else {
            return;
        };
        let prev = slot.agent.clone();
        let next = next_agent(prev.as_ref(), read, live);
        if !next.changed && next.note.is_none() {
            return;
        }
        let slot = self.panes.get_mut(&id).expect("looked up above");
        if next.agent.as_ref().map(|a| (&a.name, a.state))
            != prev.as_ref().map(|a| (&a.name, a.state))
        {
            slot.since = now_secs();
        }
        slot.agent = next.agent.clone();
        // Woken (its background shell exited) before its finish was seen: that news is old, and
        // its next finish must be told again.
        if live
            && next
                .agent
                .as_ref()
                .is_some_and(|a| a.state == Some(agent::AgentState::Working))
            && prev.and_then(|p| p.state) != Some(agent::AgentState::Working)
        {
            slot.told = false;
            slot.rang = false;
        }
        cx.notify();
        if let Some(note) = next.note {
            let waiting = next
                .agent
                .as_ref()
                .is_some_and(|a| a.state == Some(agent::AgentState::Waiting));
            let event = if waiting {
                NotifyEvent::AgentWaiting
            } else {
                NotifyEvent::AgentDone
            };
            self.needs_you(
                id,
                Some(Note {
                    title: None,
                    body: note,
                }),
                Some(event),
                cx,
            );
        }
        if next.first
            && let Some(a) = &next.agent
        {
            self.suggest_notifications(&a.name, cx);
        }
        agents_changed(cx);
    }

    /// Once ever, and only while notifications are off: turning them on is what asks the OS.
    fn suggest_notifications(&mut self, name: &str, cx: &mut Context<Self>) {
        if Settings::get(cx).notify || super::saved::notify_hint_shown(cx) {
            return;
        }
        super::saved::set_notify_hint_shown(cx);
        let toast = Toast::info("While GitViber isn't the app in front: Settings → Notifications.")
            .title(format!("Get a notification when {name} finishes?"));
        if let Some(ws) = self.workspace() {
            ws.update(cx, |ws, cx| ws.toast(toast, cx));
        }
    }

    /// Looks up the agents `only` (or every pane) run now, off the UI thread.
    pub(super) fn refresh_agents(&mut self, only: Option<Vec<PaneId>>, cx: &mut Context<Self>) {
        let ids: Vec<(PaneId, u32)> = self
            .panes
            .iter()
            .filter(|(id, _)| only.as_ref().is_none_or(|o| o.contains(id)))
            .filter_map(|(id, s)| Some((*id, s.view.read(cx).pty_id()?)))
            .collect();
        if ids.is_empty() {
            return;
        }
        let core = self.core.clone();
        let task = cx.spawn(async move |this, cx| {
            let ptys: Vec<u32> = ids.iter().map(|(_, p)| *p).collect();
            let running = cx
                .background_spawn(async move { pty_api::pty_agents(&core, ptys) })
                .await;
            let Ok(running) = running else { return };
            let _ = this.update(cx, |this, cx| {
                for (id, pty) in ids {
                    // Another shell in the pane since: what was read is of the old one.
                    if this.panes.get(&id).and_then(|s| s.view.read(cx).pty_id()) == Some(pty) {
                        this.apply_agent(
                            id,
                            running.get(&pty).map(PaneAgent::from_core),
                            false,
                            cx,
                        );
                    }
                }
            });
        });
        self.agent_tasks.push(task);
        // Done ones go; a lookup takes a moment, so there are a few at most.
        if self.agent_tasks.len() > 8 {
            drop(self.agent_tasks.remove(0));
        }
    }

    /// The program in `id` reported its state in-band: taken as a change of its state file is, so
    /// the newer of the two stands. One not found yet is looked up; the lookup brings its state.
    pub(super) fn agent_reported(
        &mut self,
        id: PaneId,
        state: agent::AgentState,
        cx: &mut Context<Self>,
    ) {
        match self.panes.get(&id).and_then(|s| s.agent.clone()) {
            Some(agent) => self.apply_agent(
                id,
                Some(PaneAgent {
                    state: Some(state),
                    ..agent
                }),
                true,
                cx,
            ),
            None => self.refresh_agents(Some(vec![id]), cx),
        }
    }

    /// The shell prompts again: the agent there exited, and isn't one to resume.
    pub(super) fn agent_prompted(&mut self, id: PaneId, cx: &mut Context<Self>) {
        if self.panes.get(&id).is_some_and(|s| s.agent.is_some()) {
            self.apply_agent(id, None, false, cx);
            // So core stops watching its state file.
            self.refresh_agents(Some(vec![id]), cx);
        }
    }

    /// Core's events: an agent's state file changed; a notification of ours was clicked.
    pub(super) fn on_core_event(
        &mut self,
        event: &Event,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            Event::AgentState(s) => {
                let Some(id) = self
                    .panes
                    .iter()
                    .find(|(_, p)| p.view.read(cx).pty_id() == Some(s.id))
                    .map(|(id, _)| *id)
                else {
                    return;
                };
                let Some(agent) = self.panes.get(&id).and_then(|p| p.agent.clone()) else {
                    return;
                };
                // Its state file gone: the agent with it.
                let read = s.state.map(|st| PaneAgent {
                    state: agent::AgentState::parse(st),
                    ..agent
                });
                self.apply_agent(id, read, true, cx);
            }
            Event::NotificationClick(Some(target)) => {
                let mut parts = target.split(':');
                if let (Some("pane"), Some(run), Some(id)) =
                    (parts.next(), parts.next(), parts.next())
                    && run == run_token()
                    && let Ok(id) = id.parse::<PaneId>()
                    && self.panes.contains_key(&id)
                {
                    self.reveal_pane(id, window, cx);
                }
            }
            _ => {}
        }
    }

    /// A pane in view with the keys (a notification's, or one picked in the Agents menu): its
    /// worktree, its tab, the panel open.
    pub(crate) fn reveal_pane(&mut self, id: PaneId, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self
            .sets
            .iter()
            .find(|(_, s)| s.groups.iter().any(|g| g.panes.contains(&id)))
            .map(|(r, _)| r.clone())
        else {
            return;
        };
        window.activate_window();
        if root != self.root
            && let Some(project) = self
                .workspace()
                .and_then(|ws| ws.read(cx).project().cloned())
        {
            project.update(cx, |p, cx| p.switch_worktree(root.clone(), cx));
            self.set_worktree(root, window, cx);
        }
        if let Some(ws) = self.workspace() {
            ws.update(cx, |ws, cx| ws.set_open(Slot::Bottom, true, cx));
        }
        if self.zoomed && self.active_group().is_some_and(|g| g.focused != id) {
            self.zoomed = false;
        }
        self.focus_pane(id, cx);
        self.focus_active(window, cx);
        cx.notify();
    }
}
