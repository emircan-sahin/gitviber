//! A taller macOS title bar for the overlay window, done the native way: an empty
//! unified-compact toolbar. tauri's trafficLightPosition instead moves the buttons by hand
//! on every redraw, which also runs in full screen and leaves them misplaced there.
//! In full screen the toolbar would stay pinned over the page's own top bar, so it auto-hides
//! there and slides in with the menu bar, as in any full-screen app.

#[cfg(target_os = "macos")]
use objc2::runtime::{AnyObject, Sel};

#[cfg(target_os = "macos")]
type OptionsFn = unsafe extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject, usize) -> usize;

/// tao's own window:willUseFullScreenPresentationOptions:, which ours wraps.
#[cfg(target_os = "macos")]
static TAO_OPTIONS: std::sync::OnceLock<OptionsFn> = std::sync::OnceLock::new();

/// A full-screen window takes its presentation options from its delegate, not from NSApp
/// (setting them there was ignored), so auto-hiding the toolbar has to be answered here.
#[cfg(target_os = "macos")]
unsafe extern "C-unwind" fn full_screen_options(
    this: *mut AnyObject,
    sel: Sel,
    window: *mut AnyObject,
    proposed: usize,
) -> usize {
    const AUTO_HIDE_MENU_BAR: usize = 1 << 2;
    const HIDE_MENU_BAR: usize = 1 << 3;
    const FULL_SCREEN: usize = 1 << 10;
    const AUTO_HIDE_TOOLBAR: usize = 1 << 11;

    let options = match TAO_OPTIONS.get() {
        Some(tao) => tao(this, sel, window, proposed),
        None => proposed,
    };
    // AppKit throws on AutoHideToolbar without AutoHideMenuBar, which can't go with HideMenuBar.
    if options & FULL_SCREEN != 0 && options & HIDE_MENU_BAR == 0 {
        options | AUTO_HIDE_MENU_BAR | AUTO_HIDE_TOOLBAR
    } else {
        options
    }
}

#[cfg(target_os = "macos")]
pub fn setup<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>) {
    use block2::RcBlock;
    use objc2::runtime::{Bool, Imp};
    use objc2::{class, msg_send, sel};
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

        // Wrap once: a second wrap would make ours its own "original" and recurse.
        let delegate: *mut AnyObject = msg_send![ns_window, delegate];
        let method = (!delegate.is_null())
            .then(|| {
                (*delegate)
                    .class()
                    .instance_method(sel!(window:willUseFullScreenPresentationOptions:))
            })
            .flatten();
        if let (Some(method), None) = (method, TAO_OPTIONS.get()) {
            let ours: Imp = std::mem::transmute(full_screen_options as OptionsFn);
            let tao = method.set_implementation(ours);
            let _ = TAO_OPTIONS.set(std::mem::transmute::<Imp, OptionsFn>(tao));
        }
    }

    // Tell the page on the will/did notifications rather than on resize, which only lands
    // once the animation is over: until then the buttons and the page's inset overlap.
    // Hiding the toolbar by hand doesn't work: AppKit saves its visibility on the way in
    // and restores it on the way out, so it came back hidden and the buttons moved up.
    let steps = [
        (c"NSWindowDidEnterFullScreenNotification", true),
        (c"NSWindowWillExitFullScreenNotification", false),
    ];
    for (name, fullscreen) in steps {
        let window = window.clone();
        let block = RcBlock::new(move |_note: NonNull<AnyObject>| {
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
