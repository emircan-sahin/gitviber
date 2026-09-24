//! The freedesktop.org Trash (https://specifications.freedesktop.org/trash-spec/latest/), so
//! the file manager lists what GitViber trashed and can restore it.

use std::fs::{DirBuilder, OpenOptions};
use std::io::{ErrorKind, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{DirBuilderExt, MetadataExt};
use std::path::{Path, PathBuf};

/// Moves `path` (absolute) to the Trash and returns where it ended up there.
pub fn move_to_trash(path: &Path) -> Result<PathBuf, String> {
    let meta = path.symlink_metadata().map_err(|e| e.to_string())?;
    let home = home_trash()?;
    if meta.dev() == device(&home) {
        return put(&home, path, path, rename);
    }
    // Another filesystem, like a discarded version's copy on a tmpfs /tmp: a file is copied into
    // the home Trash (as KDE does), a folder goes to that filesystem's own .Trash-$uid.
    if !meta.is_dir() {
        return put(&home, path, path, |from, to| {
            crate::fs::copy_entry(from, to)?;
            // Left in place, the file mustn't also stay in the Trash without its .trashinfo.
            std::fs::remove_file(from).map_err(|e| {
                let _ = std::fs::remove_file(to);
                e.to_string()
            })
        });
    }
    let top = mount_point(path, meta.dev());
    let trash = top.join(format!(".Trash-{}", unsafe { libc::getuid() }));
    let recorded = path.strip_prefix(&top).unwrap_or(path);
    put(&trash, recorded, path, rename)
}

fn rename(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::rename(from, to).map_err(|e| e.to_string())
}

fn home_trash() -> Result<PathBuf, String> {
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| Path::new(&h).join(".local/share")))
        .ok_or("Can't find the Trash: HOME is not set")?;
    Ok(data.join("Trash"))
}

/// The device of `path`, or of its nearest existing ancestor (the Trash may not exist yet).
fn device(path: &Path) -> u64 {
    path.ancestors()
        .find_map(|p| p.metadata().ok())
        .map_or(0, |m| m.dev())
}

/// The top directory of the filesystem `path` is on.
fn mount_point(path: &Path, dev: u64) -> PathBuf {
    let mut top = path;
    while let Some(parent) = top.parent() {
        if parent.metadata().map_or(true, |m| m.dev() != dev) {
            break;
        }
        top = parent;
    }
    top.to_path_buf()
}

/// Reserves a name in `trash` by creating its .trashinfo (recording `recorded` as the original
/// path), then moves `src` into files/ under that name.
fn put(
    trash: &Path,
    recorded: &Path,
    src: &Path,
    move_to: impl FnOnce(&Path, &Path) -> Result<(), String>,
) -> Result<PathBuf, String> {
    let (files, info) = (trash.join("files"), trash.join("info"));
    for dir in [&files, &info] {
        DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
            .map_err(|e| e.to_string())?;
    }
    let name = src.file_name().ok_or("Can't move this to the Trash")?;
    let contents = format!(
        "[Trash Info]\nPath={}\nDeletionDate={}\n",
        encode(recorded),
        now()
    );
    let (mut file, info_path, dst) = (1u32..)
        .find_map(|n| {
            let mut candidate = name.to_os_string();
            if n > 1 {
                candidate.push(format!(".{n}"));
            }
            let mut info_name = candidate.clone();
            info_name.push(".trashinfo");
            let info_path = info.join(info_name);
            let file = match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&info_path)
            {
                Ok(f) => f,
                Err(e) if e.kind() == ErrorKind::AlreadyExists => return None,
                Err(e) => return Some(Err(e.to_string())),
            };
            let dst = files.join(candidate);
            // An entry left in files/ without its .trashinfo still holds the name.
            if dst.symlink_metadata().is_ok() {
                let _ = std::fs::remove_file(&info_path);
                return None;
            }
            Some(Ok((file, info_path, dst)))
        })
        .expect("names run out")?;
    let moved = file
        .write_all(contents.as_bytes())
        .map_err(|e| e.to_string())
        .and_then(|()| move_to(src, &dst));
    if moved.is_err() {
        let _ = std::fs::remove_file(&info_path);
    }
    moved.map(|()| dst)
}

/// A path as the spec (and a file:// URI) wants it: percent-encoded bytes, `/` kept.
pub fn encode(path: &Path) -> String {
    let mut out = String::new();
    for &b in path.as_os_str().as_bytes() {
        if b.is_ascii_alphanumeric() || b"/-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Local time as YYYY-MM-DDThh:mm:ss.
fn now() -> String {
    unsafe {
        let t = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&t, &mut tm);
        format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
            tm.tm_year + 1900,
            tm.tm_mon + 1,
            tm.tm_mday,
            tm.tm_hour,
            tm.tm_min,
            tm.tm_sec
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn sandbox(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("gitviber-trash-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn encodes_like_a_file_uri() {
        assert_eq!(encode(Path::new("/a b/ç,%.txt")), "/a%20b/%C3%A7%2C%25.txt");
    }

    #[test]
    fn records_the_original_and_keeps_names_apart() {
        let dir = sandbox("put");
        let trash = dir.join("Trash");
        for _ in 0..2 {
            let src = dir.join("a b.txt");
            fs::write(&src, "x").unwrap();
            put(&trash, &src, &src, rename).unwrap();
            assert!(!src.exists());
        }
        assert!(trash.join("files/a b.txt").is_file());
        assert!(trash.join("files/a b.txt.2").is_file());
        let info = fs::read_to_string(trash.join("info/a b.txt.2.trashinfo")).unwrap();
        let path = encode(&dir.join("a b.txt"));
        assert!(info.starts_with(&format!("[Trash Info]\nPath={path}\nDeletionDate=")));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn skips_a_name_taken_in_files_and_cleans_up_on_failure() {
        let dir = sandbox("taken");
        let trash = dir.join("Trash");
        fs::create_dir_all(trash.join("files")).unwrap();
        fs::write(trash.join("files/f"), "orphan").unwrap();
        let src = dir.join("f");
        fs::write(&src, "x").unwrap();
        let dst = put(&trash, &src, &src, rename).unwrap();
        assert_eq!(dst, trash.join("files/f.2"));
        assert!(!trash.join("info/f.trashinfo").exists());

        fs::write(&src, "x").unwrap();
        let err = put(&trash, &src, &src, |_, _| Err("nope".into()));
        assert_eq!(err, Err("nope".to_string()));
        assert!(!trash.join("info/f.3.trashinfo").exists());
        fs::remove_dir_all(&dir).unwrap();
    }
}
