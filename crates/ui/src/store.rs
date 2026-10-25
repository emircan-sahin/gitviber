//! Where the app keeps its files, and writing them without blocking a frame or losing what's
//! there: a file that can't be read is moved aside rather than overwritten, a save writes only
//! the keys this process changed into the file as it is on disk (another process's edits to the
//! rest stay), and one writer per file runs at a time, newest last.

use std::collections::{BTreeSet, HashMap};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Condvar, LazyLock, Mutex, OnceLock};

use gpui_kit::{App, AppContext as _};
use serde_json::{Map, Value};

/// As tauri.conf.json's identifier, so the native app's files sit where the old app's did.
pub const APP_ID: &str = "app.gitviber.desktop";

static CONFIG_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Makes `dir` the config dir for this process unless GITVIBER_CONFIG_DIR names one: for an
/// example binary, which shouldn't rewrite the app's own files. The first call wins.
pub fn use_config_dir(dir: PathBuf) {
    let _ = CONFIG_DIR.set(dir);
}

/// The app's config dir: ~/Library/Application Support/<id> on macOS, $XDG_CONFIG_HOME/<id> on
/// Linux, %APPDATA%\<id> on Windows. GITVIBER_CONFIG_DIR puts it elsewhere (tests, a second profile).
pub fn config_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("GITVIBER_CONFIG_DIR") {
        return dir.into();
    }
    if let Some(dir) = CONFIG_DIR.get() {
        return dir.clone();
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

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Replaces `path` with `text` through a temporary file of its own (two writers never share
/// one), synced before the rename, so a crash mid-write leaves the old file.
pub fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let n = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp = dir.join(format!(".{name}.{}.{n}.tmp", std::process::id()));
    let written = (|| {
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        std::fs::rename(&tmp, path)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written
}

/// `<name>.bad-<unix seconds>` beside `path`.
fn aside(path: &Path) -> Option<PathBuf> {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    let name = path.file_name()?.to_string_lossy().into_owned();
    Some(path.with_file_name(format!("{name}.bad-{secs}")))
}

/// Moves an unreadable file aside to `<name>.bad-<unix seconds>`, for the user to look at.
pub fn quarantine(path: &Path) -> Option<PathBuf> {
    let aside = aside(path)?;
    std::fs::rename(path, &aside).ok()?;
    eprintln!(
        "gitviber: {} couldn't be read; it's kept as {}",
        path.display(),
        aside.display()
    );
    Some(aside)
}

/// Copies a file that read only in part to `<name>.bad-<unix seconds>`, before it's saved
/// without what didn't read.
pub fn keep_copy(path: &Path) -> Option<PathBuf> {
    let aside = aside(path)?;
    std::fs::copy(path, &aside).ok()?;
    eprintln!(
        "gitviber: part of {} couldn't be read and is left out; the file as it was is kept as {}",
        path.display(),
        aside.display()
    );
    Some(aside)
}

/// A file's JSON object; None when it isn't there. One that can't be read or isn't a JSON
/// object is moved aside (`quarantine`) and read as none, so the next save can't overwrite it.
pub fn read_object(path: &Path) -> Option<Map<String, Value>> {
    let object = match std::fs::read_to_string(path) {
        Ok(text) => match serde_json::from_str::<Value>(&text) {
            Ok(Value::Object(map)) => Some(map),
            _ => {
                quarantine(path);
                None
            }
        },
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => {
            quarantine(path);
            None
        }
    };
    files()
        .0
        .lock()
        .unwrap()
        .entry(path.to_path_buf())
        .or_default()
        .last = object.clone().unwrap_or_default();
    object
}

/// What a save writes over the file as it is now: `ours`' keys this process `changed`, and the
/// ones the file lacks; the rest stays as the file has it.
pub fn merge(
    on_disk: Option<Map<String, Value>>,
    ours: &Map<String, Value>,
    changed: &BTreeSet<String>,
) -> Map<String, Value> {
    let mut out = on_disk.unwrap_or_default();
    for (key, value) in ours {
        if changed.contains(key) || !out.contains_key(key) {
            out.insert(key.clone(), value.clone());
        }
    }
    out
}

/// What a file's writer has to write next.
enum Pending {
    /// The newest object, and every key changed since the last write, merged into the file.
    Merge(Map<String, Value>, BTreeSet<String>),
    /// The whole text, as it is.
    Text(String),
}

#[derive(Default)]
struct FileState {
    /// What this process last read or saved, to tell which keys a save changed.
    last: Map<String, Value>,
    waiting: Option<Pending>,
    writing: bool,
    /// The writer took `waiting` and is writing it now. Only this is waited for on a flush: a
    /// writer still queued may never run (a headless executor), so its work is done inline.
    in_flight: bool,
}

type Files = (Mutex<HashMap<PathBuf, FileState>>, Condvar);

fn files() -> &'static Files {
    static FILES: LazyLock<Files> = LazyLock::new(|| (Mutex::new(HashMap::new()), Condvar::new()));
    &FILES
}

fn write_pending(path: &Path, pending: Pending) {
    match pending {
        Pending::Merge(ours, changed) => write_merged(path, &ours, &changed),
        Pending::Text(text) => {
            if let Err(err) = write_atomic(path, &text) {
                eprintln!("gitviber: couldn't write {}: {err}", path.display());
            }
        }
    }
}

fn write_merged(path: &Path, ours: &Map<String, Value>, changed: &BTreeSet<String>) {
    let on_disk =
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| match serde_json::from_str(&text) {
                Ok(Value::Object(map)) => Some(map),
                _ => None,
            });
    let merged = merge(on_disk, ours, changed);
    let text = serde_json::to_string_pretty(&Value::Object(merged)).unwrap_or_default();
    if let Err(err) = write_atomic(path, &text) {
        eprintln!("gitviber: couldn't write {}: {err}", path.display());
    }
}

/// Saves `ours` off the main thread: only the keys that differ from what this process last read
/// or saved are written into the file. Saves made while one is being written wait and go as one.
pub fn save_object(cx: &mut App, path: PathBuf, ours: Map<String, Value>) {
    let (lock, _) = files();
    {
        let mut map = lock.lock().unwrap();
        let file = map.entry(path.clone()).or_default();
        let changed: BTreeSet<String> = ours
            .iter()
            .filter(|(k, v)| file.last.get(*k) != Some(*v))
            .map(|(k, _)| k.clone())
            .collect();
        if changed.is_empty() {
            return;
        }
        file.last = ours.clone();
        let keys = match file.waiting.take() {
            Some(Pending::Merge(_, mut keys)) => {
                keys.extend(changed);
                keys
            }
            _ => changed,
        };
        file.waiting = Some(Pending::Merge(ours, keys));
        if file.writing {
            return;
        }
        file.writing = true;
    }
    spawn_writer(cx, path);
}

/// Writes `text` over the file off the main thread, one writer per file, newest text last: a
/// slider's drag of saves can't land out of order. A failure is reported and the app goes on.
pub fn write_in_background(cx: &mut App, path: PathBuf, text: String) {
    {
        let mut map = files().0.lock().unwrap();
        let file = map.entry(path.clone()).or_default();
        file.waiting = Some(Pending::Text(text));
        if file.writing {
            return;
        }
        file.writing = true;
    }
    spawn_writer(cx, path);
}

fn spawn_writer(cx: &mut App, path: PathBuf) {
    let (lock, done) = files();
    cx.background_spawn(async move {
        loop {
            let next = {
                let mut map = lock.lock().unwrap();
                let file = map.entry(path.clone()).or_default();
                let next = file.waiting.take();
                file.in_flight = next.is_some();
                if next.is_none() {
                    file.writing = false;
                    done.notify_all();
                }
                next
            };
            let Some(pending) = next else { break };
            write_pending(&path, pending);
            if let Some(file) = lock.lock().unwrap().get_mut(&path) {
                file.in_flight = false;
            }
            done.notify_all();
        }
    })
    .detach();
}

/// Writes what is still waiting, on this thread, after the write in flight: for quitting, when
/// background work stops.
pub fn flush(_: &mut App) {
    let (lock, done) = files();
    let mut map = lock.lock().unwrap();
    while map.values().any(|f| f.in_flight) {
        map = done.wait(map).unwrap();
    }
    let waiting: Vec<(PathBuf, Pending)> = map
        .iter_mut()
        .filter_map(|(path, f)| f.waiting.take().map(|p| (path.clone(), p)))
        .collect();
    drop(map);
    for (path, pending) in waiting {
        write_pending(&path, pending);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        crate::test_support::temp_dir(&format!("store-{name}"))
    }

    #[test]
    fn an_unreadable_file_is_kept_aside() {
        let dir = temp("bad");
        let path = dir.join("settings.json");
        std::fs::write(&path, "{\"uiScale\": 1.2,").unwrap();
        assert!(read_object(&path).is_none());
        assert!(!path.exists());
        let aside: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("settings.json.bad-")
            })
            .collect();
        assert_eq!(aside.len(), 1);
        assert_eq!(
            std::fs::read_to_string(aside[0].path()).unwrap(),
            "{\"uiScale\": 1.2,"
        );
        // A missing file is no reason to move anything.
        assert!(read_object(&dir.join("none.json")).is_none());
    }

    #[test]
    fn a_save_keeps_what_another_process_wrote() {
        let disk = serde_json::json!({ "a": 1, "b": 2, "theirs": true })
            .as_object()
            .cloned();
        let ours = serde_json::json!({ "a": 10, "b": 2, "c": 3 })
            .as_object()
            .cloned()
            .unwrap();
        let changed: BTreeSet<String> = ["a".to_string()].into();
        let merged = merge(disk, &ours, &changed);
        assert_eq!(
            Value::Object(merged),
            serde_json::json!({ "a": 10, "b": 2, "c": 3, "theirs": true })
        );
    }

    /// A headless executor runs a queued writer only when it's driven: a flush mustn't wait on it.
    #[gpui_kit::test]
    fn a_flush_writes_what_a_writer_never_started(cx: &mut gpui_kit::TestAppContext) {
        let path = temp("flush").join("state.json");
        let ours = serde_json::json!({ "a": 1 }).as_object().cloned().unwrap();
        cx.update(|cx| {
            save_object(cx, path.clone(), ours);
            flush(cx);
        });
        let back: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(back, serde_json::json!({ "a": 1 }));
        // The queued writer finds nothing left, and a later save still goes out.
        cx.run_until_parked();
        let ours = serde_json::json!({ "a": 2 }).as_object().cloned().unwrap();
        cx.update(|cx| save_object(cx, path.clone(), ours));
        cx.run_until_parked();
        let back: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(back, serde_json::json!({ "a": 2 }));
    }

    #[test]
    fn writers_racing_on_one_file_leave_valid_json() {
        let dir = temp("race");
        let path = dir.join("state.json");
        let threads: Vec<_> = (0..8)
            .map(|i| {
                let path = path.clone();
                std::thread::spawn(move || {
                    for j in 0..50 {
                        let text = serde_json::to_string(
                            &serde_json::json!({ "writer": i, "n": j, "pad": "x".repeat(4096) }),
                        )
                        .unwrap();
                        write_atomic(&path, &text).unwrap();
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(serde_json::from_str::<Value>(&text).is_ok());
        let left: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(left.is_empty());
    }
}
