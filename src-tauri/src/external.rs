//! Files outside the repo that terminal output names (an agent's screenshot in /tmp, a dotfile
//! in ~): read to show in a read-only tab, never written.

use crate::fs::{read_media_at, read_text_at, Kind};
use crate::git::FileText;
use serde::Serialize;
use std::path::PathBuf;

/// What a link's path is on disk, spelled in full.
#[derive(Serialize, Debug, PartialEq)]
pub struct Found {
    pub path: String,
    pub kind: Kind,
}

/// An absolute path, or one in the home folder (`~`, `~/x`), as its real path.
fn resolve(path: &str) -> Result<PathBuf, String> {
    let home = || {
        std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from)
            .ok_or("HOME is not set")
    };
    let full = match path.strip_prefix("~") {
        Some("") => home()?,
        Some(rest) if rest.starts_with('/') => home()?.join(&rest[1..]),
        _ => PathBuf::from(path),
    };
    if !full.is_absolute() {
        return Err(format!("not an absolute path: {path}"));
    }
    full.canonicalize().map_err(|e| e.to_string())
}

/// Each path's real path and whether it's a file or a folder; None when there's neither.
pub fn find(paths: &[String]) -> Vec<Option<Found>> {
    paths
        .iter()
        .map(|p| {
            let real = resolve(p).ok()?;
            let meta = real.metadata().ok()?;
            let kind = if meta.is_dir() {
                Kind::Dir
            } else if meta.is_file() {
                Kind::File
            } else {
                return None;
            };
            Some(Found {
                path: real.to_string_lossy().into_owned(),
                kind,
            })
        })
        .collect()
}

pub fn read_file(path: &str) -> FileText {
    resolve(path).map(|p| read_text_at(&p)).unwrap_or_default()
}

pub fn read_media(path: &str) -> Result<Vec<u8>, String> {
    read_media_at(&resolve(path)?)
}

pub fn reveal(path: &str) -> Result<(), String> {
    crate::launch::reveal(resolve(path)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_files_and_folders_and_nothing_else() {
        let dir = std::env::temp_dir().join(format!("gitviber-external-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.png"), b"x").unwrap();
        let real = dir.canonicalize().unwrap();
        let ask = |p: &str| find(&[p.to_string()]).pop().unwrap();
        let file = ask(&dir.join("a.png").to_string_lossy());
        assert_eq!(file.as_ref().map(|f| &f.kind), Some(&Kind::File));
        assert_eq!(
            file.unwrap().path,
            real.join("a.png").to_string_lossy().into_owned()
        );
        assert_eq!(ask(&dir.to_string_lossy()).map(|f| f.kind), Some(Kind::Dir));
        assert!(ask(&dir.join("nope").to_string_lossy()).is_none());
        // Relative paths are the repo's, not this module's.
        assert!(ask("a.png").is_none());
        assert!(read_file(&dir.join("a.png").to_string_lossy()).exists);
        assert_eq!(
            read_media(&dir.join("a.png").to_string_lossy()),
            Ok(b"x".to_vec())
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_home_folder_has_a_tilde() {
        if let Some(home) = std::env::var_os("HOME") {
            let real = PathBuf::from(home).canonicalize().unwrap();
            assert_eq!(resolve("~").unwrap(), real);
            assert_eq!(resolve("~/").unwrap(), real);
        }
        assert!(resolve("~other/x").is_err());
    }
}
