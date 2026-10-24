//! Where this install lives, putting an update in its place, and starting it again.

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::Error;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Bundle {
    /// The .app the binary runs from.
    App(PathBuf),
    /// The AppImage file.
    AppImage(PathBuf),
    /// /usr/bin/gitviber from the .deb or .rpm.
    Packaged,
}

impl Bundle {
    pub(crate) fn path(&self) -> Option<&Path> {
        match self {
            Bundle::App(p) | Bundle::AppImage(p) => Some(p),
            Bundle::Packaged => None,
        }
    }
}

pub(crate) fn bundle() -> Option<Bundle> {
    let exe = std::env::current_exe().ok()?;
    if cfg!(target_os = "macos") {
        return app_of(&exe).map(Bundle::App);
    }
    // APPIMAGE is inherited by whatever runs in an AppImage's terminal, a packaged GitViber too:
    // it's this one only when the binary runs from inside the image's mount.
    if let (Some(image), Some(dir)) = (std::env::var_os("APPIMAGE"), std::env::var_os("APPDIR"))
        && exe.starts_with(&dir)
    {
        return Some(Bundle::AppImage(image.into()));
    }
    (exe == Path::new("/usr/bin/gitviber")).then_some(Bundle::Packaged)
}

/// X.app/Contents/MacOS/gitviber → X.app.
pub(crate) fn app_of(exe: &Path) -> Option<PathBuf> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let app = contents.parent()?;
    (macos.file_name() == Some(OsStr::new("MacOS"))
        && contents.file_name() == Some(OsStr::new("Contents"))
        && app.extension() == Some(OsStr::new("app")))
    .then(|| app.to_path_buf())
}

/// The latest.json keys this install takes, in order (tauri-plugin-updater's
/// `{os}-{arch}-{bundle}`, then `{os}-{arch}`).
pub(crate) fn targets(bundle: Option<Bundle>) -> Vec<String> {
    let os = if cfg!(target_os = "macos") {
        "darwin"
    } else {
        "linux"
    };
    let arch = std::env::consts::ARCH;
    let kind = match bundle {
        Some(Bundle::App(_)) => Some("app"),
        Some(Bundle::AppImage(_)) => Some("appimage"),
        // A development build checks as the platform's own bundle would.
        None if cfg!(target_os = "macos") => Some("app"),
        _ => None,
    };
    let mut out: Vec<String> = kind
        .map(|k| format!("{os}-{arch}-{k}"))
        .into_iter()
        .collect();
    out.push(format!("{os}-{arch}"));
    out
}

/// Whether the folder holding `path` takes new files without an administrator.
pub(crate) fn writable(path: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let Some(dir) = path.parent() else {
        return false;
    };
    let Ok(dir) = std::ffi::CString::new(dir.as_os_str().as_bytes()) else {
        return false;
    };
    // SAFETY: a NUL-terminated path that lives through the call.
    unsafe { libc::access(dir.as_ptr(), libc::W_OK) == 0 }
}

/// Unpacks the signed .app.tar.gz beside `app` and swaps it in. The archive holds one folder,
/// the bundle, as Tauri's updater expects too (it drops the first path component).
pub(crate) fn install_app(archive: &Path, app: &Path) -> Result<(), Error> {
    let (parent, name) = split(app)?;
    let staging = parent.join(hidden(name, "update"));
    let _ = fs::remove_dir_all(&staging);
    fs::create_dir(&staging).map_err(|e| not_writable(e, parent))?;
    let result = (|| {
        let status = Command::new("tar")
            .arg("-xzf")
            .arg(archive)
            .arg("-C")
            .arg(&staging)
            .stdin(Stdio::null())
            .status()?;
        if !status.success() {
            return Err(Error::Io(std::io::Error::other(
                "couldn't unpack the update",
            )));
        }
        swap_in(&single_bundle(&staging)?, app)
    })();
    let _ = fs::remove_dir_all(&staging);
    result
}

