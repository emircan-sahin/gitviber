//! Conversions between Rust and Foundation (macOS) for raw objc2 messages: strings and data,
//! both ways. Only these; the calls into a framework stay with the module that makes them.
//! Raw `msg_send` rather than the objc2-* framework crates, which aren't otherwise linked.
//! Returned objects are autoreleased: call inside a pool.

use objc2::msg_send;
use objc2::runtime::{AnyClass, AnyObject};
use std::ffi::{c_char, CStr, CString};

/// An autoreleased NSString; null if Foundation is somehow missing.
pub unsafe fn ns_string(s: &CStr) -> *mut AnyObject {
    let Some(cls) = AnyClass::get(c"NSString") else {
        return std::ptr::null_mut();
    };
    msg_send![cls, stringWithUTF8String: s.as_ptr()]
}

pub unsafe fn rust_string(s: *mut AnyObject) -> Option<String> {
    if s.is_null() {
        return None;
    }
    let utf8: *const c_char = msg_send![s, UTF8String];
    (!utf8.is_null()).then(|| CStr::from_ptr(utf8).to_string_lossy().into_owned())
}

/// An NSData's bytes; None for nil or empty.
pub unsafe fn bytes(data: *mut AnyObject) -> Option<Vec<u8>> {
    if data.is_null() {
        return None;
    }
    let len: usize = msg_send![data, length];
    let ptr: *const u8 = msg_send![data, bytes];
    (!ptr.is_null() && len > 0).then(|| std::slice::from_raw_parts(ptr, len).to_vec())
}

/// Text from outside (a terminal's notification) can hold a NUL, which a C string can't.
pub fn c_string(s: &str) -> CString {
    CString::new(s.replace('\0', "")).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strings_both_ways() {
        objc2::rc::autoreleasepool(|_| unsafe {
            let s = ns_string(&c_string("a\0b ü"));
            assert_eq!(rust_string(s), Some("ab ü".into()));
            assert_eq!(rust_string(std::ptr::null_mut()), None);
            assert_eq!(bytes(std::ptr::null_mut()), None);
        });
    }
}
