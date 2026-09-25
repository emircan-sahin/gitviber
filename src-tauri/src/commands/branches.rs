use crate::journal::{Action, Mode};
use crate::state::{in_repo, journaled, watch_network, AppState, Res};
use crate::{git, network};
use tauri::ipc::Channel;
use tauri::State;

#[tauri::command]
pub async fn branches(state: State<'_, AppState>) -> Res<Vec<git::Branch>> {
    in_repo(&state, git::branches).await
}

#[tauri::command]
pub async fn switch_branch(state: State<'_, AppState>, name: String, create: bool) -> Res<()> {
    let label = if create {
        format!("Create branch {name}")
    } else {
        format!("Switch to {name}")
    };
    journaled(&state, Action::new(label, Mode::Keep), move |r| {
        git::switch_branch(r, &name, create)
    })
    .await
}

#[tauri::command]
pub async fn delete_branches(
    state: State<'_, AppState>,
    names: Vec<String>,
    force: bool,
) -> Res<()> {
    let label = match names.as_slice() {
        [one] => format!("Delete branch {one}"),
        all => format!("Delete {} branches", all.len()),
    };
    journaled(&state, Action::new(label, Mode::Keep), move |r| {
        git::delete_branches(r, &names, force)
    })
    .await
}

#[tauri::command]
pub async fn create_branch(
    state: State<'_, AppState>,
    name: String,
    base: String,
    switch: bool,
) -> Res<()> {
    let label = format!("Create branch {name}");
    journaled(&state, Action::new(label, Mode::Keep), move |r| {
        git::create_branch(r, &name, &base, switch)
    })
    .await
}

/// Only the local rename is undoable; what `remote` did on the remote stays.
#[tauri::command]
pub async fn rename_branch(
    state: State<'_, AppState>,
    old: String,
    new: String,
    remote: bool,
    op: String,
    progress: Channel<network::Progress>,
) -> Res<()> {
    let label = format!("Rename {old} to {new}");
    let net = watch_network(&state, op, progress);
    journaled(&state, Action::new(label, Mode::Keep), move |r| {
        git::rename_branch(r, &old, &new, remote, &net)
    })
    .await
}

#[tauri::command]
pub async fn set_upstream(
    state: State<'_, AppState>,
    branch: String,
    upstream: Option<String>,
) -> Res<()> {
    in_repo(&state, move |r| {
        git::set_upstream(r, &branch, upstream.as_deref())
    })
    .await
}

#[tauri::command]
pub async fn tags(state: State<'_, AppState>) -> Res<Vec<String>> {
    in_repo(&state, git::tags).await
}

#[tauri::command]
pub async fn delete_remote_branch(
    state: State<'_, AppState>,
    name: String,
    op: String,
    progress: Channel<network::Progress>,
) -> Res<()> {
    let net = watch_network(&state, op, progress);
    in_repo(&state, move |r| git::delete_remote_branch(r, &name, &net)).await
}

#[tauri::command]
pub async fn create_tag(
    state: State<'_, AppState>,
    name: String,
    sha: String,
    message: Option<String>,
) -> Res<()> {
    let label = format!("Create tag {name}");
    journaled(
        &state,
        Action::new(label, Mode::Keep).with_tags(),
        move |r| git::create_tag(r, &name, &sha, message.as_deref()),
    )
    .await
}

#[tauri::command]
pub async fn delete_tag(state: State<'_, AppState>, name: String) -> Res<()> {
    let label = format!("Delete tag {name}");
    journaled(
        &state,
        Action::new(label, Mode::Keep).with_tags(),
        move |r| git::delete_tag(r, &name),
    )
    .await
}

/// Remote tag changes are not undoable: others may have fetched them already.
#[tauri::command]
pub async fn push_tags(
    state: State<'_, AppState>,
    names: Vec<String>,
    op: String,
    progress: Channel<network::Progress>,
) -> Res<String> {
    let net = watch_network(&state, op, progress);
    in_repo(&state, move |r| git::push_tags(r, &names, &net)).await
}

#[tauri::command]
pub async fn delete_remote_tag(
    state: State<'_, AppState>,
    name: String,
    op: String,
    progress: Channel<network::Progress>,
) -> Res<String> {
    let net = watch_network(&state, op, progress);
    in_repo(&state, move |r| git::delete_remote_tag(r, &name, &net)).await
}

#[tauri::command]
pub async fn remote_tags(
    state: State<'_, AppState>,
    op: String,
    progress: Channel<network::Progress>,
) -> Res<git::RemoteTags> {
    let net = watch_network(&state, op, progress);
    in_repo(&state, move |r| git::remote_tags(r, &net)).await
}

#[tauri::command]
pub async fn switch_tracking(state: State<'_, AppState>, remote_ref: String) -> Res<()> {
    let local = remote_ref
        .split_once('/')
        .map_or(remote_ref.as_str(), |(_, b)| b);
    let label = format!("Switch to {local}");
    journaled(&state, Action::new(label, Mode::Keep), move |r| {
        git::switch_tracking(r, &remote_ref)
    })
    .await
}

#[tauri::command]
pub async fn set_push_default(state: State<'_, AppState>, remote: String) -> Res<()> {
    in_repo(&state, move |r| git::set_push_default(r, &remote)).await
}
