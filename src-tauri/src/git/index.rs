//! Staging, unstaging and discarding whole files.

use super::{has_head, new_gitlink, run_text, run_with, untracked_nested_root};
use std::collections::HashSet;
use std::path::Path;

pub(super) fn with_paths<'a>(mut args: Vec<&'a str>, paths: &'a [String]) -> Vec<&'a str> {
    args.push("--");
    args.extend(paths.iter().map(String::as_str));
    args
}

/// Runs git on `paths` read from stdin: on argv, some 16k paths (an agent's unignored
/// node_modules) fail with "Argument list too long". GIT_LITERAL_PATHSPECS applies here too.
fn run_on(repo: &Path, args: &[&str], paths: &[String]) -> Result<(), String> {
    // An empty pathspec file would mean the whole tree.
    if paths.is_empty() {
        return Ok(());
    }
    let mut args = args.to_vec();
    args.extend(["--pathspec-from-file=-", "--pathspec-file-nul"]);
    run_with(repo, &args, &[], Some(paths.join("\0").as_bytes())).map(|_| ())
}

pub fn stage(repo: &Path, paths: &[String]) -> Result<(), String> {
    stage_with(repo, paths, false)
}

/// Refuses untracked nested repositories (an agent's worktree) unless `allow_nested`: git
/// would stage one as a gitlink, a pointer to its current commit, and none of its files.
pub fn stage_with(repo: &Path, paths: &[String], allow_nested: bool) -> Result<(), String> {
    if !allow_nested {
        if let Some(p) = nested_repos(repo, paths)?.first() {
            return Err(format!(
                "{p} is a separate git repository (a worktree or nested repo), so it was not staged. \
                 git would record only a pointer to its current commit, not its files. \
                 Commit inside it instead."
            ));
        }
    }
    run_on(repo, &["add", "-A"], paths)
}

/// Nested repositories at or under `paths` that `git add` would turn into new gitlinks (or
/// whose files it would take as ours). Plain files outside any nested repo skip the status call,
/// which runs on the whole tree: `status` takes no pathspec file, and argv can't hold them all.
fn nested_repos(repo: &Path, paths: &[String]) -> Result<Vec<String>, String> {
    if !paths
        .iter()
        .any(|p| repo.join(p).is_dir() || untracked_nested_root(repo, p).is_some())
    {
        return Ok(vec![]);
    }
    let wanted: HashSet<&str> = paths.iter().map(|p| p.trim_end_matches('/')).collect();
    let asked = |p: &str| {
        wanted.contains(".")
            || Path::new(p.trim_end_matches('/'))
                .ancestors()
                .any(|a| a.to_str().is_some_and(|a| wanted.contains(a)))
    };
    let args = ["status", "--porcelain=v2", "-z", "--untracked-files=all"];
    let raw = run_text(repo, &args)?;
    let mut found = vec![];
    let mut records = raw.split('\0');
    while let Some(rec) = records.next() {
        if let Some(p) = rec.strip_prefix("? ") {
            if asked(p) {
                found.extend(untracked_nested_root(repo, p));
            }
        } else if let Some(kind @ ('1' | '2')) = rec.chars().next() {
            let fields: Vec<&str> = rec.splitn(if kind == '1' { 9 } else { 10 }, ' ').collect();
            if kind == '2' {
                records.next();
            }
            if new_gitlink(&fields) {
                found.extend(fields.last().filter(|p| asked(p)).map(|p| p.to_string()));
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
    run_on(repo, &base, paths)
}

/// Reverts tracked files in the worktree to their index version. Untracked files are left alone.
pub fn discard(repo: &Path, paths: &[String]) -> Result<(), String> {
    run_on(repo, &["restore", "--worktree"], paths)
}
