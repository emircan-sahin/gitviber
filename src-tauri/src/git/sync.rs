//! Push, pull and fetch, and the pull request draft and push target they feed.

use super::{
    command, config_value, ensure_idle, publish_remote, remote_url, remotes, run, run_network,
    run_text, stoppable,
};
use crate::network::{self, Net};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PushTarget {
    pub remote: String,
    /// The remote branch it lands on, e.g. origin/dev; None until it exists there.
    pub branch: Option<String>,
    /// Commits it doesn't have yet.
    pub ahead: u32,
}

/// `@{push}` for a branch. Under the default `push.default=simple`, git won't name it for a
/// triangular setup although the push itself works; `current` is what it does then.
pub(super) fn push_target(repo: &Path, branch: &str) -> Option<PushTarget> {
    let mode = config_value(repo, None, "push.default");
    let mut args = vec![];
    if matches!(mode.as_deref(), None | Some("simple")) {
        args.extend(["-c", "push.default=current"]);
    }
    let reference = format!("refs/heads/{branch}");
    args.extend([
        "for-each-ref",
        "--format=%(push:remotename)%1f%(push:short)%1f%(push:track,nobracket)",
        &reference,
    ]);
    let out = run_text(repo, &args).ok()?;
    let f: Vec<&str> = out.trim_end_matches('\n').split('\x1f').collect();
    let remote = f.first().filter(|r| !r.is_empty())?.to_string();
    let track = f.get(2).copied().unwrap_or_default();
    let exists = f.get(1).is_some_and(|b| !b.is_empty()) && track != "gone";
    let ahead = track
        .split(", ")
        .find_map(|p| p.strip_prefix("ahead "))
        .and_then(|n| n.parse().ok())
        .unwrap_or(0);
    Some(PushTarget {
        branch: exists.then(|| f[1].to_string()),
        remote,
        ahead,
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PullDraft {
    /// Commits HEAD has that `base` doesn't: what the PR would bring.
    pub commits: u32,
    /// The one commit's message, when there's exactly one: GitHub titles the PR with it.
    pub subject: Option<String>,
    pub body: Option<String>,
}

/// What a PR from HEAD into `base` (a remote-tracking branch) would carry, to fill its title
/// and description the way GitHub does.
pub fn pull_draft(repo: &Path, base: &str) -> Result<PullDraft, String> {
    let r = base
        .strip_prefix("refs/remotes/")
        .filter(|r| !r.starts_with('-') && r.contains('/'))
        .ok_or_else(|| format!("not a remote-tracking branch: {base}"))?;
    run(repo, &["rev-parse", "--verify", "-q", base])
        .map_err(|_| format!("unknown branch: {r}"))?;
    let range = format!("{base}..HEAD");
    let commits = run_text(repo, &["rev-list", "--count", &range, "--"])?
        .trim()
        .parse()
        .unwrap_or(0);
    let one =
        |fmt: &str| run_text(repo, &["log", "-1", fmt, "HEAD", "--"]).map(|s| s.trim().to_string());
    Ok(PullDraft {
        subject: (commits == 1).then(|| one("--format=%s")).transpose()?,
        body: (commits == 1).then(|| one("--format=%b")).transpose()?,
        commits,
    })
}

/// What counts as pushed for HEAD: the branch `git push` lands on (a fork pushes to origin
/// while pulling from upstream), else the upstream.
pub(super) fn pushed_base(repo: &Path) -> String {
    run_text(repo, &["symbolic-ref", "--short", "-q", "HEAD"])
        .ok()
        .and_then(|b| push_target(repo, b.trim()))
        .and_then(|p| p.branch)
        .unwrap_or_else(|| "@{upstream}".into())
}

/// Makes `git push` go to `remote` for every branch, whatever each pulls from: a fork's
/// branches can then follow upstream and still be pushed to origin.
pub fn set_push_default(repo: &Path, remote: &str) -> Result<(), String> {
    if remote_url(repo, remote).is_none() {
        return Err(format!("no remote named {remote}"));
    }
    run(repo, &["config", "remote.pushDefault", remote]).map(|_| ())
}

/// Fetches `refspecs` from a remote without FETCH_HEAD, which any other fetch (the background
/// one, a terminal) may rewrite before it's read. A refspec without a destination only brings
/// in objects (a configured remote-tracking ref like origin/<base> may still update).
pub fn fetch_objects(
    repo: &Path,
    remote: &str,
    refspecs: &[String],
    net: &Net,
) -> Result<(), String> {
    let mut args = vec![
        "fetch",
        "--quiet",
        "--no-write-fetch-head",
        "--no-tags",
        remote,
    ];
    args.extend(refspecs.iter().map(String::as_str));
    run_network(repo, &args, net).map(|_| ())
}

/// `force`: after a rebase or amend the remote has the branch's old commits; replace them,
/// but only if it still has what was last fetched (`--force-with-lease`), so a push made
/// meanwhile by someone else is refused rather than lost. A fetch alone (the background
/// one) would move that lease onto their commit, so it must also have been in this branch
/// at some point (`--force-if-includes`).
/// A branch without an upstream is published (`-u`) to `remote`, or else to `publish_remote`.
pub fn push(repo: &Path, force: bool, remote: Option<&str>, net: &Net) -> Result<(), String> {
    push_as(repo, force, remote, false, net)
}

/// `push` with `--follow-tags`: annotated tags on the pushed commits go along.
pub fn push_with_tags(
    repo: &Path,
    force: bool,
    remote: Option<&str>,
    net: &Net,
) -> Result<(), String> {
    push_as(repo, force, remote, true, net)
}

/// After a push is refused as non-fast-forward: whether what only the remote has was this
/// branch's own before a rebase or an amend, the one case a force push is for. It's
/// --force-if-includes' test: the remote tip is in the branch's reflog, or behind an entry of it.
/// Commits someone else pushed never are, and those want a pull.
pub fn remote_was_ours(repo: &Path) -> bool {
    let tip = |rev: &str| run_text(repo, &["rev-parse", "--verify", "--quiet", rev]).ok();
    let (Some(remote), Ok(branch)) = (
        tip("@{push}").or_else(|| tip("@{upstream}")),
        run_text(repo, &["symbolic-ref", "--quiet", "HEAD"]),
    ) else {
        return false;
    };
    let Ok(log) = run_text(
        repo,
        &["log", "-g", "--format=%H", "-n", "200", branch.trim()],
    ) else {
        return false;
    };
    let mut args = vec!["rev-list", "--max-count=1", remote.trim(), "--not"];
    args.extend(log.lines());
    run_text(repo, &args).is_ok_and(|left| left.trim().is_empty())
}

fn push_as(
    repo: &Path,
    force: bool,
    remote: Option<&str>,
    tags: bool,
    net: &Net,
) -> Result<(), String> {
    let has_upstream = run(repo, &["rev-parse", "--abbrev-ref", "@{upstream}"]).is_ok();
    let mut args = vec!["push"];
    if force {
        args.extend(["--force-with-lease", "--force-if-includes"]);
    }
    if tags {
        args.push("--follow-tags");
    }
    let target;
    if !has_upstream {
        target = match remote {
            Some(r) if remotes(repo).iter().any(|x| x == r) => r.to_string(),
            Some(r) => return Err(format!("no remote named {r}")),
            None => publish_remote(repo)?,
        };
        args.extend(["-u", &target, "HEAD"]);
    }
    run_network(repo, &args, net).map(|_| ())
}

/// How a pull brings in what it fetched; the page sends "ff", "merge" or "rebase".
#[derive(Deserialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "kebab-case")]
pub enum PullMode {
    /// Fast-forward only
    Ff,
    Merge,
    Rebase,
}

/// Returns true if it stopped on conflicts. `autostash`: uncommitted changes in the way are
/// stashed first and reapplied after; if they conflict then, they also stay in the stash.
pub fn pull(repo: &Path, mode: PullMode, autostash: bool, net: &Net) -> Result<bool, String> {
    ensure_idle(repo)?;
    let flag = match mode {
        PullMode::Ff => "--ff-only",
        PullMode::Merge => "--no-rebase",
        PullMode::Rebase => "--rebase",
    };
    let mut args = vec!["pull", "--no-edit", flag];
    if autostash {
        args.push("--autostash");
    }
    stoppable(repo, run_network(repo, &args, net))
}

/// Every remote: a plain fetch takes only the current branch's (on a fork's dev tracking
/// upstream/dev, upstream alone), leaving origin's branches stale.
pub fn fetch(repo: &Path, net: &Net) -> Result<(), String> {
    run_network(repo, &["fetch", "--all", "--prune"], net).map(|_| ())
}

/// Downloads a Git LFS file's object (git lfs pull for that path) so it can be shown.
pub fn lfs_pull(repo: &Path, path: &str, net: &Net) -> Result<(), String> {
    if path.starts_with('-') || path.contains(['\n', '\r']) {
        return Err(format!("not a file path: {path}"));
    }
    let args = ["lfs", "pull", "--include", path, "--exclude", ""];
    network::run(command(repo, &args), "git lfs pull", net, None)
        .map(|_| ())
        .map_err(|e| {
            if e.contains("'lfs' is not a git command") {
                "Git LFS isn't installed (brew install git-lfs, then git lfs install).".into()
            } else {
                e
            }
        })
}

/// When this repo last fetched (FETCH_HEAD's mtime, Unix seconds); None if it never has.
/// Each worktree keeps its own FETCH_HEAD, and a fetch from any of them updates the remote
/// branches for all, so a linked worktree also counts the main one's.
pub fn last_fetch(repo: &Path) -> Option<u64> {
    let own = run_text(repo, &["rev-parse", "--git-path", "FETCH_HEAD"]).ok()?;
    let common = run_text(repo, &["rev-parse", "--git-common-dir"]).ok()?;
    [
        repo.join(own.trim()),
        repo.join(common.trim()).join("FETCH_HEAD"),
    ]
    .iter()
    .filter_map(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok())
    .filter_map(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
    .map(|d| d.as_secs())
    .max()
}

/// Fetches one configured remote, e.g. a fork's upstream.
pub fn fetch_remote(repo: &Path, name: &str, net: &Net) -> Result<(), String> {
    if remote_url(repo, name).is_none() {
        return Err(format!("no remote named {name}"));
    }
    run_network(repo, &["fetch", "--prune", name], net).map(|_| ())
}
