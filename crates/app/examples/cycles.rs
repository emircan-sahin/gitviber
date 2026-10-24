//! Opens and closes what the app opens and closes, many times, and prints the footprint and
//! the live view counts before and after: they should come back to where they were.
//! `cargo build -p gitviber --example cycles`, then
//! `GITVIBER_BIN=target/debug/examples/cycles shot.sh out.png 30 /repo/a /repo/b`.

use std::path::PathBuf;
use std::time::Duration;

use gitviber_app::{Palette, PaletteMode, open_settings};
use gitviber_ui::Workspace;
use gitviber_ui::memory::{footprint, format_bytes, live_counts};
use gpui_kit::component::WindowExt as _;
use gpui_kit::{App, AsyncApp};

const ROUNDS: usize = 30;

fn report(label: &str) {
    let live: Vec<String> = live_counts()
        .iter()
        .map(|(k, n)| format!("{k}={n}"))
        .collect();
    println!(
        "{label}: {} live [{}]",
        footprint().map(format_bytes).unwrap_or_default(),
        live.join(" ")
    );
}

fn main_window(cx: &mut App) -> Option<gpui_kit::AnyWindowHandle> {
    cx.windows().into_iter().next()
}

async fn round(projects: &[PathBuf], i: usize, cx: &mut AsyncApp) {
    let pause = Duration::from_millis(120);
    cx.update(open_settings);
    cx.background_executor().timer(pause).await;
    cx.update(|cx| {
        // The settings window is the newest.
        if let Some(settings) = cx
            .windows()
            .into_iter()
            .last()
            .filter(|w| Some(*w) != main_window(cx))
        {
            let _ = settings.update(cx, |_, window, _| window.remove_window());
        }
    });
    cx.background_executor().timer(pause).await;
    cx.update(|cx| {
        let Some(main) = main_window(cx) else { return };
        let _ = main.update(cx, |_, window, cx| {
            let Some(ws) = Workspace::for_window(window, cx) else {
                return;
            };
            Palette::open(
                if i.is_multiple_of(2) {
                    PaletteMode::Commands
                } else {
                    PaletteMode::Files
                },
                &ws,
                window,
                cx,
            );
        });
    });
    cx.background_executor().timer(pause).await;
    cx.update(|cx| {
        let Some(main) = main_window(cx) else { return };
        let _ = main.update(cx, |_, window, cx| {
            window.close_dialog(cx);
            if let (Some(ws), Some(root)) = (
                Workspace::for_window(window, cx),
                projects.get(i % projects.len().max(1)),
            ) {
                ws.update(cx, |ws, cx| ws.open_project(root.clone(), window, cx));
            }
        });
    });
    cx.background_executor().timer(pause).await;
}

fn main() {
    let projects: Vec<PathBuf> = std::env::args_os()
        .skip(1)
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .collect();
    gitviber_app::run(move |cx| {
        cx.spawn(async move |cx| {
            cx.background_executor().timer(Duration::from_secs(2)).await;
            report("before");
            round(&projects, 0, cx).await;
            report("after 1 round");
            for i in 1..ROUNDS {
                round(&projects, i, cx).await;
            }
            cx.background_executor().timer(Duration::from_secs(1)).await;
            report(&format!("after {ROUNDS} rounds"));
        })
        .detach();
    });
}
