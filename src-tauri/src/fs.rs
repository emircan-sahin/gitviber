//! Worktree access for the file explorer. Every path from the frontend is resolved
//! against the open repo and rejected if it would escape it.

use crate::git::{self, FileText};
use serde::Serialize;
use std::path::{Component, Path, PathBuf};

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
    Ok(full)
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
            // Follows symlinks, so a linked folder expands like a folder.
            let is_dir = e.path().is_dir();
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

#[cfg(test)]
mod tests {
    use super::resolve;
    use std::path::Path;

    #[test]
    fn rejects_escapes() {
        let root = Path::new("/tmp");
        assert!(resolve(root, "../etc/passwd").is_err());
        assert!(resolve(root, "/etc/passwd").is_err());
        assert!(resolve(root, "a/../../b").is_err());
        assert!(resolve(root, "a/b.txt").is_ok());
    }
}
