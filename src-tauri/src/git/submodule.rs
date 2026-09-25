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
/// too, fetching what's missing.
pub fn submodule_update(repo: &Path, net: &Net) -> Result<(), String> {
    let args = ["submodule", "update", "--init", "--recursive", "--progress"];
    network::run(command(repo, &args), "git submodule update", net, None).map(|_| ())
}
