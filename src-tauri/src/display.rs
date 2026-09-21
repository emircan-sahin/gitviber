//! WKWebView renders at ~60fps by default even on 120Hz ProMotion displays
//! ("Prefer Page Rendering Updates near 60fps"), which makes scrolling a virtualized
//! list feel stuttery next to native apps. We switch that WebKit feature off.

#[cfg(target_os = "macos")]
pub fn unlock_high_refresh_rate<R: tauri::Runtime>(webview: &tauri::WebviewWindow<R>) {
    use objc2::runtime::{AnyClass, AnyObject, Bool, Sel};
    use objc2::{msg_send, sel};
    use std::ffi::{c_char, CStr};

    const FEATURE: &CStr = c"PreferPageRenderingUpdatesNear60FPSEnabled";

    let _ = webview.with_webview(|wv| unsafe {
        let web_view = wv.inner() as *mut AnyObject;
        let Some(prefs_class) = AnyClass::get(c"WKPreferences") else {
            return;
        };
        // Private WebKit API: bail out quietly if a future macOS drops it.
        let features_sel: Sel = sel!(_features);
        let responds: Bool = msg_send![prefs_class, respondsToSelector: features_sel];
        if !responds.as_bool() {
            return;
        }
        let config: *mut AnyObject = msg_send![web_view, configuration];
        let prefs: *mut AnyObject = msg_send![config, preferences];
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
    });
}

#[cfg(not(target_os = "macos"))]
pub fn unlock_high_refresh_rate<R: tauri::Runtime>(_webview: &tauri::WebviewWindow<R>) {}
