//! Worktree access for the file explorer. Every path from the frontend is resolved
//! against the open repo and rejected if it would escape it.

use crate::git::{self, FileText};
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, OnceLock};

pub fn resolve(root: &Path, rel: &str) -> Result<PathBuf, String> {
    resolve_under(root, &root.canonicalize().map_err(|e| e.to_string())?, rel)
}

/// `resolve` with the root's real path read once, for many paths under it.
fn resolve_under(root: &Path, real_root: &Path, rel: &str) -> Result<PathBuf, String> {
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
    let real = confine(real_root, &full).ok_or_else(escape)?;
    // The name check above misses a link like `docs -> .git`, which survives a clone.
    if in_git_dir(root, real_root, &real) {
        return Err(format!("refusing to touch git internals: {rel}"));
    }
    Ok(full)
}

/// The real path of `full`, or of its deepest part that exists, when that is inside `real_root`
/// (a canonical root). Symlinks can point anywhere, and a path that doesn't exist yet can't be
/// canonicalized; a dangling link is refused, as writing would follow it.
pub(crate) fn confine(real_root: &Path, full: &Path) -> Option<PathBuf> {
    let mut probe = full;
    let real = loop {
        match probe.canonicalize() {
            Ok(p) => break p,
            Err(_) if probe.symlink_metadata().is_ok() => return None,
            Err(_) => probe = probe.parent()?,
        }
    };
    real.starts_with(real_root).then_some(real)
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

/// Whether the explorer shows `path` as a folder. Follows symlinks, so a linked folder expands
/// like a folder, unless it leads into the git dir (resolve() would refuse to list it anyway).
fn expands(root: &Path, real_root: &Path, path: &Path) -> bool {
    path.is_dir()
        && !(path.symlink_metadata().is_ok_and(|m| m.is_symlink())
            && path
                .canonicalize()
                .is_ok_and(|p| in_git_dir(root, real_root, &p)))
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
            Entry {
                name,
                path,
                is_dir: expands(root, &real_root, &e.path()),
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
    // Unsorted: the explorer orders them (compareEntries), filtered or not.
    Ok(entries)
}

/// Every file quick open offers: tracked and untracked, not ignored. `is_file` drops tracked
/// files deleted from the worktree and submodules, which `--cached` still lists.
pub fn list_files(root: &Path) -> Result<Vec<String>, String> {
    let args = [
        "ls-files",
        "-z",
        "--cached",
        "--others",
        "--exclude-standard",
        "--deduplicate",
    ];
    let out = git::run(root, &args)?;
    Ok(out
        .split(|&b| b == 0)
        .filter(|p| !p.is_empty())
        .map(|p| String::from_utf8_lossy(p).into_owned())
        .filter(|p| root.join(p).is_file())
        .collect())
}

/// What the explorer's filter adds to `list_files`: ignored files one by one, and an ignored
/// folder (node_modules) as one entry, not walked into; it opens lazily, as in the tree.
/// `ls-files --ignored --directory` would also list a folder that only holds ignored files
/// (app/ with just app/.env) as ignored; `matching` lists what a pattern matches, as the tree dims.
pub fn list_ignored(root: &Path) -> Result<Vec<Entry>, String> {
    let args = [
        "status",
        "--porcelain=v2",
        "-z",
        "--ignored=matching",
        "--untracked-files=normal",
        "--ignore-submodules=all",
    ];
    let out = git::run(root, &args)?;
    let real_root = root.canonicalize().map_err(|e| e.to_string())?;
    let mut fields = out.split(|&b| b == 0);
    let mut entries = vec![];
    while let Some(field) = fields.next() {
        // A rename's old path is the next field, and any name may start with "! ".
        if field.starts_with(b"2 ") {
            fields.next();
        } else if let Some(raw) = field.strip_prefix(b"! ") {
            let raw = String::from_utf8_lossy(raw);
            let path = raw.trim_end_matches('/');
            entries.push(Entry {
                name: path.rsplit('/').next().unwrap_or(path).to_string(),
                path: path.to_string(),
                // git prints a linked folder as a file, with no trailing "/".
                is_dir: raw.ends_with('/') || expands(root, &real_root, &root.join(path)),
                ignored: true,
            });
        }
    }
    Ok(entries)
}

/// What a path in the repo is on disk.
#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    File,
    Dir,
}

/// Each path's kind, None when it's missing or outside the repo: a terminal link to an ignored
/// file or folder, which the file list doesn't have, is one stat.
pub fn kinds(root: &Path, rels: &[String]) -> Vec<Option<Kind>> {
    let Ok(real) = root.canonicalize() else {
        return rels.iter().map(|_| None).collect();
    };
    rels.iter()
        .map(|rel| {
            let meta = resolve_under(root, &real, rel)
                .and_then(|p| p.metadata().map_err(|e| e.to_string()));
            match meta {
                Ok(m) if m.is_dir() => Some(Kind::Dir),
                Ok(m) if m.is_file() => Some(Kind::File),
                _ => None,
            }
        })
        .collect()
}

/// Writes a file inside the repo (used to save a resolved conflict).
pub fn write_file(root: &Path, rel: &str, content: &str) -> Result<(), String> {
    std::fs::write(resolve(root, rel)?, content).map_err(|e| e.to_string())
}

pub fn read_file(root: &Path, rel: &str) -> FileText {
    resolve(root, rel)
        .map(|p| read_text_at(&p))
        .unwrap_or_default()
}

/// A resolved file's text; one too large says so, and one that can't be read doesn't exist.
pub(crate) fn read_text_at(path: &Path) -> FileText {
    match git::read_regular(path) {
        Ok(Some(bytes)) => git::to_file_text(bytes),
        Ok(None) => FileText {
            too_large: true,
            exists: true,
            ..Default::default()
        },
        Err(_) => FileText::default(),
    }
}

/// A diff's working-tree side. A symlink is its target path, as git stores and diffs it (the
/// file it points to may be another one, or outside the repo). That text is never written back.
pub fn read_diff_side(root: &Path, rel: &str) -> FileText {
    let link = resolve_entry(root, rel).ok().filter(|p| is_link(p));
    match link.and_then(|p| std::fs::read_link(p).ok()) {
        Some(target) => FileText {
            lossy: true,
            ..git::to_file_text(target.into_os_string().into_encoded_bytes())
        },
        None => read_file(root, rel),
    }
}

pub fn read_media(root: &Path, rel: &str) -> Result<Vec<u8>, String> {
    read_media_at(&resolve(root, rel)?)
}

/// A resolved file's bytes, up to MAX_MEDIA_BYTES; FIFOs and devices are refused.
pub(crate) fn read_media_at(path: &Path) -> Result<Vec<u8>, String> {
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
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
pub(crate) fn resolve_entry(root: &Path, rel: &str) -> Result<PathBuf, String> {
    entry_under(root, &root.canonicalize().map_err(|e| e.to_string())?, rel)
}

/// `resolve_entry` with the root's real path read once, for many entries under it.
fn entry_under(root: &Path, real_root: &Path, rel: &str) -> Result<PathBuf, String> {
    let rel_path = Path::new(rel);
    let name = match rel_path.components().next_back() {
        Some(Component::Normal(n)) if !n.eq_ignore_ascii_case(".git") => n,
        _ => return Err(format!("not a file or folder in the repository: {rel}")),
    };
    let parent = rel_path.parent().unwrap_or(Path::new(""));
    Ok(resolve_under(root, real_root, &parent.to_string_lossy())?.join(name))
}

fn io_error(rel: &str, e: std::io::Error) -> String {
    if e.kind() == std::io::ErrorKind::AlreadyExists {
        format!("{rel} already exists")
    } else {
        e.to_string()
    }
}

/// A new entry's path, with the folders typed before its name ("a/b.ts") made.
fn new_entry(root: &Path, rel: &str) -> Result<PathBuf, String> {
    let path = resolve_entry(root, rel)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            let file = Path::new(rel).ancestors().find(|a| root.join(a).is_file());
            file.map_or(e.to_string(), |f| {
                format!("{} is a file, not a folder", f.display())
            })
        })?;
    }
    Ok(path)
}

