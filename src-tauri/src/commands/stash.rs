//! Stashes are not undo entries: they move no branch. A dropped stash's commit stays findable
//! by its sha.

use crate::git;
use crate::journal::{files_label, Action, Mode};
use crate::lines;
use crate::state::{in_repo, indexed_once, journaled, AppState, Res};
use std::path::Path;
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

/// Stashes the lines of an unstaged diff a request picks; the file's old version goes to the
/// Trash, so Undo puts it back (the stash stays: drop it from Stashes).
#[tauri::command]
pub async fn stash_lines(
    state: State<'_, AppState>,
    message: String,
    mut request: lines::Request,
) -> Res<()> {
    request.action = "stash".into();
    let (journal, index) = (state.journal.clone(), state.index.clone());
    in_repo(&state, move |r| {
        let paths = [request.path.clone()];
        let label = files_label("Stash lines of", &paths);
        let write = move |r: &Path| lines::stash(r, &message, &request).map(|_| vec![]);
        journal
            .replace(r, label, "stashed", &paths, &index, write)
            .map(|_| ())
    })
    .await
}

/// Puts one file of a stash in the working tree; the version it replaces goes to the Trash.
#[tauri::command]
pub async fn stash_restore_file(
    state: State<'_, AppState>,
    sha: String,
    path: String,
    untracked: bool,
) -> Res<()> {
    let (journal, index) = (state.journal.clone(), state.index.clone());
    in_repo(&state, move |r| {
        let paths = [path.clone()];
        let label = files_label("Take from a stash", &paths);
        let write =
            move |r: &Path| git::stash_restore_file(r, &sha, &path, untracked).map(|_| vec![]);
        journal
            .replace(r, label, "before stash", &paths, &index, write)
            .map(|_| ())
    })
    .await
}

/// Gives a stash another message; it moves to the top of the list.
#[tauri::command]
pub async fn stash_rename(state: State<'_, AppState>, sha: String, message: String) -> Res<()> {
    in_repo(&state, move |r| git::stash_rename(r, &sha, &message)).await
}
