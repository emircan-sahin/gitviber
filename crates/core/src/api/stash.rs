//! Stashes are not undo entries: they move no branch. A dropped stash's commit stays findable
//! by its sha.

use crate::git;
use crate::journal::{files_label, Action, Mode};
use crate::lines;
use crate::state::{Core, Res};
use std::path::Path;

pub fn stashes(core: &Core) -> Res<Vec<git::Stash>> {
    core.in_repo(git::stashes)
}

pub fn stash_files(core: &Core, sha: String) -> Res<git::StashFiles> {
    core.in_repo(move |r| git::stash_files(r, &sha))
}

pub fn stash_push(
    core: &Core,
    message: String,
    untracked: bool,
    staged: Option<bool>,
    paths: Option<Vec<String>>,
) -> Res<()> {
    let paths = paths.unwrap_or_default();
    core.indexed_once(move |r| {
        let what = git::StashWhat {
            untracked,
            staged: staged.unwrap_or(false),
            paths: &paths,
        };
        git::stash_push(r, &message, what)
    })
}

/// True when applying stopped on conflicts (the stash is kept then).
pub fn stash_branch(core: &Core, name: String, sha: String) -> Res<bool> {
    let label = format!("Branch {name} from a stash");
    core.journaled(Action::new(label, Mode::Keep), move |r| {
        git::stash_branch(r, &name, &sha)
    })
}

/// True when it stopped on conflicts.
pub fn stash_apply(core: &Core, sha: String, pop: bool) -> Res<bool> {
    core.indexed_once(move |r| git::stash_apply(r, &sha, pop))
}

pub fn stash_drop(core: &Core, sha: String) -> Res<()> {
    core.in_repo(move |r| git::stash_drop(r, &sha))
}

/// Stashes the lines of an unstaged diff a request picks; the file's old version goes to the
/// Trash, so Undo puts it back (the stash stays: drop it from Stashes).
pub fn stash_lines(core: &Core, message: String, mut request: lines::Request) -> Res<()> {
    request.action = "stash".into();
    let (journal, index) = (core.journal.clone(), core.index.clone());
    core.in_repo(move |r| {
        let paths = [request.path.clone()];
        let label = files_label("Stash lines of", &paths);
        let write = move |r: &Path| lines::stash(r, &message, &request).map(|_| vec![]);
        journal
            .replace(r, label, "stashed", &paths, &index, write)
            .map(|_| ())
    })
}

/// Puts one file of a stash in the working tree; the version it replaces goes to the Trash.
pub fn stash_restore_file(core: &Core, sha: String, path: String, untracked: bool) -> Res<()> {
    let (journal, index) = (core.journal.clone(), core.index.clone());
    core.in_repo(move |r| {
        let paths = [path.clone()];
        let label = files_label("Take from a stash", &paths);
        let write =
            move |r: &Path| git::stash_restore_file(r, &sha, &path, untracked).map(|_| vec![]);
        journal
            .replace(r, label, "before stash", &paths, &index, write)
            .map(|_| ())
    })
}

/// Gives a stash another message; it moves to the top of the list.
pub fn stash_rename(core: &Core, sha: String, message: String) -> Res<()> {
    core.in_repo(move |r| git::stash_rename(r, &sha, &message))
}