pub fn create_file(root: &Path, rel: &str) -> Result<(), String> {
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(new_entry(root, rel)?)
        .map(|_| ())
        .map_err(|e| io_error(rel, e))
}

pub fn create_dir(root: &Path, rel: &str) -> Result<(), String> {
    std::fs::create_dir(new_entry(root, rel)?).map_err(|e| io_error(rel, e))
}

pub fn rename_entry(root: &Path, from: &str, to: &str) -> Result<(), String> {
    let (src, dst) = (resolve_entry(root, from)?, resolve_entry(root, to)?);
    src.symlink_metadata().map_err(|e| e.to_string())?;
    // On APFS a case-only rename finds the source itself at the destination.
    if dst.symlink_metadata().is_ok() {
        if !same_entry(&src, &dst) {
            return Err(format!("{to} already exists"));
        }
        return std::fs::rename(src, dst).map_err(|e| e.to_string());
    }
    rename_exclusive(&src, &dst).map_err(|e| io_error(to, e))
}

/// `dst` is `src` spelled differently (APFS ignores case and Unicode normalization): same
/// inode, and no entry with exactly `dst`'s name exists, so it isn't a hard link.
#[cfg(unix)]
fn same_entry(src: &Path, dst: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    let same_inode = match (src.symlink_metadata(), dst.symlink_metadata()) {
        (Ok(x), Ok(y)) => x.dev() == y.dev() && x.ino() == y.ino(),
        _ => false,
    };
    let (Some(parent), Some(name)) = (dst.parent(), dst.file_name()) else {
        return false;
    };
    same_inode
        && std::fs::read_dir(parent)
            .is_ok_and(|mut entries| !entries.any(|e| e.is_ok_and(|e| e.file_name() == name)))
}

