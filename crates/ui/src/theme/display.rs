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

unsafe fn ns_string(s: &CStr) -> Option<*mut AnyObject> {
    let class = AnyClass::get(c"NSString")?;
    let string: *mut AnyObject = unsafe { msg_send![class, stringWithUTF8String: s.as_ptr()] };
    (!string.is_null()).then_some(string)
}
