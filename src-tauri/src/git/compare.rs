//! The Compare screen: any two points of history (branch, remote branch, tag, commit) against
//! each other, and what merging one into HEAD would do.

use super::{
    has_head, range_files, run, run_text, run_with, validate_full_ref, validate_rev, FileChange,
    REF_KINDS,
};
use serde::Serialize;
use std::path::Path;

/// Two points compared: the commits each has the other lacks, and the files between them.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Comparison {
    /// The commit ids the two points name.
    pub base: String,
    pub head: String,
    /// Where the files' diff starts (each file's old side): the merge base, or `base` itself.
    pub from: String,
    /// No commit in common: a merge base diff has nowhere to start, so `from` is `base`.
    pub unrelated: bool,
    /// Commits `head` has that `base` doesn't.
    pub ahead: u32,
    /// Commits `base` has that `head` doesn't.
    pub behind: u32,
    pub files: Vec<FileChange>,
}

/// What merging a point into HEAD brings, found without touching the working tree.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MergeCheck {
    /// Commits it has that HEAD doesn't; none: nothing to merge.
    pub incoming: u32,
    /// No commit in common: git won't merge them.
    pub unrelated: bool,
    /// The files that would conflict; none: git is too old to check (merge-tree --write-tree, 2.38).
    pub conflicts: Option<Vec<String>>,
}

/// The commit a point names: HEAD, a full ref (refs/heads/…, refs/remotes/…, refs/tags/…) or a
/// commit id. Never an option, a range or an expression.
fn commit_of(repo: &Path, point: &str) -> Result<String, String> {
    if point.starts_with("refs/") {
        validate_full_ref(repo, point)?;
    } else if point != "HEAD" {
        validate_rev(point)?;
    }
    run_text(
        repo,
        &[
            "rev-parse",
            "--verify",
            "-q",
            &format!("{point}^{{commit}}"),
        ],
    )
    .map(|s| s.trim().to_string())
    .map_err(|_| {
        let name = REF_KINDS
            .iter()
            .find_map(|(prefix, _)| point.strip_prefix(prefix))
            .unwrap_or(point);
        format!("{name} doesn't exist here. Fetch, or pick another branch.")
    })
}

/// `head` against `base`. `merge_base`: the files are what `head` changed since the two parted
/// (a pull request's), not everything that differs, which also undoes what `base` gained since.
pub fn compare(
    repo: &Path,
    base: &str,
    head: &str,
    merge_base: bool,
) -> Result<Comparison, String> {
    let base = commit_of(repo, base)?;
    let head = commit_of(repo, head)?;
    let counts = run_text(
        repo,
        &[
            "rev-list",
            "--left-right",
            "--count",
            &format!("{base}...{head}"),
        ],
    )?;
    let mut n = counts.split_whitespace().map(|x| x.parse().unwrap_or(0));
    let (behind, ahead) = (n.next().unwrap_or(0), n.next().unwrap_or(0));
    let common = run_text(repo, &["merge-base", &base, &head])
        .ok()
        .map(|s| s.trim().to_string());
    let unrelated = common.is_none();
    let from = common
        .filter(|_| merge_base)
        .unwrap_or_else(|| base.clone());
    let files = range_files(repo, &from, &head)?;
    Ok(Comparison {
        base,
        head,
        from,
        unrelated,
        ahead,
        behind,
        files,
    })
}

pub fn merge_check(repo: &Path, with: &str) -> Result<MergeCheck, String> {
    let with = commit_of(repo, with)?;
    if !has_head(repo) {
        return Err("There are no commits yet.".into());
    }
    let counted = run_text(repo, &["rev-list", "--count", &format!("HEAD..{with}")])?;
    let incoming = counted.trim().parse().unwrap_or(0);
    let unrelated = run(repo, &["merge-base", "HEAD", &with]).is_err();
    if incoming == 0 || unrelated {
        return Ok(MergeCheck {
            incoming,
            unrelated,
            conflicts: Some(vec![]),
        });
    }
    // Exit 1: it conflicts. Either way the first entry is the merged tree, then the conflicted files.
    let out = run_with(
        repo,
        &[
            "merge-tree",
            "--write-tree",
            "--name-only",
            "--no-messages",
            "-z",
            "HEAD",
            &with,
        ],
        &[1],
        None,
    );
    let conflicts = out.ok().map(|raw| {
        raw.split(|b| *b == 0)
            .skip(1)
            .filter(|p| !p.is_empty())
            .map(|p| String::from_utf8_lossy(p).into_owned())
            .collect()
    });
    Ok(MergeCheck {
        incoming,
        unrelated,
        conflicts,
    })
}
