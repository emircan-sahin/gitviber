use crate::journal::{Action, Mode};
use crate::state::{in_repo, journaled, watch_network, with_index_lock, AppState, Res};
use crate::{git, network};
use tauri::ipc::Channel;
use tauri::State;

#[tauri::command]
pub async fn push(
    state: State<'_, AppState>,
    force: Option<bool>,
    remote: Option<String>,
    tags: Option<bool>,
    op: String,
    progress: Channel<network::Progress>,
) -> Res<()> {
    let net = watch_network(&state, op, progress);
    let push = if tags.unwrap_or(false) {
        git::push_with_tags
    } else {
        git::push
    };
    in_repo(&state, move |r| {
        push(r, force.unwrap_or(false), remote.as_deref(), &net)
    })
    .await
}

/// See git::remote_was_ours.
#[tauri::command]
pub async fn remote_was_ours(state: State<'_, AppState>) -> Res<bool> {
    in_repo(&state, |r| Ok(git::remote_was_ours(r))).await
}

/// The bool results below mean "stopped on conflicts".
#[tauri::command]
pub async fn pull(
    state: State<'_, AppState>,
    mode: git::PullMode,
    autostash: Option<bool>,
    op: String,
    progress: Channel<network::Progress>,
) -> Res<bool> {
    let label = match mode {
        git::PullMode::Ff => "Pull",
        git::PullMode::Merge => "Pull (merge)",
        git::PullMode::Rebase => "Pull (rebase)",
    };
    let net = watch_network(&state, op, progress);
    journaled(&state, Action::new(label, Mode::Keep), move |r| {
        git::pull(r, mode, autostash.unwrap_or(false), &net)
    })
    .await
}

#[tauri::command]
pub async fn merge(
    state: State<'_, AppState>,
    name: String,
    how: Option<git::MergeKind>,
) -> Res<bool> {
    let how = how.unwrap_or(git::MergeKind::Ff);
    let label = match how {
        git::MergeKind::Squash => format!("Squash merge {name}"),
        git::MergeKind::Ff | git::MergeKind::NoFf => format!("Merge {name}"),
    };
    journaled(&state, Action::new(label, Mode::Keep), move |r| {
        git::merge(r, &name, how)
    })
    .await
}

#[tauri::command]
pub async fn rebase(state: State<'_, AppState>, onto: String) -> Res<bool> {
    let label = format!("Rebase onto {onto}");
    journaled(&state, Action::new(label, Mode::Keep), move |r| {
        git::rebase(r, &onto)
    })
    .await
}

// These finish (or call off) the action that stopped on conflicts; its entry is recorded then.
#[tauri::command]
pub async fn op_continue(state: State<'_, AppState>) -> Res<bool> {
    let lock = state.index.clone();
    journaled(&state, Action::new("Continue", Mode::Keep), move |r| {
        with_index_lock(&lock, r, git::op_continue)
    })
    .await
}

#[tauri::command]
pub async fn op_abort(state: State<'_, AppState>) -> Res<()> {
    journaled(&state, Action::new("Abort", Mode::Keep), git::op_abort).await
}

#[tauri::command]
pub async fn rebase_skip(state: State<'_, AppState>) -> Res<bool> {
    journaled(&state, Action::new("Skip", Mode::Keep), git::rebase_skip).await
}

#[tauri::command]
pub async fn fetch(
    state: State<'_, AppState>,
    op: String,
    progress: Channel<network::Progress>,
) -> Res<()> {
    let net = watch_network(&state, op, progress);
    in_repo(&state, move |r| git::fetch(r, &net)).await
}

#[tauri::command]
pub async fn submodules(state: State<'_, AppState>) -> Res<Vec<git::Submodule>> {
    in_repo(&state, git::submodules).await
}

#[tauri::command]
pub async fn submodule_update(
    state: State<'_, AppState>,
    op: String,
    progress: Channel<network::Progress>,
) -> Res<()> {
    let net = watch_network(&state, op, progress);
    in_repo(&state, move |r| git::submodule_update(r, &net)).await
}

#[tauri::command]
pub async fn lfs_pull(
    state: State<'_, AppState>,
    path: String,
    op: String,
    progress: Channel<network::Progress>,
) -> Res<()> {
    let net = watch_network(&state, op, progress);
    in_repo(&state, move |r| git::lfs_pull(r, &path, &net)).await
}

#[tauri::command]
pub async fn last_fetch(state: State<'_, AppState>) -> Res<Option<u64>> {
    in_repo(&state, move |r| Ok(git::last_fetch(r))).await
}

/// Stops the network command the page started under `op`; it then fails with "git:cancelled".
#[tauri::command]
pub fn cancel_network(state: State<'_, AppState>, op: String) {
    state.network.cancel(&op)
}
