use crate::journal::{Action, Mode};
use crate::state::{
    in_repo, indexed, indexed_once, journaled, read_repo, watch_network, with_index_lock, AppState,
    Res,
};
use crate::{git, github, lines, network, suggest};
use tauri::ipc::Channel;
use tauri::State;

#[tauri::command]
pub async fn status(state: State<'_, AppState>) -> Res<git::RepoStatus> {
    read_repo(&state, |r| {
        let mut st = git::status(r)?;
        st.web_url = st.origin.as_ref().map(|url| {
            url.as_deref()
                .and_then(github::parse_remote)
                .map(|g| format!("https://github.com/{}/{}", g.owner, g.name))
        });
        Ok(st)
    })
    .await
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

/// Removes a stale index.lock that `path`, from git's error, names; see git::remove_index_lock.
/// Not while one of ours writes the index.
#[tauri::command]
pub async fn remove_index_lock(state: State<'_, AppState>, path: String) -> Res<()> {
    indexed_once(&state, move |r| git::remove_index_lock(r, &path)).await
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
    op: String,
    progress: Channel<network::Progress>,
) -> Res<()> {
    let subject = message.lines().next().unwrap_or("").trim();
    let label = match (options.amend, subject) {
        (true, "") => "Amend last commit".to_string(),
        (true, s) => format!("Amend \"{s}\""),
        (false, s) => format!("Commit \"{s}\""),
    };
    let lock = state.index.clone();
    let net = watch_network(&state, op, progress);
    journaled(&state, Action::new(label, Mode::Soft).ok_only(), move |r| {
        with_index_lock(&lock, r, |r| git::commit(r, &message, &options, &net))
    })
    .await
}

/// Staged files GitHub would refuse, asked about before they're committed.
#[tauri::command]
pub async fn large_staged(state: State<'_, AppState>) -> Res<Vec<git::LargeFile>> {
    in_repo(&state, git::large_staged).await
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
    let cancel = state.suggest.start(suggest::Kind::Message);
    let flag = cancel.clone();
    let out = in_repo(&state, move |r| {
        suggest::run(r, &command, &prompt, scope, &flag)
    })
    .await;
    state.suggest.finish(suggest::Kind::Message, &cancel);
    out
}

/// The same, for a pull request's title and description from HEAD into `base`.
#[tauri::command]
pub async fn suggest_pull(
    state: State<'_, AppState>,
    command: String,
    prompt: String,
    base: String,
) -> Res<String> {
    let cancel = state.suggest.start(suggest::Kind::Pull);
    let flag = cancel.clone();
    let out = in_repo(&state, move |r| {
        suggest::run_pull(r, &command, &prompt, &base, &flag)
    })
    .await;
    state.suggest.finish(suggest::Kind::Pull, &cancel);
    out
}

#[tauri::command]
pub fn suggest_cancel(state: State<'_, AppState>, kind: suggest::Kind) {
    state.suggest.cancel(kind)
}

#[tauri::command]
pub async fn resolve_side(state: State<'_, AppState>, path: String, side: git::Side) -> Res<()> {
    indexed(&state, move |r| git::resolve_side(r, &path, side)).await
}
