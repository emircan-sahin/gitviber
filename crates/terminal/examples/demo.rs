//! A window with the terminal panel running the user's shell in a folder (the first argument
//! that is one, else the current folder), for looking at it without the rest of the app.
//!
//! `--run <command>` types a command at the first prompt; `--split <command>` splits right and
//! types one there; `--find <text>` searches for it; `--memtest` prints the process's memory
//! with one idle shell, after `seq 1 100000` in a second, after closing it, and after ten more
//! open/flood/close cycles.
//! `GITVIBER_BACKGROUND=1` opens it without taking focus; with `GITVIBER_SNAPSHOT=out.png` (snap.sh)
//! it draws off screen into a picture instead (GITVIBER_APPEARANCE=light for the light theme).

use std::path::PathBuf;
use std::time::Duration;

use gitviber_terminal::TerminalPanel;
use gitviber_ui::workspace::Workspace;
use gpui_kit::{AppContext as _, Entity, Window, px};

struct Args {
    root: PathBuf,
    run: Option<String>,
    split: Option<String>,
    memtest: bool,
    find: Option<String>,
}

fn args() -> Args {
    let mut out = Args {
        root: std::env::current_dir().unwrap_or_default(),
        run: None,
        split: None,
        memtest: false,
        find: None,
    };
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--run" => out.run = it.next(),
            "--split" => out.split = it.next(),
            "--memtest" => out.memtest = true,
            "--find" => out.find = it.next(),
            dir if std::path::Path::new(dir).is_dir() => out.root = PathBuf::from(dir),
            _ => {}
        }
    }
    out
}

/// The process's footprint and the terminals alive, as the app's memory readout counts them.
fn memory() -> String {
    let mb = gitviber_ui::memory::footprint().map_or(0, |b| b / 1_048_576);
    let live: Vec<String> = gitviber_ui::memory::live_counts()
        .iter()
        .map(|(k, n)| format!("{k}={n}"))
        .collect();
    format!("footprint={mb}MB live: {}", live.join(" "))
}

fn main() {
    let a = args();
    let background = std::env::var("GITVIBER_BACKGROUND").is_ok_and(|v| v == "1");
    let snapshot = std::env::var_os("GITVIBER_SNAPSHOT").is_some();
    gitviber_ui::dev::run(move |window, cx| {
        gitviber_terminal::init(cx);
        let ws = Workspace::for_window(window, cx).expect("the dev host's workspace");
        let ctx = ws.update(cx, |ws, cx| ws.context(cx));
        let panel = cx.new(|cx| TerminalPanel::for_workspace(&ctx, window, cx));
        let root = a.root.clone();
        let run = |cmd: &Option<String>| cmd.as_ref().map(|c| format!("{c}\r"));
        let first = if a.memtest { None } else { run(&a.run) };
        panel.update(cx, |p, cx| p.open_terminal(root.clone(), first, window, cx));
        if let Some(cmd) = &a.split {
            split_later(&panel, cmd.clone(), window, cx);
        }
        if a.memtest {
            memtest(&panel, root.clone(), window, cx);
        }
        if let Some(query) = a.find.clone() {
            let panel = panel.downgrade();
            window
                .spawn(cx, async move |cx| {
                    cx.background_executor()
                        .timer(Duration::from_millis(4000))
                        .await;
                    let _ = panel.update_in(cx, |p, window, cx| p.find_text(&query, window, cx));
                })
                .detach();
        }
        if background && !snapshot {
            repaint_while_covered(window, cx);
        }
        panel.into()
    });
}

/// macOS stops GPUI's frames for a window covered by others, as a background launch's is: a
/// resize makes AppKit ask for one, so the screenshot shows the terminal as it is now.
fn repaint_while_covered(window: &mut Window, cx: &mut gpui_kit::App) {
    window
        .spawn(cx, async move |cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(700))
                    .await;
                let ok = cx.update(|window, _| {
                    let size = window.viewport_size();
                    window.resize(gpui_kit::size(size.width, size.height + px(1.)));
                    window.resize(size);
                });
                if ok.is_err() {
                    break;
                }
            }
        })
        .detach();
}

/// Splits once the first shell has started, so the split starts where it is.
fn split_later(
    panel: &Entity<TerminalPanel>,
    cmd: String,
    window: &mut Window,
    cx: &mut gpui_kit::App,
) {
    let panel = panel.downgrade();
    window
        .spawn(cx, async move |cx| {
            cx.background_executor()
                .timer(Duration::from_millis(1500))
                .await;
            let _ = panel.update_in(cx, |p, window, cx| {
                p.split_running(
                    gitviber_terminal::layout::Dir::Row,
                    None,
                    Some(format!("{cmd}\r")),
                    window,
                    cx,
                );
            });
        })
        .detach();
}

/// The memory with one idle shell, after a 100k-line flood in a second one, after closing that,
/// and after ten more open/flood/close cycles.
fn memtest(
    panel: &Entity<TerminalPanel>,
    root: PathBuf,
    window: &mut Window,
    cx: &mut gpui_kit::App,
) {
    let panel = panel.downgrade();
    window
        .spawn(cx, async move |cx| {
            let exec = cx.background_executor().clone();
            let wait = |ms| exec.timer(Duration::from_millis(ms));
            wait(5000).await;
            eprintln!("memtest: one idle shell: {}", memory());
            let _ = panel.update_in(cx, |p, window, cx| {
                p.open_terminal(root.clone(), Some("seq 1 100000\r".into()), window, cx)
            });
            wait(9000).await;
            eprintln!("memtest: after seq 1 100000: {}", memory());
            let _ = panel.update_in(cx, |p, window, cx| p.close_tab_at(1, window, cx));
            wait(3000).await;
            eprintln!("memtest: after closing it: {}", memory());
            for _ in 0..10 {
                let _ = panel.update_in(cx, |p, window, cx| {
                    p.open_terminal(root.clone(), Some("seq 1 100000\r".into()), window, cx)
                });
                // Done by then: a busy one would ask before closing.
                wait(5000).await;
                let _ = panel.update_in(cx, |p, window, cx| p.close_tab_at(1, window, cx));
                wait(500).await;
            }
            wait(3000).await;
            let tabs = panel
                .update(cx, |p, cx| {
                    p.panes().filter(|v| !v.read(cx).has_exited()).count()
                })
                .unwrap_or(0);
            eprintln!(
                "memtest: after 10 open/flood/close cycles ({tabs} terminal left): {}",
                memory()
            );
        })
        .detach();
}
