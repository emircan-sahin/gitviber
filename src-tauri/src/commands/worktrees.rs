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

/// What handing `branch` back to the main folder would change, or why it can't be done.
#[tauri::command]
pub async fn main_back_plan(state: State<'_, AppState>, branch: String) -> Res<git::MainBack> {
    in_repo(&state, move |r| git::main_back_plan(r, &branch)).await
}

/// Detaches the worktree that holds `branch` and switches the main folder to it. Not in the
/// undo history, which follows one folder's HEAD: the toast's Undo calls `undo_main_back`.
#[tauri::command]
pub async fn move_main_back(state: State<'_, AppState>, branch: String) -> Res<git::MainBack> {
    in_repo(&state, move |r| git::move_main_back(r, &branch)).await
}

#[tauri::command]
pub async fn undo_main_back(state: State<'_, AppState>, plan: git::MainBack) -> Res<()> {
    in_repo(&state, move |r| git::undo_main_back(r, &plan)).await
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
