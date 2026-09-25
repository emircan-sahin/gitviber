//! Whether this build updates itself (tauri-plugin-updater, driven by src/lib/app/updates.ts).
//! Release builds do; a debug build only with GITVIBER_UPDATER=1, to try the check against a
//! local latest.json (endpoints overridden with `--config`).

/// The updater plugin is registered only then: it fails to start without its config.
pub fn enabled(config: &tauri::Config) -> bool {
    config.plugins.0.contains_key("updater")
        && (!cfg!(debug_assertions) || std::env::var_os("GITVIBER_UPDATER").is_some())
}

/// "install" in place (the .app, an AppImage), "download" from the Releases page (a .deb or .rpm
/// belongs to the package manager, and installing one asks for a password), None when off.
pub fn mode(config: &tauri::Config) -> Option<&'static str> {
    if !enabled(config) {
        None
    } else if cfg!(target_os = "linux") && std::env::var_os("APPIMAGE").is_none() {
        Some("download")
    } else {
        Some("install")
    }
}
