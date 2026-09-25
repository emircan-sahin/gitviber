//! Checking a pull request's branch out, here or in a new worktree.

use super::{fetch_remote, Session};
use crate::git;
use crate::network::Net;
use std::path::Path;

/// Where `checkout` fetches a PR in `to` from, and the `owner` its local branch is named by:
/// the original's PRs get their own local names; origin's keep pr/<n>.
pub fn pr_source(
    session: &Session,
    repo: &Path,
    to: Option<&str>,
) -> Result<(String, Option<String>), String> {
    let remote = fetch_remote(session, repo, to)?;
    let owner = to
        .filter(|_| remote != "origin")
        .and_then(|t| t.split('/').next())
        .map(str::to_string);
    Ok((remote, owner))
}

/// Switches to the PR's branch: the real branch when it lives on origin (`same_repo`),
/// otherwise `pr/<n>`, fetched from `remote` (the repository the PR is in). A fork's original
/// numbers its PRs separately, so its are `pr/<owner>/<n>` (`owner` set).
/// An existing local branch is only fast-forwarded, never reset: unpushed work on it is
/// kept, and a branch that diverged from the PR is reported instead of silently used.
pub fn checkout(
    repo: &Path,
    remote: &str,
    owner: Option<&str>,
    number: u64,
    head_ref: &str,
    same_repo: bool,
    net: &Net,
) -> Result<(), String> {
    let (local, branch, exists) = pr_branch(repo, owner, number, head_ref, same_repo)?;
    // Fetched into refs, never read from FETCH_HEAD: another fetch (the background one, a
    // terminal) may rewrite that in between. A local branch is only ever fast-forwarded.
    if same_repo {
        let tracked = format!("origin/{local}");
        let refspec = format!("+refs/heads/{local}:refs/remotes/{tracked}");
        git::fetch_objects(repo, "origin", &[refspec], net)?;
        if exists {
            git::switch_branch(repo, &local, false)?;
            return git::run(repo, &["merge", "--ff-only", "--quiet", &tracked])
                .map(|_| ())
                .map_err(|_| diverged(&local));
        }
        // Not `git switch <head_ref>`: in a fork, upstream often has a same-named branch and
        // git's guess then refuses ("matched multiple remote tracking branches").
        return git::run(repo, &["switch", "-c", &local, "--track", &tracked]).map(|_| ());
    }
    let source = format!("refs/pull/{number}/head");
    let current =
        git::run_text(repo, &["symbolic-ref", "-q", "HEAD"]).is_ok_and(|h| h.trim() == branch);
    if current {
        // git won't fetch into the checked-out branch; a pull fast-forwards it the same way.
        let pull = [
            "pull",
            "--ff-only",
            "--no-rebase",
            "--no-edit",
            remote,
            &source,
        ];
        return git::run_network(repo, &pull, net).map(|_| ()).map_err(|e| {
            if e.contains("fast-forward") {
                diverged(&local)
            } else {
                e
            }
        });
    }
    fetch_pr_into(repo, remote, number, &branch, net)
        .map_err(|e| e.unwrap_or_else(|| diverged(&local)))?;
    git::switch_branch(repo, &local, false)?;
    if !exists {
        follow_pr(repo, &local, remote, number)?;
    }
    Ok(())
}

/// Fetches PR `number` from `remote` straight into `branch`. Not forced: it creates the branch
/// or fast-forwards it, and refuses one that diverged, which is `Err(None)`.
fn fetch_pr_into(
    repo: &Path,
    remote: &str,
    number: u64,
    branch: &str,
    net: &Net,
) -> Result<(), Option<String>> {
    let spec = format!("refs/pull/{number}/head:{branch}");
    // Not fetch_objects: its --quiet drops the "(non-fast-forward)" line this looks for.
    let args = ["fetch", "--no-write-fetch-head", "--no-tags", remote, &spec];
    git::run_network(repo, &args, net)
        .map(|_| ())
        .map_err(|e| (!e.contains("non-fast-forward")).then_some(e))
}

/// The local branch `checkout` puts PR `number` on, its full ref, and whether it exists yet.
fn pr_branch(
    repo: &Path,
    owner: Option<&str>,
    number: u64,
    head_ref: &str,
    same_repo: bool,
) -> Result<(String, String, bool), String> {
    let local = match (same_repo, owner) {
        (true, _) => head_ref.to_string(),
        (false, Some(o)) => format!("pr/{o}/{number}"),
        (false, None) => format!("pr/{number}"),
    };
    git::validate_branch(repo, &local)?;
    let branch = format!("refs/heads/{local}");
    let exists = git::run(repo, &["rev-parse", "--verify", "-q", &branch]).is_ok();
    Ok((local, branch, exists))
}

fn diverged(local: &str) -> String {
    format!("Local branch {local} has diverged from the pull request; reconcile it first.")
}

/// Like `gh pr checkout`: the branch follows the PR, so Pull brings its new commits,
/// and it never looks unpublished (a Publish would copy it into origin).
fn follow_pr(repo: &Path, local: &str, remote: &str, number: u64) -> Result<(), String> {
    let key = |k: &str| format!("branch.{local}.{k}");
    git::run(repo, &["config", &key("remote"), remote])?;
    let merge = format!("refs/pull/{number}/head");
    git::run(repo, &["config", &key("merge"), &merge]).map(|_| ())
}

/// `checkout`, into a new worktree in `dir` (see `git::add_worktree`) instead of this one;
/// returns its path. The branch is brought up to date the same way, only fast-forwarded.
#[allow(clippy::too_many_arguments)]
pub fn checkout_worktree(
    repo: &Path,
    remote: &str,
    owner: Option<&str>,
    number: u64,
    head_ref: &str,
    same_repo: bool,
    dir: Option<&str>,
    net: &Net,
) -> Result<String, String> {
    let (local, branch, exists) = pr_branch(repo, owner, number, head_ref, same_repo)?;
    // git refuses both the fetch and the worktree for a branch that's out somewhere already.
    if let Some(w) = git::worktrees(repo)?
        .into_iter()
        .find(|w| w.branch.as_deref() == Some(local.as_str()))
    {
        return Err(format!("{local} is already checked out in {}", w.path));
    }
    // A taken folder is refused before the fetch leaves a branch and its tracking behind.
    git::worktree_target(repo, &local, dir)?;
    if same_repo {
        let tracked = format!("refs/remotes/origin/{local}");
        let refspec = format!("+refs/heads/{local}:{tracked}");
        git::fetch_objects(repo, "origin", &[refspec], net)?;
        if exists {
            // `merge --ff-only` without a checkout: moved when behind, kept when ahead.
            let within =
                |a: &str, b: &str| git::run(repo, &["merge-base", "--is-ancestor", a, b]).is_ok();
            if within(&branch, &tracked) {
                git::run(repo, &["update-ref", &branch, &tracked])?;
            } else if !within(&tracked, &branch) {
                return Err(diverged(&local));
            }
        } else {
            git::run(repo, &["branch", "--track", &local, &tracked])?;
        }
    } else {
        fetch_pr_into(repo, remote, number, &branch, net)
            .map_err(|e| e.unwrap_or_else(|| diverged(&local)))?;
        if !exists {
            follow_pr(repo, &local, remote, number)?;
        }
    }
    git::add_worktree(repo, &local, None, dir)
}
