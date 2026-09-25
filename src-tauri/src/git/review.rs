//! Reviewing a branch: everything it changed since it left its base, uncommitted work included.

use super::{
    apply_numstat, change, count_lines, disk_oid, parse_name_status, parse_numstat, parted_at, run,
    untracked_nested_root, CountBudget, FileChange,
};
use serde::Serialize;
use std::collections::HashSet;
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
    let merge_base = parted_at(repo, base)?;
    let diff = |format: &str| run(repo, &["diff", "-z", "-M", format, &merge_base, "--"]);
    let mut files = parse_name_status(&diff("--name-status")?);
    apply_numstat(&mut files, &parse_numstat(&diff("--numstat")?));

    let raw = run(repo, &["ls-files", "-z", "--others", "--exclude-standard"])?;
    let untracked: HashSet<String> = raw
        .split(|b| *b == 0)
        .filter(|p| !p.is_empty())
        .map(|p| String::from_utf8_lossy(p).into_owned())
        // Another repository (a worktree kept inside this one) has no diff to review.
        .filter(|p| untracked_nested_root(repo, p).is_none())
        .collect();
    // Untracked again after its deletion was staged or committed (`git rm --cached`): the file
    // is on disk, so it's one row, the untracked one, not a deletion too.
    files.retain(|f| f.status != "D" || !untracked.contains(&f.path));
    let mut budget = CountBudget::default();
    for path in untracked {
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
