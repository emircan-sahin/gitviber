use crate::journal::{Action, Mode};
use crate::state::{in_repo, indexed, journaled, with_index_lock, AppState, Res};
use crate::{git, lines, suggest};
use tauri::State;

#[tauri::command]
pub async fn status(state: State<'_, AppState>) -> Res<git::RepoStatus> {
    in_repo(&state, git::status).await
}

#[tauri::command]
pub async fn branch_review(state: State<'_, AppState>, base: String) -> Res<git::BranchReview> {
    in_repo(&state, move |r| git::branch_review(r, &base)).await
}

#[tauri::command]
pub async fn stage(state: State<'_, AppState>, paths: Vec<String>, allow_nested: bool) -> Res<()> {
    indexed(&state, move |r| git::stage_with(r, &paths, allow_nested)).await
}

#[tauri::command]
pub async fn unstage(state: State<'_, AppState>, paths: Vec<String>) -> Res<()> {
    indexed(&state, move |r| git::unstage(r, &paths)).await
}

#[tauri::command]
pub async fn discard(state: State<'_, AppState>, paths: Vec<String>) -> Res<()> {
    let (journal, index) = (state.journal.clone(), state.index.clone());
    in_repo(&state, move |r| {
        journal.discard(r, &paths, || {
            with_index_lock(&index, r, |r| git::discard(r, &paths))
        })
    })
    .await
}

/// Stages, unstages or discards some of a file's changed lines; a discard can be undone.
#[tauri::command]
pub async fn change_lines(state: State<'_, AppState>, request: lines::Request) -> Res<()> {
    if request.action != "discard" {
        return indexed(&state, move |r| lines::run(r, &request)).await;
    }
    let journal = state.journal.clone();
    in_repo(&state, move |r| {
        journal.discard(r, std::slice::from_ref(&request.path), || {
            lines::run(r, &request)
        })
    })
    .await
}

#[tauri::command]
pub async fn commit(
    state: State<'_, AppState>,
    message: String,
    options: git::CommitOptions,
) -> Res<()> {
    let subject = message.lines().next().unwrap_or("").trim();
    let label = match (options.amend, subject) {
        (true, "") => "Amend last commit".to_string(),
        (true, s) => format!("Amend \"{s}\""),
        (false, s) => format!("Commit \"{s}\""),
    };
    let lock = state.index.clone();
    journaled(&state, Action::new(label, Mode::Soft), move |r| {
        with_index_lock(&lock, r, |r| git::commit(r, &message, &options))
    })
    .await
}

#[tauri::command]
pub async fn commit_template(state: State<'_, AppState>) -> Res<Option<String>> {
    in_repo(&state, move |r| Ok(git::commit_template(r))).await
}

#[tauri::command]
pub async fn recent_authors(state: State<'_, AppState>) -> Res<Vec<String>> {
    in_repo(&state, git::recent_authors).await
}

/// Runs the user's own agent CLI for a commit message (off unless they set one up).
#[tauri::command]
pub async fn suggest_message(
    state: State<'_, AppState>,
    command: String,
    prompt: String,
    scope: suggest::Scope,
) -> Res<String> {
    let cancel = state.suggest.start();
    let flag = cancel.clone();
    let out = in_repo(&state, move |r| {
        suggest::run(r, &command, &prompt, scope, &flag)
    })
    .await;
    state.suggest.finish(&cancel);
    out
}

#[tauri::command]
pub fn suggest_cancel(state: State<'_, AppState>) {
    state.suggest.cancel()
}

#[tauri::command]
pub async fn resolve_side(state: State<'_, AppState>, path: String, side: git::Side) -> Res<()> {
    indexed(&state, move |r| git::resolve_side(r, &path, side)).await
}
