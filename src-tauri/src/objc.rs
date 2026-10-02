//! Raw objc2 messages to Foundation and AppKit (macOS): strings and data both ways, and the apps
//! LaunchServices knows. Raw `msg_send` rather than the objc2-* framework crates, which aren't
//! otherwise linked. Unless it says so, a function returning objects runs inside an autorelease pool.

use objc2::encode::{Encode, Encoding};
use objc2::msg_send;
use objc2::runtime::{AnyClass, AnyObject, Bool};
use std::ffi::{c_char, CStr, CString};
use std::path::{Path, PathBuf};

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

/// Where LaunchServices has the app, as `open -b` would find it.
pub fn app_path(bundle_id: &str) -> Option<PathBuf> {
    let id = CString::new(bundle_id).ok()?;
    let path = objc2::rc::autoreleasepool(|_| unsafe {
        let workspace: *mut AnyObject = msg_send![AnyClass::get(c"NSWorkspace")?, sharedWorkspace];
        let url: *mut AnyObject =
            msg_send![workspace, URLForApplicationWithBundleIdentifier: ns_string(&id)];
        if url.is_null() {
            return None;
        }
        rust_string(msg_send![url, path]).map(PathBuf::from)
    })?;
    // LaunchServices still knows an app that was just moved to the Trash.
    (!path.to_string_lossy().contains("/.Trash/")).then_some(path)
}

#[repr(C)]
struct Rect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

const PAIR: [Encoding; 2] = [f64::ENCODING, f64::ENCODING];
unsafe impl Encode for Rect {
    const ENCODING: Encoding = Encoding::Struct(
        "CGRect",
        &[
            Encoding::Struct("CGPoint", &PAIR),
            Encoding::Struct("CGSize", &PAIR),
        ],
    );
}

/// Finder's icon for `bundle`, as its Dock and the Open With menu show it: a PNG `px` wide,
/// drawn into a bitmap of that size, as the icon's own TIFF holds every size up to 1024.
pub fn app_icon(bundle: &Path, px: u32) -> Option<Vec<u8>> {
    const PNG: usize = 4; // NSBitmapImageFileTypePNG
    const SOURCE_OVER: usize = 2; // NSCompositingOperationSourceOver
    let path = c_string(&bundle.to_string_lossy());
    let side = f64::from(px);
    objc2::rc::autoreleasepool(|_| unsafe {
        let workspace: *mut AnyObject = msg_send![AnyClass::get(c"NSWorkspace")?, sharedWorkspace];
        let image: *mut AnyObject = msg_send![workspace, iconForFile: ns_string(&path)];
        if image.is_null() {
            return None;
        }
        let rep: *mut AnyObject = msg_send![AnyClass::get(c"NSBitmapImageRep")?, alloc];
        let rep: *mut AnyObject = msg_send![
            rep,
            initWithBitmapDataPlanes: std::ptr::null_mut::<*mut u8>(),
            pixelsWide: px as isize,
            pixelsHigh: px as isize,
            bitsPerSample: 8isize,
            samplesPerPixel: 4isize,
            hasAlpha: Bool::YES,
            isPlanar: Bool::NO,
            colorSpaceName: ns_string(c"NSDeviceRGBColorSpace"),
            bytesPerRow: 0isize,
            bitsPerPixel: 0isize
        ];
        if rep.is_null() {
            return None;
        }
        let png = (|| {
            let graphics = AnyClass::get(c"NSGraphicsContext")?;
            let context: *mut AnyObject =
                msg_send![graphics, graphicsContextWithBitmapImageRep: rep];
            if context.is_null() {
                return None;
            }
            let _: () = msg_send![graphics, saveGraphicsState];
            let _: () = msg_send![graphics, setCurrentContext: context];
            let whole = Rect {
                x: 0.0,
                y: 0.0,
                w: 0.0,
                h: 0.0,
            };
            let into = Rect {
                x: 0.0,
                y: 0.0,
                w: side,
                h: side,
            };
            let _: () = msg_send![image, drawInRect: into, fromRect: whole, operation: SOURCE_OVER, fraction: 1.0f64];
            let _: () = msg_send![graphics, restoreGraphicsState];
            let none: *mut AnyObject = msg_send![AnyClass::get(c"NSDictionary")?, dictionary];
            bytes(msg_send![rep, representationUsingType: PNG, properties: none])
        })();
        let _: () = msg_send![rep, release];
        png
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Terminal ships with every Mac.
    #[test]
    fn draws_an_apps_icon() {
        let terminal = app_path("com.apple.Terminal").unwrap();
        let png = app_icon(&terminal, 32).unwrap();
        assert!(png.starts_with(b"\x89PNG"));
        // IHDR's width and height.
        assert_eq!(png[16..24], [0, 0, 0, 32, 0, 0, 0, 32]);
        assert!(app_path("com.example.not-an-app").is_none());
    }

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
