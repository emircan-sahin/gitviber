//! The browser in the GPUI window: a "browser" tab per page (tab.rs) with its native view kept
//! on the tab's bounds (placer.rs), what the views report (hub.rs), agents' tabs taken into the
//! workspace (agents.rs), and asking the worktree's agent about a page (ask.rs).

mod agents;
mod ask;
pub mod devices;
pub mod format;
pub mod hub;
mod placer;
mod store;
pub mod tab;
#[cfg(test)]
mod tests;
pub mod url;

pub use agents::open_new_tab;
pub use tab::{BrowserTab, Saved};

use gitviber_ui::actions::browser as act;
use gitviber_ui::workspace::{register_opener, Selection, TabHandle, Workspace};
use gpui::{App, AppContext as _, Window};

/// A new tab's id: a page's view is kept by it, whatever it loads.
pub fn new_id() -> String {
    let hex = gitviber_core::local_socket::random_hex(16).unwrap_or_else(|| {
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        format!("{n:032x}")
    });
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// The window's own NSView, which the pages lie beside; None where there's none (tests, Linux).
pub(crate) fn native_view(window: &Window) -> Option<std::ptr::NonNull<std::ffi::c_void>> {
    #[cfg(target_os = "macos")]
    {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        match HasWindowHandle::window_handle(window).ok()?.as_raw() {
            RawWindowHandle::AppKit(h) => Some(h.ns_view),
            _ => None,
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = window;
        None
    }
}

/// The "browser" opener, ⇧⌘B, the views' events, the agents' socket and pane env, and the app's
/// settings sent on: once, after gitviber-ui's init and core.
pub fn init(cx: &mut App) {
    let core = gitviber_ui::core(cx);
    let sink = hub::install(cx, agents::workspace_event);
    // Saved sessions in the config folder, where the old app left them (browser-sessions/); a
    // demo's or a test's own folder keeps them off the user's.
    crate::install(crate::Handle::with_dirs(
        core.clone(),
        sink,
        Some(gitviber_ui::store::config_dir()),
        gitviber_core::paths::cache_dir(),
    ));
    // Each terminal's shell can run `gitviber browser` and says which pane it is.
    core.ptys.set_extra_env(Box::new(|id| {
        crate::server::env(id)
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect()
    }));
    store::load(cx);
    // A closed pane's tabs are the user's from then on (agent.rs), told on the main thread.
    core.ptys.on_killed(Box::new(|id| {
        if let Some(handle) = crate::installed() {
            let _ = handle.run_on_main_thread(move || crate::agent_pane_closed(id));
        }
    }));
    register_opener(cx, "browser", open);
    cx.on_action(|_: &act::Open, cx| {
        if let Some(w) = cx.active_window() {
            let _ = w.update(cx, |_, window, cx| open_new_tab(window, cx));
        }
    });
    cx.on_action(|_: &act::ClearData, cx| {
        if let Some(w) = cx.active_window() {
            let _ = w.update(cx, |_, window, cx| agents::clear_data(window, cx));
        }
    });
    agents::watch(cx);
    // Picks' pictures from earlier sessions, past the day an agent might read them in.
    if let Some(cache) = gitviber_core::paths::cache_dir() {
        let folder = crate::picks::folder(&cache);
        std::thread::spawn(move || crate::picks::prune(&folder, std::time::SystemTime::now()));
    }
}

/// A tab's view from its selection. Runs inside the workspace's update: nothing here reads it.
fn open(selection: &Selection, window: &mut Window, cx: &mut App) -> Option<TabHandle> {
    let saved = Saved::of(selection)?;
    let workspace = Workspace::for_window(window, cx)?.downgrade();
    let root = gitviber_ui::has_core(cx)
        .then(|| gitviber_ui::core(cx).repo().ok())
        .flatten()
        .map(|r| crate::root_key(&r.to_string_lossy()))
        .unwrap_or_default();
    let tab = cx.new(|cx| BrowserTab::new(saved, root, workspace, window, cx));
    Some(TabHandle::new(tab))
}
