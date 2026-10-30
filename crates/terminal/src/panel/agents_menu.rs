//! The top bar's Agents button: every agent running in a terminal, in any window, by tab, the
//! tabs whose agents need the user first, how long each has been in its state, and a click to its
//! pane. Shown once there's an agent. Its rows take the arrows, Enter and Escape, as a menu's do.

use gitviber_ui::memory::Live;
use gitviber_ui::widgets::Ends;
use gitviber_ui::windows::TOP_BAR_HEIGHT;
use gitviber_ui::workspace::{BarItem, BarSlot, PanelContext, Toast, register_bar_item};
use gpui::{Anchor, AnyWindowHandle, KeyDownEvent, Render, SharedString, rems};
use gpui_kit::component::popover::Popover;

use super::attention::{AgentNews, all_agents, look_dot, menu_dot};
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
        let first = rows(&all_agents(cx)).first().map(|(pane, _)| *pane);
        cx.defer(move |cx| {
            let _ = handle.update(cx, |_, window, cx| match button {
                Some(b) if first.is_some() => b.update(cx, |b, cx| {
                    b.open = true;
                    // From the keys: the first row is the one Enter picks, as in a menu.
                    b.selected = first;
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
    /// The highlighted row's pane: the arrows' or the pointer's.
    selected: Option<PaneId>,
    focus: FocusHandle,
    scroll: ScrollHandle,
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
            selected: None,
            focus: cx.focus_handle(),
            scroll: ScrollHandle::new(),
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

    fn close(&mut self, cx: &mut Context<Self>) {
        self.open = false;
        self.selected = None;
        self.tick(cx);
        cx.notify();
    }
}

/// The rows in the order the menu shows them, each pane with its panel.
fn rows(
    agents: &[(WeakEntity<TerminalPanel>, AgentEntry)],
) -> Vec<(PaneId, WeakEntity<TerminalPanel>)> {
    let list: Vec<AgentEntry> = agents.iter().map(|(_, e)| e.clone()).collect();
    by_tab(&list)
        .into_iter()
        .flatten()
        .filter_map(|e| {
            let panel = agents.iter().find(|(_, a)| a.pane == e.pane)?.0.clone();
            Some((e.pane, panel))
        })
        .collect()
}

/// The row an arrow moves to: from none, the first (down) or the last (up); no wrapping.
fn step(order: &[PaneId], from: Option<PaneId>, k: &gpui::Keystroke) -> Option<PaneId> {
    let at = from.and_then(|p| order.iter().position(|o| *o == p));
    let to = gitviber_ui::widgets::list_step(k, at, order.len(), Ends::Stop, false)?;
    order.get(to).copied()
}

/// A row picked: the menu closes and its pane comes into view with the keys.
fn pick(
    button: &WeakEntity<AgentsButton>,
    pane: PaneId,
    panel: Option<&WeakEntity<TerminalPanel>>,
    window: &mut Window,
    cx: &mut App,
) {
    let _ = button.update(cx, |b, cx| b.close(cx));
    if let Some(p) = panel.and_then(|p| p.upgrade()) {
        p.update(cx, |p, cx| p.reveal_pane(pane, window, cx));
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
            self.selected = None;
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
        let (selected, focus, scroll) = (self.selected, self.focus.clone(), self.scroll.clone());
        Popover::new("terminal-agents-menu")
            .anchor(Anchor::TopRight)
            .p_0()
            .track_focus(&self.focus)
            .open(self.open)
            .on_open_change(cx.listener(|this, open: &bool, _, cx| {
                if *open {
                    this.open = true;
                    this.tick(cx);
                    cx.notify();
                } else {
                    this.close(cx);
                }
            }))
            .trigger(trigger)
            .content(move |_, window, cx| {
                let agents = all_agents(cx);
                let t = cx.app_theme().clone();
                let now = super::attention::now_secs();
                let order = rows(&agents);
                let panel_of = |pane: PaneId| {
                    order
                        .iter()
                        .find(|(p, _)| *p == pane)
                        .map(|(_, panel)| panel.clone())
                };
                let list: Vec<AgentEntry> = agents.iter().map(|(_, e)| e.clone()).collect();
                let tabs = by_tab(&list);
                // Headers say where once for a tab's agents; one agent alone keeps its own line.
                let headed = tabs.len() > 1 || tabs.first().is_some_and(|t| t.len() > 1);
                // Below the top bar, the popover's offset and its margin from the window's edge.
                let room = (window.viewport_size().height - TOP_BAR_HEIGHT - px(16.)).max(px(120.));
                // Each row's place among the scroller's children: after the title and its header.
                let mut child = 0;
                let slots: Vec<usize> = tabs
                    .iter()
                    .flat_map(|tab| {
                        child += 1 + usize::from(headed);
                        let first = child;
                        child += tab.len() - 1;
                        first..first + tab.len()
                    })
                    .collect();
                let keys = {
                    let (weak, order) = (weak.clone(), order.clone());
                    move |e: &KeyDownEvent, window: &mut Window, cx: &mut App| {
                        let panes: Vec<PaneId> = order.iter().map(|(p, _)| *p).collect();
                        match e.keystroke.key.as_str() {
                            "up" | "down" | "home" | "end" | "pageup" | "pagedown" => {
                                let _ = weak.update(cx, |b, cx| {
                                    let Some(to) = step(&panes, b.selected, &e.keystroke) else {
                                        return;
                                    };
                                    b.selected = Some(to);
                                    if let Some(&child) = b
                                        .selected
                                        .and_then(|s| panes.iter().position(|p| *p == s))
                                        .and_then(|row| slots.get(row))
                                    {
                                        b.scroll.scroll_to_item(child);
                                    }
                                    cx.notify();
                                });
                            }
                            "enter" => {
                                let Some(pane) = weak
                                    .upgrade()
                                    .and_then(|b| b.read(cx).selected)
                                    .filter(|s| panes.contains(s))
                                else {
                                    return;
                                };
                                let panel = order.iter().find(|(p, _)| *p == pane).map(|(_, w)| w);
                                pick(&weak, pane, panel, window, cx);
                            }
                            _ => return,
                        }
                        cx.stop_propagation();
                    }
                };
                let mut col = v_flex()
                    .id("terminal-agents-rows")
                    .track_focus(&focus)
                    .on_key_down(keys)
                    .track_scroll(&scroll)
                    .overflow_y_scroll()
                    .max_h(room)
                    .w(px(320.))
                    .p_1()
                    .gap_0p5()
                    .child(
                        div()
                            .px_2()
                            .pt_2()
                            .pb_1()
                            .text_size(rems(0.6875))
                            .font_weight(t.medium())
                            .text_color(t.subtle)
                            .child("AGENTS"),
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
                                .text_color(t.subtle)
                                .child(Icon::new(IconName::SquareTerminal).size(px(12.)))
                                .child(div().truncate().child(head.clone())),
                        );
                    }
                    for e in tab {
                        let look = e.look();
                        let lit = selected == Some(e.pane);
                        let place = place_of(&e.cwd, cx);
                        let mut state = e.state.label().to_string();
                        if look == Some(Look::Unread) {
                            state.push_str(", not viewed");
                        }
                        if e.since > 0. {
                            state.push_str(&format!(" · {}", short_duration(now - e.since)));
                        }
                        let news = look.is_some_and(Look::is_news);
                        // A highlighted row is in the primary's ink, its dot and state too.
                        let ink = lit.then_some(t.primary_foreground);
                        let soft = ink.map_or(t.muted_foreground, |i| i.opacity(0.7));
                        let state_color = match (ink, look) {
                            (Some(i), _) if news => i,
                            (None, Some(Look::Needs)) => t.destructive,
                            (None, Some(Look::Unread)) => t.info,
                            _ => soft,
                        };
                        let (pane, panel) = (e.pane, panel_of(e.pane));
                        let (hover, click) = (weak.clone(), weak.clone());
                        col = col.child(
                            h_flex()
                                .id(("agent-row", e.pane))
                                .items_start()
                                .gap_2()
                                .px_2()
                                .py_1p5()
                                .rounded(px(3.))
                                .cursor_pointer()
                                .when(lit, |d| d.bg(t.primary).text_color(t.primary_foreground))
                                .text_size(rems(0.75))
                                .on_hover(move |on, _, cx| {
                                    let _ = hover.update(cx, |b, cx| {
                                        if *on {
                                            b.selected = Some(pane);
                                        } else if b.selected == Some(pane) {
                                            b.selected = None;
                                        }
                                        cx.notify();
                                    });
                                })
                                .child(
                                    div()
                                        .h(px(16.))
                                        .w(px(10.))
                                        .flex()
                                        .items_center()
                                        .children(look.map(|l| menu_dot(l, ink, cx))),
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
                                                // Its pane's number, as the pane's header counts it.
                                                .when(e.of > 1, |d| {
                                                    d.child(
                                                        div()
                                                            .flex_none()
                                                            .font_family(t.fonts.code.clone())
                                                            .text_size(rems(0.625))
                                                            .text_color(
                                                                ink.unwrap_or(t.foreground)
                                                                    .opacity(0.7),
                                                            )
                                                            .child(format!("{} / {}", e.at, e.of)),
                                                    )
                                                })
                                                .when_some(e.pane_name.clone(), |d, n| {
                                                    d.child(
                                                        div()
                                                            .min_w_0()
                                                            .truncate()
                                                            .text_size(rems(0.65625))
                                                            .text_color(soft)
                                                            .child(n),
                                                    )
                                                })
                                                .child(
                                                    div()
                                                        .ml_auto()
                                                        .flex_none()
                                                        .text_size(rems(0.65625))
                                                        .text_color(state_color)
                                                        .child(state),
                                                ),
                                        )
                                        // The header says where, unless this pane opened elsewhere.
                                        .when(!headed || place != head, |d| {
                                            d.child(
                                                div()
                                                    .truncate()
                                                    .text_size(rems(0.65625))
                                                    .text_color(soft)
                                                    .child(place),
                                            )
                                        }),
                                )
                                .on_click(move |_, window, cx| {
                                    pick(&click, pane, panel.as_ref(), window, cx)
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
    use super::*;
    use crate::agent::PaneAgent;
    use gpui::TestAppContext;

    #[test]
    fn arrows_step_through_the_rows_without_wrapping() {
        let order = [7, 3, 9];
        let key = |k: &str| gpui::Keystroke::parse(k).unwrap();
        assert_eq!(step(&order, None, &key("down")), Some(7));
        assert_eq!(step(&order, None, &key("up")), Some(9));
        assert_eq!(step(&order, Some(3), &key("down")), Some(9));
        assert_eq!(step(&order, Some(9), &key("down")), Some(9));
        assert_eq!(step(&order, Some(7), &key("up")), Some(7));
        assert_eq!(step(&order, Some(9), &key("home")), Some(7));
        assert_eq!(step(&[], None, &key("down")), None);
    }

    /// Two windows' panels, each with an agent in its first tab: two tabs in the menu, each row
    /// going to its own panel. Each panel used to count from 1, so both were "tab 1, pane 1".
    #[gpui::test]
    fn two_windows_agents_stay_apart(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::init(cx);
        });
        let core = Arc::new(Core::new(Arc::new(|_| {})));
        let dir = std::env::temp_dir();
        let new_panel = |window: &mut Window, cx: &mut Context<TerminalPanel>| {
            TerminalPanel::new(
                core.clone(),
                dir.clone(),
                TerminalConfig::default(),
                TerminalTheme::dark(),
                window,
                cx,
            )
        };
        let (a, cx) = cx.add_window_view(|window, cx| new_panel(window, cx));
        let b = cx.update(|window, cx| cx.new(|cx| new_panel(window, cx)));
        for panel in [&a, &b] {
            panel.update_in(cx, |p, window, cx| {
                p.open_terminal(dir.clone(), None, window, cx);
                p.panes.values_mut().next().expect("a pane").agent = Some(PaneAgent {
                    name: "Claude Code".into(),
                    command: None,
                    session: None,
                    cwd: None,
                    state: None,
                });
            });
            let weak = panel.downgrade();
            cx.update(|_, cx| cx.default_global::<LivePanels>().0.push(weak));
        }
        let agents = cx.update(|_, cx| all_agents(cx));
        let list: Vec<AgentEntry> = agents.iter().map(|(_, e)| e.clone()).collect();
        assert_eq!(by_tab(&list).len(), 2, "{list:?}");
        let rows = rows(&agents);
        assert_eq!(rows.len(), 2);
        assert_ne!(rows[0].0, rows[1].0);
        for (pane, panel) in &rows {
            let panel = panel.upgrade().expect("its panel");
            assert!(panel.read_with(cx, |p, _| p.panes.contains_key(pane)));
        }
        for panel in [&a, &b] {
            panel.update(cx, |p, cx| p.close_all(cx));
        }
        cx.update(|window, _| window.remove_window());
        cx.run_until_parked();
    }

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
