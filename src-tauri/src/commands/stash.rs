//! Stashes are not undo entries: they move no branch. A dropped stash's commit stays findable
//! by its sha.

use crate::git;
use crate::journal::{Action, Mode};
use crate::state::{in_repo, indexed_once, journaled, AppState, Res};
use tauri::State;

#[tauri::command]
pub async fn stashes(state: State<'_, AppState>) -> Res<Vec<git::Stash>> {
    in_repo(&state, git::stashes).await
}

#[tauri::command]
pub async fn stash_files(state: State<'_, AppState>, sha: String) -> Res<git::StashFiles> {
    in_repo(&state, move |r| git::stash_files(r, &sha)).await
}

#[tauri::command]
pub async fn stash_push(
    state: State<'_, AppState>,
    message: String,
    untracked: bool,
    staged: Option<bool>,
    paths: Option<Vec<String>>,
) -> Res<()> {
    let paths = paths.unwrap_or_default();
    indexed_once(&state, move |r| {
        let what = git::StashWhat {
            untracked,
            staged: staged.unwrap_or(false),
            paths: &paths,
        };
        git::stash_push(r, &message, what)
    })
    .await
}

/// True when applying stopped on conflicts (the stash is kept then).
#[tauri::command]
pub async fn stash_branch(state: State<'_, AppState>, name: String, sha: String) -> Res<bool> {
    let label = format!("Branch {name} from a stash");
    journaled(&state, Action::new(label, Mode::Keep), move |r| {
        git::stash_branch(r, &name, &sha)
    })
    .await
}

/// True when it stopped on conflicts.
#[tauri::command]
pub async fn stash_apply(state: State<'_, AppState>, sha: String, pop: bool) -> Res<bool> {
    indexed_once(&state, move |r| git::stash_apply(r, &sha, pop)).await
}

#[tauri::command]
pub async fn stash_drop(state: State<'_, AppState>, sha: String) -> Res<()> {
    in_repo(&state, move |r| git::stash_drop(r, &sha)).await
}
