//! `tauri dev` runs the bare binary, which macOS won't give notifications: UNUserNotificationCenter
//! needs an app bundle. So a debug build copies itself into `target/debug/GitViber Dev.app`, signs
//! it ad hoc under its own bundle id and runs that copy in its place, with the same arguments and
//! the same pid, which `tauri dev` keeps watching. Taken from MonoCode (macos.rs).
//!
//! Its own id, not the shipped app's: the installed GitViber keeps its notification settings and
//! its place in LaunchServices. If anything fails, the bare binary goes on as before, and the
//! Notifications page says why there's no permission to ask for.

use std::ffi::OsStr;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

pub const ID: &str = "app.gitviber.desktop.dev";
const NAME: &str = "GitViber Dev";
/// Another name for the bundle (and its Dock icon), as MonoCode's MONOCODE_DEV_APP_NAME.
const NAME_ENV: &str = "GITVIBER_DEV_APP_NAME";
const EXECUTABLE: &str = "gitviber";
const ICNS: &[u8] = include_bytes!("../icons/icon.icns");
const ASSETS_CAR: &[u8] = include_bytes!("../icons/Assets.car");

/// Runs the bundled copy instead, unless this is it; returns only when that can't be done.
/// First thing in main, before any thread: its children, which get no pipes, need no
/// process::spawning.
pub fn relaunch() {
    if let Err(e) = try_relaunch() {
        eprintln!("gitviber: dev bundle: {e}; going on without one, so without notifications");
    }
}

fn try_relaunch() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    if bundle_of(&exe).is_some() {
        return Ok(());
    }
    let name = std::env::var(NAME_ENV)
        .ok()
        .and_then(|n| sanitized_name(&n))
        .unwrap_or_else(|| NAME.into());
    let app = exe.parent().ok_or("no folder")?.join(format!("{name}.app"));
    let bundled = app.join("Contents/MacOS").join(EXECUTABLE);
    let stamp = app.join("Contents/.source");
    let source = stamp_of(&exe)?;
    // The copy and its signature are made again only for a new build: signing reads all 70 MB.
    if std::fs::read_to_string(&stamp).ok().as_deref() != Some(&source) || !bundled.is_file() {
        build(&app, &name, &exe, &bundled)?;
        sign(&app)?;
        std::fs::write(&stamp, &source).map_err(|e| e.to_string())?;
    }
    keep_webkit_data();
    Err(Command::new(&bundled)
        .args(std::env::args_os().skip(1))
        .exec()
        .to_string())
}

fn build(app: &Path, name: &str, exe: &Path, bundled: &Path) -> Result<(), String> {
    let err = |e: std::io::Error| e.to_string();
    let resources = app.join("Contents/Resources");
    std::fs::create_dir_all(bundled.parent().ok_or("no folder")?).map_err(err)?;
    std::fs::create_dir_all(&resources).map_err(err)?;
    std::fs::write(app.join("Contents/Info.plist"), plist(name)).map_err(err)?;
    std::fs::write(resources.join("AppIcon.icns"), ICNS).map_err(err)?;
    std::fs::write(resources.join("Assets.car"), ASSETS_CAR).map_err(err)?;
    // A copy, not a link: signing rewrites it, and the original is what runs this code.
    let _ = std::fs::remove_file(bundled);
    std::fs::copy(exe, bundled).map_err(err)?;
    std::fs::set_permissions(bundled, std::fs::Permissions::from_mode(0o755)).map_err(err)?;
    // A newer modification time makes LaunchServices read the plist and icon again.
    if let Ok(dir) = std::fs::File::open(app) {
        let _ = dir.set_modified(std::time::SystemTime::now());
    }
    Ok(())
}

/// The linker's ad-hoc signature names the binary `gitviber-<hash>`, and UNUserNotificationCenter
/// refuses, without asking, an app whose signing identifier isn't its bundle id.
fn sign(app: &Path) -> Result<(), String> {
    // No pipes: codesign's own complaint goes to the `tauri dev` terminal.
    let status = Command::new("/usr/bin/codesign")
        .args(["--force", "--sign", "-", "--identifier", ID])
        .arg(app)
        .status()
        .map_err(|e| format!("codesign: {e}"))?;
    status
        .success()
        .then_some(())
        .ok_or_else(|| format!("codesign failed ({status})"))
}

/// The build's size and modification time, which a new build changes.
fn stamp_of(exe: &Path) -> Result<String, String> {
    let meta = std::fs::metadata(exe).map_err(|e| e.to_string())?;
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos());
    Ok(format!("{}:{modified}", meta.len()))
}