#[cfg(not(unix))]
fn same_entry(_: &Path, _: &Path) -> bool {
    false
}

/// fs::rename silently replaces an existing file; RENAME_EXCL makes the kernel refuse
/// instead, with no window between checking and renaming.
#[cfg(target_os = "macos")]
fn rename_exclusive(src: &Path, dst: &Path) -> std::io::Result<()> {
    use std::ffi::{c_char, c_int, c_uint, CString};
    use std::os::unix::ffi::OsStrExt;
    extern "C" {
        fn renamex_np(from: *const c_char, to: *const c_char, flags: c_uint) -> c_int;
    }
    const RENAME_EXCL: c_uint = 0x4;
    let c_path = |p: &Path| CString::new(p.as_os_str().as_bytes()).map_err(std::io::Error::other);
    let (from, to) = (c_path(src)?, c_path(dst)?);
    match unsafe { renamex_np(from.as_ptr(), to.as_ptr(), RENAME_EXCL) } {
        0 => Ok(()),
        _ => Err(std::io::Error::last_os_error()),
    }
}

/// Elsewhere a file created between this check and the rename is still replaced.
#[cfg(not(target_os = "macos"))]
fn rename_exclusive(src: &Path, dst: &Path) -> std::io::Result<()> {
    if dst.symlink_metadata().is_ok() {
        return Err(std::io::ErrorKind::AlreadyExists.into());
    }
    std::fs::rename(src, dst)
}

pub fn trash(root: &Path, rel: &str) -> Result<(), String> {
    let path = resolve_entry(root, rel)?;
    path.symlink_metadata().map_err(|e| e.to_string())?;
    move_to_trash(&path).map(|_| ())
}

