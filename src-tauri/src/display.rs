//! WKWebView renders at ~60fps by default even on 120Hz ProMotion displays
//! ("Prefer Page Rendering Updates near 60fps"), which makes scrolling a virtualized
//! list feel stuttery next to native apps. We switch that WebKit feature off.

#[cfg(target_os = "macos")]
pub fn unlock_high_refresh_rate<R: tauri::Runtime>(webview: &tauri::WebviewWindow<R>) {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;

    let _ = webview.with_webview(|wv| unsafe {
        let web_view = wv.inner() as *mut AnyObject;
        let config: *mut AnyObject = msg_send![web_view, configuration];
        let prefs: *mut AnyObject = msg_send![config, preferences];
        unlock_preferences(&*prefs);
    });
}

/// The same for a web view made from these preferences, before it is made: the browser tabs'
/// pages (browser/macos) would otherwise run at 60fps on a 120Hz display, a game's frames halved.
#[cfg(target_os = "macos")]
pub fn unlock_preferences(prefs: &objc2::runtime::AnyObject) {
    use objc2::runtime::{AnyClass, AnyObject, Bool, Sel};
    use objc2::{msg_send, sel};
    use std::ffi::{c_char, CStr};

    const FEATURE: &CStr = c"PreferPageRenderingUpdatesNear60FPSEnabled";

    let Some(prefs_class) = AnyClass::get(c"WKPreferences") else {
        return;
    };
    // Private WebKit API: bail out quietly if a future macOS drops it.
    let features_sel: Sel = sel!(_features);
    let responds: Bool = unsafe { msg_send![prefs_class, respondsToSelector: features_sel] };
    if !responds.as_bool() {
        return;
    }
    unsafe {
        let features: *mut AnyObject = msg_send![prefs_class, _features];
        let count: usize = msg_send![features, count];
        for i in 0..count {
            let feature: *mut AnyObject = msg_send![features, objectAtIndex: i];
            let key: *mut AnyObject = msg_send![feature, key];
            let utf8: *const c_char = msg_send![key, UTF8String];
            if !utf8.is_null() && CStr::from_ptr(utf8) == FEATURE {
                let _: () = msg_send![prefs, _setEnabled: Bool::NO, forFeature: feature];
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub fn unlock_high_refresh_rate<R: tauri::Runtime>(_webview: &tauri::WebviewWindow<R>) {}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject, Bool};
    use objc2::sel;

    fn near_60fps(prefs: &AnyObject) -> bool {
        let class = AnyClass::get(c"WKPreferences").unwrap();
        unsafe {
            let features: *mut AnyObject = msg_send![class, _features];
            let count: usize = msg_send![features, count];
            for i in 0..count {
                let feature: *mut AnyObject = msg_send![features, objectAtIndex: i];
                let key: *mut AnyObject = msg_send![feature, key];
                let utf8: *const std::ffi::c_char = msg_send![key, UTF8String];
                if std::ffi::CStr::from_ptr(utf8) == c"PreferPageRenderingUpdatesNear60FPSEnabled" {
                    let on: Bool = msg_send![prefs, _isEnabledForFeature: feature];
                    return on.as_bool();
                }
            }
        }
        panic!("this WebKit has no such feature");
    }

    /// WebKit's default holds the cap; lifting it sticks on a fresh preferences object.
    #[test]
    fn lifts_the_60fps_cap_of_a_preferences_object() {
        let class = AnyClass::get(c"WKPreferences").unwrap();
        if !class.responds_to(sel!(_features)) {
            return;
        }
        let prefs: *mut AnyObject = unsafe { msg_send![class, new] };
        let prefs = unsafe { &*prefs };
        assert!(near_60fps(prefs));
        super::unlock_preferences(prefs);
        assert!(!near_60fps(prefs));
    }
}
