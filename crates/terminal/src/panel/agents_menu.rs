//! The top bar's Agents button: every agent running in a terminal, in any window, by tab, the
//! tabs whose agents need the user first, how long each has been in its state, and a click to its
//! pane. Shown once there's an agent.

use gitviber_ui::memory::Live;
use gitviber_ui::workspace::{BarItem, BarSlot, PanelContext, Toast, register_bar_item};
use gpui::{Anchor, AnyWindowHandle, Render, SharedString, rems};
use gpui_kit::component::popover::Popover;

use super::attention::{AgentNews, all_agents, look_dot};
use super::*;
use crate::agent::{AgentEntry, Look, agents_waiting, by_tab, most_urgent};

/// Each window's button, for Show Agents.
#[derive(Default)]
struct Buttons(Vec<(AnyWindowHandle, WeakEntity<AgentsButton>)>);

impl gpui::Global for Buttons {}

pub(super) fn register(cx: &mut App) {
    register_bar_item(
        cx,
        BarItem::new("agents", BarSlot::TopActions, bar).order(5),
    );
    cx.on_action(|_: &terminal::ShowAgents, cx| {
        let Some(handle) = cx.active_window() else {
            return;
        };
        let button = cx.try_global::<Buttons>().and_then(|b| {
            b.0.iter()
                .find(|(w, _)| *w == handle)
                .and_then(|(_, b)| b.upgrade())
        });
        let none = all_agents(cx).is_empty();
        cx.defer(move |cx| {
            let _ = handle.update(cx, |_, window, cx| match button {
                Some(b) if !none => b.update(cx, |b, cx| {
                    b.open = true;
                    b.tick(cx);
                    cx.notify();
                }),
                _ => Toast::info(
                    "Claude Code and other coding agents show here while they run in a terminal.",
                )
                .title("No agents in the terminals")
                .show(window, cx),
            });
        });
    });
}

fn bar(_: &PanelContext, window: &mut Window, cx: &mut App) -> Option<AnyElement> {
    let button = window.use_keyed_state("terminal-agents-button", cx, AgentsButton::new);
    Some(button.into_any_element())
}

pub(super) struct AgentsButton {
    open: bool,
    /// The times move on while it's open; nothing runs while it's closed.
    ticking: Option<Task<()>>,
    _subs: Vec<Subscription>,
    _live: Live,
}

impl AgentsButton {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        cx.default_global::<AgentNews>();
        let me = cx.weak_entity();
        let buttons = cx.default_global::<Buttons>();
        buttons.0.retain(|(_, b)| b.upgrade().is_some());
        buttons.0.push((window.window_handle(), me));
        AgentsButton {
            open: false,
            ticking: None,
            _subs: vec![cx.observe_global::<AgentNews>(|_, cx| cx.notify())],
            _live: Live::new("AgentsButton"),
        }
    }

    fn tick(&mut self, cx: &mut Context<Self>) {
        if !self.open {
            self.ticking = None;
            return;
        }
        self.ticking.get_or_insert_with(|| {
            cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor()
                        .timer(std::time::Duration::from_secs(30))
                        .await;
                    if this.update(cx, |_, cx| cx.notify()).is_err() {
                        break;
                    }
                }
            })
        });
    }
}

/// "<1m", "42m", "3h 5m", "2d": how long an agent has been in its state, as a glance needs it.
/// No seconds: the menu redraws every 30 s.
fn short_duration(secs: f64) -> String {
    let m = secs.max(0.) as u64 / 60;
    match m {
        0 => "<1m".into(),
        1..60 => format!("{m}m"),
        60..1440 => format!("{}h {}m", m / 60, m % 60),
        _ => format!("{}d", m / 1440),
    }
}

/// "2 need you · 1 working".
fn summary(list: &[AgentEntry]) -> String {
    let waiting = agents_waiting(list);
    let working = list
        .iter()
        .filter(|e| e.state == crate::agent::Shown::Working)
        .count();
    let mut parts = vec![];
    if waiting > 0 {
        parts.push(format!(
            "{waiting} {} you",
            if waiting == 1 { "needs" } else { "need" }
        ));
    }
    if working > 0 {
        parts.push(format!("{working} working"));
    }
    parts.join(" · ")
}

