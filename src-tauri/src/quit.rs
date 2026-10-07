//! ⌘Q and closing the main window. The page isn't unloaded on the way out, so what it saves on
//! pagehide (the terminals' output, unsaved edits) was lost: it's told first (lib/app/quit), asks
//! when an agent is working or a command runs, and ends the app itself once saved
//! (commands::app::quit). A page that doesn't answer still lets the app go after a moment.
//!
//! Logout and shutdown don't come here: tao has no applicationShouldTerminate, so macOS ends the
//! app without asking anyone, as it always did.

use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

/// How long the page has to answer before it's taken for hung.
const ANSWER_WAIT: Duration = Duration::from_secs(3);
/// How long the page has to save once it said go.
const SAVE_WAIT: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, PartialEq, Debug)]
enum Phase {
    Idle,
    /// The page was told and hasn't answered.
    Told,
    /// The page asks the user.
    Asking,
    Leaving,
}

/// The page's answer to "quit".
#[derive(Clone, Copy, PartialEq, Debug, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Answer {
    /// Something runs: it asks the user, however long that takes.
    Asking,
    /// The user cancelled.
    Stay,
    /// It saves now.
    Go,
}

#[derive(PartialEq, Debug)]
enum Then {
    Nothing,
    /// Tells the page; `now`: to save without asking.
    Tell {
        now: bool,
    },
    Raise,
    Leave,
}

/// The phase and the ⌘Q it belongs to: a hung page's timer from an earlier one must not end a later one.
static QUIT: Mutex<(Phase, u32)> = Mutex::new((Phase::Idle, 0));

fn on_quit(phase: Phase) -> (Phase, Then) {
    match phase {
        Phase::Idle => (Phase::Told, Then::Tell { now: false }),
        // A second ⌘Q while the page decides quits anyway, as macOS apps do; it still saves.
        Phase::Told | Phase::Asking => (Phase::Leaving, Then::Tell { now: true }),
        Phase::Leaving => (Phase::Leaving, Then::Nothing),
    }
}

fn on_answer(phase: Phase, answer: Answer) -> (Phase, Then) {
    match (phase, answer) {
        (Phase::Told, Answer::Asking) => (Phase::Asking, Then::Raise),
        (Phase::Told | Phase::Asking, Answer::Stay) => (Phase::Idle, Then::Nothing),
        (Phase::Told | Phase::Asking, Answer::Go) => (Phase::Leaving, Then::Leave),
        // Late: a second ⌘Q decided already.
        (phase, _) => (phase, Then::Nothing),
    }
}

/// Whether the app is on its way out: the main window may close then.
pub fn quitting() -> bool {
    QUIT.lock().unwrap_or_else(|e| e.into_inner()).0 == Phase::Leaving
}

/// ⌘Q, or the main window's close.
pub fn request(app: &AppHandle) {
    let (then, round) = step(on_quit);
    let Then::Tell { now } = then else {
        return;
    };
    // The workspace's page only: the settings window's would end the app before it saved.
    let _ = app.emit_to("main", "quit", now);
    if now {
        return leave_soon(app);
    }
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(ANSWER_WAIT);
        if *QUIT.lock().unwrap_or_else(|e| e.into_inner()) == (Phase::Told, round) {
            app.exit(0);
        }
    });
}

/// The page's answer (commands::app::quit_answer).
pub fn answer(app: &AppHandle, answer: Answer) {
    match step(|p| on_answer(p, answer)).0 {
        // The ask shows on the main window: with Settings in front it went unseen.
        Then::Raise => crate::opened::raise(app),
        Then::Leave => leave_soon(app),
        _ => {}
    }
}

/// Moves the phase on; a new ⌘Q (into Told) starts a new round.
fn step(f: impl FnOnce(Phase) -> (Phase, Then)) -> (Then, u32) {
    let mut q = QUIT.lock().unwrap_or_else(|e| e.into_inner());
    let (phase, then) = f(q.0);
    if phase == Phase::Told && q.0 != Phase::Told {
        q.1 = q.1.wrapping_add(1);
    }
    q.0 = phase;
    (then, q.1)
}

/// Ends the app once the page had its moment to save, if it hasn't ended it itself.
fn leave_soon(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(SAVE_WAIT);
        app.exit(0);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quit_asks_the_page_then_waits_for_go() {
        assert_eq!(
            on_quit(Phase::Idle),
            (Phase::Told, Then::Tell { now: false })
        );
        assert_eq!(
            on_answer(Phase::Told, Answer::Go),
            (Phase::Leaving, Then::Leave)
        );
        assert_eq!(
            on_answer(Phase::Told, Answer::Asking),
            (Phase::Asking, Then::Raise)
        );
        assert_eq!(
            on_answer(Phase::Asking, Answer::Go),
            (Phase::Leaving, Then::Leave)
        );
    }

    #[test]
    fn cancel_keeps_the_app_and_a_later_quit_asks_again() {
        assert_eq!(
            on_answer(Phase::Asking, Answer::Stay),
            (Phase::Idle, Then::Nothing)
        );
        assert_eq!(on_quit(Phase::Idle).0, Phase::Told);
    }

    #[test]
    fn a_second_quit_while_deciding_leaves_at_once() {
        for p in [Phase::Told, Phase::Asking] {
            assert_eq!(on_quit(p), (Phase::Leaving, Then::Tell { now: true }));
        }
        assert_eq!(on_quit(Phase::Leaving), (Phase::Leaving, Then::Nothing));
    }

    #[test]
    fn a_late_answer_changes_nothing() {
        for a in [Answer::Asking, Answer::Stay, Answer::Go] {
            assert_eq!(
                on_answer(Phase::Leaving, a),
                (Phase::Leaving, Then::Nothing)
            );
            assert_eq!(on_answer(Phase::Idle, a), (Phase::Idle, Then::Nothing));
        }
        assert_eq!(
            on_answer(Phase::Asking, Answer::Asking),
            (Phase::Asking, Then::Nothing)
        );
    }
}
