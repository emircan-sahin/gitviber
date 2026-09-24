//! Staging, unstaging and discarding some of a file's changed lines. The diff the view showed
//! says which lines are which; the file's new version (in the index, or on disk for a discard)
//! is worked out from the two texts and written whole, so there is no patch for git to reject.

use crate::git::{self, FileText};
use serde::Deserialize;
use similar::{Algorithm, ChangeTag, TextDiff};
use std::collections::HashSet;
use std::path::Path;
use std::time::Duration;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    pub path: String,
    /// The diff shown: "unstaged" (index → worktree) or "staged" (HEAD → index).
    pub kind: String,
    /// "stage" or "discard" an unstaged change, "unstage" a staged one.
    pub action: String,
    /// The two sides as shown (None: the file isn't there), so one that changed since isn't
    /// edited blind.
    pub original: Option<String>,
    pub modified: Option<String>,
    /// The chosen lines: removed ones by their line in `original`, added ones in `modified`.
    pub removed: Vec<u32>,
    pub added: Vec<u32>,
}

/// `base` with some of the changes that make it `target`: its lines in `drop` go, and `target`'s
/// lines in `take` come in where the diff has them. Lines keep their own endings.
pub fn apply(base: &str, target: &str, drop: &HashSet<u32>, take: &HashSet<u32>) -> String {
    // As diff::rows, which numbered the lines the view showed.
    let diff = TextDiff::configure()
        .algorithm(Algorithm::Patience)
        .timeout(Duration::from_secs(2))
        .diff_lines(base, target);
    let newline = if base.contains("\r\n") || target.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut out = String::with_capacity(base.len().max(target.len()));
    for change in diff.iter_all_changes() {
        let keep = match change.tag() {
            ChangeTag::Equal => true,
            ChangeTag::Delete => !change
                .old_index()
                .is_some_and(|i| drop.contains(&(i as u32 + 1))),
            ChangeTag::Insert => change
                .new_index()
                .is_some_and(|i| take.contains(&(i as u32 + 1))),
        };
        if keep {
            // A last line without a newline that is no longer last gets one.
            if !out.is_empty() && !out.ends_with('\n') {
                out.push_str(newline);
            }
            out.push_str(change.value());
        }
    }
    out
}

pub fn run(repo: &Path, req: &Request) -> Result<(), String> {
    let action = req.action.as_str();
    match (req.kind.as_str(), action) {
        ("unstaged", "stage" | "discard") | ("staged", "unstage") => {}
        _ => return Err(format!("can't {action} lines of a {} diff", req.kind)),
    }
    let now = git::diff_pair(repo, &req.kind, &req.path, None, None, None, None, |p| {
        crate::fs::read_file(repo, p)
    })?;
    for (shown, live) in [
        (&req.original, &now.original),
        (&req.modified, &now.modified),
    ] {
        if live.binary || live.too_large || live.lossy || live.lfs_missing.is_some() {
            return Err("Only text files can be staged by line.".into());
        }
        if shown.as_deref() != live.exists.then_some(live.text.as_str()) {
            return Err(format!(
                "{} changed since it was shown. Try again.",
                req.path
            ));
        }
    }
    let removed: HashSet<u32> = req.removed.iter().copied().collect();
    let added: HashSet<u32> = req.added.iter().copied().collect();
    let (old, new) = (text(&now.original), text(&now.modified));
    match action {
        // The index gets the chosen changes.
        "stage" => {
            let result = apply(old, new, &removed, &added);
            let deletion = !now.modified.exists && result.is_empty();
            write_index(repo, &req.path, (!deletion).then_some(&result))
        }
        // The index loses them: back toward HEAD.
        "unstage" => {
            let result = apply(new, old, &added, &removed);
            let unadded = !now.original.exists && result.is_empty();
            write_index(repo, &req.path, (!unadded).then_some(&result))
        }
        // The file on disk loses them, back toward the index.
        _ => {
            let result = apply(new, old, &added, &removed);
            let path = crate::fs::resolve(repo, &req.path)?;
            if !now.original.exists && result.is_empty() {
                return Err("Discard the whole file to remove it.".into());
            }
            std::fs::write(&path, result).map_err(|e| e.to_string())?;
            if !now.modified.exists {
                // Recreated: with the index's mode.
                set_mode(&path, &index_mode(repo, &req.path));
            }
            Ok(())
        }
    }
}

