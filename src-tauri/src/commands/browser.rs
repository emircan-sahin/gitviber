//! The browser tab's native view (browser/). Sync, so they run on the main thread, where
//! AppKit's views belong; the snapshot waits on WebKit, so it's async.

use crate::browser::{self, Go, PageState, Rect};
use crate::state::{blocking, Res};
use tauri::{AppHandle, WebviewWindow};

/// Only the workspace has browser tabs; the settings window places nothing.
#[tauri::command]
pub fn browser_create(
    window: WebviewWindow,
    id: String,
    root: String,
    url: String,
) -> Res<PageState> {
    if window.label() != "main" {
        return Err("Browser tabs open in the main window.".into());
    }
    browser::create(&window, &id, &root, &url)
}

#[tauri::command]
pub fn browser_place(id: String, rect: Rect) -> Res<()> {
    browser::place(&id, rect)
}

#[tauri::command]
pub fn browser_hide(id: String) {
    browser::hide(&id);
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

#[tauri::command]
pub async fn browser_snapshot(app: AppHandle, id: String) -> Res<Option<String>> {
    blocking(move || browser::snapshot(&app, id)).await
}
