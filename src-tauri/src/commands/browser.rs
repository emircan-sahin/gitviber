//! The browser tab's native view (browser/). Sync, so they run on the main thread, where
//! AppKit's views belong; the snapshot waits on WebKit, so it's async.

use crate::browser::{self, ports, Created, Go, Rect};
use crate::state::{blocking, AppState, Res};
use tauri::{AppHandle, State, WebviewWindow};

/// Only the workspace has browser tabs; the settings window places nothing.
#[tauri::command]
pub fn browser_create(
    window: WebviewWindow,
    id: String,
    root: String,
    url: String,
) -> Res<Created> {
    if window.label() != "main" {
        return Err("Browser tabs open in the main window.".into());
    }
    browser::create(&window, &id, &root, &url)
}

#[tauri::command]
pub fn browser_place(id: String, rect: Rect) -> Res<()> {
    browser::place(&id, rect)
}

/// `aside`: only while something of the app page's is over it, its tab still on show.
#[tauri::command]
pub fn browser_hide(id: String, aside: bool) {
    browser::hide(&id, aside);
}

#[tauri::command]
pub fn browser_close(id: String) {
    browser::close(&id);
}

#[tauri::command]
pub fn browser_navigate(id: String, url: String) -> Res<()> {
    browser::navigate(&id, &url)
}

#[tauri::command]
pub fn browser_go(id: String, to: Go) -> Res<()> {
    browser::go(&id, to)
}

/// `page`: into the page; else back to the app page (the address bar takes it).
#[tauri::command]
pub fn browser_focus(id: String, page: bool) {
    browser::focus(&id, page);
}

/// The chords the app's commands are bound to (keybindings.ts): only these leave a page.
#[tauri::command]
pub fn browser_set_app_keys(chords: Vec<String>) {
    browser::set_app_keys(chords);
}

/// The memory policy (Settings → Browser): hidden views kept alive, and minutes to park.
#[tauri::command]
pub fn browser_configure(live_hidden: u32, park_after_min: u32) {
    browser::configure(live_hidden, park_after_min);
}

/// An agent in `dir` finished its turn: its worktree's pages load again (the setting's on).
#[tauri::command]
pub fn browser_agent_done(dir: String) {
    browser::agent_done(&dir);
}

#[tauri::command]
pub fn browser_inspect(id: String) -> Res<bool> {
    browser::inspect(&id)
}

#[tauri::command]
pub async fn browser_clear_data(app: AppHandle) -> Res<()> {
    blocking(move || browser::clear_data(&app)).await
}

/// What the terminals `ptys` run serves on: their shells' process trees' listening TCP ports.
#[tauri::command]
pub async fn browser_ports(state: State<'_, AppState>, ptys: Vec<u32>) -> Res<Vec<ports::Port>> {
    let shells: Vec<u32> = ptys.iter().filter_map(|&id| state.ptys.shell(id)).collect();
    blocking(move || Ok(ports::listening(&shells))).await
}

#[tauri::command]
pub async fn browser_snapshot(app: AppHandle, id: String) -> Res<Option<String>> {
    blocking(move || browser::snapshot(&app, id)).await
}
