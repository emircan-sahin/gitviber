//! Paths handed to the app from outside its window: `gitviber <path>` (`path:line:column` too),
//! a folder dropped on the Dock icon or opened with Finder's Open With, a second launch. Queued
//! until the page takes them, since a cold launch delivers them before it has loaded.

use crate::git;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};

#[derive(Default)]
pub struct Opened(Mutex<Vec<PathBuf>>);

/// Queues `paths` and tells the page. Not resolved here: this runs on the main thread, where a
/// stalled network volume would freeze the window; `take` resolves them off it.
pub fn push(app: &AppHandle, paths: Vec<PathBuf>) {
    if paths.is_empty() {
        return;
    }
    app.state::<Opened>()
        .0
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .extend(paths);
    let _ = app.emit("opened", ());
    raise(app);
}

/// Brings the window forward, once the page has shown it: a still-hidden window at launch waits
/// for its theme (main.tsx), or it would flash unstyled.
pub fn raise(app: &AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    if window.is_visible().unwrap_or(false) {
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

#[derive(Serialize, Default)]
pub struct Taken {
    pub open: Vec<Target>,
    /// Paths that name nothing on disk, as they were given.
    pub missing: Vec<String>,
}

#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Target {
    /// The folder to open as a project: the path's own, or the one its file is in.
    pub folder: String,
    /// That file, from its repository's root (the folder's, outside one).
    pub file: Option<String>,
    pub line: Option<u32>,
    pub column: Option<u32>,
}

/// What's queued, emptied: the folders to open, with a file to show when one was named.
pub fn take(app: &AppHandle) -> Taken {
    let paths = std::mem::take(
        &mut *app
            .state::<Opened>()
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner()),
    );
    let mut taken = Taken::default();
    for p in paths
        .into_iter()
        .flat_map(|p| requested(&p).unwrap_or_else(|| vec![p]))
    {
        match target(&p) {
            Some(t) => taken.open.push(t),
            None => taken.missing.push(p.to_string_lossy().into_owned()),
        }
    }
    taken
}

/// `open` on macOS only carries paths that exist, so resources/gitviber writes one that doesn't
/// (`a.ts:12`) into a request file named so, and opens that. Read once: the file goes.
fn requested(path: &Path) -> Option<Vec<PathBuf>> {
    let name = path.file_name()?.to_str()?;
    let dir = path.parent()?.canonicalize().ok()?;
    let temp = [std::env::temp_dir(), "/tmp".into()].map(|t| t.canonicalize().ok());
    // A plain file in a temp folder: not a FIFO to hang on, nor a link to somewhere else.
    if !name.starts_with("gitviber-open.")
        || !temp.contains(&Some(dir))
        || !std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_file())
    {
        return None;
    }
    let text = std::fs::read_to_string(path).ok()?;
    let _ = std::fs::remove_file(path);
    Some(
        text.lines()
            .filter(|l| !l.is_empty())
            .map(PathBuf::from)
            .collect(),
    )
}

fn target(spec: &Path) -> Option<Target> {
    let (path, line, column) = position(spec)?;
    let path = path.canonicalize().ok()?;
    let text = |p: &Path| p.to_string_lossy().into_owned();
    if path.is_dir() {
        return Some(Target {
            folder: text(&path),
            file: None,
            line: None,
            column: None,
        });
    }
    let dir = path.parent()?;
    // The page opens the folder's repository, then the file by its path from there.
    let prefix = git::run_text(dir, &["rev-parse", "--show-prefix"]).unwrap_or_default();
    let name = path.file_name()?.to_string_lossy();
    Some(Target {
        folder: text(dir),
        file: Some(format!("{}{name}", prefix.trim_end_matches('\n'))),
        line,
        column,
    })
}

