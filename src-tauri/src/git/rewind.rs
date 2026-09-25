//! Commands that move a branch back or pick commits: reset, revert, cherry-pick.

use super::{
    commit_files, ensure_idle, listed_worktree, operation, pushed_base, run, run_text, status,
    stoppable, toplevel, validate_branch, validate_rev,
};
use serde::Deserialize;
use std::path::Path;

/// `seen` is the HEAD the user saw: an agent may have committed since, and moving HEAD
/// based on the old history would silently drop that commit.
pub(crate) fn ensure_head(repo: &Path, seen: &str) -> Result<(), String> {
    validate_rev(seen)?;
    let head = run_text(repo, &["rev-parse", "HEAD"])?;
    if head.trim() != seen {
        return Err("HEAD has moved since the history was loaded. Refresh and try again.".into());
    }
    Ok(())
}

/// Undoes the last commit (`sha`, the HEAD the user saw), keeping its changes staged.
pub fn undo_commit(repo: &Path, sha: &str) -> Result<(), String> {
    ensure_idle(repo)?;
    ensure_head(repo, sha)?;
    run(repo, &["reset", "--soft", "HEAD~1"]).map(|_| ())
}

/// Whether moving HEAD to `sha` takes commits off the branch that its push target (or
/// upstream) already has, i.e. would need a force-push. Decided by ancestry, not log order,
/// so merges count right. No upstream, or one that is gone, counts as not pushed.
pub fn drops_pushed(repo: &Path, sha: &str) -> Result<bool, String> {
    validate_rev(sha)?;
    drops_pushed_from(repo, "HEAD", &[sha])
}

/// Whether the push target has commits reachable from `from` but from none of `keep`.
pub(crate) fn drops_pushed_from(repo: &Path, from: &str, keep: &[&str]) -> Result<bool, String> {
    // Commits both sides have: everything reachable from their merge bases.
    let Ok(bases) = run_text(repo, &["merge-base", "--all", from, &pushed_base(repo)]) else {
        return Ok(false);
    };
    let mut args = vec!["rev-list", "-n1"];
    args.extend(bases.split_whitespace());
    args.push("--not");
    args.extend(keep);
    Ok(!run_text(repo, &args)?.trim().is_empty())
}

/// The commit HEAD's push target (or upstream) is at, if it has one.
pub(crate) fn pushed_tip(repo: &Path) -> Option<String> {
    let spec = format!("{}^{{commit}}", pushed_base(repo));
    run_text(repo, &["rev-parse", "--verify", "-q", &spec])
        .ok()
        .map(|s| s.trim().to_string())
}

/// "soft", "mixed" or "hard", as `git reset` takes them.
#[derive(Deserialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "kebab-case")]
pub enum ResetMode {
    Soft,
    Mixed,
    Hard,
}

/// Moves the current branch (or detached HEAD) from `head` (as the user saw it) to `sha`.
/// `mode` is "soft", "mixed" or "hard".
pub fn reset(repo: &Path, sha: &str, mode: ResetMode, head: &str) -> Result<(), String> {
    validate_rev(sha)?;
    ensure_idle(repo)?;
    ensure_head(repo, head)?;
    let flag = match mode {
        ResetMode::Soft => "--soft",
        ResetMode::Mixed => "--mixed",
        ResetMode::Hard => "--hard",
    };
    run(repo, &["reset", "-q", flag, sha]).map(|_| ())
}

/// `git revert`, returning true if it stopped on conflicts.
pub fn revert(repo: &Path, sha: &str) -> Result<bool, String> {
    validate_rev(sha)?;
    ensure_idle(repo)?;
    // A merge commit needs a mainline; relative to its first parent is what "this commit" means.
    let merge = run(repo, &["rev-parse", "--verify", "-q", &format!("{sha}^2")]).is_ok();
    let mut args = vec!["revert", "--no-edit"];
    if merge {
        args.extend(["-m", "1"]);
    }
    args.push(sha);
    let result = stoppable(repo, run(repo, &args));
    // An empty revert (already undone) fails with no conflicts, no operation left and nothing
    // changed, explaining why only on stdout. Other failures leave one of those behind.
    if result.is_err()
        && operation(repo).is_none()
        && run(repo, &["diff", "--quiet", "HEAD"]).is_ok()
    {
        return Err("This commit's changes are already undone; nothing to revert.".into());
    }
    result
}

