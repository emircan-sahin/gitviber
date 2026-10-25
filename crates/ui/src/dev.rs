//! A window for a feature crate's example binary: the app's init (assets, settings, theme,
//! keymap, core), the background launch, and a workspace with the project from the command line
//! opened in core, so a panel or tab can be looked at without the rest of the app.
//!
//! ```ignore
//! // crates/changes/examples/demo.rs
//! use gitviber_ui::{dev, workspace::{PanelContext, Workspace}};
//! use gpui_kit::AppContext as _;
//!
//! fn main() {
//!     dev::run(|window, cx| {
//!         let workspace = Workspace::for_window(window, cx).unwrap();
//!         let ctx = workspace.update(cx, |ws, cx| ws.context(cx));
//!         cx.new(|cx| gitviber_changes::ChangesPanel::new(&ctx, window, cx)).into()
//!     });
//! }
//! ```
//! `cargo build -p gitviber-changes --example demo`, then
//! `GITVIBER_BACKGROUND=1 target/debug/examples/demo /path/to/repo` for a window that doesn't take
//! the keyboard.
//!
//! With this crate's `snapshot` feature on (from the example crate's `[dev-dependencies]`) and
//! `GITVIBER_SNAPSHOT=out.png` set, `run` opens no window: it draws the same host off screen on the
//! GPU, waits `GITVIBER_SNAPSHOT_WAIT` seconds (default 3) for loads to land, writes the PNG and
//! exits. A window behind others draws only its first frame, so that's the reliable picture.

use std::path::PathBuf;

use gpui_kit::{
    AnyView, App, AppContext as _, Context, Entity, FocusHandle, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, Styled as _, Subscription, Window, div, px, size,
};

use crate::assets::Assets;
use crate::keymap::ROOT_CONTEXT;
use crate::theme::{self, ActiveAppTheme};
use crate::windows;
use crate::workspace::Workspace;

/// The project to open: the first argument that is a folder, else the current one.
pub fn project_arg() -> PathBuf {
    std::env::args_os()
        .skip(1)
        .map(PathBuf::from)
        .find(|p| p.is_dir())
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_default()
}

struct DevHost {
    view: AnyView,
    workspace: Entity<Workspace>,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl Render for DevHost {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        for toast in self.workspace.update(cx, |ws, _| ws.take_toasts()) {
            toast.show(window, cx);
        }
        if window.focused(cx).is_none() {
            self.focus.focus(window, cx);
        }
        let t = cx.app_theme();
        div()
            .key_context(ROOT_CONTEXT)
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .flex_col()
            .bg(t.background)
            .text_color(t.foreground)
            .font_family(t.fonts.ui.clone())
            .font_weight(t.fonts.ui_weight)
            .text_size(t.ui_text())
            .child(
                div()
                    .h(windows::TOP_BAR_HEIGHT)
                    .flex_none()
                    .bg(t.chrome(t.sidebar))
                    .border_b_1()
                    .border_color(t.border),
            )
            .child(div().flex_1().min_h_0().child(self.view.clone()))
    }
}

/// The workspace with the project from the command line, and `build`'s view under a top bar.
fn host(
    build: impl FnOnce(&mut Window, &mut App) -> AnyView,
    window: &mut Window,
    cx: &mut App,
) -> Entity<DevHost> {
    let workspace = cx.new(Workspace::new);
    Workspace::register_window(&workspace, window, cx);
    let root = project_arg();
    workspace.update(cx, |ws, cx| ws.open_project(root, window, cx));
    let view = build(window, cx);
    let subscriptions = theme::attach(window, cx);
    cx.new(|cx| {
        cx.observe(&workspace, |_, _, cx| cx.notify()).detach();
        DevHost {
            view,
            workspace,
            focus: cx.focus_handle(),
            _subscriptions: subscriptions,
        }
    })
}

/// An example keeps its own settings and state (in the temp folder) rather than rewriting the
/// app's, unless GITVIBER_CONFIG_DIR names a folder.
fn use_own_config_dir() {
    crate::store::use_config_dir(std::env::temp_dir().join("gitviber-dev"));
}

/// Starts the app with one window showing what `build` makes.
pub fn run(build: impl FnOnce(&mut Window, &mut App) -> AnyView + 'static) {
    // Before any thread starts: it may start the process again, as the app does.
    gitviber_core::malloc::return_freed_blocks();
    use_own_config_dir();
    #[cfg(feature = "snapshot")]
    if let Some(out) = std::env::var_os("GITVIBER_SNAPSHOT") {
        let wait = std::env::var("GITVIBER_SNAPSHOT_WAIT")
            .ok()
            .and_then(|s| s.parse::<f32>().ok())
            .unwrap_or(3.);
        let size = size(px(1100.), px(760.));
        match snapshot(std::path::Path::new(&out), size, wait, build) {
            Ok(()) => std::process::exit(0),
            Err(err) => {
                eprintln!("gitviber: snapshot failed: {err}");
                std::process::exit(1);
            }
        }
    }
    gpui_kit::application().with_assets(Assets).run(move |cx| {
        crate::init(cx);
        crate::start_core(cx);
        windows::install_app_actions(cx);
        let options = windows::options(
            None,
            size(px(1100.), px(760.)),
            size(px(480.), px(320.)),
            cx,
        );
        let opened = gpui_kit::open_window(options, cx, move |window, cx| host(build, window, cx));
        if let Err(err) = opened {
            eprintln!("gitviber: couldn't open a window: {err}");
            cx.quit();
        } else if !windows::background_launch() {
            cx.activate(true);
        }
    });
}

