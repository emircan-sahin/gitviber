//! macOS's Reduce transparency (System Settings → Accessibility → Display), which turns the
//! see-through window opaque, and the notification that it changed.

use std::ffi::CStr;
use std::ptr::NonNull;

use block2::RcBlock;
use objc2::msg_send;
use objc2::runtime::{AnyClass, AnyObject, Bool};

fn workspace() -> Option<*mut AnyObject> {
    let class = AnyClass::get(c"NSWorkspace")?;
    let workspace: *mut AnyObject = unsafe { msg_send![class, sharedWorkspace] };
    (!workspace.is_null()).then_some(workspace)
}

// Tests take the setting as off (Os::now), so only the app reads it.
#[cfg_attr(test, allow(dead_code))]
pub(super) fn reduce_transparency() -> bool {
    workspace().is_some_and(|workspace| {
        let on: Bool =
            unsafe { msg_send![workspace, accessibilityDisplayShouldReduceTransparency] };
        on.as_bool()
    })
}

/// Calls `changed` whenever an accessibility display option changes, on the main thread. One
/// observer for the app's lifetime: the notification center keeps the block.
pub(super) fn watch(changed: impl Fn() + 'static) {
    let Some(workspace) = workspace() else {
        return;
    };
    let block = RcBlock::new(move |_: NonNull<AnyObject>| changed());
    objc2::rc::autoreleasepool(|_| unsafe {
        let Some(name) = ns_string(c"NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification")
        else {
            return;
        };
        let center: *mut AnyObject = msg_send![workspace, notificationCenter];
        let none = std::ptr::null_mut::<AnyObject>();
        let _: *mut AnyObject = msg_send![
            center,
            addObserverForName: name,
            object: none,
            queue: none,
            usingBlock: &*block
        ];
    });
}

/// Calls `changed(window, on)` as an NSWindow of the app starts entering full screen (true) or
/// leaving it, or closes (false), on the main thread. Observers for the app's lifetime.
pub(super) fn watch_full_screen(changed: impl Fn(usize, bool) + 'static) {
    let changed = std::rc::Rc::new(changed);
    let steps = [
        (c"NSWindowWillEnterFullScreenNotification", true),
        (c"NSWindowWillExitFullScreenNotification", false),
        (c"NSWindowWillCloseNotification", false),
    ];
    let Some(class) = AnyClass::get(c"NSNotificationCenter") else {
        return;
    };
    objc2::rc::autoreleasepool(|_| unsafe {
        let center: *mut AnyObject = msg_send![class, defaultCenter];
        if center.is_null() {
            return;
        }
        for (name, on) in steps {
            let Some(name) = ns_string(name) else {
                continue;
            };
            let changed = changed.clone();
            let block = RcBlock::new(move |note: NonNull<AnyObject>| {
                let window: *mut AnyObject = msg_send![note.as_ptr(), object];
                if !window.is_null() {
                    changed(window as usize, on);
                }
            });
            let none = std::ptr::null_mut::<AnyObject>();
            let _: *mut AnyObject = msg_send![
                center,
                addObserverForName: name,
                object: none,
                queue: none,
                usingBlock: &*block
            ];
        }
    });
}

unsafe fn ns_string(s: &CStr) -> Option<*mut AnyObject> {
    let class = AnyClass::get(c"NSString")?;
    let string: *mut AnyObject = unsafe { msg_send![class, stringWithUTF8String: s.as_ptr()] };
    (!string.is_null()).then_some(string)
}
