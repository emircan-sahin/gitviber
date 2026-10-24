//! The GitViber app as a library: the binary's `main` and the examples run it.

mod layout;
mod menus;
mod palette;
mod placeholders;
mod settings_window;
mod status_bar;
mod tab_strip;
mod top_bar;
mod welcome;
mod workspace_view;

use std::path::PathBuf;

use gitviber_ui::assets::Assets;
use gitviber_ui::{UiState, windows};
use gpui_kit::{App, AppContext as _};

pub use palette::{Mode as PaletteMode, Palette};
pub use settings_window::open as open_settings;
pub use workspace_view::{WorkspaceView, open_main_window};

/// The project to open: `gitviber <path>`, else the one used last, while it's there.
pub fn project_to_open(cx: &App) -> Option<PathBuf> {
    first_folder(project_candidates(cx))
}

/// Where a launch could open, in order: the arguments, then the project used last.
fn project_candidates(cx: &App) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    out.extend(UiState::get(cx).recent_projects.first().map(PathBuf::from));
    out
}

/// The first of `candidates` that is a folder, as its real path. Reads the disk: off the UI
/// thread, as a gone network mount can take seconds to say so.
fn first_folder(candidates: Vec<PathBuf>) -> Option<PathBuf> {
    candidates
        .into_iter()
        .filter(|p| p.is_dir())
        .find_map(|p| p.canonicalize().ok())
}

/// Everything before the first window: the UI layer, core, the app's commands, menus and
/// placeholders.
pub fn init(cx: &mut App) {
    gitviber_ui::init(cx);
    let core = gitviber_ui::start_core(cx);
    windows::install_app_actions(cx);
    placeholders::register(cx);
    // Features after the placeholders: a panel or chip registered with the same id replaces one.
    gitviber_diff::set_core(core, cx);
    gitviber_diff::init(cx);
    gitviber_terminal::init(cx);
    gitviber_files::init(cx);
    gitviber_refs::init(cx);
    gitviber_history::init(cx);
    gitviber_github::init(cx);
    // Bridges last: they hand one feature's data to another's hooks.
    gitviber_github::install_bridges(cx);
    settings_window::install(cx);
    menus::install(cx);
}

/// Starts the app, then `started` once it's up (the main window opens first).
pub fn run(started: impl FnOnce(&mut App) + 'static) {
    #[cfg(feature = "snapshot")]
    if let Some(out) = std::env::var_os("GITVIBER_SNAPSHOT") {
        let wait = std::env::var("GITVIBER_SNAPSHOT_WAIT")
            .ok()
            .and_then(|s| s.parse::<f32>().ok())
            .unwrap_or(3.);
        let size = gpui_kit::size(gpui_kit::px(1400.), gpui_kit::px(900.));
        let shot = gitviber_ui::dev::snapshot_with(
            std::path::Path::new(&out),
            size,
            wait,
            init,
            |window, cx| workspace_view::main_view(project_to_open(cx), window, cx).into(),
        );
        if let Err(err) = shot {
            eprintln!("gitviber: snapshot failed: {err}");
            std::process::exit(1);
        }
        std::process::exit(0);
    }
    gpui_kit::application().with_assets(Assets).run(move |cx| {
        init(cx);
        let candidates = project_candidates(cx);
        open_main_window(None, cx);
        cx.spawn(async move |cx| {
            let Some(root) = cx
                .background_spawn(async move { first_folder(candidates) })
                .await
            else {
                return;
            };
            cx.update(|cx| {
                let Some(main) = cx.windows().into_iter().next() else {
                    return;
                };
                let _ = main.update(cx, |_, window, cx| {
                    if let Some(ws) = gitviber_ui::Workspace::for_window(window, cx) {
                        ws.update(cx, |ws, cx| ws.open_project(root, window, cx));
                    }
                });
            });
        })
        .detach();
        if !windows::background_launch() {
            cx.activate(true);
        }
        started(cx);
    });
}
