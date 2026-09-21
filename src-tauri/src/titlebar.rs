//! A taller macOS title bar for the overlay window, done the native way: an empty
//! unified-compact toolbar. tauri's trafficLightPosition instead moves the buttons by hand
//! on every redraw, which also runs in full screen and leaves them misplaced there.
//! In full screen the toolbar would stay pinned over the page's own top bar, so it auto-hides
//! there and slides in with the menu bar, as in any full-screen app.

#[cfg(target_os = "macos")]
pub fn setup<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>) {
    use block2::RcBlock;
    use objc2::runtime::{AnyObject, Bool};
    use objc2::{class, msg_send};
    use std::ptr::NonNull;
    use tauri::Emitter;

    // NSWindowToolbarStyleUnifiedCompact (macOS 11+).
    const UNIFIED_COMPACT: isize = 4;

    let Ok(ns_window) = window.ns_window() else {
        return;
    };
    let ns_window = ns_window as *mut AnyObject;
    unsafe {
        let toolbar: *mut AnyObject = msg_send![class!(NSToolbar), new];
        let _: () = msg_send![toolbar, setShowsBaselineSeparator: Bool::NO];
        let _: () = msg_send![ns_window, setToolbar: toolbar];
        let _: () = msg_send![toolbar, release];
        let _: () = msg_send![ns_window, setToolbarStyle: UNIFIED_COMPACT];
    }

    // Switch on the will/did notifications rather than on resize, which only lands once the
    // animation is over: until then the buttons and the page's inset overlap.
    // Hiding the toolbar ourselves doesn't work: AppKit saves its visibility on the way in
    // and restores it on the way out, so it came back hidden and the buttons moved up.
    let steps = [
        (c"NSWindowDidEnterFullScreenNotification", true),
        (c"NSWindowWillExitFullScreenNotification", false),
    ];
    for (name, fullscreen) in steps {
        let window = window.clone();
        let block = RcBlock::new(move |_note: NonNull<AnyObject>| unsafe {
            if fullscreen {
                // Only valid once in full screen and alongside AutoHideMenuBar, which full
                // screen implies anyway; AppKit drops both again on exit.
                const AUTO_HIDE_MENU_BAR: usize = 1 << 2;
                const AUTO_HIDE_TOOLBAR: usize = 1 << 11;
                let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
                let options: usize = msg_send![app, presentationOptions];
                let _: () = msg_send![app, setPresentationOptions: options | AUTO_HIDE_MENU_BAR | AUTO_HIDE_TOOLBAR];
            }
            let _ = window.emit("fullscreen", fullscreen);
        });
        unsafe {
            let center: *mut AnyObject = msg_send![class!(NSNotificationCenter), defaultCenter];
            let name: *mut AnyObject =
                msg_send![class!(NSString), stringWithUTF8String: name.as_ptr()];
            // The center keeps the observer (and block) for the app's lifetime.
            let _: *mut AnyObject = msg_send![center, addObserverForName: name, object: ns_window, queue: std::ptr::null_mut::<AnyObject>(), usingBlock: &*block];
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub fn setup<R: tauri::Runtime>(_window: &tauri::WebviewWindow<R>) {}
