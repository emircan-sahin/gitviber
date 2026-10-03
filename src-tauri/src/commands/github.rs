//! GitHub: the account, forks, pull requests and issues.

use crate::journal::{Action, Mode};
use crate::state::{blocking, in_repo, repo, watch_network, with_github, AppState, Res};
use crate::{git, github, network};
use std::path::PathBuf;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};

/// The commit to link `path` (or its `lines`) to on GitHub, so the link keeps showing that code.
#[tauri::command]
pub async fn github_permalink(
    state: State<'_, AppState>,
    path: String,
    lines: Option<(u32, u32)>,
) -> Res<git::Permalink> {
    in_repo(&state, move |r| git::permalink(r, &path, lines)).await
}

#[tauri::command]
pub async fn gh_account(app: AppHandle) -> Res<github::Account> {
    with_github(app, github::account).await
}

/// Looks for a GitHub token now, as the user asked to: gh's, and with `store` git's credential
/// store too (which may show the OS's password dialog). Returns where it came from. Nothing else
/// asks the store, so a screen load never raises that dialog.
#[tauri::command]
pub async fn gh_connect(app: AppHandle, store: bool) -> Res<String> {
    with_github(app, move |gh, r| gh.connect(r, store).map(str::to_string)).await
}

/// Set as a project opens, from what the user chose before: whether git's credential store may be
/// asked once a session without a click.
#[tauri::command]
pub fn gh_allow_store(state: State<'_, AppState>, allow: bool) {
    state.github.allow_store(allow);
}

/// gh's accounts on github.com; asked for when the repository's settings open.
#[tauri::command]
pub async fn gh_accounts() -> Res<Vec<github::GhAccount>> {
    blocking(|| Ok(github::accounts())).await
}

/// `login`: the gh account the open repository's GitHub calls use; None for gh's active one. Sent
/// as each repository opens, before its GitHub calls start, and when the pick changes.
#[tauri::command]
pub fn gh_use_account(state: State<'_, AppState>, login: Option<String>) -> Res<()> {
    github::use_account(&state.github, login)
}

#[tauri::command]
pub async fn gh_original_remote(
    state: State<'_, AppState>,
    original: String,
    fetch: bool,
    op: String,
    progress: Channel<network::Progress>,
) -> Res<Option<String>> {
    let net = watch_network(&state, op, progress);
    in_repo(&state, move |r| {
        github::original_remote(r, &original, fetch, &net)
    })
    .await
}

#[tauri::command]
pub async fn gh_sync_fork(
    app: AppHandle,
    branch: String,
    op: String,
    progress: Channel<network::Progress>,
) -> Res<String> {
    let net = watch_network(&app.state::<AppState>(), op, progress);
    with_github(app, move |gh, r| github::sync_fork(gh, r, &branch, &net)).await
}

#[tauri::command]
pub async fn pull_draft(state: State<'_, AppState>, base: String) -> Res<git::PullDraft> {
    in_repo(&state, move |r| git::pull_draft(r, &base)).await
}

#[tauri::command]
pub async fn gh_remotes(state: State<'_, AppState>) -> Res<Vec<github::Remote>> {
    in_repo(&state, move |r| Ok(github::remotes(r))).await
}

#[tauri::command]
pub async fn gh_add_original_remote(
    app: AppHandle,
    op: String,
    progress: Channel<network::Progress>,
) -> Res<String> {
    let net = watch_network(&app.state::<AppState>(), op, progress);
    with_github(app, move |gh, r| github::add_original_remote(gh, r, &net)).await
}

#[tauri::command]
pub async fn gh_protected_branches(app: AppHandle) -> Res<Vec<String>> {
    with_github(app, github::protected_branches).await
}

#[tauri::command]
pub async fn pr_list(
    app: AppHandle,
    target: Option<String>,
    filter: String,
    pages: usize,
) -> Res<Vec<github::Pull>> {
    with_github(app, move |gh, r| {
        github::list(gh, r, target.as_deref(), &filter, pages)
    })
    .await
}

#[tauri::command]
pub async fn pr_counts(app: AppHandle, target: Option<String>) -> Res<github::StateCounts> {
    with_github(app, move |gh, r| {
        github::pull_counts(gh, r, target.as_deref())
    })
    .await
}

#[tauri::command]
pub async fn pr_review_comments(
    app: AppHandle,
    target: Option<String>,
    number: u64,
) -> Res<Vec<github::ReviewComment>> {
    with_github(app, move |gh, r| {
        github::review_comments(gh, r, target.as_deref(), number)
    })
    .await
}

