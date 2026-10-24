//! The GitViber app as a library: the binary's `main` and the examples run it.

mod crash;
mod dialogs;
mod help;
mod instance;
mod layout;
#[cfg(test)]
mod leaks;
mod menus;
mod opened;
mod overlay;
mod palette;
mod placeholders;
mod projects;
mod quit;
mod settings_window;
mod status_bar;
mod tab_strip;
mod top_bar;
pub mod updates;
mod welcome;
mod workspace_view;

use std::path::PathBuf;

use gitviber_ui::assets::Assets;
use gitviber_ui::{UiState, windows};
use gpui_kit::App;

pub use instance::second_launch;
pub use palette::{Mode as PaletteMode, Palette};
pub use settings_window::open as open_settings;
pub use workspace_view::{WorkspaceView, open_main_window};

/// The project a snapshot opens: `gitviber <path>`, else the one used last, while it's there.
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
    // On the first launch the old app's data goes into the files the stores load from here on.
    gitviber_ui::legacy::import_once();
    gitviber_ui::init(cx);
    let core = gitviber_ui::start_core(cx);
    // After core's own panic hook, which it wraps; a snapshot isn't a session to warn about.
    if std::env::var_os("GITVIBER_SNAPSHOT").is_none() {
        crash::install(cx);
    }
    windows::install_app_actions(cx);
    placeholders::register(cx);
    // Features after the placeholders: a panel or chip registered with the same id replaces one.
    // The diff's own keys are only those the app's commands don't cover (Changes binds them).
    gitviber_diff::set_core(core, cx);
    gitviber_changes::init(cx);
    gitviber_terminal::init(cx);
    gitviber_browser::init(cx);
    gitviber_files::init(cx);
    gitviber_obsidian::init(cx);
    gitviber_refs::init(cx);
    gitviber_history::init(cx);
    gitviber_github::init(cx);
    gitviber_review::init(cx);
    // Bridges last: they hand one feature's data to another's hooks.
    gitviber_github::install_bridges(cx);
    settings_window::install(cx);
    help::install(cx);
    dialogs::install(cx);
    quit::install(cx);
    menus::install(cx);
    // Every store has taken its keys in its init.
    gitviber_ui::legacy::finish();
}

/// GITVIBER_SNAPSHOT_OPEN pictures more than the main window: `settings:<section>` the settings
/// window, `overlay`, `palette`, `about`… one of the main window's dialogs over it.
#[cfg(feature = "snapshot")]
fn snapshot_root(window: &mut gpui_kit::Window, cx: &mut App) -> gpui_kit::AnyView {
    let open = std::env::var("GITVIBER_SNAPSHOT_OPEN").unwrap_or_default();
    if let Some(section) = open.strip_prefix("settings") {
        let section = section
            .strip_prefix(':')
            .and_then(settings_window::Section::parse)
            .unwrap_or(settings_window::Section::Appearance);
        return settings_window::view(section, window, cx).into();
    }
    let view = workspace_view::main_view(project_to_open(cx), window, cx);
    if !open.is_empty() {
        window.defer(cx, move |window, cx| snapshot_open(&open, window, cx));
    }
    view.into()
}

#[cfg(feature = "snapshot")]
fn snapshot_open(what: &str, window: &mut gpui_kit::Window, cx: &mut App) {
    let Some(ws) = gitviber_ui::Workspace::for_window(window, cx) else {
        return;
    };
    match what {
        "palette" => Palette::open(PaletteMode::Commands, &ws, window, cx),
        "files" => Palette::open(PaletteMode::Files, &ws, window, cx),
        "overlay" => window.dispatch_action(
            Box::new(gitviber_ui::actions::workbench::ShortcutOverlay),
            cx,
        ),
        "about" | "licenses" | "clone" => open_dialog(what, window, cx),
        other => eprintln!("gitviber: nothing to open for GITVIBER_SNAPSHOT_OPEN={other}"),
    }
}

/// Opens one of the app's dialogs in `window` by name ("about", "licenses", "clone"), for the
/// examples that open and close them.
pub fn open_dialog(name: &str, window: &mut gpui_kit::Window, cx: &mut App) {
    match name {
        "about" => dialogs::about::open(window, cx),
        "licenses" => dialogs::licenses::open(window, cx),
        "clone" => dialogs::clone::open(window, cx),
        _ => {}
    }
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
            snapshot_root,
        );
        if let Err(err) = shot {
            eprintln!("gitviber: snapshot failed: {err}");
            std::process::exit(1);
        }
        std::process::exit(0);
    }
    // macOS delivers what the Dock, Finder or `open -a` opens with as URLs, before the app is up
    // on a cold launch: they wait here for core.
    let (urls_tx, mut urls_rx) = futures::channel::mpsc::unbounded::<Vec<String>>();
    let application = gpui_kit::application().with_assets(Assets);
    application.on_open_urls(move |urls| {
        let _ = urls_tx.unbounded_send(urls);
    });
    application.run(move |cx| {
        init(cx);
        open_main_window(None, cx);
        opened::install(cx);
        let (raise_tx, mut raise_rx) = futures::channel::mpsc::unbounded::<()>();
        let core = gitviber_ui::core(cx);
        instance::listen(move |paths| {
            if paths.is_empty() {
                let _ = raise_tx.unbounded_send(());
            } else {
                gitviber_core::opened::push(&core, paths);
            }
        });
        let raise = cx.spawn(async move |cx| {
            use futures::StreamExt as _;
            while raise_rx.next().await.is_some() {
                cx.update(|cx| {
                    if let Some(w) = workspace_view::main_window(cx) {
                        let _ = w.update(cx, |_, window, _| window.activate_window());
                    }
                    cx.activate(true);
                });
            }
        });
        let urls = cx.spawn(async move |cx| {
            use futures::StreamExt as _;
            while let Some(urls) = urls_rx.next().await {
                cx.update(|cx| opened::open_urls(urls, cx));
            }
        });
        cx.set_global(OpenUrls(urls, raise));
        if !windows::background_launch() {
            cx.activate(true);
        }
        started(cx);
    });
}

/// Takes the open-documents events and later launches' raises for as long as the app runs.
struct OpenUrls(
    #[allow(dead_code)] gpui_kit::Task<()>,
    #[allow(dead_code)] gpui_kit::Task<()>,
);
impl gpui_kit::Global for OpenUrls {}