/// WebKit keeps a page's storage under its app's bundle id, and the bare binary's under its name:
/// the dev settings, sessions and recent repos are in `~/Library/WebKit/gitviber`. The bundle
/// starts from a copy of them (a clone on APFS), once.
fn keep_webkit_data() {
    let Some(home) = std::env::var_os("HOME") else {
        return;
    };
    let webkit = Path::new(&home).join("Library/WebKit");
    let (from, to) = (webkit.join(EXECUTABLE), webkit.join(ID));
    if from.is_dir() && !to.exists() {
        let copied = Command::new("/bin/cp")
            .arg("-cR")
            .arg(&from)
            .arg(&to)
            .status();
        if !copied.is_ok_and(|s| s.success()) {
            eprintln!("gitviber: dev bundle: could not copy {}", from.display());
        }
    }
}

/// `…/Name.app`, when `exe` is that bundle's executable.
fn bundle_of(exe: &Path) -> Option<PathBuf> {
    let app = exe.parent()?.parent()?.parent()?;
    (app.extension() == Some(OsStr::new("app")) && app.join("Contents/Info.plist").is_file())
        .then(|| app.to_path_buf())
}

/// A name for the bundle's folder: one path component, nothing that leaves `target/debug`.
fn sanitized_name(value: &str) -> Option<String> {
    let value = value.trim();
    let mut parts = Path::new(value).components();
    match (parts.next(), parts.next()) {
        (Some(Component::Normal(part)), None) if part == OsStr::new(value) => Some(value.into()),
        _ => None,
    }
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn plist(name: &str) -> String {
    let name = escape(name);
    let version = env!("CARGO_PKG_VERSION");
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleDevelopmentRegion</key>
	<string>en</string>
	<key>CFBundleDisplayName</key>
	<string>{name}</string>
	<key>CFBundleExecutable</key>
	<string>{EXECUTABLE}</string>
	<key>CFBundleIconFile</key>
	<string>AppIcon</string>
	<key>CFBundleIconName</key>
	<string>AppIcon</string>
	<key>CFBundleIdentifier</key>
	<string>{ID}</string>
	<key>CFBundleInfoDictionaryVersion</key>
	<string>6.0</string>
	<key>CFBundleName</key>
	<string>{name}</string>
	<key>CFBundlePackageType</key>
	<string>APPL</string>
	<key>CFBundleShortVersionString</key>
	<string>{version}</string>
	<key>CFBundleVersion</key>
	<string>{version}</string>
	<key>LSMinimumSystemVersion</key>
	<string>13.0</string>
	<key>NSHighResolutionCapable</key>
	<true/>
</dict>
</plist>
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_is_one_plain_component() {
        assert_eq!(
            sanitized_name("  GitViber Dev 2 "),
            Some("GitViber Dev 2".into())
        );
        for bad in ["", "  ", ".", "..", "../Other", "/tmp/Other", "Foo/Bar"] {
            assert_eq!(sanitized_name(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn the_plist_names_the_dev_app_and_its_own_id() {
        let text = plist("Dev <&> Build");
        assert!(text.contains("<string>Dev &lt;&amp;&gt; Build</string>"));
        assert!(text.contains("<string>app.gitviber.desktop.dev</string>"));
        assert!(!text.contains("<string>app.gitviber.desktop</string>"));
        assert!(text.contains("<key>CFBundleExecutable</key>\n\t<string>gitviber</string>"));
    }

    /// Signed under the dev id, which UNUserNotificationCenter checks against the bundle's.
    #[test]
    fn builds_and_signs_a_bundle() {
        let root = std::env::temp_dir().join(format!("gitviber-devsign-{}", std::process::id()));
        let app = root.join("Test.app");
        let bundled = app.join("Contents/MacOS").join(EXECUTABLE);
        build(&app, "Test", Path::new("/usr/bin/true"), &bundled).unwrap();
        sign(&app).unwrap();
        assert_eq!(bundle_of(&bundled), Some(app.clone()));
        let shown = crate::process::spawning(|| {
            Command::new("/usr/bin/codesign")
                .arg("-dv")
                .arg(&app)
                .output()
        })
        .unwrap();
        // codesign -d prints to stderr.
        let shown = String::from_utf8_lossy(&shown.stderr);
        assert!(
            shown.contains("Identifier=app.gitviber.desktop.dev"),
            "{shown}"
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn knows_its_own_bundle() {
        let root = std::env::temp_dir().join(format!("gitviber-devbundle-{}", std::process::id()));
        let exe = root.join("GitViber Dev.app/Contents/MacOS/gitviber");
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        assert_eq!(bundle_of(&exe), None, "no Info.plist yet");
        std::fs::write(root.join("GitViber Dev.app/Contents/Info.plist"), "x").unwrap();
        assert_eq!(bundle_of(&exe), Some(root.join("GitViber Dev.app")));
        assert_eq!(bundle_of(&root.join("gitviber")), None);
        std::fs::remove_dir_all(&root).unwrap();
    }
}
