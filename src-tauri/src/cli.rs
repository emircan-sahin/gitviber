//! `gitviber <path>` in a terminal. macOS links the bundled script (resources/gitviber) into a
//! Homebrew bin folder, never asking for a password (Homebrew's cask links it too); .deb and
//! .rpm already put the binary on PATH, and an AppImage gets a small launcher in ~/.local/bin.

use std::path::{Path, PathBuf};

/// Where Homebrew puts commands (Apple silicon, then Intel), in order. Both are on a Homebrew
/// user's PATH and theirs to write; nothing asks for an administrator's password.
#[cfg(target_os = "macos")]
const BIN_DIRS: [&str; 2] = ["/opt/homebrew/bin", "/usr/local/bin"];

/// Installs the command; returns where it is.
#[cfg(target_os = "macos")]
pub fn install() -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let script =
        script(&exe).ok_or("The command comes with the installed app, not a development build")?;
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

/// The folder of the bundled script, which the terminal's shells get on PATH (pty.rs); None in
/// a development build, which has none. Never Contents/MacOS: its `gitviber` is the app itself.
pub fn bin_dir() -> Option<PathBuf> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    command_dir(&std::env::current_exe().ok()?)
}

/// None under a folder with a `:` in its name: PATH would split it in two.
fn command_dir(exe: &Path) -> Option<PathBuf> {
    let dir = script(exe)?.parent()?.to_path_buf();
    (!dir.to_string_lossy().contains(':')).then_some(dir)
}

/// Contents/MacOS/gitviber → Contents/Resources/bin/gitviber (tauri.conf.json's macOS files).
fn script(exe: &Path) -> Option<PathBuf> {
    exe.parent()
        .and_then(Path::parent)
        .map(|contents| contents.join("Resources/bin/gitviber"))
        .filter(|p| p.is_file())
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

#[cfg(test)]
mod tests {
    use super::{command_dir, script};

    #[test]
    fn the_script_is_found_beside_the_bundles_executable_only() {
        let dir = std::env::temp_dir().join(format!("gitviber-cli-{}", std::process::id()));
        let exe = dir.join("GitViber.app/Contents/MacOS/gitviber");
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        std::fs::write(&exe, "").unwrap();
        // Nothing bundled, as in a development build.
        assert_eq!(script(&exe), None);
        let bundled = dir.join("GitViber.app/Contents/Resources/bin/gitviber");
        std::fs::create_dir_all(bundled.parent().unwrap()).unwrap();
        std::fs::write(&bundled, "").unwrap();
        assert_eq!(script(&exe), Some(bundled));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_colon_in_the_apps_folder_keeps_it_off_path() {
        let dir = std::env::temp_dir().join(format!("gitviber-cli-colon-{}", std::process::id()));
        for (folder, on_path) in [("Apps", true), ("a:b", false)] {
            let contents = dir.join(folder).join("GitViber.app/Contents");
            std::fs::create_dir_all(contents.join("MacOS")).unwrap();
            std::fs::create_dir_all(contents.join("Resources/bin")).unwrap();
            std::fs::write(contents.join("Resources/bin/gitviber"), "").unwrap();
            let found = command_dir(&contents.join("MacOS/gitviber"));
            assert_eq!(
                found,
                on_path.then(|| contents.join("Resources/bin")),
                "{folder}"
            );
        }
        let _ = std::fs::remove_dir_all(dir);
    }
}
