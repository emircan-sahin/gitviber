//! The terminal's ⌘V and file drops (src/lib/terminal/terminals.ts), as the AI CLIs in it take them:
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
    let dir = user_temp().join("gitviber").join("paste");
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    // A screenshot can hold anything: only its owner reads it.
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(&dir).map_err(|e| e.to_string())?;
    // A day is long past any CLI reading its attachment.
    let old = SystemTime::now() - Duration::from_secs(24 * 60 * 60);
    for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
        let stale = entry
            .metadata()
            .and_then(|m| m.modified())
            .is_ok_and(|t| t < old);
        if stale {
            let path = entry.path();
            let _ = if path.is_dir() {
                std::fs::remove_dir_all(path)
            } else {
                std::fs::remove_file(path)
            };
        }
    }
    Ok(dir)
}

/// macOS's temp dir is the user's own; Linux's /tmp is everyone's, and the first user to paste
/// would own the folder, so there it's the session's runtime dir (or ~/.cache).
fn user_temp() -> PathBuf {
    #[cfg(target_os = "linux")]
    {
        let runtime = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from);
        if let Some(dir) = runtime.filter(|d| d.is_absolute() && d.is_dir()) {
            return dir;
        }
        if let Some(home) = std::env::var_os("HOME") {
            return Path::new(&home).join(".cache");
        }
    }
    std::env::temp_dir()
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

/// `bytes` saved as `name` in a folder of their own, so a copy pasted into Finder keeps the name.
pub fn save_named(name: &str, bytes: &[u8]) -> Result<String, String> {
    // Claimed by creating it: two copies in the same millisecond would both find a name free.
    let dir = loop {
        let dir = new_paste_file("copy")?;
        match std::fs::create_dir(&dir) {
            Ok(()) => break dir,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.to_string()),
        }
    };
    let path = dir.join(name);
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().into_owned())
}

/// Files onto the pasteboard as Finder's ⌘C puts them, so they paste as files into Finder, Slack
/// or a terminal (which pastes their paths); an image also carries its picture for apps that
/// only take one.
#[cfg(target_os = "macos")]
pub fn copy_files(paths: Vec<String>) -> Result<(), String> {
    objc2::rc::autoreleasepool(|_| unsafe { pasteboard::write_files(&paths) })
}

#[cfg(not(target_os = "macos"))]
pub fn copy_files(_paths: Vec<String>) -> Result<(), String> {
    Err("Copying files is only supported on macOS".into())
}

/// Text a program in the terminal copies with OSC 52 (terminals.ts), or a patch git wrote: the
/// webview writes the clipboard only during a key or a click, and these arrive later.
#[cfg(target_os = "macos")]
pub fn write_text(text: &str) -> Result<(), String> {
    objc2::rc::autoreleasepool(|_| unsafe { pasteboard::write_text(text) })
}

