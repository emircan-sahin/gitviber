//! Merge, rebase and bisect, and finishing or calling off one stopped on conflicts.

use super::{run, run_text, run_with, stage, validate_ref, validate_rev, with_paths, Operation};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub(crate) fn git_dir(repo: &Path) -> Option<std::path::PathBuf> {
    run_text(repo, &["rev-parse", "--absolute-git-dir"])
        .ok()
        .map(|s| s.trim().into())
}

fn read_trim(path: std::path::PathBuf) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
}

pub fn operation(repo: &Path) -> Option<Operation> {
    operation_in(&git_dir(repo)?)
}

/// `operation` for the repository whose git dir is `dir`.
pub(super) fn operation_in(dir: &Path) -> Option<Operation> {
    // `git am` also uses rebase-apply/, marked by an `applying` file.
    if dir.join("rebase-apply/applying").exists() {
        return Some(Operation {
            kind: "am".into(),
            subject: None,
            step: None,
            total: None,
        });
    }
    for rebase in ["rebase-merge", "rebase-apply"] {
        let d = dir.join(rebase);
        if d.is_dir() {
            let (step, total) = if rebase == "rebase-merge" {
                ("msgnum", "end")
            } else {
                ("next", "last")
            };
            return Some(Operation {
                kind: "rebase".into(),
                // "detached HEAD" when the rebase didn't start on a branch.
                subject: read_trim(d.join("head-name"))
                    .and_then(|h| h.strip_prefix("refs/heads/").map(str::to_string)),
                step: read_trim(d.join(step)).and_then(|v| v.parse().ok()),
                total: read_trim(d.join(total)).and_then(|v| v.parse().ok()),
            });
        }
    }
    let simple = |kind: &str, file: &str| {
        dir.join(file).exists().then(|| Operation {
            kind: kind.into(),
            subject: read_trim(dir.join("MERGE_MSG"))
                .and_then(|m| m.lines().next().map(str::to_string)),
            step: None,
            total: None,
        })
    };
    simple("merge", "MERGE_HEAD")
        .or_else(|| simple("cherry-pick", "CHERRY_PICK_HEAD"))
        .or_else(|| simple("revert", "REVERT_HEAD"))
        .or_else(|| {
            dir.join("BISECT_LOG").exists().then(|| Operation {
                kind: "bisect".into(),
                subject: None,
                step: None,
                total: None,
            })
        })
        .or_else(|| {
            // A multi-commit cherry-pick/revert paused between picks leaves only sequencer/.
            let todo = read_trim(dir.join("sequencer/todo"))?;
            let kind = if todo.starts_with("revert") {
                "revert"
            } else {
                "cherry-pick"
            };
            Some(Operation {
                kind: kind.into(),
                subject: None,
                step: None,
                total: None,
            })
        })
}

/// Runs an operation that may stop on conflicts. Stopping is not an error: it returns
/// Ok(true) so the UI can switch to resolving. Anything else that fails is an error.
fn has_conflicts(repo: &Path) -> bool {
    run(repo, &["diff", "--name-only", "--diff-filter=U"]).is_ok_and(|o| !o.is_empty())
}

fn run_stoppable(repo: &Path, args: &[&str]) -> Result<bool, String> {
    stoppable(repo, run(repo, args))
}

/// Ok(true) only when the operation stopped on conflicts. Any other failure (a hook, GPG,
/// dirty worktree) is returned as the real git error instead of looking like conflicts.
pub(crate) fn stoppable(repo: &Path, result: Result<Vec<u8>, String>) -> Result<bool, String> {
    let conflicts = has_conflicts(repo);
    match result {
        Ok(_) => Ok(conflicts),
        Err(_) if conflicts => Ok(true),
        Err(e) => Err(e),
    }
}

/// Starting a new merge/rebase/pull on top of an unfinished one would be misreported as conflicts.
pub(crate) fn ensure_idle(repo: &Path) -> Result<(), String> {
    match operation(repo) {
        Some(op) if op.kind == "bisect" => {
            Err("A bisect is in progress. Finish or stop it first.".into())
        }
        Some(op) => Err(format!(
            "A {} is in progress. Continue or abort it first.",
            op.kind
        )),
        None => Ok(()),
    }
}

/// "ff" (fast-forward when it can), "no-ff" (always a merge commit) or "squash".
#[derive(Deserialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "kebab-case")]
pub enum MergeKind {
    Ff,
    NoFf,
    Squash,
}

