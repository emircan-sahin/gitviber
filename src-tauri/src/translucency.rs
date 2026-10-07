//! Window opacity and Background blur (Settings → Appearance), macOS only: below 100% the window
//! goes clear, its web view stops drawing a background, and the window server blurs the desktop
//! behind it; the page then paints its surfaces at that opacity (translucency.ts, index.css).
//! At 100% none of this runs: the window stays opaque, with no compositing cost at all.

/// The windows (labels) that are clear now: the background color each had before, retained, to
/// put back, and the blur they have.
#[cfg(target_os = "macos")]
static CLEAR: std::sync::Mutex<std::collections::BTreeMap<String, Clear>> =
    std::sync::Mutex::new(std::collections::BTreeMap::new());

#[cfg(target_os = "macos")]
struct Clear {
    /// An NSColor, retained.
    color: usize,
    blur: u32,
}

#[cfg(target_os = "macos")]
fn clear() -> std::sync::MutexGuard<'static, std::collections::BTreeMap<String, Clear>> {
    CLEAR.lock().unwrap_or_else(|e| e.into_inner())
}

/// The settings' highest blur (BACKGROUND_BLUR in settings.ts): past it the window server's blur
/// costs more for little difference.
#[cfg(target_os = "macos")]
const MAX_BLUR: u32 = 40;

/// Clear with `blur`, or solid. On the main thread, where AppKit's windows belong.
#[cfg(target_os = "macos")]
pub fn set<R: tauri::Runtime>(
    window: &tauri::WebviewWindow<R>,
    on: bool,
    blur: u32,
) -> Result<(), String> {
    use objc2::runtime::{AnyObject, Bool};
    use objc2::{class, msg_send};

    let ns_window = window.ns_window().map_err(|e| e.to_string())? as *mut AnyObject;
    let mut windows = clear();
    let was = windows.remove(window.label());
    let blur = blur.min(MAX_BLUR);
    unsafe {
        if on {
            let color = match was.as_ref() {
                Some(was) => was.color,
                None => {
                    let color: *mut AnyObject = msg_send![ns_window, backgroundColor];
                    let _: *mut AnyObject = msg_send![color, retain];
                    let _: () = msg_send![ns_window, setOpaque: Bool::NO];
                    // Not quite clear: the window server sends clicks on fully clear pixels to
                    // the window below.
                    let almost: *mut AnyObject =
                        msg_send![class!(NSColor), colorWithWhite: 1.0f64, alpha: 0.001f64];
                    let _: () = msg_send![ns_window, setBackgroundColor: almost];
                    color as usize
                }
            };
            set_blur(ns_window, blur);
            windows.insert(window.label().to_string(), Clear { color, blur });
        } else if let Some(was) = was.as_ref() {
            set_blur(ns_window, 0);
            let color = was.color as *mut AnyObject;
            let _: () = msg_send![ns_window, setOpaque: Bool::YES];
            let _: () = msg_send![ns_window, setBackgroundColor: color];
            let _: () = msg_send![color, release];
        }
        if was.is_some() != on {
            // A clear window's shadow is drawn from what it shows: drawn again for the change.
            let _: () = msg_send![ns_window, invalidateShadow];
        }
    }
    drop(windows);
    if was.is_some() != on {
        draw_background(window, !on)?;
    }
    Ok(())
}

/// The window server's blur behind a clear window. Private, as for every terminal that blurs,
/// Terminal.app too; looked up rather than linked, so a macOS without it only goes unblurred.
#[cfg(target_os = "macos")]
fn set_blur(ns_window: *mut objc2::runtime::AnyObject, radius: u32) {
    use objc2::msg_send;

    let Some((connection, set_radius)) = blur_calls() else {
        return;
    };
    unsafe {
        // None yet before the window was first on screen: show() sets it again.
        let number: isize = msg_send![ns_window, windowNumber];
        if let (Ok(number), Ok(radius)) = (u32::try_from(number), i32::try_from(radius)) {
            if number > 0 {
                set_radius(connection(), number, radius);
            }
        }
    }
}

#[cfg(target_os = "macos")]
type MainConnection = unsafe extern "C" fn() -> i32;
#[cfg(target_os = "macos")]
type SetBlurRadius = unsafe extern "C" fn(i32, u32, i32) -> i32;

