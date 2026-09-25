//! The two sides of a file's diff, as text or as media bytes.

use super::{blob, gitlink, lfs_media, lfs_text, run_text, smudged, validate_rev, FileText};
use serde::Serialize;
use std::path::Path;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffPair {
    pub original: FileText,
    pub modified: FileText,
    pub rows: Vec<crate::diff::Row>,
    /// The ignored whitespace hid changed lines.
    pub whitespace_hidden: bool,
    /// Every changed line differs only in its line ending.
    pub eol_only: bool,
}

/// Where each side of a diff lives: a git revision ("" = the index), or None for the worktree.
fn sides(
    kind: &str,
    sha: Option<&str>,
    base: Option<&str>,
) -> Result<(Option<String>, Option<String>), String> {
    let rev = |r: &str| Some(r.to_string());
    Ok(match kind {
        "unstaged" => (rev(""), None),
        "staged" => (rev("HEAD"), rev("")),
        "worktree" => (rev("HEAD"), None),
        "commit" => {
            let sha = sha.ok_or("missing commit")?;
            validate_rev(sha)?;
            (Some(format!("{sha}^")), rev(sha))
        }
        "range" => {
            let (Some(base), Some(sha)) = (base, sha) else {
                return Err("missing range".into());
            };
            validate_rev(base)?;
            validate_rev(sha)?;
            (rev(base), rev(sha))
        }
        // A branch review: its merge base against the working tree (see `branch_review`).
        "base" => {
            let base = base.ok_or("missing base")?;
            validate_rev(base)?;
            (rev(base), None)
        }
        other => return Err(format!("unknown diff kind: {other}")),
    })
}

/// Media previews load whole files into the webview; past this they are refused.
pub const MAX_MEDIA_BYTES: u64 = 512 * 1024 * 1024;

/// Raw bytes of one side of a diff, for image / audio / video previews.
#[allow(clippy::too_many_arguments)]
pub fn media(
    repo: &Path,
    kind: &str,
    path: &str,
    old_path: Option<&str>,
    sha: Option<&str>,
    base: Option<&str>,
    original: bool,
    worktree: impl Fn(&str) -> Result<Vec<u8>, String>,
) -> Result<Vec<u8>, String> {
    let (a, b) = sides(kind, sha, base)?;
    let (rev, path) = if original {
        (a, old_path.unwrap_or(path))
    } else {
        (b, path)
    };
    let Some(rev) = rev else {
        return lfs_media(repo, worktree(path)?);
    };
    let spec = format!("{rev}:{path}");
    let size: u64 = run_text(repo, &["cat-file", "-s", &spec])?
        .trim()
        .parse()
        .map_err(|_| "bad blob size")?;
    if size > MAX_MEDIA_BYTES {
        return Err("File is too large to preview".into());
    }
    lfs_media(repo, smudged(repo, &spec)?)
}

/// `kind`: "unstaged" (index → worktree), "staged" (HEAD → index), "worktree" (HEAD → worktree),
/// "commit" (parent → commit), "range" (base → sha, e.g. a pull request) or "base" (base → worktree).
/// `whitespace`: "all" or "amount" to ignore those changes (see `diff::whitespace_mode`).
#[allow(clippy::too_many_arguments)]
pub fn diff_pair(
    repo: &Path,
    kind: &str,
    path: &str,
    old_path: Option<&str>,
    sha: Option<&str>,
    base: Option<&str>,
    whitespace: Option<&str>,
    worktree: impl Fn(&str) -> FileText,
) -> Result<DiffPair, String> {
    let (a, b) = sides(kind, sha, base)?;
    let read = |rev: Option<String>, p: &str| {
        let f = match &rev {
            Some(rev) => blob(repo, rev, p),
            None => worktree(p),
        };
        if f.exists {
            lfs_text(repo, f)
        } else {
            gitlink(repo, rev.as_deref(), p).unwrap_or(f)
        }
    };
    let original = read(a, old_path.unwrap_or(path));
    let modified = read(b, path);
    let textual = |f: &FileText| !f.binary && !f.too_large && f.lfs_missing.is_none();
    let (rows, whitespace_hidden) = if textual(&original) && textual(&modified) {
        let ws = crate::diff::whitespace_mode(whitespace);
        crate::diff::rows(&original.text, &modified.text, ws)
    } else {
        (vec![], false)
    };
    let eol_only =
        rows.iter().any(|r| r.k != 0) && crate::diff::eol_only(&original.text, &modified.text);
    Ok(DiffPair {
        original,
        modified,
        rows,
        whitespace_hidden,
        eol_only,
    })
}
