use crate::journal::{Action, Mode};
use crate::state::{short, Core, Res};
use crate::{fs, git, rewrite};
use std::path::Path;

pub fn log(
    core: &Core,
    rev: Option<String>,
    skip: u32,
    limit: u32,
    filter: Option<git::LogFilter>,
    all: Option<git::GraphRefs>,
) -> Res<Vec<git::Commit>> {
    let filter = filter.unwrap_or_default();
    // Searching every diff for code, or one path's history (followed through renames), can
    // rightly take minutes on a long history.
    let slow = filter.code.as_deref().is_some_and(|c| !c.is_empty()) || !filter.paths.is_empty();
    let read = move |r: &Path| {
        // Only pathspecs, but the rule holds: no path from the frontend reaches outside the repo.
        for p in &filter.paths {
            fs::resolve(r, p)?;
        }
        match (all, rev) {
            (Some(_), Some(_)) => Err("all branches, or one: not both".into()),
            (Some(refs), None) => git::log_all(r, &refs, skip, limit, &filter),
            (None, rev) => git::log_filtered(r, rev.as_deref(), skip, limit, &filter),
        }
    };
    if slow {
        core.in_repo(read)
    } else {
        core.read_repo(read)
    }
}

pub fn log_compare(
    core: &Core,
    with: String,
    incoming: bool,
    skip: u32,
    limit: u32,
) -> Res<Vec<git::Commit>> {
    core.in_repo(move |r| git::log_compare(r, &with, incoming, skip, limit))
}

pub fn compare_counts(core: &Core, with: String) -> Res<(u32, u32)> {
    core.in_repo(move |r| git::compare_counts(r, &with))
}

pub fn bisect_start(core: &Core, good: String) -> Res<git::BisectStep> {
    core.in_repo(move |r| git::bisect_start(r, &good))
}

pub fn bisect_mark(core: &Core, verdict: String) -> Res<git::BisectStep> {
    core.in_repo(move |r| git::bisect_mark(r, &verdict))
}

pub fn reflog(core: &Core, limit: u32) -> Res<Vec<git::ReflogEntry>> {
    core.in_repo(move |r| git::reflog(r, limit))
}

/// The working tree against commit `sha` itself, no merge base: History's Compare with Working Tree.
pub fn compare_worktree(core: &Core, sha: String) -> Res<git::BranchReview> {
    core.in_repo(move |r| git::worktree_review(r, &sha))
}

pub fn compare_files(core: &Core, with: String) -> Res<git::CompareFiles> {
    core.in_repo(move |r| git::compare_files(r, &with))
}

/// Two points of history compared (Compare screen): `merge_base` for what a pull request shows.
pub fn compare(core: &Core, base: String, head: String, merge_base: bool) -> Res<git::Comparison> {
    core.read_repo(move |r| git::compare(r, &base, &head, merge_base))
}

/// The commits `head` has that `base` doesn't, newest first (both commit ids).
pub fn log_between(
    core: &Core,
    base: String,
    head: String,
    skip: u32,
    limit: u32,
) -> Res<Vec<git::Commit>> {
    core.read_repo(move |r| git::log_between(r, &base, &head, skip, limit))
}

/// What merging a point into HEAD would bring, and whether it would conflict.
pub fn merge_check(core: &Core, with: String) -> Res<git::MergeCheck> {
    core.read_repo(move |r| git::merge_check(r, &with))
}

pub fn find_commit(core: &Core, sha: String) -> Res<Option<git::Commit>> {
    core.in_repo(move |r| git::find_commit(r, &sha))
}

pub fn known_commits(core: &Core, shas: Vec<String>) -> Res<Vec<Option<String>>> {
    core.read_repo(move |r| git::known_commits(r, &shas))
}

pub fn commit_files(core: &Core, sha: String) -> Res<Vec<git::FileChange>> {
    core.in_repo(move |r| git::commit_files(r, &sha))
}

/// What a run of a PR's commits changed together: from the first one's parent to the last.
pub fn range_files(core: &Core, from: String, to: String) -> Res<Vec<git::FileChange>> {
    core.in_repo(move |r| git::range_files(r, &from, &to))
}

pub fn blame(core: &Core, path: String) -> Res<git::Blame> {
    core.in_repo(move |r| {
        fs::resolve(r, &path)?;
        git::blame(r, &path)
    })
}

pub fn commit_details(core: &Core, sha: String) -> Res<git::CommitDetails> {
    core.in_repo(move |r| git::commit_details(r, &sha))
}

