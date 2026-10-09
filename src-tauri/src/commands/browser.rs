//! The browser tab's native view (browser/). Sync, so they run on the main thread, where
//! AppKit's views belong; the snapshot waits on WebKit, so it's async.

use crate::browser::{self, ports, Created, Go, Rect, Screen};
use crate::state::{blocking, AppState, Res};
use tauri::{AppHandle, State, WebviewWindow};

/// Only the workspace has browser tabs; the settings window places nothing.
#[tauri::command]
pub fn browser_create(
    window: WebviewWindow,
    id: String,
    root: String,
    url: String,
    ua: Option<String>,
    zoom: Option<f64>,
    dark: Option<bool>,
) -> Res<Created> {
    if window.label() != "main" {
        return Err("Browser tabs open in the main window.".into());
    }
    browser::create(&window, &id, &root, &url, &browser::Look { ua, zoom, dark })
}

/// `screen`: the device it shows, in device mode.
#[tauri::command]
pub fn browser_place(id: String, rect: Rect, screen: Option<Screen>) -> Res<()> {
    browser::place(&id, rect, screen)
}

/// The device's user agent (None: WebKit's own); the page loads again when it changes.
#[tauri::command]
pub fn browser_set_agent(id: String, ua: Option<String>) -> Res<()> {
    browser::set_agent(&id, ua.as_deref())
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

/// Settings → Browser: hidden views kept alive, minutes to park, the console on or off, and
/// whether agents may use the browser (`gitviber browser`).
#[tauri::command]
pub fn browser_configure(
    app: AppHandle,
    live_hidden: u32,
    park_after_min: u32,
    console: bool,
    agent_control: bool,
) {
    browser::configure(live_hidden, park_after_min, console);
    browser::server::set_enabled(&app, agent_control);
}

/// The element picker on or off; what it picks comes as `browser-picked`.
#[tauri::command]
pub fn browser_pick(id: String, on: bool) -> Res<()> {
    browser::pick(&id, on)
}

/// What the page logged as errors and warnings, oldest first, with a mark where each load began.
#[tauri::command]
pub fn browser_console(id: String) -> Res<Vec<browser::console::Entry>> {
    browser::console_entries(&id)
}

#[tauri::command]
pub fn browser_console_clear(id: String) -> Res<()> {
    browser::console_clear(&id)
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

/// The TCP ports the programs in terminals `ptys` listen on, everything under their shells.
#[tauri::command]
pub async fn browser_ports(state: State<'_, AppState>, ptys: Vec<u32>) -> Res<Vec<ports::Port>> {
    let shells: Vec<u32> = ptys.iter().filter_map(|&id| state.ptys.shell(id)).collect();
    blocking(move || Ok(ports::listening(&shells))).await
}

/// The next match of `text` in the page (or the one before): whether there was one; None where
/// WebKit can't find.
#[tauri::command]
pub async fn browser_find(
    app: AppHandle,
    id: String,
    text: String,
    backwards: bool,
    case_sensitive: bool,
) -> Res<Option<bool>> {
    blocking(move || browser::find(&app, id, text, backwards, case_sensitive)).await
}

/// The page's zoom outside device mode, 1 for 100%.
#[tauri::command]
pub fn browser_zoom(id: String, zoom: f64) -> Res<()> {
    browser::zoom(&id, zoom)
}

/// The page light or dark, or as the app (None).
#[tauri::command]
pub fn browser_appearance(id: String, dark: Option<bool>) -> Res<()> {
    browser::appearance(&id, dark)
}

#[tauri::command]
pub async fn browser_snapshot(app: AppHandle, id: String) -> Res<Option<String>> {
    blocking(move || browser::snapshot(&app, id)).await
}
