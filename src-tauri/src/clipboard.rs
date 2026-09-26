//! The terminal's ⌘V and file drops (src/lib/terminal/paste.ts), as the AI CLIs in it take them:
//! a copied Finder file pastes its path, and so does an image, saved to a PNG first. Claude Code,
//! Codex and Gemini attach an image path that arrives in a paste; the webview's own paste only
//! carries text, so an image on the pasteboard pasted nothing.

use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Serialize, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Paste {
    Files { paths: Vec<String> },
    Text { text: String },
    Image { path: String },
    Empty,
}

/// Pasted images, out of the repo (no git noise) and per user; a CLI may read one well after the paste.
fn paste_dir() -> Result<PathBuf, String> {
    let dir = std::env::temp_dir().join("gitviber").join("paste");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    // A day is long past any CLI reading its attachment.
    let old = SystemTime::now() - Duration::from_secs(24 * 60 * 60);
    for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
        let stale = entry
            .metadata()
            .and_then(|m| m.modified())
            .is_ok_and(|t| t < old);
        if stale {
            let _ = std::fs::remove_file(entry.path());
        }
    }
    Ok(dir)
}

/// A new file in the paste folder, named `name` after a timestamp.
fn new_paste_file(name: &str) -> Result<PathBuf, String> {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    let dir = paste_dir()?;
    let mut n = 0;
    loop {
        let path = dir.join(format!("{ms}-{n}-{name}"));
        if !path.exists() {
            return Ok(path);
        }
        n += 1;
    }
}

/// Dropped files, with the ones macOS takes back once the drag ends (a screenshot's floating
/// thumbnail hands over a file in TemporaryItems) copied to the paste folder first.
pub fn keep_dropped(paths: Vec<String>) -> Vec<String> {
    paths
        .into_iter()
        .map(|p| {
            let path = Path::new(&p);
            if !p.contains("/TemporaryItems/") || !path.is_file() {
                return p;
            }
            let name = path
                .file_name()
                .map_or("dropped".into(), |n| n.to_string_lossy());
            new_paste_file(&name)
                .and_then(|to| {
                    std::fs::copy(path, &to)
                        .map(|_| to)
                        .map_err(|e| e.to_string())
                })
                .map_or(p.clone(), |to| to.to_string_lossy().into_owned())
        })
        .collect()
}

/// What ⌘V pastes into the terminal: copied files' paths first (a Finder copy also carries the
/// file's name as text and its icon as an image), then text, then an image saved as a PNG.
#[cfg(target_os = "macos")]
pub fn read() -> Result<Paste, String> {
    let (files, text, png) = objc2::rc::autoreleasepool(|_| unsafe { pasteboard::read() });
    if !files.is_empty() {
        return Ok(Paste::Files { paths: files });
    }
    if let Some(text) = text.filter(|t| !t.is_empty()) {
        return Ok(Paste::Text { text });
    }
    let Some(png) = png else {
        return Ok(Paste::Empty);
    };
    let path = new_paste_file("paste.png")?;
    std::fs::write(&path, png).map_err(|e| e.to_string())?;
    Ok(Paste::Image {
        path: path.to_string_lossy().into_owned(),
    })
}

#[cfg(not(target_os = "macos"))]
pub fn read() -> Result<Paste, String> {
    Ok(Paste::Empty)
}