/// A file's size and modification time, or None when it's gone: enough to tell later
/// whether anything wrote it since.
pub type Stamp = Option<(u64, std::time::SystemTime)>;

pub fn stamp(root: &Path, rel: &str) -> Stamp {
    let meta = resolve_entry(root, rel).ok()?.symlink_metadata().ok()?;
    Some((meta.len(), meta.modified().ok()?))
}

/// Puts a copy of a file (or symlink) in the Trash as `name`, and says where it went there.
pub fn trash_copy(root: &Path, rel: &str, name: &str) -> Result<PathBuf, String> {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let src = resolve_entry(root, rel)?;
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    // Under test the copy stays for `put_back`: beside the repo, in the sandbox that removes it.
    let tmp = match root.parent() {
        Some(sandbox) if cfg!(test) => sandbox.to_path_buf(),
        _ => std::env::temp_dir(),
    };
    let dir = tmp.join(format!("gitviber-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let copy = dir.join(name);
    let trashed = copy_entry(&src, &copy).and_then(|()| stash(&copy));
    if !cfg!(test) {
        let _ = std::fs::remove_dir_all(&dir);
    }
    trashed
}

#[cfg(not(test))]
fn stash(copy: &Path) -> Result<PathBuf, String> {
    move_to_trash(copy)
}

/// `cargo test` must not fill the user's Trash: the copy stays in its temporary folder.
#[cfg(test)]
fn stash(copy: &Path) -> Result<PathBuf, String> {
    Ok(copy.to_path_buf())
}

/// Writes `copy` (from `trash_copy`) back over `rel`, or removes `rel` when there's none.
pub fn put_back(root: &Path, rel: &str, copy: Option<&Path>) -> Result<(), String> {
    let dst = resolve_entry(root, rel)?;
    if let Some(c) = copy {
        c.symlink_metadata()
            .map_err(|_| format!("The discarded version of {rel} is no longer in the Trash."))?;
    }
    // Copying onto a link would write where it points.
    if copy.is_none_or(is_link) || is_link(&dst) {
        match std::fs::remove_file(&dst) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.to_string()),
            _ => {}
        }
    }
    copy.map_or(Ok(()), |c| copy_entry(c, &dst))
}

fn is_link(p: &Path) -> bool {
    p.symlink_metadata().is_ok_and(|m| m.is_symlink())
}