impl Render for AgentsButton {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let agents = all_agents(cx);
        if agents.is_empty() {
            self.open = false;
            self.ticking = None;
            return div().into_any_element();
        }
        let list: Vec<AgentEntry> = agents.iter().map(|(_, e)| e.clone()).collect();
        let sum = summary(&list);
        let loudest = most_urgent(list.iter().map(AgentEntry::look));
        let tip: SharedString = if sum.is_empty() {
            "Agents".into()
        } else {
            format!("Agents · {sum}").into()
        };
        let trigger = Button::new("terminal-agents")
            .ghost()
            .small()
            .icon(Icon::new(IconName::Bot))
            .tooltip(tip)
            .child(
                div().absolute().top(px(3.)).right(px(3.)).children(
                    loudest
                        .filter(|l| *l != Look::Done)
                        .map(|l| look_dot(l, cx)),
                ),
            );
        let weak = cx.weak_entity();
        Popover::new("terminal-agents-menu")
            .anchor(Anchor::TopRight)
            .open(self.open)
            .on_open_change(cx.listener(|this, open: &bool, _, cx| {
                this.open = *open;
                this.tick(cx);
                cx.notify();
            }))
            .trigger(trigger)
            .content(move |_, _, cx| {
                let agents = all_agents(cx);
                let t = cx.app_theme().clone();
                let now = super::attention::now_secs();
                let list: Vec<AgentEntry> = agents.iter().map(|(_, e)| e.clone()).collect();
                let panel_of = |pane: u64| {
                    agents
                        .iter()
                        .find(|(_, e)| e.pane == pane)
                        .map(|(p, _)| p.clone())
                };
                let tabs = by_tab(&list);
                // Headers say where once for a tab's agents; one agent alone keeps its own line.
                let headed = tabs.len() > 1 || tabs.first().is_some_and(|t| t.len() > 1);
                let mut col = v_flex().w(px(320.)).p_1().gap_0p5().child(
                    div()
                        .px_2()
                        .py_1()
                        .text_size(rems(0.6875))
                        .font_weight(t.semibold())
                        .text_color(t.muted_foreground)
                        .child("Agents"),
                );
                for tab in tabs {
                    let first = tab[0].clone();
                    let place_of = |cwd: &str, cx: &App| {
                        panel_of(first.pane)
                            .and_then(|p| p.upgrade())
                            .map(|p| p.read(cx).place_of(Path::new(cwd), cx))
                            .unwrap_or_else(|| folder_name(Path::new(cwd)))
                    };
                    let head = first
                        .tab_name
                        .clone()
                        .unwrap_or_else(|| place_of(&first.tab_cwd, cx));
                    if headed {
                        col = col.child(
                            h_flex()
                                .gap_1p5()
                                .px_2()
                                .pt_1p5()
                                .pb_0p5()
                                .text_size(rems(0.6875))
                                .text_color(t.muted_foreground)
                                .child(Icon::new(IconName::SquareTerminal).size(px(12.)))
                                .child(div().truncate().child(head.clone())),
                        );
                    }
                    for e in tab {
                        let look = e.look();
                        let place = place_of(&e.cwd, cx);
                        let mut state = e.state.label().to_string();
                        if look == Some(Look::Unread) {
                            state.push_str(", not viewed");
                        }
                        if e.since > 0. {
                            state.push_str(&format!(" · {}", short_duration(now - e.since)));
                        }
                        let news = look.is_some_and(Look::is_news);
                        let state_color = match look {
                            Some(Look::Needs) => t.destructive,
                            Some(Look::Unread) => t.info,
                            _ => t.muted_foreground,
                        };
                        let (pane, panel, close) = (e.pane, panel_of(e.pane), weak.clone());
                        col = col.child(
                            h_flex()
                                .id(("agent-row", e.pane))
                                .items_start()
                                .gap_2()
                                .px_2()
                                .py_1p5()
                                .rounded(px(3.))
                                .cursor_pointer()
                                .hover(|s| s.bg(t.hover))
                                .text_size(rems(0.75))
                                .child(
                                    div()
                                        .h(px(16.))
                                        .w(px(10.))
                                        .flex()
                                        .items_center()
                                        .children(look.map(|l| look_dot(l, cx))),
                                )
                                .child(
                                    v_flex()
                                        .flex_1()
                                        .min_w_0()
                                        .child(
                                            h_flex()
                                                .gap_2()
                                                .min_w_0()
                                                .child(
                                                    div()
                                                        .truncate()
                                                        .font_weight(t.medium())
                                                        .child(e.name.clone()),
                                                )
                                                .when(e.of > 1, |d| {
                                                    d.child(
                                                        div()
                                                            .flex_none()
                                                            .text_size(rems(0.625))
                                                            .text_color(t.muted_foreground)
                                                            .child(format!("{} / {}", e.at, e.of)),
                                                    )
                                                })
                                                .when_some(e.pane_name.clone(), |d, n| {
                                                    d.child(
                                                        div()
                                                            .min_w_0()
                                                            .truncate()
                                                            .text_size(rems(0.65625))
                                                            .text_color(t.muted_foreground)
                                                            .child(n),
                                                    )
                                                })
                                                .child(
                                                    div()
                                                        .ml_auto()
                                                        .flex_none()
                                                        .text_size(rems(0.65625))
                                                        .text_color(if news {
                                                            state_color
                                                        } else {
                                                            t.muted_foreground
                                                        })
                                                        .child(state),
                                                ),
                                        )
                                        // The header says where, unless this pane opened elsewhere.
                                        .when(!headed || place != head, |d| {
                                            d.child(
                                                div()
                                                    .truncate()
                                                    .text_size(rems(0.65625))
                                                    .text_color(t.muted_foreground)
                                                    .child(place),
                                            )
                                        }),
                                )
                                .on_click(move |_, window, cx| {
                                    let _ = close.update(cx, |b, cx| {
                                        b.open = false;
                                        b.tick(cx);
                                        cx.notify();
                                    });
                                    if let Some(p) = panel.as_ref().and_then(|p| p.upgrade()) {
                                        p.update(cx, |p, cx| p.reveal_pane(pane, window, cx));
                                    }
                                }),
                        );
                    }
                }
                col
            })
            .into_any_element()
    }
}

