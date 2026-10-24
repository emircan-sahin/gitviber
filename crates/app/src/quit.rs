//! ⌘Q and closing the main window: asks first while leaving would stop something (an agent
//! working, a command running in a terminal, a push or pull on its way), then saves the state,
//! ends the terminals' shells and goes. A second ⌘Q while it asks quits at once, as macOS apps do.

use std::time::{Duration, Instant};

use gitviber_terminal::TerminalPanel;
use gitviber_ui::actions::app;
use gitviber_ui::{Settings, state};
use gpui_kit::{App, AppContext as _, Global, Task};

/// A held ⌘Q repeats its key equivalent: a second quit sooner than this is the same press.
const REPEAT: Duration = Duration::from_secs(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Idle,
    /// Looking at what runs, or asking the user about it.
    Asking,
    Leaving,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Then {
    Nothing,
    /// Look at what runs, and ask if anything does.
    Ask,
    Leave,
}

/// `since`: how long ago the quit being decided came.
pub fn on_quit(phase: Phase, since: Duration, ask: bool) -> (Phase, Then) {
    match phase {
        Phase::Idle if ask => (Phase::Asking, Then::Ask),
        Phase::Idle => (Phase::Leaving, Then::Leave),
        Phase::Asking if since < REPEAT => (phase, Then::Nothing),
        Phase::Asking => (Phase::Leaving, Then::Leave),
        Phase::Leaving => (Phase::Leaving, Then::Nothing),
    }
}

/// The user's answer, or what was found running (`go` when nothing was).
pub fn on_answer(phase: Phase, go: bool) -> (Phase, Then) {
    match phase {
        Phase::Asking if go => (Phase::Leaving, Then::Leave),
        Phase::Asking => (Phase::Idle, Then::Nothing),
        // Late: a second ⌘Q decided already.
        phase => (phase, Then::Nothing),
    }
}

struct Quit {
    phase: Phase,
    at: Option<Instant>,
    _asking: Option<Task<()>>,
}

impl Global for Quit {}

fn get(cx: &mut App) -> &mut Quit {
    if !cx.has_global::<Quit>() {
        cx.set_global(Quit {
            phase: Phase::Idle,
            at: None,
            _asking: None,
        });
    }
    cx.global_mut::<Quit>()
}

/// Registered after the shared window commands: the last listener runs first and stops there.
pub fn install(cx: &mut App) {
    cx.on_action(|_: &app::Quit, cx| request(cx));
}

/// ⌘Q, or the main window's close.
pub fn request(cx: &mut App) {
    let ask = Settings::get(cx).ask_before_quit;
    let q = get(cx);
    let since = q.at.map_or(Duration::MAX, |at| at.elapsed());
    let (phase, then) = on_quit(q.phase, since, ask);
    if phase == Phase::Asking && q.phase != Phase::Asking {
        q.at = Some(Instant::now());
    }
    q.phase = phase;
    match then {
        Then::Leave => leave(cx),
        Then::Ask => {
            let running = stopped_by_leaving(cx);
            let task = cx.spawn(async move |cx| {
                let running = running.await;
                cx.update(|cx| ask_about(running, cx));
            });
            get(cx)._asking = Some(task);
        }
        Then::Nothing => {}
    }
}

fn answered(go: bool, cx: &mut App) {
    let q = get(cx);
    let (phase, then) = on_answer(q.phase, go);
    q.phase = phase;
    if then == Then::Leave {
        leave(cx);
    }
}

/// What leaving now stops that's worth asking about: a push or pull on its way, the terminals'
/// commands and working agents. An agent done with its turn is resumed on the next run.
fn stopped_by_leaving(cx: &mut App) -> Task<Vec<String>> {
    let net = gitviber_refs::net::NetActivity::global(cx)
        .read(cx)
        .current()
        .map(|r| r.label.to_string());
    let ptys: Vec<u32> = crate::workspace_view::main_workspace(cx)
        .and_then(|ws| ws.read(cx).built_panel("terminal").cloned())
        .and_then(|p| p.view().downcast::<TerminalPanel>().ok())
        .map(|panel| {
            panel
                .read(cx)
                .panes()
                .filter_map(|p| p.read(cx).pty_id())
                .collect()
        })
        .unwrap_or_default();
    if !gitviber_ui::has_core(cx) {
        return Task::ready(net.into_iter().collect());
    }
    let core = gitviber_ui::core(cx);
    cx.background_spawn(async move {
        let busy: Vec<(bool, Option<&gitviber_terminal::agent::PaneAgent>)> = ptys
            .iter()
            .map(|id| (core.ptys.busy(Some(&[*id])) > 0, None))
            .collect();
        let mut out: Vec<String> = net.into_iter().collect();
        out.extend(gitviber_terminal::agent::quit_stops(&busy));
        out
    })
}

fn ask_about(running: Vec<String>, cx: &mut App) {
    if get(cx).phase != Phase::Asking {
        return;
    }
    if running.is_empty() {
        return answered(true, cx);
    }
    // On the main window, brought forward: with Settings in front the ask went unseen.
    let Some(handle) = crate::workspace_view::main_window(cx)
        .or_else(|| cx.active_window())
        .or_else(|| cx.windows().into_iter().next())
    else {
        return answered(true, cx);
    };
    let message = format!(
        "Quitting GitViber stops what's still running: {}.",
        running.join(", ")
    );
    let asked = handle.update(cx, |_, window, cx| {
        window.activate_window();
        gitviber_refs::ask::ask(
            gitviber_refs::ask::Ask::new("Quit GitViber", message, "Quit").danger(),
            window,
            cx,
        )
    });
    let Ok(asked) = asked else {
        return answered(true, cx);
    };
    let task = cx.spawn(async move |cx| {
        let go = asked.await == Ok(true);
        cx.update(|cx| answered(go, cx));
    });
    get(cx)._asking = Some(task);
}

/// Saves what waits to be saved, ends the shells (a pty's child would outlive the app), quits.
fn leave(cx: &mut App) {
    get(cx).phase = Phase::Leaving;
    state::flush(cx);
    crate::crash::quitting();
    if gitviber_ui::has_core(cx) {
        gitviber_ui::core(cx).ptys.kill_all();
    }
    cx.quit();
}

#[cfg(test)]
mod tests {
    use super::*;

    const LATER: Duration = Duration::from_secs(5);

    #[test]
    fn quit_asks_then_leaves_on_go() {
        assert_eq!(
            on_quit(Phase::Idle, LATER, true),
            (Phase::Asking, Then::Ask)
        );
        assert_eq!(
            on_answer(Phase::Asking, true),
            (Phase::Leaving, Then::Leave)
        );
        // Asking is off: straight out.
        assert_eq!(
            on_quit(Phase::Idle, LATER, false),
            (Phase::Leaving, Then::Leave)
        );
    }

    #[test]
    fn cancel_keeps_the_app_and_a_later_quit_asks_again() {
        assert_eq!(
            on_answer(Phase::Asking, false),
            (Phase::Idle, Then::Nothing)
        );
        assert_eq!(on_quit(Phase::Idle, LATER, true).0, Phase::Asking);
    }

    #[test]
    fn a_second_quit_while_asking_leaves_at_once() {
        assert_eq!(
            on_quit(Phase::Asking, LATER, true),
            (Phase::Leaving, Then::Leave)
        );
        assert_eq!(
            on_quit(Phase::Leaving, LATER, true),
            (Phase::Leaving, Then::Nothing)
        );
    }

    #[test]
    fn a_held_quit_key_repeating_is_one_press() {
        let soon = Duration::from_millis(80);
        assert_eq!(
            on_quit(Phase::Asking, soon, true),
            (Phase::Asking, Then::Nothing)
        );
    }

    #[test]
    fn a_late_answer_changes_nothing() {
        for go in [true, false] {
            assert_eq!(
                on_answer(Phase::Leaving, go),
                (Phase::Leaving, Then::Nothing)
            );
            assert_eq!(on_answer(Phase::Idle, go), (Phase::Idle, Then::Nothing));
        }
    }
}
