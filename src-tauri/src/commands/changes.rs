use crate::journal::{files_label, Action, Mode};
use crate::state::{
    blocking, in_repo, indexed, indexed_once, journaled, read_repo, watch_network, with_index_lock,
    AppState, Res,
};
use crate::{git, github, handoff, lines, network, suggest};
use std::path::Path;
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
        let label = files_label("Discard", &paths);
        let list = paths.clone();
        let write = move |r: &Path| git::discard(r, &list).map(|_| vec![]);
        journal
            .replace(r, label, "discarded", &paths, &index, write)
            .map(|_| ())
    })
    .await
}

/// Stages, unstages or discards some of a file's changed lines; a discard can be undone.
#[tauri::command]
pub async fn change_lines(state: State<'_, AppState>, request: lines::Request) -> Res<()> {
    if request.action != "discard" {
        return indexed(&state, move |r| lines::run(r, &request)).await;
    }
    let (journal, index) = (state.journal.clone(), state.index.clone());
    in_repo(&state, move |r| {
        let paths = [request.path.clone()];
        let label = files_label("Discard", &paths);
        let write = move |r: &Path| lines::run(r, &request).map(|_| vec![]);
        journal
            .replace(r, label, "discarded", &paths, &index, write)
            .map(|_| ())
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

/// The same, for a guided review of a commit, a pull request or the branch since it left its
/// base, or for its risks (`kind`): the two run side by side, on the same range.
#[tauri::command]
pub async fn suggest_guide(
    state: State<'_, AppState>,
    command: String,
    prompt: String,
    target: suggest::Target,
    agent: suggest::Agent,
    kind: suggest::Kind,
) -> Res<suggest::Guided> {
    if !matches!(kind, suggest::Kind::Guide | suggest::Kind::Risks) {
        return Err("not a guided review's run".into());
    }
    let cancel = state.suggest.start(kind);
    let flag = cancel.clone();
    let out = in_repo(&state, move |r| {
        suggest::run_guide(r, &command, &prompt, &target, &agent, &flag)
    })
    .await;
    state.suggest.finish(kind, &cancel);
    out
}

/// A guided review handed to Claude Code in a terminal: the line to type at the shell's prompt.
#[tauri::command]
pub async fn handoff_command(
    command: String,
    model: Option<String>,
    effort: Option<String>,
    name: String,
    context: String,
    prompt: Option<String>,
) -> Res<String> {
    blocking(move || {
        handoff::command(&handoff::Handoff {
            command: &command,
            model: model.as_deref(),
            effort: effort.as_deref(),
            name: &name,
            context: &context,
            prompt: prompt.as_deref(),
        })
    })
    .await
}

#[tauri::command]
pub fn suggest_cancel(state: State<'_, AppState>, kind: suggest::Kind) {
    state.suggest.cancel(kind)
}

#[tauri::command]
pub async fn resolve_side(state: State<'_, AppState>, path: String, side: git::Side) -> Res<()> {
    indexed(&state, move |r| git::resolve_side(r, &path, side)).await
}

/// A conflicted file's sides: merged again in diff3 style for each conflict's base, and how
/// each ends.
#[tauri::command]
pub async fn conflict_base(
    state: State<'_, AppState>,
    path: String,
) -> Res<Option<git::ConflictSides>> {
    read_repo(&state, move |r| git::conflict_sides(r, &path)).await
}

/// The merge and diff tools git's config names, for "Open in <tool>"; read when a menu or a
/// conflict asks.
#[tauri::command]
pub async fn external_tools(state: State<'_, AppState>) -> Res<git::ExternalTools> {
    read_repo(&state, |r| Ok(git::external_tools(r))).await
}

/// Returns once the tool is closed, so the page reads the file and the index again.
#[tauri::command]
pub async fn open_merge_tool(state: State<'_, AppState>, path: String) -> Res<()> {
    in_repo(&state, move |r| git::open_merge_tool(r, &path)).await
}

#[tauri::command]
pub async fn open_diff_tool(state: State<'_, AppState>, path: String, staged: bool) -> Res<()> {
    in_repo(&state, move |r| git::open_diff_tool(r, &path, staged)).await
}