/// A comment on a line of the PR's diff, or a reply in a thread (`reply_to`).
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn pr_comment_line(
    app: AppHandle,
    target: Option<String>,
    number: u64,
    commit: String,
    path: String,
    line: u64,
    side: String,
    reply_to: Option<u64>,
    body: String,
) -> Res<github::ReviewComment> {
    with_github(app, move |gh, r| {
        github::comment_line(
            gh,
            r,
            target.as_deref(),
            number,
            &commit,
            &path,
            line,
            &side,
            reply_to,
            &body,
        )
    })
    .await
}

/// The GitHub account's repositories, for cloning one; works with no repository open.
#[tauri::command]
pub async fn gh_own_repos(app: AppHandle) -> Res<Vec<github::OwnRepo>> {
    blocking(move || {
        let state = app.state::<AppState>();
        let cwd = repo(&state)
            .ok()
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
            .unwrap_or_else(std::env::temp_dir);
        github::own_repos(&state.github, &cwd)
    })
    .await
}

/// CI's rollup for each commit GitHub has checks on (see github::ci_states).
#[tauri::command]
pub async fn ci_states(
    app: AppHandle,
    target: Option<String>,
    shas: Vec<String>,
) -> Res<std::collections::HashMap<String, String>> {
    with_github(app, move |gh, r| {
        github::ci_states(gh, r, target.as_deref(), &shas)
    })
    .await
}

/// Why check run `id` failed: its output, annotations and job log's tail. Asked for from its row.
#[tauri::command]
pub async fn check_failure(
    app: AppHandle,
    target: Option<String>,
    id: u64,
) -> Res<github::CheckFailure> {
    with_github(app, move |gh, r| {
        github::check_failure(gh, r, target.as_deref(), id)
    })
    .await
}

#[tauri::command]
pub async fn pr_detail(
    app: AppHandle,
    target: Option<String>,
    number: u64,
) -> Res<github::PullDetail> {
    with_github(app, move |gh, r| {
        github::detail(gh, r, target.as_deref(), number)
    })
    .await
}

