use crate::git;
use crate::journal::{Action, Mode};
use crate::state::{in_repo, journaled, read_repo, AppState, Res};
use tauri::State;

#[tauri::command]
pub async fn worktrees(state: State<'_, AppState>) -> Res<Vec<git::Worktree>> {
    read_repo(&state, move |r| git::worktrees(r).map(git::with_live_locks)).await
}

#[tauri::command]
pub async fn add_worktree(
    state: State<'_, AppState>,
    branch: String,
    base: Option<String>,
    track: bool,
    dir: Option<String>,
) -> Res<String> {
    in_repo(&state, move |r| {
        git::add_worktree(r, &branch, base.as_deref(), track, dir.as_deref())
    })
    .await
}

/// How many ignored files `.worktreeinclude` would copy into a new worktree.
#[tauri::command]
pub async fn worktree_includes(state: State<'_, AppState>) -> Res<usize> {
    in_repo(&state, git::include_count).await
}

#[tauri::command]
pub async fn rename_worktree(
    state: State<'_, AppState>,
    path: String,
    branch: String,
    move_folder: bool,
) -> Res<String> {
    in_repo(&state, move |r| {
        git::rename_worktree(r, &path, &branch, move_folder)
    })
    .await
}

#[tauri::command]
pub async fn lock_worktree(
    state: State<'_, AppState>,
    path: String,
    reason: Option<String>,
) -> Res<()> {
    in_repo(&state, move |r| {
        git::lock_worktree(r, &path, reason.as_deref())
    })
    .await
}

#[tauri::command]
pub async fn unlock_worktree(state: State<'_, AppState>, path: String) -> Res<()> {
    in_repo(&state, move |r| git::unlock_worktree(r, &path)).await
}

#[tauri::command]
pub async fn worktree_state(
    state: State<'_, AppState>,
    path: String,
    upstream: bool,
) -> Res<git::WorktreeState> {
    in_repo(&state, move |r| git::worktree_state(r, &path, upstream)).await
}

#[tauri::command]
pub async fn remove_worktree(state: State<'_, AppState>, path: String, force: bool) -> Res<()> {
    in_repo(&state, move |r| git::remove_worktree(r, &path, force)).await
}

/// What removing a worktree would delete that isn't a change: its ignored files, sized.
#[tauri::command]
pub async fn worktree_ignored(state: State<'_, AppState>, path: String) -> Res<git::IgnoredFiles> {
    in_repo(&state, move |r| git::worktree_ignored(r, &path)).await
}

/// Removes merged worktrees and the branches known merged; Undo brings those branches back. The
/// folders go first, outside the journal: a big node_modules takes seconds to delete, and a
/// commit or an undo would wait for it.
#[tauri::command]
pub async fn clean_up_worktrees(
    state: State<'_, AppState>,
    list: Vec<git::CleanUp>,
) -> Res<git::CleanedUp> {
    let (mut out, branches) =
        in_repo(&state, move |r| Ok(git::remove_merged_worktrees(r, &list))).await?;
    if branches.is_empty() {
        return Ok(out);
    }
    // Named for what Undo can bring back: the folders are gone for good.
    let action = Action::new("Delete merged worktrees' branches", Mode::Keep);
    journaled(&state, action, move |r| {
        git::delete_merged_branches(r, &mut out, branches);
        Ok(out)
    })
    .await
}
