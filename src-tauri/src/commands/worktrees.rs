use crate::git;
use crate::state::{in_repo, AppState, Res};
use tauri::State;

#[tauri::command]
pub async fn worktrees(state: State<'_, AppState>) -> Res<Vec<git::Worktree>> {
    in_repo(&state, move |r| git::worktrees(r).map(git::with_live_locks)).await
}

#[tauri::command]
pub async fn add_worktree(
    state: State<'_, AppState>,
    branch: String,
    base: Option<String>,
    dir: Option<String>,
) -> Res<String> {
    in_repo(&state, move |r| {
        git::add_worktree(r, &branch, base.as_deref(), dir.as_deref())
    })
    .await
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
pub async fn worktree_state(state: State<'_, AppState>, path: String) -> Res<git::WorktreeState> {
    in_repo(&state, move |r| git::worktree_state(r, &path)).await
}

#[tauri::command]
pub async fn remove_worktree(state: State<'_, AppState>, path: String, force: bool) -> Res<()> {
    in_repo(&state, move |r| git::remove_worktree(r, &path, force)).await
}
