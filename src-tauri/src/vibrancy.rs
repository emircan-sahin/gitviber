//! The optional translucent background (Settings → Appearance): macOS's sidebar material behind
//! the page, whose title bar, side panels and status bar turn see-through (index.css).
//! The window itself stays opaque, as Finder's does: a behind-window effect view shows the desktop
//! all the same, and the shadow, corners and compositing stay as they are with the setting off.
//! Only the webview stops drawing its background, and only while the material is on, so nothing
//! changes for anyone who never turns it on.

/// The windows (labels) the material is on in now, for `reset` on a page load: each window's
/// page asks for its own (the main one, the settings one).
#[cfg(target_os = "macos")]
static ON: std::sync::Mutex<std::collections::BTreeSet<String>> =
    std::sync::Mutex::new(std::collections::BTreeSet::new());

#[cfg(target_os = "macos")]
fn on() -> std::sync::MutexGuard<'static, std::collections::BTreeSet<String>> {
    ON.lock().unwrap_or_else(|e| e.into_inner())
}

/// On the main thread, which apply_vibrancy insists on.
#[cfg(target_os = "macos")]
pub fn set<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>, on: bool) -> Result<(), String> {
    use window_vibrancy::{
        apply_vibrancy, clear_vibrancy, NSVisualEffectMaterial, NSVisualEffectState,
    };

    // Directly rather than through tauri's set_effects: its clear does nothing on macOS, and each
    // apply stacks one more effect view.
    clear_vibrancy(window).map_err(|e| e.to_string())?;
    self::on().remove(window.label());
    if on {
        // Inactive, the material turns solid by itself, as the page does (translucency.ts).
        apply_vibrancy(
            window,
            NSVisualEffectMaterial::Sidebar,
            Some(NSVisualEffectState::FollowsWindowActiveState),
            None,
        )
        .map_err(|e| e.to_string())?;
        self::on().insert(window.label().to_string());
    }
    draw_background(window, !on)
}

/// WKWebView has no public switch for its own background; this private key is the one wry's
/// transparent webviews use. On the main thread with_webview runs at once, in the same turn as the
/// material, so no frame shows one without the other.
#[cfg(target_os = "macos")]
fn draw_background<R: tauri::Runtime>(
    window: &tauri::WebviewWindow<R>,
    draws: bool,
) -> Result<(), String> {
    use objc2::runtime::{AnyObject, Bool};
    use objc2::{class, msg_send};

    window
        .with_webview(move |webview| unsafe {
            let view = webview.inner() as *mut AnyObject;
            let value: *mut AnyObject =
                msg_send![class!(NSNumber), numberWithBool: Bool::new(draws)];
            let key: *mut AnyObject =
                msg_send![class!(NSString), stringWithUTF8String: c"drawsBackground".as_ptr()];
            let _: () = msg_send![view, setValue: value, forKey: key];
        })
        .map_err(|e| e.to_string())
}

/// A page starting to load (a reload, ⌘R) starts solid: it asks for the material again if it wants
/// it, and one turned off just before the reload doesn't stay behind.
#[cfg(target_os = "macos")]
pub fn reset<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>) {
    if on().contains(window.label()) {
        let w = window.clone();
        let _ = window.run_on_main_thread(move || {
            let _ = set(&w, false);
        });
    }
}

/// macOS's Reduce transparency (System Settings → Accessibility → Display).
#[cfg(target_os = "macos")]
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
pub fn set<R: tauri::Runtime>(_window: &tauri::WebviewWindow<R>, _on: bool) -> Result<(), String> {
    Err("Translucency is only supported on macOS".into())
}

#[cfg(not(target_os = "macos"))]
pub fn reset<R: tauri::Runtime>(_window: &tauri::WebviewWindow<R>) {}

#[cfg(not(target_os = "macos"))]
pub fn reduce_transparency() -> bool {
    false
}