fn text(f: &FileText) -> &str {
    if f.exists {
        &f.text
    } else {
        ""
    }
}

/// Puts `text` in the index as `path`'s content (through its clean filters, as `git add` would),
/// or with None takes `path` out of the index.
fn write_index(repo: &Path, path: &str, text: Option<&str>) -> Result<(), String> {
    let Some(text) = text else {
        git::run(repo, &["update-index", "--force-remove", "--", path])?;
        return Ok(());
    };
    let oid = git::run_with(
        repo,
        &["hash-object", "-w", "--path", path, "--stdin"],
        &[],
        Some(text.as_bytes()),
    )?;
    let oid = String::from_utf8_lossy(&oid).trim().to_string();
    let mode = index_mode(repo, path);
    let info = format!("{mode},{oid},{path}");
    git::run(repo, &["update-index", "--add", "--cacheinfo", &info])?;
    Ok(())
}

/// The mode `path` has in the index, else in HEAD, else on disk (a new file).
fn index_mode(repo: &Path, path: &str) -> String {
    let first = |args: &[&str]| {
        git::run_text(repo, args)
            .ok()
            .and_then(|out| out.split_whitespace().next().map(str::to_string))
    };
    first(&["ls-files", "-s", "--", path])
        .or_else(|| first(&["ls-tree", "HEAD", "--", path]))
        .unwrap_or_else(|| disk_mode(&repo.join(path)))
}

#[cfg(unix)]
fn disk_mode(path: &Path) -> String {
    use std::os::unix::fs::PermissionsExt;
    let exec = std::fs::metadata(path).is_ok_and(|m| m.permissions().mode() & 0o111 != 0);
    if exec { "100755" } else { "100644" }.into()
}

#[cfg(not(unix))]
fn disk_mode(_: &Path) -> String {
    "100644".into()
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: &str) {
    use std::os::unix::fs::PermissionsExt;
    if mode == "100755" {
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755));
    }
}

#[cfg(not(unix))]
fn set_mode(_: &Path, _: &str) {}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(lines: &[u32]) -> HashSet<u32> {
        lines.iter().copied().collect()
    }

    #[test]
    fn takes_only_the_chosen_changes() {
        let old = "a\nb\nc\nd\n";
        let new = "a\nB\nc\nd\ne\n";
        // b→B is removed line 2 and added line 2; e is added line 5.
        assert_eq!(apply(old, new, &set(&[2]), &set(&[2])), "a\nB\nc\nd\n");
        assert_eq!(apply(old, new, &set(&[]), &set(&[5])), "a\nb\nc\nd\ne\n");
        assert_eq!(apply(old, new, &set(&[2]), &set(&[])), "a\nc\nd\n");
        assert_eq!(apply(old, new, &set(&[2]), &set(&[2, 5])), new);
        assert_eq!(apply(old, new, &set(&[]), &set(&[])), old);
    }

    #[test]
    fn keeps_line_endings_and_a_missing_last_newline() {
        let old = "a\r\nb\r\n";
        let new = "a\r\nx\r\nb\r\n";
        assert_eq!(apply(old, new, &set(&[]), &set(&[2])), new);
        // No newline at the end, and a line added after: the old last line gets one.
        let old = "a\nb";
        let new = "a\nb\nc\n";
        assert_eq!(apply(old, new, &set(&[]), &set(&[3])), "a\nb\nc\n");
        // Only the newline itself: b (no newline) becomes b\n.
        assert_eq!(apply(old, new, &set(&[2]), &set(&[2])), "a\nb\n");
        assert_eq!(apply("a\n", "a\nb", &set(&[]), &set(&[2])), "a\nb");
    }

    #[test]
    fn works_backward_for_unstage_and_discard() {
        // HEAD → index added x and y; unstaging y keeps x.
        let head = "a\nb\n";
        let index = "a\nx\ny\nb\n";
        assert_eq!(apply(index, head, &set(&[3]), &set(&[])), "a\nx\nb\n");
        // HEAD had c, the index removed it; unstaging brings it back.
        let head = "a\nc\nb\n";
        let index = "a\nb\n";
        assert_eq!(apply(index, head, &set(&[]), &set(&[2])), head);
    }
}
