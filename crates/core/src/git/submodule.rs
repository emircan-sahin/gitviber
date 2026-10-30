//! Submodules and updating them.

use super::{command, run_text};
use crate::network::{self, Net};
use serde::Serialize;
use std::path::Path;

/// A submodule: its folder, the commit the repository records for it, and how the checkout
/// there stands against that ("missing": not set up yet; "moved": on another commit;
/// "conflict"; "ok").
#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Submodule {
    pub path: String,
    pub sha: String,
    pub state: String,
}

pub fn submodules(repo: &Path) -> Result<Vec<Submodule>, String> {
    if !repo.join(".gitmodules").exists() {
        return Ok(vec![]);
    }
    let out = run_text(repo, &["submodule", "status"])?;
    Ok(out
        .lines()
        .filter_map(|l| {
            let state = match l.chars().next()? {
                '-' => "missing",
                '+' => "moved",
                'U' => "conflict",
                _ => "ok",
            };
            let mut parts = l[1..].split_whitespace();
            let sha = parts.next()?.to_string();
            let path = parts.next()?.to_string();
            Some(Submodule {
                path,
                sha,
                state: state.into(),
            })
        })
        .collect())
}

/// Sets up and checks out every submodule at the commit the repository records, nested ones
/// too, fetching what's missing. In a linked worktree, those the main one has are cloned from
/// there rather than downloaded again.
pub fn submodule_update(repo: &Path, net: &Net) -> Result<(), String> {
    if let Some(main) = super::main_worktree(repo) {
        update_from(repo, &set_up_in(repo, Path::new(&main)), net)?;
    }
    let args = ["submodule", "update", "--init", "--recursive", "--progress"];
    network::run(command(repo, &args), "git submodule update", net, None).map(|_| ())
}

/// A new worktree gets the submodules `from` has set up, cloned from there; the ones `from`
/// left alone stay that way, as nothing is downloaded the user didn't have.
pub(crate) fn submodules_like(worktree: &Path, from: &Path, net: &Net) -> Result<(), String> {
    if !worktree.join(".gitmodules").exists() {
        return Ok(());
    }
    update_from(worktree, &set_up_in(worktree, from), net)
}

/// The submodules `repo` hasn't set up that the worktree `from` has: each path, and its
/// repository there.
fn set_up_in(repo: &Path, from: &Path) -> Vec<(String, String)> {
    if repo.canonicalize().ok() == from.canonicalize().ok() {
        return vec![];
    }
    let missing = submodules(repo).unwrap_or_default();
    missing
        .into_iter()
        .filter(|s| s.state == "missing")
        .filter_map(|s| {
            let dir = from.join(&s.path);
            let out = run_text(
                &dir,
                &["rev-parse", "--show-toplevel", "--absolute-git-dir"],
            )
            .ok()?;
            let (top, git_dir) = out.trim().split_once('\n')?;
            // A folder never set up reads as `from` itself.
            (Path::new(top).canonicalize().ok()? == dir.canonicalize().ok()?)
                .then(|| (s.path, git_dir.to_string()))
        })
        .collect()
}

/// Clones each submodule with its objects taken from `reference`, then copied (--dissociate:
/// it doesn't depend on that repository afterwards); only what that one lacks is downloaded.
fn update_from(repo: &Path, list: &[(String, String)], net: &Net) -> Result<(), String> {
    for (path, reference) in list {
        let args = [
            "submodule",
            "update",
            "--init",
            "--progress",
            "--reference",
            reference,
            "--dissociate",
            "--",
            path,
        ];
        network::run(command(repo, &args), "git submodule update", net, None)?;
    }
    Ok(())
}