/// Draws `build`'s host off screen on the GPU into a PNG at `out`, after letting background
/// loads run for `wait` seconds of real time.
#[cfg(feature = "snapshot")]
pub fn snapshot(
    out: &std::path::Path,
    size: gpui_kit::Size<gpui_kit::Pixels>,
    wait: f32,
    build: impl FnOnce(&mut Window, &mut App) -> AnyView + 'static,
) -> Result<(), Box<dyn std::error::Error>> {
    use_own_config_dir();
    snapshot_with(
        out,
        size,
        wait,
        |cx| {
            crate::init(cx);
            crate::start_core(cx);
            windows::install_app_actions(cx);
        },
        move |window, cx| host(build, window, cx).into(),
    )
}

/// The off-screen app, on gpui's test executor run in real time.
#[cfg(feature = "snapshot")]
fn headless_app(
    text_system: std::sync::Arc<dyn gpui_kit::PlatformTextSystem>,
) -> gpui_kit::HeadlessAppContext {
    let cx = gpui_kit::HeadlessAppContext::with_platform(
        text_system,
        std::sync::Arc::new(Assets),
        gpui_snapshot_platform::current_headless_renderer,
    );
    // Core's threads wake it. Unless parking is allowed, the test scheduler takes such a wake
    // for non-determinism and symbolizes a backtrace: the app's snapshot peaked at 332 MB
    // resident instead of 162, the binary's debug info read in.
    cx.allow_parking();
    cx
}

/// `snapshot` for any root: `setup` does the app's init, `root` builds the window's view.
#[cfg(feature = "snapshot")]
pub fn snapshot_with(
    out: &std::path::Path,
    size: gpui_kit::Size<gpui_kit::Pixels>,
    wait: f32,
    setup: impl FnOnce(&mut App),
    root: impl FnOnce(&mut Window, &mut App) -> AnyView + 'static,
) -> Result<(), Box<dyn std::error::Error>> {
    use std::time::{Duration, Instant};

    let mut cx = headless_app(gpui_snapshot_platform::current_platform(true).text_system());
    cx.update(setup);
    let window = cx.open_window(size, move |window, cx| {
        let view = root(window, cx);
        cx.new(|cx| gpui_kit::base::Root::new(view, window, cx))
    })?;
    // Core answers on its own threads; the test dispatcher only runs what's already queued.
    let tick = Duration::from_millis(16);
    let until = Instant::now() + Duration::from_secs_f32(wait.max(0.));
    loop {
        cx.run_until_parked();
        if Instant::now() >= until {
            break;
        }
        std::thread::sleep(tick);
        cx.advance_clock(tick);
    }
    cx.update_window(window.into(), |_, window, _| window.refresh())?;
    cx.run_until_parked();
    let image = cx.capture_screenshot(window.into())?;
    image.save(out)?;
    Ok(())
}

#[cfg(all(test, feature = "snapshot"))]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;
    use std::time::{Duration, Instant};

    /// A task waiting for another thread, as views wait for core's (its repo watcher answers
    /// whenever the disk settles). With the scheduler's thread check on, the wake grew the
    /// footprint 9.7 MB and the resident set 17 -> 68 MB here; it should cost nothing.
    #[test]
    fn a_wake_from_another_thread_costs_no_memory() {
        let cx = super::headless_app(std::sync::Arc::new(gpui_kit::NoopTextSystem));
        let (tx, rx) = futures::channel::oneshot::channel::<()>();
        let done = Rc::new(Cell::new(false));
        let flag = done.clone();
        cx.foreground_executor()
            .spawn(async move {
                let _ = rx.await;
                flag.set(true);
            })
            .detach();
        cx.run_until_parked();
        let before = crate::memory::footprint().unwrap();
        std::thread::spawn(move || tx.send(()));
        let until = Instant::now() + Duration::from_secs(5);
        while !done.get() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(5));
            cx.run_until_parked();
        }
        assert!(done.get(), "the task never woke");
        let grown = crate::memory::footprint().unwrap().saturating_sub(before);
        assert!(grown < 2 << 20, "a wake grew the footprint {grown} bytes");
    }
}
