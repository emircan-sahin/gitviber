//! The stash: saving, applying, branching from and dropping entries.

use super::{
    command, commit_files, ensure_idle, run, run_text, run_with, stoppable, validate_branch,
    validate_rev, FileChange,
};
use crate::lines::StashPart;
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

/// Gives the stash `sha` the message `message` and puts it on top, as lazygit's rename does (a
/// drop and a store), but storing first: the stash is never missing, whatever fails. It is
/// stored as a copy of its commit with the new message, since `git stash store` of a commit
/// that is already the top one changes nothing. A message without the "On branch:" prefix git
/// writes keeps the prefix the old one had.
pub fn stash_rename(repo: &Path, sha: &str, message: &str) -> Result<(), String> {
    let message = message.trim();
    if message.is_empty() || message.contains(['\n', '\r']) {
        return Err("A stash needs a one-line name.".into());
    }
    validate_rev(sha)?;
    let old = stashes(repo)?
        .into_iter()
        .find(|s| s.sha == sha)
        .ok_or("That stash is gone (dropped or popped elsewhere).")?;
    let full = match old.message.split_once(": ") {
        Some((prefix, _)) if prefix.starts_with("On ") || prefix.starts_with("WIP on ") => {
            format!(
                "On {}: {message}",
                prefix
                    .trim_start_matches("WIP on ")
                    .trim_start_matches("On ")
            )
        }
        _ => message.to_string(),
    };
    let tree = format!("{sha}^{{tree}}");
    let tree = run_text(repo, &["rev-parse", "--verify", &tree])?;
    let family = run_text(repo, &["rev-list", "--parents", "-n", "1", sha])?;
    let mut args = vec!["commit-tree", tree.trim()];
    for parent in family.split_whitespace().skip(1) {
        args.extend(["-p", parent]);
    }
    args.extend(["-m", &full]);
    let copy = run_text(repo, &args)?;
    run(repo, &["stash", "store", "-m", &full, copy.trim()])?;
    // Stashes pushed since moved it down, and one can land between looking and dropping. git's
    // "Dropped (<sha>)" names the stash it looked up before taking the lock, not the one it
    // dropped (a push in between lost the user's stash), so the list says what went, and one
    // that isn't ours is put back.
    for _ in 0..3 {
        let list = stashes(repo)?;
        let at = list
            .iter()
            .find(|s| s.sha == sha)
            .ok_or("The old stash went away while renaming; the renamed one is there.")?;
        run(repo, &["stash", "drop", &format!("stash@{{{}}}", at.index)])?;
        let after = stashes(repo)?;
        if !after.iter().any(|s| s.sha == sha) {
            return Ok(());
        }
        if let Some(wrong) = list.iter().find(|s| !after.iter().any(|a| a.sha == s.sha)) {
            run(repo, &["stash", "store", "-m", &wrong.message, &wrong.sha])?;
        }
    }
    Err("The stashes kept changing while renaming; the old name is still there.".into())
}

/// Stashes the lines `part` says (tracked or new files alike) without touching the index: the
/// stash is built as objects (its base is the index's own tree, so it holds just the chosen
/// changes, which also keeps the staged ones out of it) and stored, and only then does the
/// working tree lose them. A scratch copy of the index makes the trees, so the real file is
/// only read: nothing here can leave it changed or lost.
pub fn stash_lines(repo: &Path, message: &str, parts: &[StashPart]) -> Result<(), String> {
    ensure_idle(repo)?;
    let head = run_text(repo, &["rev-parse", "--verify", "-q", "HEAD"])
        .map_err(|_| "Stash needs a first commit.".to_string())?;
    let head = head.trim();
    let index = crate::patch::Scratch::index(repo)?;
    let on_scratch = |args: &[&str], input: Option<&[u8]>| -> Result<String, String> {
        let out = index.git(repo, args, input)?;
        Ok(String::from_utf8_lossy(&out).trim().to_string())
    };
    let base_tree = on_scratch(&["write-tree"], None)?;
    for part in parts {
        match &part.stashed {
            Some((mode, text)) => {
                let oid = run_with(
                    repo,
                    &["hash-object", "-w", "--path", &part.path, "--stdin"],
                    &[],
                    Some(text.as_bytes()),
                )?;
                let oid = String::from_utf8_lossy(&oid).trim().to_string();
                let info = format!("{mode},{oid},{}", part.path);
                on_scratch(&["update-index", "--add", "--cacheinfo", &info], None)?;
            }
            None => {
                on_scratch(&["update-index", "--force-remove", "--", &part.path], None)?;
            }
        }
    }
    let stash_tree = on_scratch(&["write-tree"], None)?;
    if stash_tree == base_tree {
        return Err("There are no changes to stash.".into());
    }
    let branch = run_text(repo, &["symbolic-ref", "--short", "-q", "HEAD"])
        .map(|b| b.trim().to_string())
        .unwrap_or_else(|_| "(no branch)".into());
    let subject = run_text(repo, &["log", "-1", "--format=%h %s", head])?;
    let subject = subject.trim();
    let named = message.trim();
    let label = if named.is_empty() {
        format!("WIP on {branch}: {subject}")
    } else {
        format!("On {branch}: {named}")
    };
    let commit = |tree: &str, parents: &[&str], msg: &str| -> Result<String, String> {
        let mut args = vec!["commit-tree", tree];
        for p in parents {
            args.extend(["-p", p]);
        }
        args.extend(["-m", msg]);
        Ok(run_text(repo, &args)?.trim().to_string())
    };
    // git's own stash is HEAD, the index's commit and the working tree's; here HEAD is
    // followed by a commit of the index's tree, which `stash apply` takes as the base to merge from.
    let base = commit(&base_tree, &[head], &format!("base of {label}"))?;
    let idx = commit(
        &base_tree,
        &[&base],
        &format!("index on {branch}: {subject}"),
    )?;
    let stash = commit(&stash_tree, &[&base, &idx], &label)?;
    run(repo, &["stash", "store", "-m", &label, &stash]).map(|_| ())
}

/// Puts one file of a stash in the working tree as the stash has it (an untracked file, from
/// the stash's third parent: `untracked`); the index stays as it is.
pub fn stash_restore_file(
    repo: &Path,
    sha: &str,
    path: &str,
    untracked: bool,
) -> Result<(), String> {
    validate_rev(sha)?;
    let source = if untracked {
        format!("--source={sha}^3")
    } else {
        format!("--source={sha}")
    };
    run(repo, &["restore", &source, "--worktree", "--", path]).map(|_| ())
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