/// `git cherry-pick` onto HEAD, returning true if it stopped on conflicts.
pub fn cherry_pick(repo: &Path, sha: &str) -> Result<bool, String> {
    validate_rev(sha)?;
    ensure_idle(repo)?;
    // Like revert: a merge is picked relative to its first parent.
    let merge = run(repo, &["rev-parse", "--verify", "-q", &format!("{sha}^2")]).is_ok();
    let mut args = vec!["cherry-pick"];
    if merge {
        args.extend(["-m", "1"]);
    }
    args.push(sha);
    let result = stoppable(repo, run(repo, &args));
    // Changes HEAD already has leave an empty pick in progress, with nothing to resolve.
    if result.is_err()
        && operation(repo).is_some_and(|op| op.kind == "cherry-pick")
        && run(repo, &["diff", "--cached", "--quiet"]).is_ok()
    {
        let _ = run(repo, &["cherry-pick", "--abort"]);
        return Err("This branch already has these changes; nothing to cherry-pick.".into());
    }
    result
}

/// Another worktree of this repo with a branch checked out, where a commit can be picked onto
/// that branch. Returns its top folder as git names it, which is how the journal keys it.
pub fn pick_target(repo: &Path, path: &str) -> Result<std::path::PathBuf, String> {
    let w = listed_worktree(repo, path)?;
    if w.current || w.bare || w.prunable || w.branch.is_none() {
        return Err(format!("{path} has no branch to cherry-pick onto here"));
    }
    toplevel(Path::new(&w.path)).map(Into::into)
}

/// `cherry_pick` in another worktree (`target`, from `pick_target`). Refused up front when
/// local changes there are in the way: git wants a clean index, and won't touch a changed file.
pub fn cherry_pick_into(target: &Path, sha: &str) -> Result<bool, String> {
    validate_rev(sha)?;
    ensure_idle(target)?;
    let touched: std::collections::HashSet<String> = commit_files(target, sha)?
        .into_iter()
        .flat_map(|f| [Some(f.path), f.old_path])
        .flatten()
        .collect();
    let st = status(target)?;
    let mut blocking: Vec<&str> = st.staged.iter().map(|f| f.path.as_str()).collect();
    blocking.extend(
        st.unstaged
            .iter()
            .chain(&st.conflicted)
            .map(|f| f.path.as_str())
            .filter(|p| touched.contains(*p)),
    );
    if !blocking.is_empty() {
        blocking.sort_unstable();
        blocking.dedup();
        let branch = st.branch.as_deref().unwrap_or("that worktree");
        let shown = blocking
            .iter()
            .take(5)
            .copied()
            .collect::<Vec<_>>()
            .join(", ");
        let more = blocking.len().saturating_sub(5);
        let more = if more > 0 {
            format!(" and {more} more")
        } else {
            String::new()
        };
        return Err(format!(
            "{branch} has uncommitted changes in the way ({shown}{more}). Commit or stash them in that worktree first."
        ));
    }
    cherry_pick(target, sha)
}

/// Detached checkout of a commit. Git refuses if local changes would be overwritten.
pub fn checkout_commit(repo: &Path, sha: &str) -> Result<(), String> {
    validate_rev(sha)?;
    run(repo, &["switch", "--detach", sha]).map(|_| ())
}

pub fn create_branch_at(repo: &Path, name: &str, sha: &str) -> Result<(), String> {
    validate_rev(sha)?;
    validate_branch(repo, name)?;
    run(repo, &["switch", "-c", name, sha]).map(|_| ())
}
