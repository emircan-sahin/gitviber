use crate::journal::{Action, Mode};
use crate::state::{with_index_lock, Core, Res};
use crate::{askpass, git, network, rewrite};

pub fn push(
    core: &Core,
    force: Option<bool>,
    remote: Option<String>,
    tags: Option<bool>,
    op: String,
    progress: impl Fn(network::Progress) + Send + Sync + 'static,
) -> Res<()> {
    let net = core.watch_network(op, progress);
    let push = if tags.unwrap_or(false) {
        git::push_with_tags
    } else {
        git::push
    };
    core.in_repo(move |r| push(r, force.unwrap_or(false), remote.as_deref(), &net))
}

/// See git::remote_was_ours.
pub fn remote_was_ours(core: &Core) -> Res<bool> {
    core.in_repo(|r| Ok(git::remote_was_ours(r)))
}

/// The bool results below mean "stopped on conflicts".
pub fn pull(
    core: &Core,
    mode: git::PullMode,
    autostash: Option<bool>,
    op: String,
    progress: impl Fn(network::Progress) + Send + Sync + 'static,
) -> Res<bool> {
    let label = match mode {
        git::PullMode::Ff => "Pull",
        git::PullMode::Merge => "Pull (merge)",
        git::PullMode::Rebase => "Pull (rebase)",
    };
    let net = core.watch_network(op, progress);
    core.journaled(Action::new(label, Mode::Keep), move |r| {
        git::pull(r, mode, autostash.unwrap_or(false), &net)
    })
}

pub fn merge(
    core: &Core,
    name: String,
    how: Option<git::MergeKind>,
    autostash: Option<bool>,
) -> Res<bool> {
    let how = how.unwrap_or(git::MergeKind::Ff);
    // Compare hands over a full ref; the undo list names the branch.
    let shown = git::short_ref(&name);
    let label = match how {
        git::MergeKind::Squash => format!("Squash merge {shown}"),
        git::MergeKind::Ff | git::MergeKind::NoFf => format!("Merge {shown}"),
    };
    core.journaled(Action::new(label, Mode::Keep), move |r| {
        git::merge(r, &name, how, autostash.unwrap_or(false))
    })
}

pub fn rebase(
    core: &Core,
    onto: String,
    autostash: Option<bool>,
    update_refs: Option<bool>,
) -> Res<bool> {
    let label = format!("Rebase onto {onto}");
    core.journaled(Action::new(label, Mode::Keep), move |r| {
        git::rebase(r, &onto, autostash.unwrap_or(false), update_refs)
    })
}

/// The local branches a rebase onto `onto` would replay commits of, to offer moving them along.
pub fn rebase_stacked(core: &Core, onto: String) -> Res<rewrite::Stacked> {
    core.in_repo(move |r| rewrite::stacked_onto(r, &onto))
}

// These finish (or call off) the action that stopped on conflicts; its entry is recorded then.
pub fn op_continue(core: &Core) -> Res<bool> {
    let lock = core.index.clone();
    core.journaled(Action::new("Continue", Mode::Keep), move |r| {
        with_index_lock(&lock, r, git::op_continue)
    })
}

pub fn op_abort(core: &Core) -> Res<()> {
    core.journaled(Action::new("Abort", Mode::Keep), |r| {
        rewrite::before_abort(r)?;
        git::op_abort(r)?;
        rewrite::after_abort(r)
    })
}

pub fn rebase_skip(core: &Core) -> Res<bool> {
    core.journaled(Action::new("Skip", Mode::Keep), git::rebase_skip)
}

pub fn fetch(
    core: &Core,
    op: String,
    progress: impl Fn(network::Progress) + Send + Sync + 'static,
) -> Res<()> {
    let net = core.watch_network(op, progress);
    core.in_repo(move |r| git::fetch(r, &net))
}

pub fn submodules(core: &Core) -> Res<Vec<git::Submodule>> {
    core.in_repo(git::submodules)
}

pub fn submodule_update(
    core: &Core,
    op: String,
    progress: impl Fn(network::Progress) + Send + Sync + 'static,
) -> Res<()> {
    let net = core.watch_network(op, progress);
    core.in_repo(move |r| git::submodule_update(r, &net))
}

pub fn lfs_pull(
    core: &Core,
    path: String,
    op: String,
    progress: impl Fn(network::Progress) + Send + Sync + 'static,
) -> Res<()> {
    let net = core.watch_network(op, progress);
    core.in_repo(move |r| git::lfs_pull(r, &path, &net))
}

pub fn last_fetch(core: &Core) -> Res<Option<u64>> {
    core.in_repo(move |r| Ok(git::last_fetch(r)))
}

/// Stops the network command the page started under `op`; it then fails with "git:cancelled".
pub fn cancel_network(core: &Core, op: String) {
    core.network.cancel(&op)
}

/// The page's answer to a git or ssh prompt; None is Cancel. Never logged or kept.
pub fn askpass_answer(_core: &Core, id: u64, answer: Option<String>) {
    askpass::answer(id, answer)
}

/// The page listens for prompts; until then (and while it reloads) they're declined at once.
pub fn askpass_ready(_core: &Core) {
    askpass::page_ready()
}