impl TerminalPanel {
    /// The worktree a folder is in, with its branch ("app · main"); else the folder's name.
    pub(super) fn place_of(&self, cwd: &Path, cx: &App) -> String {
        let repo = self
            .workspace()
            .and_then(|ws| ws.read(cx).project().cloned())
            .and_then(|p| gitviber_refs::RepoState::existing(&p, cx));
        let mut known: Vec<(PathBuf, Option<String>)> = self
            .worktrees
            .iter()
            .map(|w| (w.path.clone(), w.branch.clone()))
            .collect();
        if let Some(repo) = &repo {
            known.extend(
                repo.read(cx)
                    .worktrees
                    .iter()
                    .map(|w| (PathBuf::from(&w.path), w.branch.clone())),
            );
        }
        known
            .into_iter()
            .filter(|(p, _)| cwd.starts_with(p))
            .max_by_key(|(p, _)| p.as_os_str().len())
            .map(|(p, branch)| {
                format!(
                    "{} · {}",
                    folder_name(&p),
                    branch.as_deref().unwrap_or("detached")
                )
            })
            .unwrap_or_else(|| folder_name(cwd))
    }
}

#[cfg(test)]
mod tests {
    use super::short_duration;

    #[test]
    fn durations_at_a_glance() {
        assert_eq!(short_duration(-5.), "<1m");
        assert_eq!(short_duration(59.), "<1m");
        assert_eq!(short_duration(42. * 60. + 30.), "42m");
        assert_eq!(short_duration(3. * 3600. + 5. * 60.), "3h 5m");
        assert_eq!(short_duration(23. * 3600. + 59. * 60.), "23h 59m");
        assert_eq!(short_duration(50. * 3600.), "2d");
    }
}
