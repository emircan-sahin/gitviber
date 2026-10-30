//! For the tests of every GitViber crate (with `install_core`). nextest runs each test in a
//! process of its own, so a folder named by the pid is the test's alone while it runs.

use std::path::PathBuf;
use std::sync::Mutex;

use gpui_kit::App;

use crate::Settings;

/// The folders `temp_dir` made in this process, removed when it exits.
static MADE: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// `$TMPDIR/gitviber-<name>-<pid>`, emptied the first time this process asks for it and removed
/// when the process exits, a failed test's too: pids come round again, and a later test given
/// this one's would read what it left.
pub fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("gitviber-{name}-{}", std::process::id()));
    let mut made = MADE.lock().unwrap_or_else(|e| e.into_inner());
    if !made.contains(&dir) {
        let _ = std::fs::remove_dir_all(&dir);
        if made.is_empty() {
            // SAFETY: registers a plain function; it runs once, on the process's exit.
            unsafe { atexit(remove_made) };
        }
        made.push(dir.clone());
    }
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// `temp_dir(name)`, made the config folder (`GITVIBER_CONFIG_DIR`) for the rest of the test.
pub fn config_dir(name: &str) -> PathBuf {
    let dir = temp_dir(name);
    // SAFETY: each test runs in its own process (nextest), and sets this before it starts threads.
    unsafe { std::env::set_var("GITVIBER_CONFIG_DIR", &dir) };
    dir
}

/// The default settings and the theme built from them: globals the app always has, which shared
/// widgets read as they render (a tooltip's chord, the theme's colors). Without touching the
/// settings file; call after gpui-kit's init.
pub fn install_theme(cx: &mut App) {
    cx.set_global(Settings::default());
    crate::theme::refresh(cx);
}

extern "C" fn remove_made() {
    for dir in MADE.lock().unwrap_or_else(|e| e.into_inner()).iter() {
        let _ = std::fs::remove_dir_all(dir);
    }
}

unsafe extern "C" {
    fn atexit(f: extern "C" fn()) -> std::ffi::c_int;
}
