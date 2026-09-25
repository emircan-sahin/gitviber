use crate::state::{blocking, in_repo, AppState, Res};
use crate::{definitions, fs, git, grep, open_in};
use tauri::State;

#[tauri::command]
pub async fn diff_pair(
    state: State<'_, AppState>,
    kind: String,
    path: String,
    old_path: Option<String>,
    sha: Option<String>,
    base: Option<String>,
    whitespace: Option<String>,
) -> Res<git::DiffPair> {
    in_repo(&state, move |r| {
        git::diff_pair(
            r,
            &kind,
            &path,
            old_path.as_deref(),
            sha.as_deref(),
            base.as_deref(),
            whitespace.as_deref(),
            |p| fs::read_file(r, p),
        )
    })
    .await
}

/// Raw bytes, not JSON: a video as a number array would be several times its size.
#[tauri::command]
pub async fn media(
    state: State<'_, AppState>,
    kind: String,
    path: String,
    old_path: Option<String>,
    sha: Option<String>,
    base: Option<String>,
    original: bool,
) -> Res<tauri::ipc::Response> {
    let bytes = in_repo(&state, move |r| {
        git::media(
            r,
            &kind,
            &path,
            old_path.as_deref(),
            sha.as_deref(),
            base.as_deref(),
            original,
            |p| fs::read_media(r, p),
        )
    })
    .await?;
    Ok(tauri::ipc::Response::new(bytes))
}

#[tauri::command]
pub async fn list_dir(state: State<'_, AppState>, path: String) -> Res<Vec<fs::Entry>> {
    in_repo(&state, move |r| fs::list_dir(r, &path)).await
}

#[tauri::command]
pub async fn list_files(state: State<'_, AppState>) -> Res<Vec<String>> {
    in_repo(&state, fs::list_files).await
}

/// Search in files; a newer search stops this one, which then fails with grep::CANCELLED.
#[tauri::command]
pub async fn search_files(state: State<'_, AppState>, query: grep::Query) -> Res<grep::Found> {
    in_repo(&state, move |r| grep::search(r, &query)).await
}

/// Stops the running search: its query was cleared, or the view went.
#[tauri::command]
pub fn cancel_search() {
    grep::cancel();
}

/// Go to Definition in the code view; a newer lookup stops this one (definitions::CANCELLED).
#[tauri::command]
pub async fn definitions(
    state: State<'_, AppState>,
    request: definitions::Request,
) -> Res<Vec<definitions::Location>> {
    in_repo(&state, move |r| definitions::find(r, &request)).await
}

/// Go to References in the code view; a newer lookup stops this one (definitions::CANCELLED).
#[tauri::command]
pub async fn references(
    state: State<'_, AppState>,
    request: definitions::Request,
) -> Res<Vec<definitions::Location>> {
    in_repo(&state, move |r| definitions::references(r, &request)).await
}

#[tauri::command]
pub async fn read_file(state: State<'_, AppState>, path: String) -> Res<git::FileText> {
    in_repo(&state, move |r| Ok(fs::read_file(r, &path))).await
}

#[tauri::command]
pub async fn tree_paths(state: State<'_, AppState>, rev: String) -> Res<Vec<String>> {
    in_repo(&state, move |r| git::tree_paths(r, &rev)).await
}

#[tauri::command]
pub async fn text_at(state: State<'_, AppState>, rev: String, path: String) -> Res<git::FileText> {
    in_repo(&state, move |r| git::text_at(r, &rev, &path)).await
}

#[tauri::command]
pub async fn write_file(state: State<'_, AppState>, path: String, content: String) -> Res<()> {
    in_repo(&state, move |r| fs::write_file(r, &path, &content)).await
}

#[tauri::command]
pub async fn create_file(state: State<'_, AppState>, path: String) -> Res<()> {
    in_repo(&state, move |r| fs::create_file(r, &path)).await
}

#[tauri::command]
pub async fn create_dir(state: State<'_, AppState>, path: String) -> Res<()> {
    in_repo(&state, move |r| fs::create_dir(r, &path)).await
}

#[tauri::command]
pub async fn rename_path(state: State<'_, AppState>, from: String, to: String) -> Res<()> {
    in_repo(&state, move |r| fs::rename_entry(r, &from, &to)).await
}

#[tauri::command]
pub async fn trash_path(state: State<'_, AppState>, path: String) -> Res<()> {
    in_repo(&state, move |r| fs::trash(r, &path)).await
}

#[tauri::command]
pub async fn reveal_path(state: State<'_, AppState>, path: String) -> Res<()> {
    in_repo(&state, move |r| fs::reveal(r, &path)).await
}

/// The apps "Open in…" can use, as installed right now.
#[tauri::command]
pub async fn open_in_apps() -> Res<Vec<open_in::Installed>> {
    blocking(|| Ok(open_in::installed())).await
}

/// `path` in the open worktree ("" for all of it) in a known app; editors go to `line`.
#[tauri::command]
pub async fn open_in(
    state: State<'_, AppState>,
    app: String,
    path: String,
    line: Option<u32>,
) -> Res<()> {
    in_repo(&state, move |r| open_in::open(r, &path, line, &app)).await
}

/// The same with the user's own command template.
#[tauri::command]
pub async fn open_in_custom(
    state: State<'_, AppState>,
    command: String,
    path: String,
    line: Option<u32>,
) -> Res<()> {
    in_repo(&state, move |r| {
        open_in::open_custom(r, &path, line, &command)
    })
    .await
}
