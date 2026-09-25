//! Files of a commit or tree: paths, changed files, text at a revision.

use super::{
    apply_numstat, blob, change, parse_numstat, run, run_text, validate_rev, validate_tree_rev,
    FileChange, FileText,
};
use std::path::Path;

/// Every file in a commit's tree (see `validate_tree_rev`), for the code view's links.
pub fn tree_paths(repo: &Path, rev: &str) -> Result<Vec<String>, String> {
    validate_tree_rev(rev)?;
    let out = run(
        repo,
        &["ls-tree", "-r", "-z", "--full-tree", "--name-only", rev],
    )?;
    Ok(out
        .split(|&b| b == 0)
        .filter(|p| !p.is_empty())
        .map(|p| String::from_utf8_lossy(p).into_owned())
        .collect())
}

/// A file as it is in a commit's tree (see `validate_tree_rev`); a missing one doesn't exist.
pub fn text_at(repo: &Path, rev: &str, path: &str) -> Result<FileText, String> {
    validate_tree_rev(rev)?;
    Ok(blob(repo, rev, path))
}

/// Files changed by a commit, compared with its first parent (so merges show what they brought in).
pub fn commit_files(repo: &Path, sha: &str) -> Result<Vec<FileChange>, String> {
    validate_rev(sha)?;
    let parent = format!("{sha}^");
    let has_parent = run(repo, &["rev-parse", "--verify", "-q", &parent]).is_ok();
    if has_parent {
        range_files(repo, &parent, sha)
    } else {
        tree_files(repo, &["--root", sha])
    }
}

/// Files changed between two commits (e.g. a PR's merge base and its head).
pub fn range_files(repo: &Path, from: &str, to: &str) -> Result<Vec<FileChange>, String> {
    tree_files(repo, &[from, to])
}

fn tree_files(repo: &Path, range: &[&str]) -> Result<Vec<FileChange>, String> {
    let mut args = vec![
        "diff-tree",
        "-r",
        "-z",
        "-M",
        "--no-commit-id",
        "--name-status",
    ];
    args.extend(range);
    let mut files = parse_name_status(&run(repo, &args)?);
    let mut args = vec!["diff-tree", "-r", "-z", "-M", "--no-commit-id", "--numstat"];
    args.extend(range);
    apply_numstat(&mut files, &parse_numstat(&run(repo, &args)?));
    Ok(files)
}

/// Parses `--name-status -z`; a rename or copy carries its old path.
pub(super) fn parse_name_status(raw: &[u8]) -> Vec<FileChange> {
    let mut files = vec![];
    let mut tokens = raw
        .split(|b| *b == 0)
        .map(|t| String::from_utf8_lossy(t).into_owned());
    while let Some(code) = tokens.next() {
        let Some(letter) = code.chars().next() else {
            continue;
        };
        if letter == 'R' || letter == 'C' {
            let (Some(old), Some(new)) = (tokens.next(), tokens.next()) else {
                break;
            };
            files.push(change(&new, Some(&old), letter));
        } else if let Some(path) = tokens.next() {
            files.push(change(&path, None, letter));
        }
    }
    files
}

pub fn merge_base(repo: &Path, a: &str, b: &str) -> Result<String, String> {
    validate_rev(a)?;
    validate_rev(b)?;
    run_text(repo, &["merge-base", a, b]).map(|s| s.trim().to_string())
}
