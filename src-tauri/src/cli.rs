//! `gitviber <path>` in a terminal. macOS links the bundled script (resources/gitviber) into a
//! Homebrew bin folder, never asking for a password (Homebrew's cask links it too); .deb and
//! .rpm already put the binary on PATH, and an AppImage gets a small launcher in ~/.local/bin.

/// Where Homebrew puts commands (Apple silicon, then Intel), in order. Both are on a Homebrew
/// user's PATH and theirs to write; nothing asks for an administrator's password.
#[cfg(target_os = "macos")]
const BIN_DIRS: [&str; 2] = ["/opt/homebrew/bin", "/usr/local/bin"];

/// Installs the command; returns where it is.
#[cfg(target_os = "macos")]
pub fn install() -> Result<String, String> {
    use std::path::Path;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    // Contents/MacOS/gitviber → Contents/Resources/bin/gitviber (tauri.conf.json's macOS files).
    let script = exe
        .parent()
        .and_then(Path::parent)
        .map(|contents| contents.join("Resources/bin/gitviber"))
        .filter(|p| p.is_file())
        .ok_or("The command comes with the installed app, not a development build")?;
    // Run from Downloads (macOS moves it to a random read-only place) or from the disk image, the
    // link would point somewhere gone by tomorrow.
    let place = script.to_string_lossy();
    if place.contains("/AppTranslocation/") || place.starts_with("/Volumes/") {
        return Err("Move GitViber to Applications first, then install the command".into());
    }
    for dir in BIN_DIRS {
        let link = Path::new(dir).join("gitviber");
        if std::fs::read_link(&link).is_ok_and(|to| to == script) {
            return Ok(link.to_string_lossy().into_owned());
        }
        // Something of that name that isn't a link is another program's.
        if link
            .symlink_metadata()
            .is_ok_and(|m| !m.file_type().is_symlink())
        {
            continue;
        }
        if !Path::new(dir).is_dir() {
            continue;
        }
        let _ = std::fs::remove_file(&link);
        if std::os::unix::fs::symlink(&script, &link).is_ok() {
            return Ok(link.to_string_lossy().into_owned());
        }
    }
    let folder = script.parent().unwrap_or(&script).to_string_lossy();
    Err(format!(
        "No folder on your PATH takes it without a password. Add GitViber's to your shell \
         instead, e.g. in ~/.zprofile: export PATH=\"$PATH:{folder}\""
    ))
}

#[cfg(target_os = "linux")]
pub fn install() -> Result<String, String> {
    use std::os::unix::fs::PermissionsExt;
    let Some(image) = std::env::var_os("APPIMAGE") else {
        let packaged = "/usr/bin/gitviber";
        return std::path::Path::new(packaged)
            .is_file()
            .then(|| packaged.to_string())
            .ok_or_else(|| "gitviber isn't on PATH: install the .deb or .rpm package".into());
    };
    let home = std::env::var_os("HOME").ok_or("HOME is not set")?;
    let bin = std::path::Path::new(&home).join(".local/bin");
    std::fs::create_dir_all(&bin).map_err(|e| e.to_string())?;
    let launcher = bin.join("gitviber");
    // Only our own launcher is replaced, as macOS leaves another program's link alone.
    const MARK: &str = "# GitViber's launcher";
    if std::fs::read_to_string(&launcher).is_ok_and(|s| !s.contains(MARK))
        || launcher
            .symlink_metadata()
            .is_ok_and(|m| m.file_type().is_symlink())
    {
        return Err(format!(
            "{} is another program's; remove it first",
            launcher.display()
        ));
    }
    let image = image.to_string_lossy().replace('\'', r"'\''");
    std::fs::write(
        &launcher,
        format!("#!/bin/sh\n{MARK}\nexec '{image}' \"$@\"\n"),
    )
    .and_then(|()| std::fs::set_permissions(&launcher, std::fs::Permissions::from_mode(0o755)))
    .map_err(|e| e.to_string())?;
    Ok(launcher.to_string_lossy().into_owned())
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn install() -> Result<String, String> {
    Err("Installing the command isn't supported on this platform yet".into())
}
