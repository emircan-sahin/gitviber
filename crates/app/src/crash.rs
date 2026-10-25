//! When something breaks. A panic off the UI thread (a background load) leaves the app running:
//! it's said in a toast instead of passing unseen, while core's hook writes it to the log. One on
//! the UI thread ends the app, as GPUI can't unwind through the platform; the next launch says
//! the last one didn't end cleanly, and where the log is.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use futures::StreamExt as _;
use futures::channel::mpsc;
use gitviber_ui::store;
use gitviber_ui::workspace::Toast;
use gpui_kit::{App, AppContext as _, Global, Task};

use crate::help::toast;

/// Toasts for a burst of panics (one looping) stop after the first in this time.
const QUIET: Duration = Duration::from_secs(5);

struct Crashes(#[allow(dead_code)] Task<()>);
impl Global for Crashes {}

fn marker() -> PathBuf {
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
    // The last launch's marker still there: it didn't get to quit.
    let path = marker();
    let unclean = path.exists();
    mark(&path, cx);
    if unclean {
        // Runs once `run` has opened the window and returned: with its first frames.
        cx.spawn(async move |cx| {
            cx.update(|cx| {
                toast(
                    Toast::warning("Help → Show Logs has what happened, if it was an error.")
                        .title("GitViber didn't quit cleanly last time"),
                    cx,
                )
            })
        })
        .detach();
    }
    cx.set_global(Crashes(task));
    cx.on_app_quit(|_| {
        quitting();
        async {}
    })
    .detach();
}

/// Written at once, so a crash later in the launch is caught. The sync that makes a power cut
/// count too (F_FULLFSYNC, 11–19 ms) runs off the UI thread, and opens without creating: a quit
/// before it leaves no marker behind.
fn mark(path: &Path, cx: &App) {
    let written = path
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| std::fs::write(path, std::process::id().to_string()));
    if written.is_ok() {
        let path = path.to_path_buf();
        cx.background_spawn(async move {
            if let Ok(file) = std::fs::File::options().write(true).open(&path) {
                let _ = file.sync_all();
            }
        })
        .detach();
    }
}

/// A clean quit: the next launch has nothing to say.
pub fn quitting() {
    let _ = std::fs::remove_file(marker());
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::component::WindowExt as _;
    use gpui_kit::{
        AnyWindowHandle, Context, IntoElement, Render, TestAppContext, Window, WindowOptions, div,
    };

    struct Host;
    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
        }
    }

    /// A launch as `run` makes it: installed, then the window opened in the same turn. Returns
    /// the marker and the window.
    fn launch(name: &str, crashed: bool, cx: &mut TestAppContext) -> (PathBuf, AnyWindowHandle) {
        let dir = gitviber_ui::test_support::temp_dir(&format!("crash-{name}"));
        if crashed {
            std::fs::write(dir.join("running"), "1").unwrap();
        }
        // SAFETY: the tests that read the config dir all point it at a temporary folder.
        unsafe { std::env::set_var("GITVIBER_CONFIG_DIR", &dir) };
        cx.update(gitviber_ui::init);
        let window = cx.update(|cx| {
            install(cx);
            gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| cx.new(|_| Host))
                .unwrap()
                .0
        });
        (dir.join("running"), window)
    }

    fn toasts(window: AnyWindowHandle, cx: &mut TestAppContext) -> usize {
        window
            .update(cx, |_, window, cx| window.notifications(cx).len())
            .unwrap()
    }

    /// No clock is advanced: the toast waits on no timer.
    #[gpui_kit::test]
    fn an_unclean_end_is_said_with_the_first_frames(cx: &mut TestAppContext) {
        let (marker, window) = launch("unclean", true, cx);
        cx.run_until_parked();
        assert_eq!(toasts(window, cx), 1);
        quitting();
        assert!(!marker.exists());
    }

    #[gpui_kit::test]
    fn the_marker_is_down_before_the_window_and_a_quit_takes_it(cx: &mut TestAppContext) {
        let (marker, window) = launch("clean", false, cx);
        assert!(marker.exists(), "written before the window's first frame");
        // Before the background sync has run: it finds no file, and makes none.
        quitting();
        cx.run_until_parked();
        assert!(!marker.exists());
        assert_eq!(toasts(window, cx), 0);
    }

    #[test]
    fn a_panic_reads_as_its_first_line_and_place() {
        assert_eq!(
            summary("index out of bounds\nmore", Some("src/a.rs:3".into())),
            "index out of bounds (src/a.rs:3)"
        );
        assert_eq!(summary("", None), "");
    }
}
