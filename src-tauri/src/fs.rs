//! Worktree access for the file explorer. Every path from the frontend is resolved
//! against the open repo and rejected if it would escape it.

use crate::git::{self, FileText};
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, OnceLock};

pub fn resolve(root: &Path, rel: &str) -> Result<PathBuf, String> {
    let rel_path = Path::new(rel);
    let escape = || format!("path outside repository: {rel}");
    if rel_path
        .components()
        .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
    {
        return Err(escape());
    }
    // Writing .git/config or a hook is code execution on the next git call. APFS ignores
    // case, so ".GIT" is the same directory.
    if rel_path
        .components()
        .any(|c| c.as_os_str().eq_ignore_ascii_case(".git"))
    {
        return Err(format!("refusing to touch git internals: {rel}"));
    }
    let full = root.join(rel_path);
    let real_root = root.canonicalize().map_err(|e| e.to_string())?;
    // Symlinks can point anywhere, and a path that doesn't exist yet can't be canonicalized:
    // check the deepest existing ancestor, and refuse dangling links (writing would follow them).
    let mut probe = full.as_path();
    let real = loop {
        match probe.canonicalize() {
            Ok(p) => break p,
            Err(_) if probe.symlink_metadata().is_ok() => return Err(escape()),
            Err(_) => probe = probe.parent().ok_or_else(escape)?,
        }
    };
    if !real.starts_with(&real_root) {
        return Err(escape());
    }
    // The name check above misses a link like `docs -> .git`, which survives a clone.
    if in_git_dir(root, &real_root, &real) {
        return Err(format!("refusing to touch git internals: {rel}"));
    }
    Ok(full)
}

fn in_git_dir(root: &Path, real_root: &Path, real: &Path) -> bool {
    // A worktree nested inside its own git dir must still be usable.
    git_dirs(root)
        .iter()
        .any(|d| real.starts_with(d) && !real_root.starts_with(d))
}

/// Canonical git dir and common dir (they differ in linked worktrees). Cached: resolve() runs
/// per file and they never change for a repo. Outside a repo (unit tests) there is none.
fn git_dirs(root: &Path) -> Vec<PathBuf> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, Vec<PathBuf>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(dirs) = cache.lock().unwrap().get(root) {
        return dirs.clone();
    }
    let Ok(out) = git::run(
        root,
        &["rev-parse", "--absolute-git-dir", "--git-common-dir"],
    ) else {
        return vec![];
    };
    let dirs: Vec<PathBuf> = String::from_utf8_lossy(&out)
        .lines()
        .filter_map(|l| root.join(l).canonicalize().ok())
        .collect();
    cache
        .lock()
        .unwrap()
        .insert(root.to_path_buf(), dirs.clone());
    dirs
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub ignored: bool,
}

pub fn list_dir(root: &Path, rel: &str) -> Result<Vec<Entry>, String> {
    let dir = resolve(root, rel)?;
    let real_root = root.canonicalize().map_err(|e| e.to_string())?;
    let mut entries: Vec<Entry> = std::fs::read_dir(&dir)
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .filter(|e| e.file_name() != ".git")
        .map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let path = if rel.is_empty() {
                name.clone()
            } else {
                format!("{rel}/{name}")
            };
            // Follows symlinks, so a linked folder expands like a folder, unless it leads
            // into the git dir (resolve() would refuse to list it anyway).
            let is_dir = e.path().is_dir()
                && !(e.file_type().is_ok_and(|t| t.is_symlink())
                    && e.path()
                        .canonicalize()
                        .is_ok_and(|p| in_git_dir(root, &real_root, &p)));
            Entry {
                name,
                path,
                is_dir,
                ignored: false,
            }
        })
        .collect();

    // Trailing slash lets directory-only patterns like `node_modules/` match.
    let probe: Vec<String> = entries
        .iter()
        .map(|e| {
            if e.is_dir {
                format!("{}/", e.path)
            } else {
                e.path.clone()
            }
        })
        .collect();
    let ignored: std::collections::HashSet<String> =
        git::ignored(root, &probe).into_iter().collect();
    for e in &mut entries {
        e.ignored = ignored.contains(&e.path);
    }

    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(entries)
}

