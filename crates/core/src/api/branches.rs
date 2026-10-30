use crate::journal::{Action, Mode};
use crate::state::{Core, Res};
use crate::{git, network};

pub fn branches(core: &Core) -> Res<Vec<git::Branch>> {
    core.read_repo(git::branches)
}

pub fn merged_upstream(core: &Core) -> Res<Vec<String>> {
    core.in_repo(|r| Ok(git::merged_upstream(r)))
}

/// Ahead, behind or gone from their upstreams: asked for when the branch picker opens.
pub fn branch_tracking(core: &Core) -> Res<Vec<git::Tracking>> {
    core.read_repo(git::branch_tracking)
}

/// Where this worktree's HEAD went last, for the picker's Recent group; asked for when it opens.
pub fn recent_branches(core: &Core) -> Res<Vec<String>> {
    core.read_repo(|r| git::recent_branches(r, 12))
}

/// Deletes merged branches: `merged` ones into HEAD, and `upstream` ones git sees as unmerged.
pub fn delete_merged(core: &Core, merged: Vec<String>, upstream: Vec<String>) -> Res<()> {
    let label = match (merged.as_slice(), upstream.as_slice()) {
        ([one], []) | ([], [one]) => format!("Delete branch {one}"),
        _ => format!("Delete {} branches", merged.len() + upstream.len()),
    };
    core.journaled(Action::new(label, Mode::Keep), move |r| {
        git::delete_merged(r, &merged, &upstream)
    })
}

/// `op`: a Git LFS repository downloads the branch's files first, with progress and Cancel.
/// A branch made here downloads nothing, so `op` isn't registered then.
pub fn switch_branch(
    core: &Core,
    name: String,
    create: bool,
    op: String,
    progress: impl Fn(network::Progress) + Send + Sync + 'static,
) -> Res<()> {
    let label = if create {
        format!("Create branch {name}")
    } else {
        format!("Switch to {name}")
    };
    let net = if create {
        network::Net::default()
    } else {
        core.watch_network(op, progress)
    };
    core.journaled(Action::new(label, Mode::Keep), move |r| {
        git::switch_branch(r, &name, create, &net)
    })
}

/// Asked before switching away from a detached HEAD.
pub fn left_behind(core: &Core) -> Res<usize> {
    core.read_repo(git::left_behind)
}

pub fn delete_branches(core: &Core, names: Vec<String>, force: bool) -> Res<()> {
    let label = match names.as_slice() {
        [one] => format!("Delete branch {one}"),
        all => format!("Delete {} branches", all.len()),
    };
    core.journaled(Action::new(label, Mode::Keep), move |r| {
        git::delete_branches(r, &names, force)
    })
}

pub fn create_branch(core: &Core, name: String, base: String, switch: bool) -> Res<()> {
    let label = format!("Create branch {name}");
    core.journaled(Action::new(label, Mode::Keep), move |r| {
        git::create_branch(r, &name, &base, switch)
    })
}

/// Only the local rename is undoable; what `remote` did on the remote stays.
pub fn rename_branch(
    core: &Core,
    old: String,
    new: String,
    remote: bool,
    op: String,
    progress: impl Fn(network::Progress) + Send + Sync + 'static,
) -> Res<()> {
    let label = format!("Rename {old} to {new}");
    let net = core.watch_network(op, progress);
    core.journaled(Action::new(label, Mode::Keep), move |r| {
        git::rename_branch(r, &old, &new, remote, &net)
    })
}

pub fn set_upstream(core: &Core, branch: String, upstream: Option<String>) -> Res<()> {
    core.in_repo(move |r| git::set_upstream(r, &branch, upstream.as_deref()))
}

pub fn tags(core: &Core) -> Res<Vec<String>> {
    core.in_repo(git::tags)
}

pub fn delete_remote_branch(
    core: &Core,
    name: String,
    op: String,
    progress: impl Fn(network::Progress) + Send + Sync + 'static,
) -> Res<()> {
    let net = core.watch_network(op, progress);
    core.in_repo(move |r| git::delete_remote_branch(r, &name, &net))
}

pub fn create_tag(core: &Core, name: String, sha: String, message: Option<String>) -> Res<()> {
    let label = format!("Create tag {name}");
    core.journaled(Action::new(label, Mode::Keep).with_tags(), move |r| {
        git::create_tag(r, &name, &sha, message.as_deref())
    })
}

pub fn delete_tag(core: &Core, name: String) -> Res<()> {
    let label = format!("Delete tag {name}");
    core.journaled(Action::new(label, Mode::Keep).with_tags(), move |r| {
        git::delete_tag(r, &name)
    })
}

/// Remote tag changes are not undoable: others may have fetched them already.
pub fn push_tags(
    core: &Core,
    names: Vec<String>,
    op: String,
    progress: impl Fn(network::Progress) + Send + Sync + 'static,
) -> Res<String> {
    let net = core.watch_network(op, progress);
    core.in_repo(move |r| git::push_tags(r, &names, &net))
}

pub fn delete_remote_tag(
    core: &Core,
    name: String,
    op: String,
    progress: impl Fn(network::Progress) + Send + Sync + 'static,
) -> Res<String> {
    let net = core.watch_network(op, progress);
    core.in_repo(move |r| git::delete_remote_tag(r, &name, &net))
}

pub fn remote_tags(
    core: &Core,
    op: String,
    progress: impl Fn(network::Progress) + Send + Sync + 'static,
) -> Res<git::RemoteTags> {
    let net = core.watch_network(op, progress);
    core.in_repo(move |r| git::remote_tags(r, &net))
}

pub fn switch_tracking(core: &Core, remote_ref: String) -> Res<()> {
    let local = remote_ref
        .split_once('/')
        .map_or(remote_ref.as_str(), |(_, b)| b);
    let label = format!("Switch to {local}");
    core.journaled(Action::new(label, Mode::Keep), move |r| {
        git::switch_tracking(r, &remote_ref)
    })
}

pub fn set_push_default(core: &Core, remote: String) -> Res<()> {
    core.in_repo(move |r| git::set_push_default(r, &remote))
}
