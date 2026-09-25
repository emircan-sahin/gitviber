//! Local and remote branches: listing, creating, switching, renaming, deleting, upstreams.

use super::{run, run_network, run_text, validate_base, validate_branch, worktrees};
use crate::network::{self, Net};
use serde::Serialize;
use std::path::Path;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Branch {
    pub name: String,
    /// Remote-tracking branch like origin/main (can be merged/rebased onto, or checked out).
    pub remote: bool,
    pub current: bool,
    pub upstream: Option<String>,
    pub timestamp: i64,
    /// Checked out in another worktree (its path), where git refuses to switch to it.
    pub worktree: Option<String>,
    /// Local, not HEAD, and fully contained in HEAD: deleting it loses no commits. The
    /// default branch never counts, so "clean up merged" can't take main from under a feature.
    pub merged: bool,
    /// What its remote's HEAD points at (origin/main): never offered for deletion.
    pub remote_default: bool,
}

pub fn branches(repo: &Path) -> Result<Vec<Branch>, String> {
    let raw = run_text(
        repo,
        &[
            "for-each-ref",
            "--sort=-committerdate",
            "--format=%(refname)%1f%(refname:short)%1f%(HEAD)%1f%(upstream:short)%1f%(committerdate:unix)%1f%(worktreepath)%1f%(symref)",
            "refs/heads",
            "refs/remotes",
        ],
    )?;
    // A bare main repo "holds" its HEAD branch too, but has no working tree to open.
    let bare: Vec<String> = worktrees(repo)
        .unwrap_or_default()
        .into_iter()
        .filter(|w| w.bare)
        .map(|w| w.path)
        .collect();
    // Empty on an unborn HEAD, where nothing is merged yet.
    let merged = run_text(
        repo,
        &[
            "for-each-ref",
            "--merged=HEAD",
            "--format=%(refname)",
            "refs/heads",
        ],
    )
    .unwrap_or_default();
    let merged: Vec<&str> = merged.lines().collect();
    let default = default_branch(repo);
    // origin/HEAD's own row names the remote default; the row itself is skipped below.
    let remote_heads: Vec<&str> = raw
        .lines()
        .filter_map(|l| l.split('\x1f').nth(6).filter(|s| !s.is_empty()))
        .collect();
    Ok(raw
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\x1f').collect();
            let elsewhere =
                f.len() == 7 && f[2] != "*" && !f[5].is_empty() && !bare.iter().any(|b| b == f[5]);
            (f.len() == 7 && !f[0].ends_with("/HEAD")).then(|| Branch {
                name: f[1].to_string(),
                remote: f[0].starts_with("refs/remotes/"),
                current: f[2] == "*",
                upstream: (!f[3].is_empty()).then(|| f[3].to_string()),
                timestamp: f[4].parse().unwrap_or(0),
                worktree: elsewhere.then(|| f[5].to_string()),
                merged: f[2] != "*" && f[1] != default && merged.contains(&f[0]),
                remote_default: remote_heads.contains(&f[0]),
            })
        })
        .collect())
}

/// What origin/HEAD points at, else "main".
pub(super) fn default_branch(repo: &Path) -> String {
    run_text(
        repo,
        &[
            "symbolic-ref",
            "--quiet",
            "--short",
            "refs/remotes/origin/HEAD",
        ],
    )
    .ok()
    .and_then(|r| r.trim().strip_prefix("origin/").map(str::to_string))
    .unwrap_or_else(|| "main".into())
}

/// `git branch -d`, or `-D` when `force`: -d refuses a branch with commits found nowhere else.
pub fn delete_branches(repo: &Path, names: &[String], force: bool) -> Result<(), String> {
    for n in names {
        validate_branch(repo, n)?;
    }
    let mut args = vec!["branch", if force { "-D" } else { "-d" }];
    args.extend(names.iter().map(String::as_str));
    run(repo, &args).map(|_| ())
}

/// Deletes "origin/feat" on origin. Refuses the remote's default branch: hosts either reject
/// it or let it go and leave every clone without one.
pub fn delete_remote_branch(repo: &Path, name: &str, net: &Net) -> Result<(), String> {
    let remotes = run_text(repo, &["remote"])?;
    let remote = remotes
        .lines()
        .filter(|r| name.starts_with(&format!("{r}/")))
        .max_by_key(|r| r.len())
        .ok_or_else(|| format!("{name} isn't a remote branch"))?;
    let branch = &name[remote.len() + 1..];
    validate_branch(repo, branch)?;
    let head = format!("refs/remotes/{remote}/HEAD");
    if run_text(repo, &["symbolic-ref", "--quiet", "--short", &head])
        .is_ok_and(|h| h.trim() == name)
    {
        return Err(format!("{name} is {remote}'s default branch"));
    }
    let target = format!("refs/heads/{branch}");
    run_network(repo, &["push", remote, "--delete", &target], net).map(|_| ())
}

