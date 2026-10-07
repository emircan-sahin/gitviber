//! Settings in a window of its own, to keep beside the workspace while trying them out
//! (src/features/settings/SettingsWindow.tsx). The page runs none of the workspace's work: the
//! terminals, the quit, the menu's commands and the window's close stay the main window's.

use crate::state::Res;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder, Window, WindowEvent};

pub const LABEL: &str = "settings";

/// Held from the look for the window to its build: two quick asks would each build one.
static OPENING: Mutex<()> = Mutex::new(());

/// Brings the settings window forward, on `section` if given (else where it is), opening it
/// first when `open`. False when there is none to bring: the page shows the dialog instead.
pub fn show(app: &AppHandle, section: Option<&str>, open: bool) -> Res<bool> {
    let section = section.unwrap_or_default();
    // It goes in the page's URL.
    if !section.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(format!("No settings section {section:?}"));
    }
    let err = |e: tauri::Error| e.to_string();
    let _opening = OPENING.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(window) = app.get_webview_window(LABEL) {
        if !section.is_empty() {
            window
                .emit_to(LABEL, "settings-section", section)
                .map_err(err)?;
        }
        window.unminimize().map_err(err)?;
        window.set_focus().map_err(err)?;
        return Ok(true);
    }
    if !open {
        return Ok(false);
    }
    let url = WebviewUrl::App(format!("settings.html?settings={section}").into());
    let builder = WebviewWindowBuilder::new(app, LABEL, url)
        .title("Settings")
        .inner_size(880.0, 620.0)
        .min_inner_size(640.0, 420.0)
        // As the main window (tauri.conf.json): the page shows it once its theme is applied.
        .visible(false)
        .background_color(tauri::window::Color(0x17, 0x17, 0x18, 0xff));
    #[cfg(target_os = "macos")]
    let builder = builder
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true);
    // Linux and Windows give it the app's menu bar too, which keeps Ctrl+Q there; its items go
    // to the page as macOS's do while it's in front (menu_target).
    crate::show_eventually(builder.build().map_err(err)?);
    Ok(true)
}

/// A menu item the settings window hands to the workspace (lib/app/settingsWindow.ts). `raise`:
/// the main window comes forward to run it, so what it does is in sight.
pub fn run_in_main(app: &AppHandle, id: &str, raise: bool) {
    if raise {
        crate::opened::raise(app);
    }
    let _ = app.emit_to("main", "menu", id);
}

/// The settings window in front is the app in front too: notifications wait (lib/app/notify).
pub fn on_event(window: &Window, event: &WindowEvent) {
    let focused = match event {
        WindowEvent::Focused(focused) => *focused,
        WindowEvent::Destroyed => false,
        _ => return,
    };
    let _ = window.emit_to("main", "settings-window-focused", focused);
}

/// Where a menu item goes: macOS has one menu bar for every window, and the settings window
/// in front gets its keys (SettingsWindow.tsx passes a click on to the workspace).
pub fn menu_target(app: &AppHandle) -> &'static str {
    match app.get_webview_window(LABEL) {
        Some(w) if w.is_focused().unwrap_or(false) => LABEL,
        _ => "main",
    }
}