/// Writes a file inside the repo (used to save a resolved conflict).
pub fn write_file(root: &Path, rel: &str, content: &str) -> Result<(), String> {
    std::fs::write(resolve(root, rel)?, content).map_err(|e| e.to_string())
}

pub fn read_file(root: &Path, rel: &str) -> FileText {
    match resolve(root, rel).and_then(|p| git::read_regular(&p)) {
        Ok(Some(bytes)) => git::to_file_text(bytes),
        Ok(None) => FileText {
            too_large: true,
            exists: true,
            ..Default::default()
        },
        Err(_) => FileText::default(),
    }
}

pub fn read_media(root: &Path, rel: &str) -> Result<Vec<u8>, String> {
    let path = resolve(root, rel)?;
    let meta = std::fs::metadata(&path).map_err(|e| e.to_string())?;
    if !meta.is_file() {
        return Err("not a regular file".into());
    }
    if meta.len() > git::MAX_MEDIA_BYTES {
        return Err("File is too large to preview".into());
    }
    std::fs::read(path).map_err(|e| e.to_string())
}

/// For operations on an entry itself (create, rename, trash): only the parent is resolved,
/// so a symlink is renamed or trashed as a link instead of being followed.
fn resolve_entry(root: &Path, rel: &str) -> Result<PathBuf, String> {
    let rel_path = Path::new(rel);
    let name = match rel_path.components().next_back() {
        Some(Component::Normal(n)) if !n.eq_ignore_ascii_case(".git") => n,
        _ => return Err(format!("not a file or folder in the repository: {rel}")),
    };
    let parent = rel_path.parent().unwrap_or(Path::new(""));
    Ok(resolve(root, &parent.to_string_lossy())?.join(name))
}

fn io_error(rel: &str, e: std::io::Error) -> String {
    if e.kind() == std::io::ErrorKind::AlreadyExists {
        format!("{rel} already exists")
    } else {
        e.to_string()
    }
}

pub fn create_file(root: &Path, rel: &str) -> Result<(), String> {
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(resolve_entry(root, rel)?)
        .map(|_| ())
        .map_err(|e| io_error(rel, e))
}

pub fn create_dir(root: &Path, rel: &str) -> Result<(), String> {
    std::fs::create_dir(resolve_entry(root, rel)?).map_err(|e| io_error(rel, e))
}

pub fn rename_entry(root: &Path, from: &str, to: &str) -> Result<(), String> {
    let (src, dst) = (resolve_entry(root, from)?, resolve_entry(root, to)?);
    src.symlink_metadata().map_err(|e| e.to_string())?;
    // fs::rename silently replaces an existing file. On APFS a case-only rename finds itself.
    if dst.symlink_metadata().is_ok() && !same_entry(&src, &dst) {
        return Err(format!("{to} already exists"));
    }
    std::fs::rename(src, dst).map_err(|e| e.to_string())
}

#[cfg(unix)]
fn same_entry(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (a.symlink_metadata(), b.symlink_metadata()) {
        (Ok(x), Ok(y)) => x.dev() == y.dev() && x.ino() == y.ino(),
        _ => false,
    }
}

#[cfg(not(unix))]
fn same_entry(_: &Path, _: &Path) -> bool {
    false
}

pub fn trash(root: &Path, rel: &str) -> Result<(), String> {
    let path = resolve_entry(root, rel)?;
    path.symlink_metadata().map_err(|e| e.to_string())?;
    move_to_trash(&path)
}

