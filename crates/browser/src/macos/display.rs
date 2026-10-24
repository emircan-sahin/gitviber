//! WebKit renders at ~60fps by default even on a 120Hz display ("Prefer Page Rendering Updates
//! near 60fps"): a page's animations and a game's frames would run at half the screen's rate.

/// The cap lifted on a web view's preferences, before the view is made from them.
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

#[cfg(test)]
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
