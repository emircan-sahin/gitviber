use crate::journal::{file_name, files_label};
use crate::state::{blocking, in_repo, short, AppState, Res};
use crate::{fs, git, patch, revert};
use std::path::Path;
use tauri::State;

/// Files' changes as a patch, for Copy as Patch (see patch::changes).
#[tauri::command]
pub async fn changes_patch(
    state: State<'_, AppState>,
    kind: String,
    paths: Vec<String>,
    sha: Option<String>,
) -> Res<String> {
    in_repo(&state, move |r| {
        patch::changes(r, &kind, &paths, sha.as_deref())
    })
    .await
}

#[tauri::command]
pub async fn commit_patch(state: State<'_, AppState>, sha: String) -> Res<String> {
    in_repo(&state, move |r| patch::commit(r, &sha)).await
}

#[tauri::command]
pub async fn stash_patch(state: State<'_, AppState>, sha: String) -> Res<String> {
    in_repo(&state, move |r| patch::stash(r, &sha)).await
}

/// Off the main thread: it diffs two whole files.
#[tauri::command]
pub async fn lines_patch(request: patch::LinesPatch) -> Res<String> {
    blocking(move || patch::lines(&request)).await
}

#[tauri::command]
pub async fn patch_preview(state: State<'_, AppState>, patch: String) -> Res<patch::Preview> {
    in_repo(&state, move |r| patch::preview(r, &patch)).await
}

/// Applies a patch to the working tree (patch::apply); undo puts the files back. Returns the
/// files left with conflict markers.
#[tauri::command]
pub async fn apply_patch(state: State<'_, AppState>, patch: String) -> Res<Vec<String>> {
    let (journal, index) = (state.journal.clone(), state.index.clone());
    in_repo(&state, move |r| {
        let bytes = patch.into_bytes();
        let paths = patch::touched(&patch::files(r, &bytes, false)?);
        let label = files_label("Apply patch to", &paths);
        let list = paths.clone();
        let write = move |r: &Path| patch::apply(r, &bytes, false, &list);
        journal.replace(r, label, "before patch", &paths, &index, write)
    })
    .await
}

/// Puts `path` back as commit `sha` has it, in the working tree only. A link is checked where
/// it is: git writes the link, not through it.
#[tauri::command]
pub async fn restore_file(state: State<'_, AppState>, sha: String, path: String) -> Res<()> {
    let (journal, index) = (state.journal.clone(), state.index.clone());
    in_repo(&state, move |r| {
        fs::resolve_entry(r, &path)?;
        let label = format!("Restore {} from {}", file_name(&path), short(&sha));
        let paths = [path.clone()];
        let write = move |r: &Path| git::restore_from(r, &sha, &path).map(|_| vec![]);
        journal
            .replace(r, label, "before restore", &paths, &index, write)
            .map(|_| ())
    })
    .await
}

/// Undoes what commit `sha` did to `path` (and its old path, a rename) in the working tree.
#[tauri::command]
pub async fn revert_file(
    state: State<'_, AppState>,
    sha: String,
    path: String,
    old_path: Option<String>,
) -> Res<Vec<String>> {
    let (journal, index) = (state.journal.clone(), state.index.clone());
    in_repo(&state, move |r| {
        let named: Vec<String> = old_path.into_iter().chain([path.clone()]).collect();
        let bytes = revert::commit_change(r, &sha, &named)?;
        let paths = patch::touched(&patch::files(r, &bytes, true)?);
        let label = format!("Revert {} in {}", short(&sha), file_name(&path));
        let list = paths.clone();
        let write = move |r: &Path| patch::apply(r, &bytes, true, &list);
        journal.replace(r, label, "before revert", &paths, &index, write)
    })
    .await
}

/// Undoes some of a commit's changed lines in the working tree's file (revert::lines).
#[tauri::command]
pub async fn revert_lines(
    state: State<'_, AppState>,
    request: revert::RevertLines,
) -> Res<Vec<String>> {
    let (journal, index) = (state.journal.clone(), state.index.clone());
    in_repo(&state, move |r| {
        let label = format!(
            "Revert lines of {} in {}",
            short(&request.sha),
            file_name(&request.path)
        );
        let paths = [request.path.clone()];
        let write = move |r: &Path| revert::lines(r, &request);
        journal.replace(r, label, "before revert", &paths, &index, write)
    })
    .await
}