/// The one folder in `dir`, when it's an app bundle.
pub(crate) fn single_bundle(dir: &Path) -> Result<PathBuf, Error> {
    let entries: Vec<PathBuf> = fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    match entries.as_slice() {
        [one] if one.join("Contents/Info.plist").is_file() => Ok(one.clone()),
        _ => Err(Error::Io(std::io::Error::other(
            "the update doesn't hold one app bundle",
        ))),
    }
}

/// Moves `new` to `current`'s place; the old one is restored if that fails, removed if not.
pub(crate) fn swap_in(new: &Path, current: &Path) -> Result<(), Error> {
    let (parent, name) = split(current)?;
    let old = parent.join(hidden(name, "old"));
    let _ = fs::remove_dir_all(&old);
    fs::rename(current, &old).map_err(|e| not_writable(e, parent))?;
    if let Err(err) = fs::rename(new, current) {
        let _ = fs::rename(&old, current);
        return Err(err.into());
    }
    let _ = fs::remove_dir_all(&old);
    // LaunchServices rereads a bundle whose date changed (its version, its icon).
    let _ = fs::File::open(current).and_then(|f| f.set_modified(std::time::SystemTime::now()));
    Ok(())
}

/// Copies the verified AppImage beside `image` and renames it over, keeping its mode. The
/// running one keeps its open file, so it carries on until it quits.
pub(crate) fn install_appimage(download: &Path, image: &Path) -> Result<(), Error> {
    let (parent, name) = split(image)?;
    let staging = parent.join(hidden(name, "update"));
    fs::copy(download, &staging).map_err(|e| not_writable(e, parent))?;
    let result = fs::metadata(image)
        .and_then(|m| fs::set_permissions(&staging, m.permissions()))
        .and_then(|()| fs::rename(&staging, image));
    if let Err(err) = result {
        let _ = fs::remove_file(&staging);
        return Err(err.into());
    }
    Ok(())
}

fn split(path: &Path) -> Result<(&Path, &OsStr), Error> {
    match (path.parent(), path.file_name()) {
        (Some(parent), Some(name)) => Ok((parent, name)),
        _ => Err(Error::NotInstalled),
    }
}

/// `.GitViber.app.update-<pid>`: out of sight in Finder, and never another process's.
fn hidden(name: &OsStr, what: &str) -> String {
    format!(".{}.{what}-{}", name.to_string_lossy(), std::process::id())
}

fn not_writable(err: std::io::Error, dir: &Path) -> Error {
    match err.kind() {
        std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::ReadOnlyFilesystem => {
            Error::NotWritable(dir.to_path_buf())
        }
        _ => Error::Io(err),
    }
}

/// Starts the installed update once this process has exited; the caller quits right after.
/// Through LaunchServices on macOS, so the new app gets Info.plist's LSEnvironment. Gives up
/// after a minute, so a quit that was cancelled doesn't reopen the app later.
pub fn relaunch() -> Result<(), Error> {
    const WAIT: &str = r#"pid=$1; shift; i=0
while kill -0 "$pid" 2>/dev/null; do
  i=$((i + 1)); [ "$i" -gt 300 ] && exit 0; sleep 0.2
done
exec "$@""#;
    let target = match bundle() {
        Some(Bundle::App(p) | Bundle::AppImage(p)) => p,
        _ => return Err(Error::NotInstalled),
    };
    let mut command = Command::new("/bin/sh");
    command
        .args(["-c", WAIT, "sh"])
        .arg(std::process::id().to_string());
    if cfg!(target_os = "macos") {
        command.arg("/usr/bin/open").arg(&target);
    } else {
        // The new image's runtime sets these for itself.
        command.arg(&target);
        for var in ["APPIMAGE", "APPDIR", "ARGV0", "OWD"] {
            command.env_remove(var);
        }
    }
    use std::os::unix::process::CommandExt as _;
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        // Its own group: what stops the app's group on the way out leaves it be.
        .process_group(0)
        .spawn()?;
    Ok(())
}
