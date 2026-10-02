//! The optional translucent background (Settings → Appearance): macOS's sidebar material behind
//! the page, whose title bar, side panels and status bar turn see-through (index.css).
//! The window itself stays opaque, as Finder's does: a behind-window effect view shows the desktop
//! all the same, and the shadow, corners and compositing stay as they are with the setting off.

#[cfg(target_os = "macos")]
#[tauri::command]
pub fn set_translucent(window: tauri::WebviewWindow, on: bool) {
    use tauri::window::Color;
    use window_vibrancy::{
        apply_vibrancy, clear_vibrancy, NSVisualEffectMaterial, NSVisualEffectState,
    };

    // Off: the configured color, as the webview was created with.
    let color = if on {
        Some(Color(0, 0, 0, 0))
    } else {
        let label = window.label();
        let windows = &tauri::Manager::config(&window).app.windows;
        windows
            .iter()
            .find(|w| w.label == label)
            .and_then(|w| w.background_color)
    };
    let w = window.clone();
    let _ = window.run_on_main_thread(move || {
        // Directly rather than through tauri's set_effects: its clear does nothing on macOS, and
        // each apply stacks one more effect view.
        let _ = clear_vibrancy(&w);
        if on {
            // Inactive, the material turns solid by itself, as the page does (settings.ts).
            let _ = apply_vibrancy(
                &w,
                NSVisualEffectMaterial::Sidebar,
                Some(NSVisualEffectState::FollowsWindowActiveState),
                None,
            );
        }
        // The webview alone: tauri's window-wide setter would turn the NSWindow clear too.
        let webview: &tauri::Webview = w.as_ref();
        let _ = webview.set_background_color(color);
    });
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
pub fn set_translucent(_on: bool) {}

/// macOS's Reduce transparency (System Settings → Accessibility → Display); the page asks again
/// each time the window comes forward.
#[cfg(target_os = "macos")]
#[tauri::command]
pub fn reduce_transparency() -> bool {
    use objc2::runtime::{AnyObject, Bool};
    use objc2::{class, msg_send};

    unsafe {
        let workspace: *mut AnyObject = msg_send![class!(NSWorkspace), sharedWorkspace];
        let reduce: Bool = msg_send![workspace, accessibilityDisplayShouldReduceTransparency];
        reduce.as_bool()
    }
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
pub fn reduce_transparency() -> bool {
    false
}