#[tauri::command]
pub async fn pr_attachments(
    app: AppHandle,
    target: Option<String>,
    number: u64,
) -> Res<std::collections::HashMap<String, String>> {
    with_github(app, move |gh, r| {
        github::attachments(gh, r, target.as_deref(), number)
    })
    .await
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn pr_files(
    app: AppHandle,
    target: Option<String>,
    number: u64,
    base_ref: String,
    base_sha: String,
    head_sha: String,
    op: String,
    progress: Channel<network::Progress>,
) -> Res<github::PullFiles> {
    let net = watch_network(&app.state::<AppState>(), op, progress);
    with_github(app, move |gh, r| {
        github::files(
            gh,
            r,
            target.as_deref(),
            number,
            &base_ref,
            &base_sha,
            &head_sha,
            &net,
        )
    })
    .await
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn pr_create(
    app: AppHandle,
    target: Option<String>,
    title: String,
    body: String,
    head: String,
    base: String,
    draft: bool,
    maintainer_edits: bool,
) -> Res<github::Pull> {
    with_github(app, move |gh, r| {
        github::create(
            gh,
            r,
            target.as_deref(),
            &title,
            &body,
            &head,
            &base,
            draft,
            maintainer_edits,
        )
    })
    .await
}

#[tauri::command]
pub async fn pr_merge(
    app: AppHandle,
    target: Option<String>,
    number: u64,
    method: String,
) -> Res<()> {
    with_github(app, move |gh, r| {
        github::merge(gh, r, target.as_deref(), number, &method)
    })
    .await
}

#[tauri::command]
pub async fn pr_set_open(
    app: AppHandle,
    target: Option<String>,
    number: u64,
    open: bool,
) -> Res<github::Pull> {
    with_github(app, move |gh, r| {
        github::set_open(gh, r, target.as_deref(), number, open)
    })
    .await
}

#[tauri::command]
pub async fn pr_review(
    app: AppHandle,
    target: Option<String>,
    number: u64,
    event: String,
    body: String,
) -> Res<()> {
    with_github(app, move |gh, r| {
        github::review(gh, r, target.as_deref(), number, &event, &body)
    })
    .await
}

#[tauri::command]
pub async fn issue_list(
    app: AppHandle,
    target: Option<String>,
    filter: String,
    labels: Vec<String>,
) -> Res<Vec<github::Issue>> {
    with_github(app, move |gh, r| {
        github::issues(gh, r, target.as_deref(), &filter, &labels)
    })
    .await
}

#[tauri::command]
pub async fn issue_counts(
    app: AppHandle,
    target: Option<String>,
    labels: Vec<String>,
) -> Res<github::StateCounts> {
    with_github(app, move |gh, r| {
        github::issue_counts(gh, r, target.as_deref(), &labels)
    })
    .await
}

#[tauri::command]
pub async fn issue_labels(app: AppHandle, target: Option<String>) -> Res<Vec<github::Label>> {
    with_github(app, move |gh, r| {
        github::issue_labels(gh, r, target.as_deref())
    })
    .await
}

#[tauri::command]
pub async fn issue_detail(
    app: AppHandle,
    target: Option<String>,
    number: u64,
) -> Res<github::IssueDetail> {
    with_github(app, move |gh, r| {
        github::issue_detail(gh, r, target.as_deref(), number)
    })
    .await
}

#[tauri::command]
pub async fn issue_create(
    app: AppHandle,
    target: Option<String>,
    title: String,
    body: String,
) -> Res<github::Issue> {
    with_github(app, move |gh, r| {
        github::issue_create(gh, r, target.as_deref(), &title, &body)
    })
    .await
}

#[tauri::command]
pub async fn issue_edit(
    app: AppHandle,
    target: Option<String>,
    number: u64,
    title: String,
    body: String,
) -> Res<github::Issue> {
    with_github(app, move |gh, r| {
        github::issue_edit(gh, r, target.as_deref(), number, &title, &body)
    })
    .await
}

#[tauri::command]
pub async fn issue_set_open(
    app: AppHandle,
    target: Option<String>,
    number: u64,
    open: bool,
    reason: String,
) -> Res<github::Issue> {
    with_github(app, move |gh, r| {
        github::issue_set_open(gh, r, target.as_deref(), number, open, &reason)
    })
    .await
}

#[tauri::command]
pub async fn issue_set_labels(
    app: AppHandle,
    target: Option<String>,
    number: u64,
    labels: Vec<String>,
) -> Res<Vec<github::Label>> {
    with_github(app, move |gh, r| {
        github::issue_set_labels(gh, r, target.as_deref(), number, &labels)
    })
    .await
}

#[tauri::command]
pub async fn issue_delete(app: AppHandle, target: Option<String>, number: u64) -> Res<()> {
    with_github(app, move |gh, r| {
        github::issue_delete(gh, r, target.as_deref(), number)
    })
    .await
}

#[tauri::command]
pub async fn issue_comment(
    app: AppHandle,
    target: Option<String>,
    number: u64,
    body: String,
) -> Res<()> {
    with_github(app, move |gh, r| {
        github::issue_comment(gh, r, target.as_deref(), number, &body)
    })
    .await
}

#[tauri::command]
pub async fn pr_checkout(
    app: AppHandle,
    target: Option<String>,
    number: u64,
    head_ref: String,
    same_repo: bool,
    op: String,
    progress: Channel<network::Progress>,
) -> Res<()> {
    let net = watch_network(&app.state::<AppState>(), op, progress);
    blocking(move || {
        let state = app.state::<AppState>();
        let r = repo(&state)?;
        let action = Action::new(format!("Check out PR #{number}"), Mode::Keep);
        state.journal.record(&r, action, |r| {
            let (remote, owner) = github::pr_source(&state.github, r, target.as_deref())?;
            github::checkout(
                r,
                &remote,
                owner.as_deref(),
                number,
                &head_ref,
                same_repo,
                &net,
            )
        })
    })
    .await
}

/// `pr_checkout` into a new worktree; returns its path. This worktree's HEAD doesn't move.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn pr_checkout_worktree(
    app: AppHandle,
    target: Option<String>,
    number: u64,
    head_ref: String,
    same_repo: bool,
    dir: Option<String>,
    op: String,
    progress: Channel<network::Progress>,
) -> Res<String> {
    let net = watch_network(&app.state::<AppState>(), op, progress);
    blocking(move || {
        let state = app.state::<AppState>();
        let r = repo(&state)?;
        let (remote, owner) = github::pr_source(&state.github, &r, target.as_deref())?;
        github::checkout_worktree(
            &r,
            &remote,
            owner.as_deref(),
            number,
            &head_ref,
            same_repo,
            dir.as_deref(),
            &net,
        )
    })
    .await
}