/// Rewords, squashes, drops, moves or splits commits of the branch, or fixes staged changes up
/// into one, and says how that ended. `branches`: the local branches on the commits it replays
/// move along.
pub fn rewrite(
    core: &Core,
    head: String,
    edit: rewrite::Edit,
    branches: bool,
) -> Res<rewrite::Outcome> {
    let what = |shas: &[String]| match shas {
        [sha] => short(sha).to_string(),
        _ => format!("{} commits", shas.len()),
    };
    let label = match &edit {
        rewrite::Edit::Reword { sha, .. } => format!("Reword {}", short(sha)),
        // Counted as the toast counts them: every commit that ends up in the one.
        rewrite::Edit::Squash {
            shas,
            onto,
            message,
        } => {
            let verb = if message.is_some() { "Squash" } else { "Fixup" };
            match &shas[..] {
                [sha] => format!("{verb} {} into {}", short(sha), short(onto)),
                _ => format!("{verb} {} commits", shas.len() + 1),
            }
        }
        rewrite::Edit::Drop { shas } => format!("Drop {}", what(shas)),
        rewrite::Edit::Move { sha, .. } => format!("Move {}", short(sha)),
        rewrite::Edit::Reorder { shas, .. } => format!("Move {}", what(shas)),
        rewrite::Edit::Split { sha } => format!("Split {}", short(sha)),
        rewrite::Edit::FixupStaged { sha } => format!("Fixup staged changes into {}", short(sha)),
    };
    // Undone, a fixup's changes come back staged, as they were.
    let mode = match edit {
        rewrite::Edit::FixupStaged { .. } => Mode::Soft,
        _ => Mode::Keep,
    };
    core.journaled(Action::new(label, mode), move |r| {
        rewrite::run(r, &head, &edit, branches)
    })
}

/// The local branches `edit` would replay commits of, to offer moving them along.
pub fn stacked_branches(core: &Core, edit: rewrite::Edit) -> Res<rewrite::Stacked> {
    core.in_repo(move |r| rewrite::stacked_for(r, &edit))
}

pub fn undo_commit(core: &Core, sha: String) -> Res<()> {
    let label = format!("Undo commit {}", short(&sha));
    core.journaled(Action::new(label, Mode::Soft), move |r| {
        git::undo_commit(r, &sha)
    })
}

pub fn reset(core: &Core, sha: String, mode: git::ResetMode, head: String) -> Res<()> {
    let label = format!("Reset to {}", short(&sha));
    // Undone the same way, except that a hard reset's lost changes can't come back.
    let back = match mode {
        git::ResetMode::Soft => Mode::Soft,
        git::ResetMode::Mixed => Mode::Mixed,
        git::ResetMode::Hard => Mode::Keep,
    };
    core.journaled(Action::new(label, back), move |r| {
        git::reset(r, &sha, mode, &head)
    })
}

pub fn drops_pushed(core: &Core, sha: Option<String>) -> Res<bool> {
    core.in_repo(move |r| match sha {
        Some(sha) => git::drops_pushed(r, &sha),
        None => git::has_pushed(r),
    })
}

pub fn revert(core: &Core, sha: String) -> Res<bool> {
    let label = format!("Revert {}", short(&sha));
    core.journaled(Action::new(label, Mode::Keep), move |r| {
        git::revert(r, &sha)
    })
}

pub fn cherry_pick(core: &Core, sha: String) -> Res<bool> {
    let label = format!("Cherry-pick {}", short(&sha));
    core.journaled(Action::new(label, Mode::Keep), move |r| {
        git::cherry_pick(r, &sha)
    })
}

/// Picks `shas`, oldest first, onto HEAD.
pub fn cherry_pick_many(core: &Core, shas: Vec<String>) -> Res<bool> {
    let label = format!("Cherry-pick {} commits", shas.len());
    core.journaled(Action::new(label, Mode::Keep), move |r| {
        git::cherry_pick_many(r, &shas)
    })
}

/// Picks onto the branch of another worktree (`path`), running git there. The entry goes in
/// that worktree's undo history, where a pick stopped on conflicts is also continued.
pub fn cherry_pick_into(core: &Core, path: String, sha: String) -> Res<bool> {
    let journal = core.journal.clone();
    core.in_repo(move |r| {
        let target = git::pick_target(r, &path)?;
        let action = Action::new(format!("Cherry-pick {}", short(&sha)), Mode::Keep);
        journal.record(&target, action, |t| git::cherry_pick_into(t, &sha))
    })
}

pub fn checkout_commit(core: &Core, sha: String) -> Res<()> {
    let label = format!("Check out {}", short(&sha));
    core.journaled(Action::new(label, Mode::Keep), move |r| {
        git::checkout_commit(r, &sha)
    })
}

pub fn create_branch_at(core: &Core, name: String, sha: String) -> Res<()> {
    let label = format!("Create branch {name}");
    core.journaled(Action::new(label, Mode::Keep), move |r| {
        git::create_branch_at(r, &name, &sha)
    })
}