#[cfg(target_os = "macos")]
mod pasteboard {
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject};
    use std::ffi::{c_char, CStr};

    unsafe fn ns_string(s: &CStr) -> *mut AnyObject {
        let Some(cls) = AnyClass::get(c"NSString") else {
            return std::ptr::null_mut();
        };
        msg_send![cls, stringWithUTF8String: s.as_ptr()]
    }

    unsafe fn rust_string(s: *mut AnyObject) -> Option<String> {
        if s.is_null() {
            return None;
        }
        let utf8: *const c_char = msg_send![s, UTF8String];
        (!utf8.is_null()).then(|| CStr::from_ptr(utf8).to_string_lossy().into_owned())
    }

    unsafe fn bytes(data: *mut AnyObject) -> Option<Vec<u8>> {
        if data.is_null() {
            return None;
        }
        let len: usize = msg_send![data, length];
        let ptr: *const u8 = msg_send![data, bytes];
        (!ptr.is_null() && len > 0).then(|| std::slice::from_raw_parts(ptr, len).to_vec())
    }

    /// The general pasteboard's file paths, text and image (as PNG). Call inside an autorelease pool.
    pub unsafe fn read() -> (Vec<String>, Option<String>, Option<Vec<u8>>) {
        let (Some(pb_class), Some(url_class)) =
            (AnyClass::get(c"NSPasteboard"), AnyClass::get(c"NSURL"))
        else {
            return (vec![], None, None);
        };
        let pb: *mut AnyObject = msg_send![pb_class, generalPasteboard];
        if pb.is_null() {
            return (vec![], None, None);
        }

        let mut files = vec![];
        let items: *mut AnyObject = msg_send![pb, pasteboardItems];
        let count: usize = if items.is_null() {
            0
        } else {
            msg_send![items, count]
        };
        let file_url = ns_string(c"public.file-url");
        for i in 0..count {
            let item: *mut AnyObject = msg_send![items, objectAtIndex: i];
            let url: *mut AnyObject = msg_send![item, stringForType: file_url];
            if url.is_null() {
                continue;
            }
            let url: *mut AnyObject = msg_send![url_class, URLWithString: url];
            let path: *mut AnyObject = if url.is_null() {
                std::ptr::null_mut()
            } else {
                msg_send![url, path]
            };
            files.extend(rust_string(path));
        }

        let text: *mut AnyObject =
            msg_send![pb, stringForType: ns_string(c"public.utf8-plain-text")];
        let text = rust_string(text);

        // Screenshots and most apps put PNG; some only TIFF, which AppKit re-encodes.
        let mut png = bytes(msg_send![pb, dataForType: ns_string(c"public.png")]);
        if png.is_none() {
            png = tiff_to_png(msg_send![pb, dataForType: ns_string(c"public.tiff")]);
        }
        (files, text, png)
    }

    unsafe fn tiff_to_png(tiff: *mut AnyObject) -> Option<Vec<u8>> {
        let (Some(rep_class), Some(dict_class)) = (
            AnyClass::get(c"NSBitmapImageRep"),
            AnyClass::get(c"NSDictionary"),
        ) else {
            return None;
        };
        if tiff.is_null() {
            return None;
        }
        let rep: *mut AnyObject = msg_send![rep_class, imageRepWithData: tiff];
        if rep.is_null() {
            return None;
        }
        let props: *mut AnyObject = msg_send![dict_class, dictionary];
        // NSBitmapImageFileTypePNG
        bytes(msg_send![rep, representationUsingType: 4usize, properties: props])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dropped_file_outside_temporary_items_keeps_its_path() {
        assert_eq!(
            keep_dropped(vec!["/Users/me/shot.png".into()]),
            vec!["/Users/me/shot.png".to_string()]
        );
    }

    #[test]
    fn a_screenshot_thumbnail_is_copied_before_macos_takes_it_back() {
        let dir = std::env::temp_dir().join(format!(
            "gv-drop-test-{}/TemporaryItems",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let shot = dir.join("Screenshot.png");
        std::fs::write(&shot, b"png").unwrap();
        let kept = keep_dropped(vec![shot.to_string_lossy().into_owned()]);
        assert_ne!(kept[0], shot.to_string_lossy());
        assert!(kept[0].ends_with("-Screenshot.png"));
        assert_eq!(std::fs::read(&kept[0]).unwrap(), b"png");
        let _ = std::fs::remove_file(&kept[0]);
        let _ = std::fs::remove_dir_all(dir.parent().unwrap());
    }
}