/// `spec` itself when it exists, so a name with a colon in it stays whole; else `path:line` or
/// `path:line:column`, as `code -g` and Zed read them. Counted from the end, so a Windows
/// drive (C:\) is never taken for a line.
fn position(spec: &Path) -> Option<(PathBuf, Option<u32>, Option<u32>)> {
    if spec.exists() {
        return Some((spec.to_path_buf(), None, None));
    }
    // grep -n and compilers end it with one more colon: `a.ts:12:`.
    let text = spec.to_str()?;
    let text = text.strip_suffix(':').unwrap_or(text);
    if Path::new(text).exists() {
        return Some((text.into(), None, None));
    }
    // Past what an editor can show, the last line will do.
    let number = |t: &str| {
        (!t.is_empty() && t.bytes().all(|b| b.is_ascii_digit())).then(|| {
            t.parse::<u64>()
                .unwrap_or(u64::MAX)
                .clamp(1, i32::MAX as u64) as u32
        })
    };
    let (rest, last) = text.rsplit_once(':')?;
    let last = number(last)?;
    if let Some((path, line)) = rest.rsplit_once(':') {
        if let Some(line) = number(line).filter(|_| Path::new(path).exists()) {
            return Some((path.into(), Some(line), Some(last)));
        }
    }
    Path::new(rest)
        .exists()
        .then(|| (rest.into(), Some(last), None))
}

/// The paths among a launch's arguments, against the folder it was run in. Flags aren't paths.
pub fn from_args(args: &[std::ffi::OsString], cwd: &Path) -> Vec<PathBuf> {
    args.iter()
        .skip(1)
        .filter(|a| !a.to_string_lossy().starts_with('-'))
        .map(|a| cwd.join(a))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_opens_its_folder_at_a_line_and_flags_are_skipped() {
        let dir = std::env::temp_dir().join(format!("gitviber-opened-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("sub/a.txt"), "").unwrap();
        std::fs::write(dir.join("sub/b:2"), "").unwrap();
        let args = [
            "gitviber",
            "--flag",
            "sub/a.txt",
            "sub",
            "sub/a.txt:12",
            "sub/a.txt:12:5",
            "sub/b:2",
            "sub/b:2:7",
            "sub:3",
            "sub/a.txt:12:",
            "sub/a.txt:",
            "sub/a.txt:99999999999999999999",
            "sub/a.txt:x",
            "missing:1",
        ]
        .map(std::ffi::OsString::from);
        let found: Vec<_> = from_args(&args, &dir)
            .iter()
            .map(|p| target(p).map(|t| (t.file, t.line, t.column)))
            .collect();
        let file = |f: &str, line, column| Some((Some(f.to_string()), line, column));
        assert_eq!(
            found,
            [
                file("a.txt", None, None),
                Some((None, None, None)),
                file("a.txt", Some(12), None),
                file("a.txt", Some(12), Some(5)),
                // A name with a colon that exists is that file; past it, the numbers are where.
                file("b:2", None, None),
                file("b:2", Some(7), None),
                Some((None, None, None)),
                file("a.txt", Some(12), None),
                file("a.txt", None, None),
                file("a.txt", Some(i32::MAX as u32), None),
                None,
                None,
            ]
        );
        // In a repository, the file is named from its root.
        git::run(&dir, &["init", "-q"]).unwrap();
        let sub = dir.join("sub").canonicalize().unwrap();
        let t = target(&dir.join("sub/a.txt:1")).unwrap();
        assert_eq!(t.folder, sub.to_string_lossy());
        assert_eq!(t.file.as_deref(), Some("sub/a.txt"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_request_file_names_the_paths_and_goes() {
        let request = std::env::temp_dir().join(format!("gitviber-open.t{}", std::process::id()));
        std::fs::write(&request, "/a/b.ts:3\n\n/c\n").unwrap();
        let paths = requested(&request).unwrap();
        assert_eq!(paths, [PathBuf::from("/a/b.ts:3"), PathBuf::from("/c")]);
        assert!(!request.exists());
        assert!(requested(Path::new("/tmp/other.txt")).is_none());
        // Only a plain file in a temp folder: not one elsewhere, nor a link.
        let dir = std::env::temp_dir().join(format!("gitviber-req-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let elsewhere = dir.join("gitviber-open.x");
        std::fs::write(&elsewhere, "/c\n").unwrap();
        assert!(requested(&elsewhere).is_none());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&elsewhere, &request).unwrap();
            assert!(requested(&request).is_none());
            std::fs::remove_file(&request).unwrap();
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