pub(crate) fn copy_entry(src: &Path, dst: &Path) -> Result<(), String> {
    #[cfg(unix)]
    if is_link(src) {
        let target = std::fs::read_link(src).map_err(|e| e.to_string())?;
        return std::os::unix::fs::symlink(target, dst).map_err(|e| e.to_string());
    }
    std::fs::copy(src, dst)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Those of `rels` in `from` that `copy_into` copies, with their paths there: files, not links,
/// inside the worktree.
pub fn copyable<'a>(
    from: &'a Path,
    rels: &'a [String],
) -> impl Iterator<Item = (&'a str, PathBuf)> + 'a {
    let real = from.canonicalize().ok();
    rels.iter().filter_map(move |rel| {
        let src = entry_under(from, real.as_deref()?, rel).ok()?;
        src.symlink_metadata()
            .is_ok_and(|m| m.is_file())
            .then_some((rel.as_str(), src))
    })
}

/// Copies the files at `rels` in `from` to the same paths in `to`, making folders as needed:
/// `.worktreeinclude`'s into a new worktree. Nothing is overwritten, and a link or a path leading
/// out of either worktree is skipped: a relative link would point elsewhere from the new one.
pub fn copy_into(from: &Path, to: &Path, rels: &[String]) {
    let Ok(real_to) = to.canonicalize() else {
        return;
    };
    for (rel, src) in copyable(from, rels) {
        let Ok(dst) = entry_under(to, &real_to, rel) else {
            continue;
        };
        if dst.symlink_metadata().is_ok() {
            continue;
        }
        if dst
            .parent()
            .is_some_and(|p| std::fs::create_dir_all(p).is_ok())
        {
            let _ = std::fs::copy(&src, &dst);
        }
    }
}

/// NSFileManager rather than `osascript` + Finder: no Automation permission prompt, no
/// Finder sound, and "Put Back" still works. Foundation is already loaded by the webview.
/// Returns where the item ended up in the Trash.
#[cfg(target_os = "macos")]
fn move_to_trash(path: &Path) -> Result<PathBuf, String> {
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
        // nil for a non-UTF-8 name; passing nil on to NSURL would raise.
        if string.is_null() {
            return Err("Can't move this name to the Trash".to_string());
        }
        let url: *mut AnyObject = msg_send![ns_url, fileURLWithPath: string];
        let manager: *mut AnyObject = msg_send![file_manager, defaultManager];
        let mut error: *mut AnyObject = null_mut();
        let mut trashed: *mut AnyObject = null_mut();
        let ok: Bool = msg_send![manager, trashItemAtURL: url, resultingItemURL: &mut trashed as *mut *mut AnyObject, error: &mut error as *mut *mut AnyObject];
        let fallback = || Err("Could not move to Trash".to_string());
        if ok.as_bool() {
            if trashed.is_null() {
                return fallback();
            }
            let path: *mut AnyObject = msg_send![trashed, path];
            let utf8: *const c_char = msg_send![path, UTF8String];
            if utf8.is_null() {
                return fallback();
            }
            return Ok(PathBuf::from(std::ffi::OsStr::from_bytes(
                CStr::from_ptr(utf8).to_bytes(),
            )));
        }
        if error.is_null() {
            return fallback();
        }
        let description: *mut AnyObject = msg_send![error, localizedDescription];
        let utf8: *const c_char = msg_send![description, UTF8String];
        if utf8.is_null() {
            return fallback();
        }
        Err(CStr::from_ptr(utf8).to_string_lossy().into_owned())
    })
}