/// NSFileManager rather than `osascript` + Finder: no Automation permission prompt, no
/// Finder sound, and "Put Back" still works. Foundation is already loaded by the webview.
#[cfg(target_os = "macos")]
fn move_to_trash(path: &Path) -> Result<(), String> {
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject, Bool};
    use std::ffi::{c_char, CStr, CString};
    use std::os::unix::ffi::OsStrExt;
    use std::ptr::null_mut;

    let c_path = CString::new(path.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
    let class = |name: &CStr| AnyClass::get(name).ok_or("Foundation is unavailable");
    let (ns_string, ns_url, file_manager) = (
        class(c"NSString")?,
        class(c"NSURL")?,
        class(c"NSFileManager")?,
    );
    objc2::rc::autoreleasepool(|_| unsafe {
        let string: *mut AnyObject = msg_send![ns_string, stringWithUTF8String: c_path.as_ptr()];
        let url: *mut AnyObject = msg_send![ns_url, fileURLWithPath: string];
        let manager: *mut AnyObject = msg_send![file_manager, defaultManager];
        let mut error: *mut AnyObject = null_mut();
        let ok: Bool = msg_send![manager, trashItemAtURL: url, resultingItemURL: null_mut::<*mut AnyObject>(), error: &mut error as *mut *mut AnyObject];
        if ok.as_bool() {
            return Ok(());
        }
        if error.is_null() {
            return Err("Could not move to Trash".to_string());
        }
        let description: *mut AnyObject = msg_send![error, localizedDescription];
        let utf8: *const c_char = msg_send![description, UTF8String];
        Err(CStr::from_ptr(utf8).to_string_lossy().into_owned())
    })
}

#[cfg(not(target_os = "macos"))]
fn move_to_trash(_: &Path) -> Result<(), String> {
    Err("Moving to Trash is only supported on macOS".into())
}

