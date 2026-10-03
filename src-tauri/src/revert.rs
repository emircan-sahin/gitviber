//! A commit's change taken back in the working tree: a file's whole change, as a patch applied
//! backwards (patch::apply), or some of its lines, merged into what's on disk.

use crate::git::{self, validate_rev, FileText};
use crate::patch::Scratch;
use serde::Deserialize;
use std::collections::HashSet;
use std::path::Path;

/// What commit `sha` did to `paths`, to apply backwards to the working tree.
pub fn commit_change(repo: &Path, sha: &str, paths: &[String]) -> Result<Vec<u8>, String> {
    validate_rev(sha)?;
    let parent = git::parent_or_empty(repo, sha)?;
    let paths: Vec<&str> = paths.iter().map(String::as_str).collect();
    let args = [&["-M", "--full-index", &parent, sha, "--"], &paths[..]].concat();
    let out = crate::patch::diff(repo, None, &args)?;
    if out.is_empty() {
        return Err("That commit didn't change this file.".into());
    }
    Ok(out)
}

#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RevertLines {
    pub sha: String,
    pub path: String,
    /// The path before the commit, when it renamed the file.
    pub old_path: Option<String>,
    /// The chosen lines of the commit's diff: removed ones by their line in the parent's
    /// version, added ones in the commit's.
    pub removed: Vec<u32>,
    pub added: Vec<u32>,
}

/// Undoes some of commit `sha`'s changed lines in the working tree's file: the commit's version
/// without them, merged three ways into what's on disk (`git merge-file`), so later edits stay.
/// Returns the file when that left conflict markers in it.
pub fn lines(repo: &Path, req: &RevertLines) -> Result<Vec<String>, String> {
    validate_rev(&req.sha)?;
    let pair = git::diff_pair(
        repo,
        "commit",
        &req.path,
        req.old_path.as_deref(),
        Some(&req.sha),
        None,
        None,
        |_| FileText::default(),
    )?;
    let disk = crate::fs::read_diff_side(repo, &req.path);
    let textual = |f: &FileText| !f.binary && !f.too_large && !f.lossy && f.lfs_missing.is_none();
    if !textual(&pair.original) || !textual(&pair.modified) || !textual(&disk) {
        return Err("Only text files can be reverted by line.".into());
    }
    if !disk.exists {
        return Err(format!("{} isn't in the working tree.", req.path));
    }
    let side = |f: &FileText| {
        if f.exists {
            f.text.clone()
        } else {
            String::new()
        }
    };
    let (before, after) = (side(&pair.original), side(&pair.modified));
    let removed: HashSet<u32> = req.removed.iter().copied().collect();
    let added: HashSet<u32> = req.added.iter().copied().collect();
    let reverted = crate::lines::apply(&after, &before, &added, &removed);
    if reverted == after {
        return Err("No changed lines are selected.".into());
    }
    let full = crate::fs::resolve(repo, &req.path)?;
    let (merged, conflicts) = if disk.text == after {
        (reverted.into_bytes(), false)
    } else {
        let base = Scratch::file(repo, after.as_bytes())?;
        let theirs = Scratch::file(repo, reverted.as_bytes())?;
        let args = [
            "merge-file",
            "-p",
            "-L",
            &req.path,
            "-L",
            crate::state::short(&req.sha),
            "-L",
            "reverted",
            full.to_str().ok_or("path isn't UTF-8")?,
            base.path()?,
            theirs.path()?,
        ];
        // Exits with the number of conflicts (at most 127), each marked with our label.
        let out = git::run_with(repo, &args, &(1..=127).collect::<Vec<_>>(), None)?;
        let marker = format!("<<<<<<< {}", req.path).into_bytes();
        let conflicts = out.windows(marker.len()).any(|w| w == marker);
        (out, conflicts)
    };
    std::fs::write(full, merged).map_err(|e| e.to_string())?;
    Ok(if conflicts {
        vec![req.path.clone()]
    } else {
        vec![]
    })
}
