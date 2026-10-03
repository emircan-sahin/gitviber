//! Handing a branch a linked worktree holds back to the main folder, which git won't switch to
//! while it's held: the linked one is detached where it stands, then the main folder takes it.

use super::{
    ensure_idle, run, run_text, validate_branch, with_live_locks, worktree_state, worktrees,
    Worktree,
};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Who held what: what a move changed, and what Undo needs to put back.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MainBack {
    pub main: String,
    /// The branch the main folder was on, which stays a branch; nothing holds it afterwards.
    pub main_branch: String,
    pub holder: String,
    /// The branch that moves to the main folder.
    pub branch: String,
}

fn folder(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map_or_else(|| path.to_string(), |n| n.to_string_lossy().into_owned())
}

/// The branch `dir` is on; None when detached.
fn on_branch(dir: &Path) -> Option<String> {
    run_text(dir, &["symbolic-ref", "--quiet", "--short", "HEAD"])
        .ok()
        .map(|s| s.trim().to_string())
}

/// Puts each folder on its branch (None: detached), in order, and says what stayed out of place.
/// A failing post-checkout hook makes git exit non-zero after it has switched, so where a
/// folder is says more than a switch's result.
fn put_back(error: String, folders: &[(&Path, Option<&str>)]) -> String {
    let stuck: Vec<String> = folders
        .iter()
        .filter(|(dir, branch)| {
            if on_branch(dir).as_deref() != *branch {
                let _ = match branch {
                    Some(b) => run(dir, &["switch", b]),
                    None => run(dir, &["switch", "--detach"]),
                };
            }
            on_branch(dir).as_deref() != *branch
        })
        .map(|(dir, _)| folder(&dir.to_string_lossy()))
        .collect();
    if stuck.is_empty() {
        error
    } else {
        format!(
            "{error}\n\nCouldn't put {} back; check what it's on.",
            stuck.join(" and ")
        )
    }
}

/// Both folders are switched under their files, so neither may have changes or a stopped operation.
fn tidy(repo: &Path, w: &Worktree) -> Result<(), String> {
    let name = folder(&w.path);
    let n = worktree_state(repo, &w.path, false)?.uncommitted;
    if n > 0 {
        let (what, them) = if n == 1 {
            ("change", "it")
        } else {
            ("changes", "them")
        };
        return Err(format!(
            "{name} has {n} uncommitted {what}; commit or stash {them} first."
        ));
    }
    ensure_idle(Path::new(&w.path)).map_err(|e| format!("{name}: {e}"))
}

/// What `move_main_back` would do for `branch`, or why it can't. Asked before the confirm, so
/// the dialog can say exactly which folders change, and again by the move itself.
pub fn main_back_plan(repo: &Path, branch: &str) -> Result<MainBack, String> {
    validate_branch(repo, branch)?;
    let list = with_live_locks(worktrees(repo)?);
    let main = list
        .iter()
        .find(|w| w.main)
        .ok_or("This repository has no main folder.")?;
    if main.bare || main.prunable || !Path::new(&main.path).is_dir() {
        return Err(
            "The main folder has no files to switch: it's a bare repository, or gone.".into(),
        );
    }
    let name = folder(&main.path);
    let Some(main_branch) = main.branch.clone() else {
        return Err(format!(
            "{name} has a detached HEAD, which switching would leave behind. Check out a branch there first."
        ));
    };
    if main_branch == branch {
        return Err(format!("{branch} is already checked out in {name}."));
    }
    let holder = list
        .iter()
        .find(|w| !w.main && w.branch.as_deref() == Some(branch))
        .ok_or_else(|| format!("{branch} isn't checked out in another worktree."))?;
    let held = folder(&holder.path);
    if holder.prunable {
        return Err(format!(
            "{held}'s folder is gone. Prune it from the worktree list first."
        ));
    }
    if holder.in_use {
        return Err(format!(
            "Something is working in {held} right now ({}); detaching it would pull the branch out from under that.",
            holder.lock_reason.as_deref().unwrap_or("it holds the lock")
        ));
    }
    tidy(repo, main)?;
    tidy(repo, holder)?;
    Ok(MainBack {
        main: main.path.clone(),
        main_branch,
        holder: holder.path.clone(),
        branch: branch.to_string(),
    })
}

/// Detaches the worktree holding `branch` at its tip (its files stay as they are), then switches
/// the main folder to it. If the switch fails, the other folder gets its branch back.
pub fn move_main_back(repo: &Path, branch: &str) -> Result<MainBack, String> {
    let plan = main_back_plan(repo, branch)?;
    let (main, holder) = (Path::new(&plan.main), Path::new(&plan.holder));
    let moved = run(holder, &["switch", "--detach"]).and_then(|_| run(main, &["switch", branch]));
    match moved {
        Ok(_) => Ok(plan),
        Err(e) => Err(put_back(
            e,
            &[(main, Some(&plan.main_branch)), (holder, Some(branch))],
        )),
    }
}

/// The reverse of `move_main_back`, while nothing has moved on: the main folder is still on the
/// branch, the other one still detached at its tip.
pub fn undo_main_back(repo: &Path, plan: &MainBack) -> Result<(), String> {
    validate_branch(repo, &plan.branch)?;
    validate_branch(repo, &plan.main_branch)?;
    let list = worktrees(repo)?;
    let find = |path: &str, main: bool| list.iter().find(|w| w.main == main && w.path == path);
    let (Some(main), Some(holder)) = (find(&plan.main, true), find(&plan.holder, false)) else {
        return Err("Those worktrees aren't there any more.".into());
    };
    let (name, held) = (folder(&main.path), folder(&holder.path));
    if main.branch.as_deref() != Some(plan.branch.as_str()) {
        return Err(format!("{name} isn't on {} any more.", plan.branch));
    }
    if holder.branch.is_some() || holder.prunable {
        return Err(format!("{held} has moved on from where it was detached."));
    }
    let tip = |w: &Worktree| run_text(Path::new(&w.path), &["rev-parse", "HEAD"]);
    if tip(main)? != tip(holder)? {
        return Err(format!(
            "{held} or {name} has new commits since; going back would leave them on a detached HEAD."
        ));
    }
    tidy(repo, main)?;
    tidy(repo, holder)?;
    let (m, h) = (Path::new(&main.path), Path::new(&holder.path));
    let undone = run(m, &["switch", "--detach"])
        .and_then(|_| run(h, &["switch", &plan.branch]))
        .and_then(|_| run(m, &["switch", &plan.main_branch]));
    match undone {
        Ok(_) => Ok(()),
        Err(e) => Err(put_back(e, &[(h, None), (m, Some(&plan.branch))])),
    }
}
