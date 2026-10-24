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
use gpui_kit::App;

pub use palette::{Mode as PaletteMode, Palette};
pub use settings_window::open as open_settings;
pub use workspace_view::{WorkspaceView, open_main_window};

/// The project to open: `gitviber <path>`, else the one used last, while it's there.
pub fn project_to_open(cx: &App) -> Option<PathBuf> {
    let from_args = std::env::args_os()
        .skip(1)
        .map(PathBuf::from)
        .find(|p| p.is_dir());
    let last = || {
        UiState::get(cx)
            .recent_projects
            .first()
            .map(PathBuf::from)
            .filter(|p| p.is_dir())
    };
    from_args.and_then(|p| p.canonicalize().ok()).or_else(last)
}

/// Everything before the first window: the UI layer, core, the app's commands, menus and
/// placeholders.
pub fn init(cx: &mut App) {
    gitviber_ui::init(cx);
    gitviber_ui::start_core(cx);
    windows::install_app_actions(cx);
    placeholders::register(cx);
    settings_window::install(cx);
    menus::install(cx);
}

/// Starts the app, then `started` once it's up (the main window opens first).
pub fn run(started: impl FnOnce(&mut App) + 'static) {
    gpui_kit::application().with_assets(Assets).run(move |cx| {
        init(cx);
        let project = project_to_open(cx);
        open_main_window(project, cx);
        if !windows::background_launch() {
            cx.activate(true);
        }
        started(cx);
    });
}