/// `how`: "ff" (git's default, fast-forward when it can), "no-ff" (always a merge commit), or
/// "squash": the branch's changes as one new commit, its subjects listed in the message.
/// `autostash`, as for `pull`; not for a squash, whose changes the stash would come back onto
/// before they're committed.
pub fn merge(repo: &Path, name: &str, how: MergeKind, autostash: bool) -> Result<bool, String> {
    ensure_idle(repo)?;
    validate_ref(repo, name)?;
    // Only ever added: a merge.autoStash the user set applies either way.
    let stash = autostash.then_some("--autostash");
    match how {
        MergeKind::Ff | MergeKind::NoFf => {
            let mut args = vec!["merge", "--no-edit"];
            args.extend((how == MergeKind::NoFf).then_some("--no-ff"));
            args.extend(stash);
            args.push(name);
            run_stoppable(repo, &args)
        }
        MergeKind::Squash => {
            let range = format!("HEAD..{name}");
            let subjects = run_text(repo, &["log", "--reverse", "--format=- %s", &range])?;
            // Conflicts stop it with the changes staged so far; the commit is then the user's.
            if run_stoppable(repo, &["merge", "--squash", name])? {
                return Ok(true);
            }
            // Nothing new on it: nothing to commit.
            if run(repo, &["diff", "--cached", "--quiet"]).is_ok() {
                return Ok(false);
            }
            let message = format!("Squash merge {name}\n\n{}", subjects.trim_end());
            run_with(repo, &["commit", "-F", "-"], &[], Some(message.as_bytes()))?;
            Ok(false)
        }
    }
}

pub fn rebase(repo: &Path, onto: &str, autostash: bool) -> Result<bool, String> {
    ensure_idle(repo)?;
    validate_ref(repo, onto)?;
    let mut args = vec!["rebase"];
    args.extend(autostash.then_some("--autostash"));
    args.push(onto);
    run_stoppable(repo, &args)
}

pub fn op_continue(repo: &Path) -> Result<bool, String> {
    let op = operation(repo).ok_or("Nothing to continue.")?;
    match op.kind.as_str() {
        "bisect" => Err("Mark the commit good or bad instead.".into()),
        // `merge --continue` refuses without an editor on some git versions; commit is equivalent.
        "merge" => run_stoppable(repo, &["commit", "--no-edit"]),
        "rebase" => run_stoppable(repo, &["rebase", "--continue"]),
        "cherry-pick" => run_stoppable(repo, &["cherry-pick", "--continue"]),
        "am" => run_stoppable(repo, &["am", "--continue"]),
        _ => run_stoppable(repo, &["revert", "--continue"]),
    }
}

pub fn op_abort(repo: &Path) -> Result<(), String> {
    let op = operation(repo).ok_or("Nothing to abort.")?;
    let kind = op.kind.as_str();
    if kind == "bisect" {
        // Back to where it started.
        return run(repo, &["bisect", "reset"]).map(|_| ());
    }
    run(repo, &[kind, "--abort"]).map(|_| ())
}

/// Where a bisect stands: the commit it checked out to test next, or the first bad commit.
#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct BisectStep {
    /// git's own words: "Bisecting: 3 revisions left to test after this (roughly 2 steps)".
    pub message: String,
    /// Set once it's found.
    pub first_bad: Option<String>,
}

fn bisect_step(out: Vec<u8>) -> BisectStep {
    let text = String::from_utf8_lossy(&out).into_owned();
    let first_bad = text
        .lines()
        // Newer git quotes the term: "<sha> is the first 'bad' commit".
        .find(|l| {
            l.ends_with(" is the first bad commit") || l.ends_with(" is the first 'bad' commit")
        })
        .and_then(|l| l.split_whitespace().next())
        .map(str::to_string);
    let message = text.lines().next().unwrap_or_default().to_string();
    BisectStep { message, first_bad }
}

/// Starts looking for the commit that broke something: `good` didn't have it, HEAD does.
pub fn bisect_start(repo: &Path, good: &str) -> Result<BisectStep, String> {
    ensure_idle(repo)?;
    validate_rev(good)?;
    run(repo, &["bisect", "start", "HEAD", good]).map(bisect_step)
}

/// Past this a side isn't shown line by line anyway.
const MERGE_BASE_MAX: usize = 4 << 20;

/// A conflicted text file's sides as the index holds them (stages 2 and 3).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConflictSides {
    /// The file merged again in diff3 style (conflict_base); None for a side binary or too big.
    pub merged: Option<String>,
    /// Whether each side's file ends with a newline. Git ends the closing marker line with one
    /// either way, so a conflict at the end of the file can't tell.
    pub ours_newline: bool,
    pub theirs_newline: bool,
}

/// The conflicted `path` merged again from its index stages in diff3 style, whatever
/// conflictstyle the merge wrote it in, so each conflict can show the base both sides changed.
/// None when a side has no text (it deleted the file, or it's binary) or is too big to show.
/// The page gets it with how each side ends (conflict_sides); the scenarios check it alone.
#[cfg(test)]
pub fn conflict_base(repo: &Path, path: &str) -> Result<Option<String>, String> {
    Ok(conflict_sides(repo, path)?.and_then(|s| s.merged))
}