#[cfg(target_os = "linux")]
fn move_to_trash(path: &Path) -> Result<PathBuf, String> {
    crate::trash::move_to_trash(path)
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn move_to_trash(_: &Path) -> Result<PathBuf, String> {
    Err("Moving to Trash is not supported on this platform yet".into())
}

/// Selects the entry in the file manager. `rel` may be empty for the repo root.
pub fn reveal(root: &Path, rel: &str) -> Result<(), String> {
    // Like the other entry actions, a link is revealed itself, wherever it points.
    let path = if rel.is_empty() {
        resolve(root, rel)?
    } else {
        resolve_entry(root, rel)?
    };
    crate::launch::reveal(path)
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
    fn list_files_skips_ignored_and_deleted() {
        let sb = Sandbox::new("list");
        let root = &sb.0;
        git::run(root, &["init", "-q"]).unwrap();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join(".gitignore"), "build/\n").unwrap();
        fs::create_dir_all(root.join("build")).unwrap();
        fs::write(root.join("build/out.js"), "x").unwrap();
        for f in ["tracked.txt", "gone.txt", "src/new file.rs"] {
            fs::write(root.join(f), "x").unwrap();
        }
        git::run(root, &["add", "tracked.txt", "gone.txt"]).unwrap();
        fs::remove_file(root.join("gone.txt")).unwrap();
        let mut files = list_files(root).unwrap();
        files.sort();
        assert_eq!(files, [".gitignore", "src/new file.rs", "tracked.txt"]);
    }

    #[test]
    fn list_ignored_lists_ignored_folders_whole() {
        let sb = Sandbox::new("ignored");
        let root = &sb.0;
        git::run(root, &["init", "-q"]).unwrap();
        fs::write(root.join(".gitignore"), ".env\nnode_modules/\n").unwrap();
        fs::create_dir_all(root.join("node_modules/pkg")).unwrap();
        fs::create_dir_all(root.join("app")).unwrap();
        for f in [".env", "app/.env", "node_modules/pkg/index.js", "kept.txt"] {
            fs::write(root.join(f), "x").unwrap();
        }
        // A staged rename from "! old.txt": its old path is a field that must not read as ignored.
        fs::write(root.join("! old.txt"), "x").unwrap();
        git::run(root, &["add", "! old.txt"]).unwrap();
        let id = [
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
        ];
        git::run(root, &[&id[..], &["commit", "-qm", "c"]].concat()).unwrap();
        git::run(root, &["mv", "! old.txt", "renamed.txt"]).unwrap();
        let mut listed: Vec<_> = list_ignored(root)
            .unwrap()
            .into_iter()
            .map(|e| (e.path, e.name, e.is_dir, e.ignored))
            .collect();
        listed.sort();
        let entry = |path: &str, name: &str, is_dir| (path.into(), name.into(), is_dir, true);
        assert_eq!(
            listed,
            [
                entry(".env", ".env", false),
                entry("app/.env", ".env", false),
                entry("node_modules", "node_modules", true),
            ]
        );
    }

    /// git prints an ignored link to a folder as a file; the filter shows it as the tree does.
    #[cfg(unix)]
    #[test]
    fn list_ignored_shows_a_linked_folder_as_a_folder() {
        use std::os::unix::fs::symlink;
        let sb = Sandbox::new("ignored-link");
        let root = &sb.0;
        git::run(root, &["init", "-q"]).unwrap();
        fs::write(root.join(".gitignore"), "vendor\ngit-link\n").unwrap();
        fs::create_dir_all(root.join("lib")).unwrap();
        symlink("lib", root.join("vendor")).unwrap();
        symlink(".git", root.join("git-link")).unwrap();
        let mut listed: Vec<_> = list_ignored(root)
            .unwrap()
            .into_iter()
            .map(|e| (e.path, e.is_dir))
            .collect();
        listed.sort();
        let want = [
            ("git-link".to_string(), false),
            ("vendor".to_string(), true),
        ];
        assert_eq!(listed, want);
        let tree = list_dir(root, "").unwrap();
        for (path, is_dir) in want {
            assert!(tree.iter().any(|e| e.path == path && e.is_dir == is_dir));
        }
    }

    #[test]
    fn kinds_of_ignored_paths_and_escapes() {
        let sb = Sandbox::new("kinds");
        let root = &sb.0;
        fs::create_dir_all(root.join("dist")).unwrap();
        fs::write(root.join("dist/index.js"), "x").unwrap();
        let ask = |p: &[&str]| kinds(root, &p.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        assert_eq!(
            ask(&[
                "dist",
                "dist/index.js",
                "missing.ts",
                "../x",
                "/etc",
                ".git"
            ]),
            [Some(Kind::Dir), Some(Kind::File), None, None, None, None]
        );
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
        create_file(root, "new/deeper/x.txt").unwrap();
        assert!(root.join("new/deeper/x.txt").is_file());
        create_dir(root, "more/dir").unwrap();
        assert!(root.join("more/dir").is_dir());
        assert_eq!(
            create_file(root, "src/main.rs/deeper/x.txt").unwrap_err(),
            "src/main.rs is a file, not a folder"
        );
    }

    #[test]
    #[cfg(unix)]
    fn rename_onto_a_hard_link_is_refused() {
        let sb = Sandbox::new("hardlink");
        let root = &sb.0;
        fs::write(root.join("a.txt"), "a").unwrap();
        fs::hard_link(root.join("a.txt"), root.join("b.txt")).unwrap();
        assert!(rename_entry(root, "a.txt", "b.txt")
            .unwrap_err()
            .contains("already exists"));
        assert!(root.join("a.txt").exists() && root.join("b.txt").exists());
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