/// CGSMainConnectionID and CGSSetWindowBackgroundBlurRadius, from SkyLight through CoreGraphics.
#[cfg(target_os = "macos")]
fn blur_calls() -> Option<(MainConnection, SetBlurRadius)> {
    static CALLS: std::sync::OnceLock<Option<(MainConnection, SetBlurRadius)>> =
        std::sync::OnceLock::new();
    *CALLS.get_or_init(|| unsafe {
        Some((
            symbol(c"CGSMainConnectionID")?,
            symbol(c"CGSSetWindowBackgroundBlurRadius")?,
        ))
    })
}

/// A C function loaded into the process, as `F`; None when there's none by that name.
#[cfg(target_os = "macos")]
unsafe fn symbol<F: Copy>(name: &std::ffi::CStr) -> Option<F> {
    let found = libc::dlsym(libc::RTLD_DEFAULT, name.as_ptr());
    (!found.is_null()).then(|| std::mem::transmute_copy(&found))
}

/// WKWebView has no public switch for its own background; this private key is the one wry's
/// transparent webviews use. On the main thread with_webview runs at once, in the same turn as the
/// window's change, so no frame shows one without the other.
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

/// A page starting to load (a reload, ⌘R) starts solid: it asks to be clear again if it wants
/// to be, and one made solid just before the reload doesn't stay clear.
#[cfg(target_os = "macos")]
pub fn reset<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>) {
    if clear().contains_key(window.label()) {
        let w = window.clone();
        let _ = window.run_on_main_thread(move || {
            let _ = set(&w, false, 0);
        });
    }
}

/// The blur again once the window is on screen: asked for while it was hidden, it had none.
#[cfg(target_os = "macos")]
pub fn shown<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>) {
    if clear().contains_key(window.label()) {
        let w = window.clone();
        let _ = window.run_on_main_thread(move || {
            let blur = clear().get(w.label()).map(|c| c.blur);
            if let (Some(blur), Ok(ns_window)) = (blur, w.ns_window()) {
                set_blur(ns_window.cast(), blur);
            }
        });
    }
}

/// A closed window's entry goes, with the color it held.
#[cfg(target_os = "macos")]
pub fn forget(label: &str) {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;

    if let Some(was) = clear().remove(label) {
        let _: () = unsafe { msg_send![was.color as *mut AnyObject, release] };
    }
}

/// Reduce transparency switched while a window is in front: the pages ask again
/// (translucency.ts). One observer, kept for the app's lifetime.
#[cfg(target_os = "macos")]
pub fn watch_reduce_transparency(app: &tauri::AppHandle) {
    use block2::RcBlock;
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    use std::ptr::NonNull;
    use tauri::Emitter;

    let app = app.clone();
    let block = RcBlock::new(move |_note: NonNull<AnyObject>| {
        let _ = app.emit("reduce-transparency", ());
    });
    unsafe {
        let workspace: *mut AnyObject = msg_send![class!(NSWorkspace), sharedWorkspace];
        let center: *mut AnyObject = msg_send![workspace, notificationCenter];
        let name =
            crate::objc::ns_string(c"NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification");
        let _: *mut AnyObject = msg_send![center, addObserverForName: name, object: std::ptr::null_mut::<AnyObject>(), queue: std::ptr::null_mut::<AnyObject>(), usingBlock: &*block];
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
pub fn set<R: tauri::Runtime>(
    _window: &tauri::WebviewWindow<R>,
    _on: bool,
    _blur: u32,
) -> Result<(), String> {
    Err("Window opacity is only supported on macOS".into())
}

#[cfg(not(target_os = "macos"))]
pub fn reset<R: tauri::Runtime>(_window: &tauri::WebviewWindow<R>) {}

#[cfg(not(target_os = "macos"))]
pub fn shown<R: tauri::Runtime>(_window: &tauri::WebviewWindow<R>) {}

#[cfg(not(target_os = "macos"))]
pub fn forget(_label: &str) {}

#[cfg(not(target_os = "macos"))]
pub fn reduce_transparency() -> bool {
    false
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn blur_calls_are_found_and_a_missing_one_is_none() {
        assert!(
            blur_calls().is_some(),
            "CoreGraphics should export the blur calls"
        );
        assert!(unsafe { symbol::<SetBlurRadius>(c"CGSNoSuchCallForGitViber") }.is_none());
    }
}
