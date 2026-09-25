//! Staging, unstaging and discarding whole files.

use super::{has_head, new_gitlink, run, run_text, untracked_nested_root};
use std::path::Path;

pub(super) fn with_paths<'a>(mut args: Vec<&'a str>, paths: &'a [String]) -> Vec<&'a str> {
    args.push("--");
    args.extend(paths.iter().map(String::as_str));
    args
}

pub fn stage(repo: &Path, paths: &[String]) -> Result<(), String> {
    stage_with(repo, paths, false)
}

/// Refuses untracked nested repositories (an agent's worktree) unless `allow_nested`: git
/// would stage one as a gitlink, a pointer to its current commit, and none of its files.
pub fn stage_with(repo: &Path, paths: &[String], allow_nested: bool) -> Result<(), String> {
    // `git add -A --` with no paths stages the whole tree, nested repos included.
    if paths.is_empty() {
        return Ok(());
    }
    if !allow_nested {
        if let Some(p) = nested_repos(repo, paths)?.first() {
            return Err(format!(
                "{p} is a separate git repository (a worktree or nested repo), so it was not staged. \
                 git would record only a pointer to its current commit, not its files. \
                 Commit inside it instead."
            ));
        }
    }
    run(repo, &with_paths(vec!["add", "-A"], paths)).map(|_| ())
}

/// Nested repositories at or under `paths` that `git add` would turn into new gitlinks (or
/// whose files it would take as ours). Plain files outside any nested repo skip the status call.
fn nested_repos(repo: &Path, paths: &[String]) -> Result<Vec<String>, String> {
    if !paths
        .iter()
        .any(|p| repo.join(p).is_dir() || untracked_nested_root(repo, p).is_some())
    {
        return Ok(vec![]);
    }
    let args = with_paths(
        vec!["status", "--porcelain=v2", "-z", "--untracked-files=all"],
        paths,
    );
    let raw = run_text(repo, &args)?;
    let mut found = vec![];
    let mut records = raw.split('\0');
    while let Some(rec) = records.next() {
        if let Some(p) = rec.strip_prefix("? ") {
            found.extend(untracked_nested_root(repo, p));
        } else if let Some(kind @ ('1' | '2')) = rec.chars().next() {
            let fields: Vec<&str> = rec.splitn(if kind == '1' { 9 } else { 10 }, ' ').collect();
            if kind == '2' {
                records.next();
            }
            if new_gitlink(&fields) {
                found.extend(fields.last().map(|p| p.to_string()));
            }
        }
    }
    Ok(found)
}

pub fn unstage(repo: &Path, paths: &[String]) -> Result<(), String> {
    let base = if has_head(repo) {
        vec!["restore", "--staged"]
    } else {
        vec!["rm", "--cached", "-q", "-r"]
    };
    run(repo, &with_paths(base, paths)).map(|_| ())
}

/// Reverts tracked files in the worktree to their index version. Untracked files are left alone.
pub fn discard(repo: &Path, paths: &[String]) -> Result<(), String> {
    run(repo, &with_paths(vec!["restore", "--worktree"], paths)).map(|_| ())
}
