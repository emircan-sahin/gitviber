use crate::journal;
use crate::state::{in_repo, repo, AppState, Res};
use tauri::State;

#[tauri::command]
pub async fn journal(state: State<'_, AppState>) -> Res<journal::View> {
    let journal = state.journal.clone();
    in_repo(&state, move |r| Ok(journal.view(r))).await
}

#[tauri::command]
pub fn journal_last(state: State<'_, AppState>) -> Res<Option<u64>> {
    Ok(state.journal.last(&repo(&state)?))
}

/// `id`: the entry the user means; refused if it is no longer the next one.
#[tauri::command]
pub async fn undo(state: State<'_, AppState>, id: Option<u64>) -> Res<journal::EntryView> {
    step(&state, false, id).await
}

#[tauri::command]
pub async fn redo(state: State<'_, AppState>, id: Option<u64>) -> Res<journal::EntryView> {
    step(&state, true, id).await
}

async fn step(
    state: &State<'_, AppState>,
    forward: bool,
    id: Option<u64>,
) -> Res<journal::EntryView> {
    let (journal, index) = (state.journal.clone(), state.index.clone());
    in_repo(state, move |r| journal.step(r, forward, id, &index)).await
}
