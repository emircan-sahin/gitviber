//! The app's own folders, where the Tauri build kept them, so an update finds its logs and
//! cache where the last version left them.

use std::path::PathBuf;

/// The bundle identifier, which names the folders.
pub const IDENTIFIER: &str = "app.gitviber.desktop";

#[cfg(unix)]
fn home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
}

/// An XDG folder, or its default under home.
#[cfg(all(unix, not(target_os = "macos")))]
fn xdg(var: &str, default: &str) -> Option<PathBuf> {
    std::env::var_os(var)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| home().map(|h| h.join(default)))
}

#[cfg(windows)]
fn local_app_data() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
}

/// ~/Library/Caches/app.gitviber.desktop on macOS: the shell integration's scripts.
pub fn cache_dir() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    let base = home().map(|h| h.join("Library/Caches"));
    #[cfg(all(unix, not(target_os = "macos")))]
    let base = xdg("XDG_CACHE_HOME", ".cache");
    #[cfg(windows)]
    let base = local_app_data();
    Some(base?.join(IDENTIFIER))
}

/// ~/Library/Logs/app.gitviber.desktop on macOS: the error log (errors.rs).
pub fn log_dir() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    return Some(home()?.join("Library/Logs").join(IDENTIFIER));
    #[cfg(all(unix, not(target_os = "macos")))]
    return Some(
        xdg("XDG_DATA_HOME", ".local/share")?
            .join(IDENTIFIER)
            .join("logs"),
    );
    #[cfg(windows)]
    return Some(local_app_data()?.join(IDENTIFIER).join("logs"));
}
