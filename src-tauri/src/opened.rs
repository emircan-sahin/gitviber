//! Folders handed to the app from outside its window: `gitviber <path>`, a folder dropped on the
//! Dock icon or opened with Finder's Open With, a second launch. Queued until the page takes
//! them, since a cold launch delivers them before it has loaded.

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

/// What's queued, emptied, as the folders to open (a file stands for its own).
pub fn take(app: &AppHandle) -> Vec<String> {
    let paths = std::mem::take(
        &mut *app
            .state::<Opened>()
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner()),
    );
    paths.iter().filter_map(|p| folder(p)).collect()
}

fn folder(path: &Path) -> Option<String> {
    let path = path.canonicalize().ok()?;
    let dir = if path.is_dir() {
        path
    } else {
        path.parent()?.to_path_buf()
    };
    Some(dir.to_string_lossy().into_owned())
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
    fn a_file_opens_its_folder_and_flags_are_skipped() {
        let dir = std::env::temp_dir().join(format!("gitviber-opened-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("sub/a.txt"), "").unwrap();
        let args =
            ["gitviber", "--flag", "sub/a.txt", "sub", "missing"].map(std::ffi::OsString::from);
        let found: Vec<_> = from_args(&args, &dir)
            .iter()
            .filter_map(|p| folder(p))
            .collect();
        let sub = dir.join("sub").canonicalize().unwrap();
        assert_eq!(found, vec![sub.to_string_lossy().to_string(); 2]);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