pub fn switch_branch(repo: &Path, name: &str, create: bool) -> Result<(), String> {
    validate_branch(repo, name)?;
    let args: Vec<&str> = if create {
        vec!["switch", "-c", name]
    } else {
        vec!["switch", name]
    };
    run(repo, &args).map(|_| ())
}

/// A new branch at `base`: HEAD, or a full ref to a local or remote branch or a tag. It doesn't
/// track `base`: it's a new line of work, and under `push.default=simple` an upstream with
/// another name would refuse its pushes. `switch` checks it out as well.
pub fn create_branch(repo: &Path, name: &str, base: &str, switch: bool) -> Result<(), String> {
    validate_branch(repo, name)?;
    validate_base(repo, base)?;
    let args = if switch {
        vec!["switch", "--no-track", "-c", name, base]
    } else {
        vec!["branch", "--no-track", name, base]
    };
    run(repo, &args).map(|_| ())
}

/// `git branch -m`. With `remote` the branch's upstream is renamed too: the new name is
/// pushed and tracked, then the old one deleted there.
pub fn rename_branch(
    repo: &Path,
    old: &str,
    new: &str,
    remote: bool,
    net: &Net,
) -> Result<(), String> {
    validate_branch(repo, old)?;
    validate_branch(repo, new)?;
    // Checked before anything moves, so a refusal leaves everything as it was.
    let upstream = remote.then(|| remote_upstream(repo, old)).transpose()?;
    run(repo, &["branch", "-m", old, new])?;
    let Some((remote, branch)) = upstream else {
        return Ok(());
    };
    let publish = format!("refs/heads/{new}:refs/heads/{new}");
    let gone = format!("refs/heads/{branch}");
    run_network(repo, &["push", "-u", &remote, &publish], net)
        .and_then(|_| run_network(repo, &["push", &remote, "--delete", &gone], net))
        .map(|_| ())
        .map_err(|e| match e.as_str() {
            network::CANCELLED => format!("Renamed to {new} here; on {remote} it was cancelled."),
            _ => format!("Renamed to {new} here, but not on {remote}: {e}"),
        })
}

/// The remote and branch name `branch` tracks. Refuses a remote's default branch: hosts
/// reject deleting it, and it would leave every clone without one.
fn remote_upstream(repo: &Path, branch: &str) -> Result<(String, String), String> {
    let reference = format!("refs/heads/{branch}");
    let out = run_text(
        repo,
        &[
            "for-each-ref",
            "--format=%(upstream:remotename)%1f%(upstream:remoteref)",
            &reference,
        ],
    )?;
    let (remote, name) = out
        .trim_end()
        .split_once('\x1f')
        .filter(|(r, _)| !r.is_empty() && *r != ".")
        .and_then(|(r, m)| Some((r.to_string(), m.strip_prefix("refs/heads/")?.to_string())))
        .ok_or_else(|| format!("{branch} doesn't track a remote branch"))?;
    let head = format!("refs/remotes/{remote}/HEAD");
    if run_text(repo, &["symbolic-ref", "--quiet", "--short", &head])
        .is_ok_and(|h| h.trim() == format!("{remote}/{name}"))
    {
        return Err(format!(
            "{remote}/{name} is {remote}'s default branch; rename it on the host instead"
        ));
    }
    Ok((remote, name))
}

/// Makes `branch` track `upstream`, a remote-tracking branch such as origin/feat, or nothing.
pub fn set_upstream(repo: &Path, branch: &str, upstream: Option<&str>) -> Result<(), String> {
    validate_branch(repo, branch)?;
    let Some(u) = upstream else {
        return run(repo, &["branch", "--unset-upstream", branch]).map(|_| ());
    };
    let full = format!("refs/remotes/{u}");
    if u.starts_with('-') || run(repo, &["rev-parse", "--verify", "-q", &full]).is_err() {
        return Err(format!("not a remote branch: {u}"));
    }
    let flag = format!("--set-upstream-to={full}");
    run(repo, &["branch", &flag, branch]).map(|_| ())
}

/// Switches to the local branch for a remote-tracking one ("upstream/dev" → dev), creating it
/// to track exactly that ref. Not `git switch dev`: with origin/dev and upstream/dev both
/// there, git's guess refuses. An existing local branch is switched to as it is; the UI asks
/// first when it tracks something else.
pub fn switch_tracking(repo: &Path, remote_ref: &str) -> Result<(), String> {
    let bad = || format!("not a remote branch: {remote_ref}");
    if remote_ref.starts_with('-') {
        return Err(bad());
    }
    let (_, local) = remote_ref.split_once('/').ok_or_else(bad)?;
    run(
        repo,
        &[
            "rev-parse",
            "--verify",
            "-q",
            &format!("refs/remotes/{remote_ref}"),
        ],
    )
    .map_err(|_| bad())?;
    validate_branch(repo, local)?;
    if run(
        repo,
        &[
            "rev-parse",
            "--verify",
            "-q",
            &format!("refs/heads/{local}"),
        ],
    )
    .is_ok()
    {
        return run(repo, &["switch", local]).map(|_| ());
    }
    run(repo, &["switch", "-c", local, "--track", remote_ref]).map(|_| ())
}