/// None when a side has no file (it deleted it).
pub fn conflict_sides(repo: &Path, path: &str) -> Result<Option<ConflictSides>, String> {
    let paths = [path.to_string()];
    // "<mode> <blob> <stage>\t<path>": 1 base (none for add/add), 2 ours, 3 theirs.
    let raw = run_text(repo, &with_paths(vec!["ls-files", "-u", "-z"], &paths))?;
    let mut blobs: [Option<&str>; 3] = [None; 3];
    for entry in raw.split('\0') {
        let mut meta = entry.split('\t').next().unwrap_or_default().split(' ');
        if let (Some(_), Some(blob), Some(n @ ("1" | "2" | "3"))) =
            (meta.next(), meta.next(), meta.next())
        {
            blobs[n.as_bytes()[0] as usize - b'1' as usize] = Some(blob);
        }
    }
    let [base, Some(ours), Some(theirs)] = blobs else {
        return Ok(None);
    };
    let read = |blob: Option<&str>| match blob {
        Some(b) => run(repo, &["cat-file", "blob", b]),
        None => Ok(vec![]),
    };
    let sides = [read(Some(ours))?, read(base)?, read(Some(theirs))?];
    let ends = |s: &[u8]| s.last() == Some(&b'\n');
    let mut out = ConflictSides {
        merged: None,
        ours_newline: ends(&sides[0]),
        theirs_newline: ends(&sides[2]),
    };
    if sides
        .iter()
        .any(|s| s.len() > MERGE_BASE_MAX || s.contains(&0))
    {
        return Ok(Some(out));
    }
    // merge-file reads files; `--object-id` would take the blobs, but only from git 2.43.
    let dir = crate::scratch::ScratchDir::new("merge")?;
    let files: Vec<String> = ["current", "base", "incoming"]
        .iter()
        .zip(&sides)
        .map(|(name, text)| {
            let f = dir.path().join(name);
            std::fs::write(&f, text).map_err(|e| e.to_string())?;
            Ok(f.to_string_lossy().into_owned())
        })
        .collect::<Result<_, String>>()?;
    let mut args = vec!["merge-file", "-p", "--diff3"];
    for label in ["current", "base", "incoming"] {
        args.extend(["-L", label]);
    }
    args.extend(files.iter().map(String::as_str));
    // Its exit code is the number of conflicts (capped at 127); a negative one is an error.
    let conflicts: Vec<i32> = (1..=127).collect();
    let merged = run_with(repo, &args, &conflicts, None)?;
    out.merged = Some(String::from_utf8_lossy(&merged).into_owned());
    Ok(Some(out))
}

/// The commit checked out is "good", "bad", or to "skip" (can't be tested).
pub fn bisect_mark(repo: &Path, verdict: &str) -> Result<BisectStep, String> {
    if !matches!(verdict, "good" | "bad" | "skip") {
        return Err(format!("unknown verdict: {verdict}"));
    }
    if operation(repo).is_none_or(|o| o.kind != "bisect") {
        return Err("No bisect is in progress.".into());
    }
    run(repo, &["bisect", verdict]).map(bisect_step)
}

pub fn rebase_skip(repo: &Path) -> Result<bool, String> {
    run_stoppable(repo, &["rebase", "--skip"])
}

/// Which version of a conflicted file to keep: "ours" or "theirs".
#[derive(Deserialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "kebab-case")]
pub enum Side {
    Ours,
    Theirs,
}

/// Resolves a conflicted file by taking one side whole. `side` is "ours" or "theirs";
/// if that side deleted the file, the resolution is to delete it.
pub fn resolve_side(repo: &Path, path: &str, side: Side) -> Result<(), String> {
    let (flag, stage_no) = match side {
        Side::Ours => ("--ours", "2"),
        Side::Theirs => ("--theirs", "3"),
    };
    let paths = [path.to_string()];
    // Unmerged index stages: 1 base, 2 ours, 3 theirs. Decide from them, never from a
    // failed checkout, so an unrelated error can't turn into deleting the file.
    let raw = run_text(repo, &with_paths(vec!["ls-files", "-u", "-z"], &paths))?;
    let stages: Vec<&str> = raw
        .split('\0')
        .filter_map(|l| l.split('\t').next()?.split(' ').nth(2))
        .collect();
    if stages.is_empty() {
        return Err(format!("{path} is not in conflict"));
    }
    if stages.contains(&stage_no) {
        run(repo, &with_paths(vec!["checkout", flag], &paths))?;
        stage(repo, &paths)
    } else {
        run(repo, &with_paths(vec!["rm", "-q"], &paths)).map(|_| ())
    }
}
