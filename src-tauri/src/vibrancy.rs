//! The optional translucent background (Settings → Appearance): macOS vibrancy behind a page whose
//! side surfaces turn see-through (index.css). The window is created transparent for it
//! (tauri.conf.json) but keeps its opaque background color until this switches it on.

#[cfg(target_os = "macos")]
#[tauri::command]
pub fn set_translucent(window: tauri::WebviewWindow, on: bool) {
    use tauri::Manager;
    use window_vibrancy::{
        apply_vibrancy, clear_vibrancy, NSVisualEffectMaterial, NSVisualEffectState,
    };

    // The window's own color would cover the blur where the page is see-through.
    let color = if on {
        Some(tauri::window::Color(0, 0, 0, 0))
    } else {
        let label = window.label();
        let windows = &window.config().app.windows;
        windows
            .iter()
            .find(|w| w.label == label)
            .and_then(|w| w.background_color)
    };
    let _ = window.set_background_color(color);

    let w = window.clone();
    let _ = window.run_on_main_thread(move || {
        // Called directly rather than through tauri's set_effects: its clear does nothing on macOS,
        // and every apply stacks one more effect view.
        let _ = clear_vibrancy(&w);
        if on {
            let _ = apply_vibrancy(
                &w,
                NSVisualEffectMaterial::Sidebar,
                Some(NSVisualEffectState::FollowsWindowActiveState),
                None,
            );
        }
    });
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
pub fn set_translucent(_on: bool) {}
