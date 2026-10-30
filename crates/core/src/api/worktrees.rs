use crate::journal::{Action, Mode};
use crate::state::{Core, Res};
use crate::{git, network, Event};
use std::collections::HashMap;

pub fn worktrees(core: &Core) -> Res<Vec<git::Worktree>> {
    core.read_repo(move |r| git::worktrees(r).map(git::with_live_locks))
}

/// `op`: a Git LFS repository downloads the branch's files first, with progress and Cancel.
pub fn add_worktree(
    core: &Core,
    branch: String,
    base: Option<String>,
    track: bool,
    dir: Option<String>,
    op: String,
    progress: impl Fn(network::Progress) + Send + Sync + 'static,
) -> Res<String> {
    let net = core.watch_network(op, progress);
    core.in_repo(move |r| {
        git::add_worktree(r, &branch, base.as_deref(), track, dir.as_deref(), &net)
    })
}

/// What handing `branch` back to the main folder would change, or why it can't be done.
pub fn main_back_plan(core: &Core, branch: String) -> Res<git::MainBack> {
    core.in_repo(move |r| git::main_back_plan(r, &branch))
}

/// Detaches the worktree that holds `branch` and switches the main folder to it. Not in the
/// undo history, which follows one folder's HEAD: the toast's Undo calls `undo_main_back`.
pub fn move_main_back(core: &Core, branch: String) -> Res<git::MainBack> {
    core.in_repo(move |r| git::move_main_back(r, &branch))
}

pub fn undo_main_back(core: &Core, plan: git::MainBack) -> Res<()> {
    core.in_repo(move |r| git::undo_main_back(r, &plan))
}

/// How many ignored files `.worktreeinclude` would copy into a new worktree.
pub fn worktree_includes(core: &Core) -> Res<usize> {
    core.in_repo(git::include_count)
}

pub fn rename_worktree(
    core: &Core,
    path: String,
    branch: String,
    move_folder: bool,
) -> Res<String> {
    core.in_repo(move |r| git::rename_worktree(r, &path, &branch, move_folder))
}

pub fn lock_worktree(core: &Core, path: String, reason: Option<String>) -> Res<()> {
    core.in_repo(move |r| git::lock_worktree(r, &path, reason.as_deref()))
}

pub fn unlock_worktree(core: &Core, path: String) -> Res<()> {
    core.in_repo(move |r| git::unlock_worktree(r, &path))
}

pub fn worktree_state(core: &Core, path: String, upstream: bool) -> Res<git::WorktreeState> {
    core.in_repo(move |r| git::worktree_state(r, &path, upstream))
}

pub fn remove_worktree(core: &Core, path: String, force: bool) -> Res<()> {
    let root = core.in_repo(move |r| {
        // Named as its browser pages were, while the folder is still there to resolve.
        let root = root_key(&path);
        git::remove_worktree(r, &path, force).map(|()| root)
    })?;
    core.emit(Event::WorktreesRemoved(vec![root]));
    Ok(())
}

/// The folder a worktree's browser pages are kept under: canonical while it's there to resolve.
pub fn root_key(path: &str) -> String {
    std::fs::canonicalize(path).map_or_else(
        |_| path.trim_end_matches('/').to_string(),
        |p| p.to_string_lossy().into_owned(),
    )
}

/// What removing a worktree would delete that isn't a change: its ignored files, sized.
pub fn worktree_ignored(core: &Core, path: String) -> Res<git::IgnoredFiles> {
    core.in_repo(move |r| git::worktree_ignored(r, &path))
}

/// Removes merged worktrees and the branches known merged; Undo brings those branches back. The
/// folders go first, outside the journal: a big node_modules takes seconds to delete, and a
/// commit or an undo would wait for it.
pub fn clean_up_worktrees(core: &Core, list: Vec<git::CleanUp>) -> Res<git::CleanedUp> {
    let (mut out, branches, roots) = core.in_repo(move |r| {
        let roots: HashMap<String, String> = list
            .iter()
            .map(|w| (w.path.clone(), root_key(&w.path)))
            .collect();
        let (out, branches) = git::remove_merged_worktrees(r, &list);
        Ok((out, branches, roots))
    })?;
    core.emit(Event::WorktreesRemoved(
        out.removed
            .iter()
            .filter_map(|p| roots.get(p).cloned())
            .collect(),
    ));
    if branches.is_empty() {
        return Ok(out);
    }
    // Named for what Undo can bring back: the folders are gone for good.
    let action = Action::new("Delete merged worktrees' branches", Mode::Keep);
    core.journaled(action, move |r| {
        git::delete_merged_branches(r, &mut out, branches);
        Ok(out)
    })
}
