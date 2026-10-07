//! Settings in a window of its own, to keep beside the workspace while trying them out
//! (src/features/settings/SettingsWindow.tsx). The page runs none of the workspace's work: the
//! terminals, the quit, the menu's commands and the window's close stay the main window's.

use crate::state::Res;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

pub const LABEL: &str = "settings";

/// Brings the settings window forward, on `section` if given (else where it is), opening it
/// first when `open`. False when there is none to bring: the page shows the dialog instead.
pub fn show(app: &AppHandle, section: Option<&str>, open: bool) -> Res<bool> {
    let section = section.unwrap_or_default();
    // It goes in the page's URL.
    if !section.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(format!("No settings section {section:?}"));
    }
    let err = |e: tauri::Error| e.to_string();
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
    let url = WebviewUrl::App(format!("index.html?settings={section}").into());
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
    let window = builder.build().map_err(err)?;
    // Linux and Windows give each window the app's menu bar, whose items act on the workspace.
    #[cfg(not(target_os = "macos"))]
    window.remove_menu().map_err(err)?;
    crate::show_eventually(window);
    Ok(true)
}

/// Where a menu item goes: macOS has one menu bar for every window, and the settings window
/// in front gets its keys (SettingsWindow.tsx passes a click on to the workspace).
pub fn menu_target(app: &AppHandle) -> &'static str {
    match app.get_webview_window(LABEL) {
        Some(w) if w.is_focused().unwrap_or(false) => LABEL,
        _ => "main",
    }
}
