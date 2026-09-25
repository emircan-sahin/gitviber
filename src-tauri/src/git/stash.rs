//! The stash: saving, applying, branching from and dropping entries.

use super::{
    command, commit_files, ensure_idle, run, run_text, stoppable, validate_branch, validate_rev,
    FileChange,
};
use crate::process::exec;
use serde::Serialize;
use std::path::Path;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stash {
    pub sha: String,
    /// Its n in stash@{n} now. Pushes and drops shift it, so actions name the stash by `sha`.
    pub index: usize,
    /// As git words it: "On main: message", or "WIP on main: <commit>" without one.
    pub message: String,
    pub author: String,
    pub timestamp: i64,
}

pub fn stashes(repo: &Path) -> Result<Vec<Stash>, String> {
    let out = run_text(repo, &["stash", "list", "--format=%H%x1f%an%x1f%ct%x1f%gs"])?;
    Ok(out
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.splitn(4, '\x1f').collect();
            (f.len() == 4).then(|| (f[0], f[1], f[2], f[3]))
        })
        .enumerate()
        .map(|(index, (sha, author, time, message))| Stash {
            sha: sha.to_string(),
            index,
            message: message.to_string(),
            author: author.to_string(),
            timestamp: time.parse().unwrap_or(0),
        })
        .collect())
}

/// stash@{n} for the stash that is commit `sha`, wherever it sits in the list now.
fn stash_ref(repo: &Path, sha: &str) -> Result<String, String> {
    validate_rev(sha)?;
    stashes(repo)?
        .iter()
        .find(|s| s.sha == sha)
        .map(|s| format!("stash@{{{}}}", s.index))
        .ok_or_else(|| "That stash is gone (dropped or popped elsewhere).".into())
}

/// What a stash takes: `untracked` files too, or only what's `staged`; with `paths`, only those
/// files. Nested repositories stay (git skips them).
pub struct StashWhat<'a> {
    pub untracked: bool,
    pub staged: bool,
    pub paths: &'a [String],
}

/// Stashes local changes (see `StashWhat`).
pub fn stash_push(repo: &Path, message: &str, what: StashWhat) -> Result<(), String> {
    let top = || run_text(repo, &["rev-parse", "-q", "--verify", "refs/stash"]).ok();
    let before = top();
    let mut args = vec!["stash".to_string(), "push".into()];
    if what.staged {
        args.push("--staged".into());
    } else if what.untracked {
        args.push("--include-untracked".into());
    }
    let message = message.trim();
    if !message.is_empty() {
        args.extend(["-m".into(), message.into()]);
    }
    // Literal pathspecs break the cleanup of stashed untracked files (they would be saved and
    // still left in place), so they're off, and each path says it's literal itself.
    if !what.paths.is_empty() {
        args.push("--".into());
        args.extend(what.paths.iter().map(|p| format!(":(literal){p}")));
    }
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let mut cmd = command(repo, &args);
    cmd.env("GIT_LITERAL_PATHSPECS", "0");
    exec(cmd, "git stash", &[], None, None)?;
    // With nothing to save git says so on stdout and still succeeds.
    if top() == before {
        return Err("There are no local changes to stash.".into());
    }
    Ok(())
}

/// Applies a stash, and with `pop` drops it once applied cleanly. On conflicts git keeps it
/// and returns true: they're resolved like a merge's, then the stash can be dropped.
pub fn stash_apply(repo: &Path, sha: &str, pop: bool) -> Result<bool, String> {
    ensure_idle(repo)?;
    let r = stash_ref(repo, sha)?;
    stoppable(
        repo,
        run(repo, &["stash", if pop { "pop" } else { "apply" }, &r]),
    )
}

/// A new branch where the stash was made, with it applied, and the stash dropped: its changes
/// back without clashing with what came since. True when applying stopped on conflicts.
pub fn stash_branch(repo: &Path, name: &str, sha: &str) -> Result<bool, String> {
    ensure_idle(repo)?;
    validate_branch(repo, name)?;
    let r = stash_ref(repo, sha)?;
    stoppable(repo, run(repo, &["stash", "branch", name, &r]))
}

pub fn stash_drop(repo: &Path, sha: &str) -> Result<(), String> {
    let r = stash_ref(repo, sha)?;
    run(repo, &["stash", "drop", &r]).map(|_| ())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StashFiles {
    /// Tracked changes, against the commit it was made on (its first parent).
    pub files: Vec<FileChange>,
    /// The root commit `-u` keeps untracked files in (its third parent); diffed from nothing.
    pub untracked_sha: Option<String>,
    pub untracked: Vec<FileChange>,
}

pub fn stash_files(repo: &Path, sha: &str) -> Result<StashFiles, String> {
    let files = commit_files(repo, sha)?;
    let third = format!("{sha}^3");
    let untracked_sha = run_text(repo, &["rev-parse", "--verify", "-q", &third])
        .ok()
        .map(|s| s.trim().to_string());
    let mut untracked = match &untracked_sha {
        Some(u) => commit_files(repo, u)?,
        None => vec![],
    };
    for f in &mut untracked {
        f.status = "?".into();
    }
    Ok(StashFiles {
        files,
        untracked_sha,
        untracked,
    })
}
