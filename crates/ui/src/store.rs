//! Where the app keeps its files, and writing them without blocking a frame.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use gpui_kit::{App, AppContext as _, Global};

/// As tauri.conf.json's identifier, so the native app's files sit where the old app's did.
pub const APP_ID: &str = "app.gitviber.desktop";

/// The app's config dir: ~/Library/Application Support/<id> on macOS, $XDG_CONFIG_HOME/<id> on
/// Linux, %APPDATA%\<id> on Windows. GITVIBER_CONFIG_DIR puts it elsewhere (tests, a second profile).
pub fn config_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("GITVIBER_CONFIG_DIR") {
        return dir.into();
    }
    let home = || {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default()
    };
    let base = if cfg!(target_os = "macos") {
        home().join("Library/Application Support")
    } else if cfg!(windows) {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(home)
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .unwrap_or_else(|| home().join(".config"))
    };
    base.join(APP_ID)
}

/// Replaces `path` with `text` through a temporary file, so a crash mid-write leaves the old one.
pub fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

/// The newest text waiting for a file, and whether a writer is on it.
type Waiting = (Option<String>, bool);

#[derive(Default)]
struct Pending(Arc<Mutex<HashMap<PathBuf, Waiting>>>);

impl Global for Pending {}

/// Writes off the main thread, one writer per file, newest text last: a slider's drag of saves
/// can't land out of order. A failure is reported and the app goes on with what it has.
pub fn write_in_background(cx: &mut App, path: PathBuf, text: String) {
    let pending = cx.default_global::<Pending>().0.clone();
    {
        let mut map = pending.lock().unwrap();
        let entry = map.entry(path.clone()).or_default();
        entry.0 = Some(text);
        if entry.1 {
            return;
        }
        entry.1 = true;
    }
    cx.background_spawn(async move {
        loop {
            let next = {
                let mut map = pending.lock().unwrap();
                let entry = map.entry(path.clone()).or_default();
                let next = entry.0.take();
                entry.1 = next.is_some();
                next
            };
            let Some(text) = next else { break };
            if let Err(err) = write_atomic(&path, &text) {
                eprintln!("gitviber: couldn't write {}: {err}", path.display());
            }
        }
    })
    .detach();
}

/// Writes what is still waiting, on this thread: for quitting, when background work stops.
pub fn flush(cx: &mut App) {
    let Some(pending) = cx.try_global::<Pending>().map(|p| p.0.clone()) else {
        return;
    };
    let waiting: Vec<(PathBuf, String)> = pending
        .lock()
        .unwrap()
        .iter_mut()
        .filter_map(|(path, entry)| Some((path.clone(), entry.0.take()?)))
        .collect();
    for (path, text) in waiting {
        let _ = write_atomic(&path, &text);
    }
}