/// Selects the entry in a Finder window. `rel` may be empty for the repo root.
pub fn reveal(root: &Path, rel: &str) -> Result<(), String> {
    let path = resolve(root, rel)?;
    // Waited on (it returns at once) so no zombie is left behind per click.
    #[cfg(target_os = "macos")]
    return match std::process::Command::new("open")
        .arg("-R")
        .arg(path)
        .status()
    {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => Err(format!("open -R failed ({s})")),
        Err(e) => Err(e.to_string()),
    };
    #[cfg(not(target_os = "macos"))]
    {
        let _ = path;
        Err("Reveal is only supported on macOS".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    struct Sandbox(PathBuf);

    impl Sandbox {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("gitviber-fs-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Sandbox(dir.canonicalize().unwrap())
        }
    }

    impl Drop for Sandbox {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn rejects_escapes() {
        let root = Path::new("/tmp");
        assert!(resolve(root, "../etc/passwd").is_err());
        assert!(resolve(root, "/etc/passwd").is_err());
        assert!(resolve(root, "a/../../b").is_err());
        assert!(resolve(root, "a/b.txt").is_ok());
    }

    #[test]
    fn entry_ops_reject_escapes_root_and_git_dir() {
        let sb = Sandbox::new("escape");
        let root = &sb.0;
        for bad in [
            "",
            ".",
            "..",
            "../x",
            "/tmp/x",
            "a/../../x",
            ".git",
            ".GIT/hooks",
            "a/.git",
        ] {
            assert!(create_file(root, bad).is_err(), "create_file {bad:?}");
            assert!(create_dir(root, bad).is_err(), "create_dir {bad:?}");
            assert!(trash(root, bad).is_err(), "trash {bad:?}");
            assert!(
                rename_entry(root, bad, "ok").is_err(),
                "rename from {bad:?}"
            );
        }
        fs::write(root.join("a.txt"), "x").unwrap();
        assert!(rename_entry(root, "a.txt", "../a.txt").is_err());
        assert!(rename_entry(root, "a.txt", ".git").is_err());
        assert!(root.join("a.txt").exists());
        assert!(!root.parent().unwrap().join("a.txt").exists());
    }

    #[test]
    #[cfg(unix)]
    fn entry_ops_do_not_follow_a_link_out_of_the_repo() {
        let sb = Sandbox::new("link");
        let outside = Sandbox::new("link-outside");
        std::os::unix::fs::symlink(&outside.0, sb.0.join("out")).unwrap();
        assert!(create_file(&sb.0, "out/x.txt").is_err());
        assert!(create_dir(&sb.0, "out/d").is_err());
        assert!(!outside.0.join("x.txt").exists());
        // The link itself is an entry of the repo and can be renamed.
        rename_entry(&sb.0, "out", "out2").unwrap();
        assert!(sb.0.join("out2").symlink_metadata().is_ok());
        assert!(outside.0.exists());
    }

    #[test]
    #[cfg(unix)]
    fn a_link_to_the_git_dir_is_off_limits() {
        let sb = Sandbox::new("gitlink");
        let root = &sb.0;
        git::run(root, &["init", "-q"]).unwrap();
        std::os::unix::fs::symlink(".git", root.join("docs")).unwrap();
        fs::write(root.join("hook.sh"), "#!/bin/sh\n").unwrap();

        assert!(create_file(root, "docs/hooks/pre-commit").is_err());
        assert!(create_dir(root, "docs/hooks/x").is_err());
        assert!(write_file(root, "docs/config", "[core]").is_err());
        assert!(rename_entry(root, "hook.sh", "docs/hooks/pre-commit").is_err());
        assert!(rename_entry(root, "docs/config", "config").is_err());
        assert!(trash(root, "docs/config").is_err());
        assert!(list_dir(root, "docs").is_err());
        assert!(!read_file(root, "docs/config").exists);
        let docs = list_dir(root, "").unwrap();
        assert!(!docs.iter().find(|e| e.name == "docs").unwrap().is_dir);

        assert!(!root.join(".git/hooks/pre-commit").exists());
        assert!(root.join(".git/config").exists());
        assert!(root.join("hook.sh").exists());
        // Ordinary files next to the link are unaffected.
        create_file(root, "notes.txt").unwrap();
    }

    #[test]
    fn creates_files_and_folders() {
        let sb = Sandbox::new("create");
        let root = &sb.0;
        create_dir(root, "src").unwrap();
        create_file(root, "src/main.rs").unwrap();
        assert!(root.join("src").is_dir());
        assert_eq!(fs::read(root.join("src/main.rs")).unwrap(), b"");
        fs::write(root.join("src/main.rs"), "keep").unwrap();
        assert!(create_file(root, "src/main.rs")
            .unwrap_err()
            .contains("already exists"));
        assert!(create_dir(root, "src")
            .unwrap_err()
            .contains("already exists"));
        assert_eq!(
            fs::read_to_string(root.join("src/main.rs")).unwrap(),
            "keep"
        );
        assert!(create_file(root, "missing/x.txt").is_err());
    }

    #[test]
    fn renames_without_overwriting() {
        let sb = Sandbox::new("rename");
        let root = &sb.0;
        fs::write(root.join("a.txt"), "a").unwrap();
        fs::write(root.join("b.txt"), "b").unwrap();
        fs::create_dir(root.join("dir")).unwrap();

        assert!(rename_entry(root, "a.txt", "b.txt")
            .unwrap_err()
            .contains("already exists"));
        assert_eq!(fs::read_to_string(root.join("b.txt")).unwrap(), "b");

        rename_entry(root, "a.txt", "dir/c.txt").unwrap();
        assert_eq!(fs::read_to_string(root.join("dir/c.txt")).unwrap(), "a");
        rename_entry(root, "dir", "folder").unwrap();
        assert!(root.join("folder/c.txt").exists());
        assert!(rename_entry(root, "nope.txt", "x.txt").is_err());

        // A case-only rename must work on case-insensitive APFS.
        rename_entry(root, "b.txt", "B.txt").unwrap();
        let names: Vec<_> = fs::read_dir(root)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert!(names.iter().any(|n| n == "B.txt"));
        assert!(!names.iter().any(|n| n == "b.txt"));
    }
}