#[cfg(target_os = "linux")]
pub fn write_text(text: &str) -> Result<(), String> {
    // GTK panics off its thread (see `contents`).
    if !gtk::is_initialized_main_thread() {
        return Err("The clipboard is unavailable".into());
    }
    gtk::Clipboard::get(&gtk::gdk::SELECTION_CLIPBOARD).set_text(text);
    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn write_text(_text: &str) -> Result<(), String> {
    Err("Copying is only supported on macOS and Linux".into())
}

/// Dropped files, with the ones macOS takes back once the drag ends (a screenshot's floating
/// thumbnail hands over a file in TemporaryItems) copied to the paste folder first.
pub fn keep_dropped(paths: Vec<String>) -> Vec<String> {
    paths
        .into_iter()
        .map(|p| {
            if !p.contains("/TemporaryItems/") {
                return p;
            }
            copy_dropped(Path::new(&p)).unwrap_or(p)
        })
        .collect()
}

/// A copy of a dropped file where macOS won't take it back, named as it was.
fn copy_dropped(path: &Path) -> Option<String> {
    if !path.is_file() {
        return None;
    }
    let name = path
        .file_name()
        .map_or("dropped".into(), |n| n.to_string_lossy());
    let to = new_paste_file(&name).ok()?;
    std::fs::copy(path, &to).ok()?;
    Some(to.to_string_lossy().into_owned())
}

/// What ⌘V found on the pasteboard (`read`).
pub enum Clip {
    /// Files, text or nothing: pasted as they are.
    Ready(Paste),
    /// An image, still to be saved as a PNG: `Image::save`, off the main thread.
    Image(Image),
}

/// A pasted image as the pasteboard held it.
pub struct Image(Raw);

enum Raw {
    #[cfg(target_os = "macos")]
    Png(pasteboard::Data),
    /// Some apps put only TIFF.
    #[cfg(target_os = "macos")]
    Tiff(pasteboard::Data),
    /// GTK's image, decoded: its pixels, as a Pixbuf can't leave its thread.
    #[cfg(target_os = "linux")]
    Pixels {
        bytes: gtk::glib::Bytes,
        colorspace: gtk::gdk_pixbuf::Colorspace,
        alpha: bool,
        bits: i32,
        width: i32,
        height: i32,
        stride: i32,
    },
}

impl Image {
    /// Saved as a PNG in the paste folder; Empty when it won't encode. Off the main thread:
    /// re-encoding a 5K TIFF took 190 ms.
    pub fn save(self) -> Result<Paste, String> {
        let path = new_paste_file("paste.png")?;
        let written = match self.0 {
            #[cfg(target_os = "macos")]
            Raw::Png(png) => std::fs::write(&path, png.bytes()),
            #[cfg(target_os = "macos")]
            Raw::Tiff(tiff) => {
                match objc2::rc::autoreleasepool(|_| unsafe { pasteboard::tiff_to_png(&tiff) }) {
                    Some(png) => std::fs::write(&path, png),
                    None => return Ok(Paste::Empty),
                }
            }
            #[cfg(target_os = "linux")]
            Raw::Pixels {
                bytes,
                colorspace,
                alpha,
                bits,
                width,
                height,
                stride,
            } => {
                let pixbuf = gtk::gdk_pixbuf::Pixbuf::from_bytes(
                    &bytes, colorspace, alpha, bits, width, height, stride,
                );
                match pixbuf.save_to_bufferv("png", &[]) {
                    Ok(png) => std::fs::write(&path, png),
                    Err(_) => return Ok(Paste::Empty),
                }
            }
        };
        written.map_err(|e| e.to_string())?;
        Ok(Paste::Image {
            path: path.to_string_lossy().into_owned(),
        })
    }
}

/// What ⌘V pastes into the terminal: copied files' paths first (a Finder copy also carries the
/// file's name as text and its icon as an image), then text, then an image. On the main thread,
/// and quick: an image is only taken, `Image::save` makes it a file.
pub fn read() -> Clip {
    contents()
}

/// The clipboard's text alone, for Apply Patch from Clipboard: the webview's own read asks the
/// user first with a Paste button.
#[cfg(target_os = "macos")]
pub fn read_text() -> Result<String, String> {
    Ok(objc2::rc::autoreleasepool(|_| unsafe { pasteboard::read_text() }).unwrap_or_default())
}

#[cfg(target_os = "linux")]
pub fn read_text() -> Result<String, String> {
    // GTK panics off its thread (see `contents`).
    if !gtk::is_initialized_main_thread() {
        return Err("The clipboard is unavailable".into());
    }
    let clipboard = gtk::Clipboard::get(&gtk::gdk::SELECTION_CLIPBOARD);
    Ok(clipboard
        .wait_for_text()
        .map(String::from)
        .unwrap_or_default())
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn read_text() -> Result<String, String> {
    Err("Reading the clipboard is only supported on macOS and Linux".into())
}

#[cfg(target_os = "macos")]
fn contents() -> Clip {
    objc2::rc::autoreleasepool(|_| unsafe { pasteboard::read() })
}

/// GTK's clipboard, read as the pasteboard is: a file manager's copied files arrive as URIs.
/// Asks once what's offered, then only for the first kind `read` takes: each request can wait
/// GTK's 30 s on an owner that has hung.
#[cfg(target_os = "linux")]
fn contents() -> Clip {
    let empty = Clip::Ready(Paste::Empty);
    // GTK panics off its thread; terminal_paste is sync so it runs there, but never crash on it.
    if !gtk::is_initialized_main_thread() {
        return empty;
    }
    let clipboard = gtk::Clipboard::get(&gtk::gdk::SELECTION_CLIPBOARD);
    let Some(targets) = clipboard.wait_for_targets() else {
        return empty;
    };
    if gtk::targets_include_uri(&targets) {
        let paths: Vec<String> = clipboard
            .wait_for_uris()
            .iter()
            .filter_map(|uri| gtk::glib::filename_from_uri(uri.as_str()).ok())
            .map(|(path, _)| path.to_string_lossy().into_owned())
            .collect();
        if !paths.is_empty() {
            return Clip::Ready(Paste::Files { paths });
        }
    }
    if gtk::targets_include_text(&targets) {
        let text = clipboard.wait_for_text().map(String::from);
        if let Some(text) = text.filter(|t| !t.is_empty()) {
            return Clip::Ready(Paste::Text { text });
        }
    }
    let image = gtk::targets_include_image(&targets, false)
        .then(|| clipboard.wait_for_image())
        .flatten();
    match image {
        Some(p) => Clip::Image(Image(Raw::Pixels {
            bytes: p.read_pixel_bytes(),
            colorspace: p.colorspace(),
            alpha: p.has_alpha(),
            bits: p.bits_per_sample(),
            width: p.width(),
            height: p.height(),
            stride: p.rowstride(),
        })),
        None => empty,
    }
}

/// Elsewhere the terminal keeps the webview's own paste (terminals.ts), so this isn't called.
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn contents() -> Clip {
    Clip::Ready(Paste::Empty)
}

#[cfg(target_os = "macos")]
mod pasteboard {
    use super::{Clip, Image, Paste, Raw};
    use crate::objc::{bytes, ns_string, rust_string};
    use objc2::msg_send;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject};
    use std::ffi::{CStr, CString};
    use std::path::Path;

    /// The pasteboard's data, kept for another thread to read.
    pub struct Data(Retained<AnyObject>);

    // SAFETY: an NSData the pasteboard handed out is a copy nothing changes, and NSData is safe
    // to read from any thread.
    unsafe impl Send for Data {}

    impl Data {
        /// None for nil or empty.
        pub unsafe fn take(data: *mut AnyObject) -> Option<Data> {
            if data.is_null() {
                return None;
            }
            let len: usize = msg_send![data, length];
            (len > 0)
                .then(|| Retained::retain(data))
                .flatten()
                .map(Data)
        }

        pub fn bytes(&self) -> &[u8] {
            unsafe {
                let len: usize = msg_send![&*self.0, length];
                // `const void *`: objc2 checks the declared type in debug builds.
                let ptr: *const std::ffi::c_void = msg_send![&*self.0, bytes];
                if ptr.is_null() {
                    return &[];
                }
                std::slice::from_raw_parts(ptr.cast::<u8>(), len)
            }
        }
    }

    /// The general pasteboard's files, else its text, else its image, taken as it is: a TIFF's
    /// re-encoding waits for `Image::save`. Call inside an autorelease pool.
    pub unsafe fn read() -> Clip {
        let empty = Clip::Ready(Paste::Empty);
        let Some(pb) = general() else {
            return empty;
        };
        let paths = files(pb);
        if !paths.is_empty() {
            return Clip::Ready(Paste::Files { paths });
        }
        let text = rust_string(msg_send![pb, stringForType: ns_string(c"public.utf8-plain-text")]);
        if let Some(text) = text.filter(|t| !t.is_empty()) {
            return Clip::Ready(Paste::Text { text });
        }
        // Screenshots and most apps put PNG.
        let raw = match data(pb, c"public.png") {
            Some(png) => Raw::Png(png),
            None => match data(pb, c"public.tiff") {
                Some(tiff) => Raw::Tiff(tiff),
                None => return empty,
            },
        };
        Clip::Image(Image(raw))
    }

    pub unsafe fn data(pb: *mut AnyObject, kind: &CStr) -> Option<Data> {
        Data::take(msg_send![pb, dataForType: ns_string(kind)])
    }

    pub unsafe fn general() -> Option<*mut AnyObject> {
        let pb: *mut AnyObject = msg_send![AnyClass::get(c"NSPasteboard")?, generalPasteboard];
        (!pb.is_null()).then_some(pb)
    }

    /// The paths of the files on the pasteboard.
    pub unsafe fn files(pb: *mut AnyObject) -> Vec<String> {
        let Some(url_class) = AnyClass::get(c"NSURL") else {
            return vec![];
        };
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
        files
    }

    /// The general pasteboard's text. Call inside an autorelease pool.
    pub unsafe fn read_text() -> Option<String> {
        rust_string(msg_send![general()?, stringForType: ns_string(c"public.utf8-plain-text")])
    }

    /// Extensions NSImage reads, whose picture goes on the pasteboard next to the file.
    const IMAGES: &[&str] = &[
        "png", "jpg", "jpeg", "gif", "webp", "bmp", "tif", "tiff", "heic", "avif", "ico",
    ];

    /// One pasteboard item per file that exists: its URL, and for a single image its picture (a
    /// PNG's own bytes, another format as TIFF).
    pub unsafe fn write_files(paths: &[String]) -> Result<(), String> {
        let (
            Some(pb_class),
            Some(url_class),
            Some(item_class),
            Some(array_class),
            Some(image_class),
        ) = (
            AnyClass::get(c"NSPasteboard"),
            AnyClass::get(c"NSURL"),
            AnyClass::get(c"NSPasteboardItem"),
            AnyClass::get(c"NSMutableArray"),
            AnyClass::get(c"NSImage"),
        )
        else {
            return Err("AppKit is unavailable".into());
        };
        let items: *mut AnyObject = msg_send![array_class, array];
        // A picture goes with a single file only, as a paste into an image app takes one: every
        // image decoded here would hold the main thread.
        let picture = paths.len() == 1;
        for path in paths {
            // A deleted file's URL would paste nothing, after "copied".
            if !Path::new(path).exists() {
                continue;
            }
            let Ok(c_path) = CString::new(path.as_str()) else {
                continue;
            };
            let ns_path = ns_string(&c_path);
            let url: *mut AnyObject = msg_send![url_class, fileURLWithPath: ns_path];
            let url_string: *mut AnyObject = msg_send![url, absoluteString];
            let item: *mut AnyObject = msg_send![item_class, new];
            let _: bool =
                msg_send![item, setString: url_string, forType: ns_string(c"public.file-url")];
            let ext = Path::new(path)
                .extension()
                .map(|e| e.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            if picture && ext == "png" {
                if let Ok(png) = std::fs::read(path) {
                    let _: bool =
                        msg_send![item, setData: ns_data(&png), forType: ns_string(c"public.png")];
                }
            } else if picture && IMAGES.contains(&ext.as_str()) {
                let image: *mut AnyObject = msg_send![image_class, alloc];
                let image: *mut AnyObject = msg_send![image, initWithContentsOfFile: ns_path];
                if !image.is_null() {
                    let tiff: *mut AnyObject = msg_send![image, TIFFRepresentation];
                    if !tiff.is_null() {
                        let _: bool =
                            msg_send![item, setData: tiff, forType: ns_string(c"public.tiff")];
                    }
                    let _: () = msg_send![image, release];
                }
            }
            let _: () = msg_send![items, addObject: item];
            let _: () = msg_send![item, release];
        }
        let count: usize = msg_send![items, count];
        if count == 0 {
            return Err("Nothing to copy".into());
        }
        let pb: *mut AnyObject = msg_send![pb_class, generalPasteboard];
        let _: isize = msg_send![pb, clearContents];
        let ok: bool = msg_send![pb, writeObjects: items];
        ok.then_some(())
            .ok_or_else(|| "Could not write to the pasteboard".into())
    }

    /// `text` as all the pasteboard holds.
    pub unsafe fn write_text(text: &str) -> Result<(), String> {
        let (Some(pb_class), Some(string_class)) =
            (AnyClass::get(c"NSPasteboard"), AnyClass::get(c"NSString"))
        else {
            return Err("AppKit is unavailable".into());
        };
        // By length (NSUTF8StringEncoding): as a C string, the text would end at its first NUL.
        let string: *mut AnyObject = msg_send![string_class, alloc];
        let string: *mut AnyObject = msg_send![string, initWithBytes: text.as_ptr().cast::<std::ffi::c_void>(), length: text.len(), encoding: 4usize];
        if string.is_null() {
            return Err("Could not write to the pasteboard".into());
        }
        let pb: *mut AnyObject = msg_send![pb_class, generalPasteboard];
        let _: isize = msg_send![pb, clearContents];
        let ok: bool =
            msg_send![pb, setString: string, forType: ns_string(c"public.utf8-plain-text")];
        let _: () = msg_send![string, release];
        ok.then_some(())
            .ok_or_else(|| "Could not write to the pasteboard".into())
    }

    pub unsafe fn ns_data(bytes: &[u8]) -> *mut AnyObject {
        let Some(cls) = AnyClass::get(c"NSData") else {
            return std::ptr::null_mut();
        };
        msg_send![cls, dataWithBytes: bytes.as_ptr().cast::<std::ffi::c_void>(), length: bytes.len()]
    }

    /// AppKit's PNG of it. Call inside an autorelease pool, on any thread.
    pub unsafe fn tiff_to_png(tiff: &Data) -> Option<Vec<u8>> {
        let (Some(rep_class), Some(dict_class)) = (
            AnyClass::get(c"NSBitmapImageRep"),
            AnyClass::get(c"NSDictionary"),
        ) else {
            return None;
        };
        let rep: *mut AnyObject = msg_send![rep_class, imageRepWithData: &*tiff.0];
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

    // Not in a folder named TemporaryItems of its own: macOS won't let one be removed again.
    #[test]
    fn a_screenshot_thumbnail_is_copied_before_macos_takes_it_back() {
        let dir = std::env::temp_dir().join(format!("gv-drop-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let shot = dir.join("Screenshot.png");
        std::fs::write(&shot, b"png").unwrap();
        let kept = copy_dropped(&shot).unwrap();
        assert!(kept.ends_with("-Screenshot.png"));
        assert_eq!(std::fs::read(&kept).unwrap(), b"png");
        let _ = std::fs::remove_file(&kept);
        let _ = std::fs::remove_dir_all(&dir);
        // One that's gone by now stays as it was dropped.
        let gone = "/private/var/folders/x/T/TemporaryItems/NSIRD_screencaptureui_x/Shot.png";
        assert_eq!(keep_dropped(vec![gone.into()]), vec![gone.to_string()]);
    }

    #[test]
    fn a_saved_version_keeps_its_name() {
        let path = save_named("logo.png", b"png").unwrap();
        assert!(path.ends_with("-copy/logo.png"));
        assert_eq!(std::fs::read(&path).unwrap(), b"png");
        let _ = std::fs::remove_dir_all(Path::new(&path).parent().unwrap());
    }

    /// A 1×1 PNG.
    #[cfg(target_os = "macos")]
    const PNG: [u8; 70] = [
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0x0d, 0x49, 0x48, 0x44, 0x52, 0,
        0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0, 0x1f, 0x15, 0xc4, 0x89, 0, 0, 0, 0x0d, 0x49, 0x44,
        0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8, 0xcf, 0xc0, 0xf0, 0x1f, 0, 0x05, 0, 0x01, 0xff, 0x89,
        0x99, 0x3d, 0x1d, 0, 0, 0, 0, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];

    /// Taken on one thread, saved on another: a PNG as it was, a TIFF re-encoded as one.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_pasted_image_is_saved_off_the_thread_that_took_it() {
        use objc2::msg_send;
        use objc2::runtime::{AnyClass, AnyObject};
        let (png, tiff) = objc2::rc::autoreleasepool(|_| unsafe {
            let data = pasteboard::ns_data(&PNG);
            let rep: *mut AnyObject = msg_send![
                AnyClass::get(c"NSBitmapImageRep").unwrap(),
                imageRepWithData: data
            ];
            let tiff: *mut AnyObject = msg_send![rep, TIFFRepresentation];
            (
                pasteboard::Data::take(data).unwrap(),
                pasteboard::Data::take(tiff).unwrap(),
            )
        });
        let save = |raw| {
            let saved = std::thread::spawn(move || Image(raw).save());
            match saved.join().unwrap().unwrap() {
                Paste::Image { path } => {
                    assert!(path.ends_with("-paste.png"), "{path}");
                    let bytes = std::fs::read(&path).unwrap();
                    let _ = std::fs::remove_file(&path);
                    bytes
                }
                other => panic!("{other:?}"),
            }
        };
        assert_eq!(save(Raw::Png(png)), PNG);
        assert!(save(Raw::Tiff(tiff)).starts_with(&PNG[..8]));
    }

    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "replaces the clipboard"]
    fn copied_text_pastes_back() {
        write_text("héllo ✅").unwrap();
        assert_eq!(read_text().unwrap(), "héllo ✅");
        // Through a C string, a NUL failed the copy.
        write_text("a\0b").unwrap();
    }

    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "replaces the clipboard"]
    fn copied_files_paste_back_as_files_with_their_picture() {
        let image = save_named("a b.png", &PNG).unwrap();
        let text = save_named("notes.txt", b"hi").unwrap();
        let read = || {
            objc2::rc::autoreleasepool(|_| unsafe {
                let pb = pasteboard::general().unwrap();
                let picture = pasteboard::data(pb, c"public.png").map(|d| d.bytes().to_vec());
                (pasteboard::files(pb), picture)
            })
        };

        copy_files(vec![image.clone()]).unwrap();
        let (files, picture) = read();
        assert_eq!(files, vec![image.clone()]);
        assert_eq!(picture.as_deref(), Some(&PNG[..]));
        // Files paste as files, not as their picture.
        assert!(matches!(
            super::read(),
            Clip::Ready(Paste::Files { paths }) if paths == vec![image.clone()]
        ));

        // Several files carry no picture, and a missing one is left out.
        let gone = format!("{text}.gone");
        copy_files(vec![image.clone(), gone, text.clone()]).unwrap();
        let (files, picture) = read();
        assert_eq!(files, vec![image.clone(), text.clone()]);
        assert_eq!(picture, None);
        for p in [image, text] {
            let _ = std::fs::remove_dir_all(Path::new(&p).parent().unwrap());
        }
    }
}
