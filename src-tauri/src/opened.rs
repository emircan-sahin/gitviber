//! Folders handed to the app from outside its window: `gitviber <path>`, a folder dropped on the
//! Dock icon or opened with Finder's Open With, a second launch. Queued until the page takes
//! them, since a cold launch delivers them before it has loaded.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};

#[derive(Default)]
pub struct Opened(Mutex<Vec<String>>);

/// Queues `paths` (a file stands for its folder), tells the page and brings the window up.
pub fn push(app: &AppHandle, paths: impl IntoIterator<Item = PathBuf>) {
    let folders: Vec<String> = paths.into_iter().filter_map(|p| folder(&p)).collect();
    if folders.is_empty() {
        return;
    }
    app.state::<Opened>()
        .0
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .extend(folders);
    let _ = app.emit("opened", ());
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// What's queued, emptied.
pub fn take(app: &AppHandle) -> Vec<String> {
    std::mem::take(
        &mut *app
            .state::<Opened>()
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner()),
    )
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
pub fn from_args(args: &[String], cwd: &Path) -> Vec<PathBuf> {
    args.iter()
        .skip(1)
        .filter(|a| !a.starts_with('-'))
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
        let args = ["gitviber", "--flag", "sub/a.txt", "sub", "missing"].map(String::from);
        let found: Vec<_> = from_args(&args, &dir)
            .iter()
            .filter_map(|p| folder(p))
            .collect();
        let sub = dir.join("sub").canonicalize().unwrap();
        assert_eq!(found, vec![sub.to_string_lossy().to_string(); 2]);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
