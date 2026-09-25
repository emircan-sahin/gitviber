//! File contents from the disk or the object store, as text, with Git LFS resolved.

use super::{command, run, run_text, MAX_MEDIA_BYTES};
use crate::lfs;
use crate::process::exec;
use serde::Serialize;
use std::io::Read;
use std::path::Path;

pub(super) const MAX_TEXT_BYTES: usize = 8 * 1024 * 1024;

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FileText {
    pub text: String,
    pub binary: bool,
    pub too_large: bool,
    /// False when the file does not exist on that side (added / deleted).
    pub exists: bool,
    /// Not valid UTF-8 (e.g. Latin-1); text was decoded lossily, so never write it back.
    pub lossy: bool,
    /// A Git LFS file whose object isn't downloaded: says so, with its size.
    pub lfs_missing: Option<String>,
}

pub fn is_binary(bytes: &[u8]) -> bool {
    bytes.iter().take(8000).any(|b| *b == 0)
}

/// Reads a regular file up to MAX_TEXT_BYTES. Ok(None) = too large. FIFOs, devices and
/// sockets are refused: reading /dev/zero or a pipe would hang or eat memory.
pub fn read_regular(path: &Path) -> Result<Option<Vec<u8>>, String> {
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if !meta.is_file() {
        return Err("not a regular file".into());
    }
    if meta.len() > MAX_TEXT_BYTES as u64 {
        return Ok(None);
    }
    let mut buf = Vec::new();
    std::fs::File::open(path)
        .and_then(|f| f.take(MAX_TEXT_BYTES as u64 + 1).read_to_end(&mut buf))
        .map_err(|e| e.to_string())?;
    Ok((buf.len() <= MAX_TEXT_BYTES).then_some(buf))
}

pub fn to_file_text(bytes: Vec<u8>) -> FileText {
    if bytes.len() > MAX_TEXT_BYTES {
        return FileText {
            too_large: true,
            exists: true,
            ..Default::default()
        };
    }
    if is_binary(&bytes) {
        return FileText {
            binary: true,
            exists: true,
            ..Default::default()
        };
    }
    match String::from_utf8(bytes) {
        Ok(text) => FileText {
            text,
            exists: true,
            ..Default::default()
        },
        Err(e) => FileText {
            text: String::from_utf8_lossy(e.as_bytes()).into_owned(),
            exists: true,
            lossy: true,
            ..Default::default()
        },
    }
}

/// Reads `<rev>:<path>` (rev "" means the index). A missing blob is not an error. The size is
/// checked first so a huge file in some old commit is never read into memory.
pub(super) fn blob(repo: &Path, rev: &str, path: &str) -> FileText {
    let spec = format!("{rev}:{path}");
    let Ok(size) = run_text(repo, &["cat-file", "-s", &spec]) else {
        return FileText::default();
    };
    if size
        .trim()
        .parse::<u64>()
        .is_ok_and(|n| n > MAX_TEXT_BYTES as u64)
    {
        return FileText {
            too_large: true,
            exists: true,
            ..Default::default()
        };
    }
    match smudged(repo, &spec) {
        Ok(bytes) => to_file_text(bytes),
        Err(_) => FileText::default(),
    }
}

/// `<rev>:<path>` the way a checkout writes it (line endings, smudge filters), so it compares
/// with the file on disk. git-lfs is told not to download: its files come out as pointers.
pub(super) fn smudged(repo: &Path, spec: &str) -> Result<Vec<u8>, String> {
    let mut cmd = command(repo, &["cat-file", "--filters", spec]);
    cmd.env("GIT_LFS_SKIP_SMUDGE", "1");
    // A filter that fails (git-lfs configured but not installed) still leaves the stored form.
    exec(cmd, "git cat-file", &[], None, None).or_else(|_| run(repo, &["cat-file", "blob", spec]))
}

/// An LFS pointer read as text stands for its object: the object's text when it's downloaded.
pub(super) fn lfs_text(repo: &Path, f: FileText) -> FileText {
    let Some(p) = f.exists.then(|| lfs::pointer(f.text.as_bytes())).flatten() else {
        return f;
    };
    match lfs::object(repo, &p).map(|o| read_regular(&o)) {
        Some(Ok(Some(bytes))) => to_file_text(bytes),
        Some(Ok(None)) => FileText {
            too_large: true,
            exists: true,
            ..Default::default()
        },
        _ => FileText {
            lfs_missing: Some(lfs::not_downloaded(&p)),
            exists: true,
            ..Default::default()
        },
    }
}

/// Media bytes, with an LFS pointer swapped for its object.
pub(super) fn lfs_media(repo: &Path, bytes: Vec<u8>) -> Result<Vec<u8>, String> {
    let Some(p) = lfs::pointer(&bytes) else {
        return Ok(bytes);
    };
    let object = lfs::object(repo, &p).ok_or_else(|| lfs::not_downloaded(&p))?;
    if p.size > MAX_MEDIA_BYTES {
        return Err("File is too large to preview".into());
    }
    std::fs::read(object).map_err(|e| e.to_string())
}

/// A submodule on one side of a diff. Git stores only its commit (mode 160000), which
/// `cat-file blob` can't read, so it's shown the way `git diff` does: "Subproject commit <sha>".
/// `rev` "" is the index, None the worktree.
pub(super) fn gitlink(repo: &Path, rev: Option<&str>, path: &str) -> Option<FileText> {
    // "<mode> <oid> <stage>\t<path>"; also confirms `path` is a submodule of this repo
    // before the worktree side runs git inside it.
    let staged = || -> Option<String> {
        let out = run_text(repo, &["ls-files", "-s", "--", path]).ok()?;
        let mut f = out.split_whitespace();
        (f.next()? == "160000").then(|| f.next().map(str::to_string))?
    };
    let sha = match rev {
        Some("") => staged()?,
        Some(rev) => {
            // "<mode> <type> <oid>\t<path>"
            let out = run_text(repo, &["ls-tree", rev, "--", path]).ok()?;
            let mut f = out.split_whitespace();
            (f.next()? == "160000").then(|| f.nth(1).map(str::to_string))??
        }
        None => {
            staged()?;
            let dir = repo.join(path);
            // Not checked out (no `.git`): git would walk up and report this repo's HEAD.
            if !dir.join(".git").exists() {
                return None;
            }
            let head = run_text(&dir, &["rev-parse", "HEAD"]).ok()?;
            let dirty = run(&dir, &["status", "--porcelain", "--untracked-files=no"])
                .is_ok_and(|o| !o.is_empty());
            format!("{}{}", head.trim(), if dirty { "-dirty" } else { "" })
        }
    };
    Some(FileText {
        text: format!("Subproject commit {sha}\n"),
        exists: true,
        ..Default::default()
    })
}
