//! Reviewing a branch: everything it changed since it left its base, uncommitted work included.

use super::{
    apply_numstat, change, count_lines, disk_oid, has_head, parse_name_status, parse_numstat, run,
    run_text, untracked_nested_root, validate_full_ref, CountBudget, FileChange,
};
use serde::Serialize;
use std::path::Path;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchReview {
    /// The merge base: each file's old side (`diff_pair` kind "base").
    pub base: String,
    pub files: Vec<FileChange>,
}

/// What HEAD's branch changed since it parted from `base` (a full ref): its commits, staged and
/// unstaged edits and untracked files, as one diff from the merge base to the working tree.
/// From the merge base, so what `base` gained since doesn't show up here as undone.
pub fn branch_review(repo: &Path, base: &str) -> Result<BranchReview, String> {
    validate_full_ref(repo, base)?;
    if !has_head(repo) {
        return Err("There are no commits yet.".into());
    }
    let merge_base = run_text(repo, &["merge-base", "HEAD", base])
        .map_err(|_| "They have no commit in common.".to_string())?
        .trim()
        .to_string();
    let diff = |format: &str| run(repo, &["diff", "-z", "-M", format, &merge_base, "--"]);
    let mut files = parse_name_status(&diff("--name-status")?);
    apply_numstat(&mut files, &parse_numstat(&diff("--numstat")?));

    let untracked = run(repo, &["ls-files", "-z", "--others", "--exclude-standard"])?;
    let mut budget = CountBudget::default();
    for raw in untracked.split(|b| *b == 0).filter(|p| !p.is_empty()) {
        let path = String::from_utf8_lossy(raw);
        // Another repository (a worktree kept inside this one) has no diff to review.
        if untracked_nested_root(repo, &path).is_some() {
            continue;
        }
        let mut f = change(&path, None, '?');
        if let Some(n) = count_lines(repo, &path, &mut budget) {
            f.additions = n;
            f.deletions = Some(0);
        }
        files.push(f);
    }
    // The new side is always the working tree, so its size and mtime tell a new edit apart.
    for f in &mut files {
        f.oid = disk_oid(repo, &f.path);
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(BranchReview {
        base: merge_base,
        files,
    })
}
