//! When something breaks. A panic off the UI thread (a background load) leaves the app running:
//! it's said in a toast instead of passing unseen, while core's hook writes it to the log. One on
//! the UI thread ends the app, as GPUI can't unwind through the platform; the next launch says
//! the last one didn't end cleanly, and where the log is.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use futures::StreamExt as _;
use futures::channel::mpsc;
use gitviber_ui::store;
use gitviber_ui::workspace::Toast;
use gpui_kit::{App, Global, Task};

use crate::help::toast;

/// Toasts for a burst of panics (one looping) stop after the first in this time.
const QUIET: Duration = Duration::from_secs(5);

struct Crashes(#[allow(dead_code)] Task<()>, #[allow(dead_code)] Task<()>);
impl Global for Crashes {}

fn marker() -> std::path::PathBuf {
    store::config_dir().join("running")
}

/// What a panic says, on one line, for the toast.
pub fn summary(payload: &str, location: Option<String>) -> String {
    let first = payload.lines().next().unwrap_or("").trim();
    match location {
        Some(at) => format!("{first} ({at})"),
        None => first.to_string(),
    }
}

/// After core has set its own hook (it logs): wraps it.
pub fn install(cx: &mut App) {
    let main = std::thread::current().id();
    let (tx, mut rx) = mpsc::unbounded::<String>();
    let tx = Mutex::new(tx);
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        previous(info);
        if std::thread::current().id() == main {
            return;
        }
        let payload = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_default();
        let at = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()));
        if let Ok(tx) = tx.lock() {
            let _ = tx.unbounded_send(summary(&payload, at));
        }
    }));
    let task = cx.spawn(async move |cx| {
        let mut last: Option<Instant> = None;
        while let Some(message) = rx.next().await {
            if last.is_some_and(|t| t.elapsed() < QUIET) {
                continue;
            }
            last = Some(Instant::now());
            cx.update(|cx| {
                toast(
                    Toast::error(format!("{message}. Help → Show Logs has the details."))
                        .title("GitViber ran into an error"),
                    cx,
                )
            });
        }
    });
    // The last launch's marker still there: it didn't get to quit. Said once the window is up.
    let path = marker();
    let unclean = path.exists();
    let _ = store::write_atomic(&path, &std::process::id().to_string());
    let said = cx.spawn(async move |cx| {
        if !unclean {
            return;
        }
        cx.background_executor()
            .timer(Duration::from_millis(800))
            .await;
        cx.update(|cx| {
            toast(
                Toast::warning("Help → Show Logs has what happened, if it was an error.")
                    .title("GitViber didn't quit cleanly last time"),
                cx,
            )
        });
    });
    cx.set_global(Crashes(task, said));
    cx.on_app_quit(|_| {
        quitting();
        async {}
    })
    .detach();
}

/// A clean quit: the next launch has nothing to say.
pub fn quitting() {
    let _ = std::fs::remove_file(marker());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_panic_reads_as_its_first_line_and_place() {
        assert_eq!(
            summary("index out of bounds\nmore", Some("src/a.rs:3".into())),
            "index out of bounds (src/a.rs:3)"
        );
        assert_eq!(summary("", None), "");
    }
}
