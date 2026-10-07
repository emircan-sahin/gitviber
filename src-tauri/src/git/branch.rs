//! Local and remote branches: listing, creating, switching, renaming, deleting, upstreams.

use super::cmd::{command, PINS};
use super::{run, run_network, run_text, run_with, validate_base, validate_branch, worktrees};
use crate::network::{self, Net};
use crate::process::{exec, spawn, spawning};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::process::Stdio;
use std::sync::{LazyLock, Mutex};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Branch {
    pub name: String,
    /// Remote-tracking branch like origin/main (can be merged/rebased onto, or checked out).
    pub remote: bool,
    pub current: bool,
    pub upstream: Option<String>,
    /// The tip's commit id.
    pub sha: String,
    pub timestamp: i64,
    /// Checked out in another worktree (its path), where git refuses to switch to it.
    pub worktree: Option<String>,
    /// Local, not HEAD, and fully contained in HEAD: deleting it loses no commits. The
    /// default branch never counts, so "clean up merged" can't take main from under a feature.
    pub merged: bool,
    /// What its remote's HEAD points at (origin/main): never offered for deletion.
    pub remote_default: bool,
    /// Commits it has that its upstream hasn't, and the other way round; 0 without one.
    pub ahead: u32,
    pub behind: u32,
    /// It has an upstream configured that no longer exists (deleted on the remote, pruned).
    pub upstream_gone: bool,
}

pub fn branches(repo: &Path) -> Result<Vec<Branch>, String> {
    let raw = run_text(
        repo,
        &[
            "for-each-ref",
            "--sort=-committerdate",
            "--format=%(refname)%1f%(refname:short)%1f%(HEAD)%1f%(upstream:short)%1f%(committerdate:unix)%1f%(worktreepath)%1f%(symref)%1f%(objectname)%1f%(upstream:track,nobracket)",
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
                f.len() == 9 && f[2] != "*" && !f[5].is_empty() && !bare.iter().any(|b| b == f[5]);
            let (ahead, behind, upstream_gone) = f.get(8).map_or((0, 0, false), |t| track(t));
            (f.len() == 9 && !f[0].ends_with("/HEAD")).then(|| Branch {
                name: f[1].to_string(),
                remote: f[0].starts_with("refs/remotes/"),
                current: f[2] == "*",
                upstream: (!f[3].is_empty()).then(|| f[3].to_string()),
                sha: f[7].to_string(),
                timestamp: f[4].parse().unwrap_or(0),
                worktree: elsewhere.then(|| f[5].to_string()),
                merged: f[2] != "*" && f[1] != default && merged.contains(&f[0]),
                remote_default: remote_heads.contains(&f[0]),
                ahead,
                behind,
                upstream_gone,
            })
        })
        .collect())
}

/// `%(upstream:track,nobracket)`: "ahead 3, behind 2", "gone", or nothing when even or untracked.
/// git writes it in English here (cmd.rs sets the locale).
fn track(t: &str) -> (u32, u32, bool) {
    let count = |word: &str| {
        t.split(", ")
            .find_map(|p| p.strip_prefix(word)?.strip_prefix(' ')?.parse().ok())
            .unwrap_or(0)
    };
    (count("ahead"), count("behind"), t == "gone")
}

/// Local branches squash- or rebase-merged on the remote, which then deleted them. Asked for
/// when the branch picker opens, never on a refresh: it reads their diffs.
pub fn merged_upstream(repo: &Path) -> Vec<String> {
    landed(repo, None)
        .into_iter()
        .map(|(name, _)| name)
        .collect()
}

/// The branches this worktree's HEAD most recently moved to, newest first, from its reflog's
/// checkouts (as GitHub Desktop reads them). A name may no longer be a branch, or be a commit
/// the checkout detached at: the picker keeps only the branches it lists.
pub fn recent_branches(repo: &Path, limit: usize) -> Result<Vec<String>, String> {
    // Bounded: a reflog can run to many thousands of entries. 128: no commit yet.
    let out = run_with(
        repo,
        &["log", "-g", "--format=%gs", "-n", "2500", "HEAD", "--"],
        &[128],
        None,
    )?;
    Ok(recent_from_reflog(&String::from_utf8_lossy(&out), limit))
}

fn recent_from_reflog(subjects: &str, limit: usize) -> Vec<String> {
    let mut names: Vec<String> = vec![];
    let mut renamed_away = HashSet::new();
    for line in subjects.lines() {
        let to = if let Some(moved) = line.strip_prefix("checkout: moving from ") {
            moved.rsplit_once(" to ").map(|(_, to)| to)
        } else if let Some(renamed) = line.strip_prefix("Branch: renamed ") {
            // The old name is gone; only what it became is recent.
            renamed.split_once(" to ").map(|(from, to)| {
                renamed_away.insert(from.trim_start_matches("refs/heads/").to_string());
                to
            })
        } else {
            None
        };
        let Some(name) = to.map(|t| t.trim_start_matches("refs/heads/")) else {
            continue;
        };
        if !renamed_away.contains(name) && !names.iter().any(|n| n == name) {
            names.push(name.to_string());
            if names.len() == limit {
                break;
            }
        }
    }
    names
}

/// Deletes branches merged here (`-d`, which git checks again) and ones merged upstream, which
/// git sees as unmerged: each checked again, then deleted only at the commit that was checked,
/// so one that moves meanwhile stays, and none checked out anywhere. Its settings go with it,
/// as `git branch -D` does.
pub fn delete_merged(repo: &Path, merged: &[String], upstream: &[String]) -> Result<(), String> {
    if !upstream.is_empty() {
        for n in upstream {
            validate_branch(repo, n)?;
        }
        let landed = landed(repo, None);
        let checked: Vec<(&String, &String)> = upstream
            .iter()
            .map(|n| {
                let at = landed.iter().find(|(l, _)| l == n).map(|(_, sha)| sha);
                at.map(|sha| (n, sha)).ok_or_else(|| {
                    format!("{n} changed since, and isn't known to be merged anymore")
                })
            })
            .collect::<Result<_, _>>()?;
        // update-ref has no checked-out guard: taking a worktree's branch leaves its HEAD unborn.
        let held = run_text(
            repo,
            &[
                "for-each-ref",
                "--format=%(refname:lstrip=2)%1f%(worktreepath)",
                "refs/heads",
            ],
        )?;
        let held = held
            .lines()
            .filter_map(|l| l.split_once('\x1f'))
            .find(|(n, path)| !path.is_empty() && upstream.iter().any(|u| u == n));
        if let Some((n, path)) = held {
            return Err(format!("{n} is checked out in {path}"));
        }
        for (n, sha) in checked {
            delete_branch_at(repo, n, sha)?;
        }
    }
    if merged.is_empty() {
        return Ok(());
    }
    delete_branches(repo, merged, false)
}

/// Local branches (or just `only`) whose upstream is gone, as a host deletes a pull request's
/// branch on merge, and whose changes are all in the default branch anyway. Gone alone proves
/// nothing: the branch may never have been pushed, or deleted unmerged, so it's the content
/// that decides, as GitHub Desktop and git-trim do.
pub(super) fn landed(repo: &Path, only: Option<&str>) -> Vec<(String, String)> {
    let pattern = only.map_or("refs/heads".into(), |b| format!("refs/heads/{b}"));
    let raw = run_text(
        repo,
        &[
            "for-each-ref",
            "--format=%(refname:lstrip=2)%1f%(upstream:track,nobracket)%1f%(objectname)%1f%(committerdate:unix)",
            &pattern,
        ],
    )
    .unwrap_or_default();
    let gone: Vec<Vec<&str>> = raw
        .lines()
        .map(|l| l.split('\x1f').collect::<Vec<_>>())
        .filter(|f| f.len() == 4 && f[1] == "gone")
        .filter(|f| only.is_none_or(|b| f[0] == b))
        .collect();
    if gone.is_empty() {
        return vec![];
    }
    let (defaults, tips) = default_tips(repo);
    gone.into_iter()
        .filter(|f| !defaults.iter().any(|d| d == f[0]))
        .filter(|f| {
            let since = f[3].parse().unwrap_or(0);
            tips.iter().any(|t| content_in(repo, t, f[2], since))
        })
        .map(|f| (f[0].to_string(), f[2].to_string()))
        .collect()
}

/// The default branch's names, and where it is here and on each remote (a fork's PRs land
/// upstream), deduped. Each remote's HEAD names its own; with none set, main or master.
fn default_tips(repo: &Path) -> (Vec<String>, Vec<String>) {
    let raw = run_text(
        repo,
        &[
            "for-each-ref",
            "--format=%(refname)%1f%(objectname)%1f%(symref)",
            "refs/heads",
            "refs/remotes",
        ],
    )
    .unwrap_or_default();
    let refs: Vec<Vec<&str>> = raw
        .lines()
        .map(|l| l.split('\x1f').collect::<Vec<_>>())
        .filter(|f| f.len() == 3)
        .collect();
    // refs/remotes/origin/HEAD → refs/remotes/origin/main
    let mut names: Vec<String> = refs
        .iter()
        .filter_map(|f| {
            let remote = f[0].strip_prefix("refs/remotes/")?.strip_suffix("/HEAD")?;
            let target = f[2].strip_prefix("refs/remotes/")?.strip_prefix(remote)?;
            target.strip_prefix('/').map(str::to_string)
        })
        .collect();
    if names.is_empty() {
        names = vec!["main".into(), "master".into()];
        if let Ok(b) = run_text(repo, &["config", "init.defaultBranch"]) {
            names.push(b.trim().to_string());
        }
    }
    let mut tips: Vec<String> = vec![];
    for f in &refs {
        let branch = f[0].strip_prefix("refs/heads/").or_else(|| {
            let (_, b) = f[0].strip_prefix("refs/remotes/")?.split_once('/')?;
            Some(b).filter(|b| *b != "HEAD")
        });
        if branch.is_some_and(|b| names.iter().any(|n| n == b)) && !tips.iter().any(|t| t == f[1]) {
            tips.push(f[1].to_string());
        }
    }
    (names, tips)
}

/// Answers by (branch commit, base commit), which never change: each picker open asks again.
static CHECKED: LazyLock<Mutex<HashMap<(String, String), bool>>> = LazyLock::new(Default::default);

/// Whether everything the commit `tip` changed is in `base`, whose history holds other commits.
fn content_in(repo: &Path, base: &str, tip: &str, since: i64) -> bool {
    let key = (tip.to_string(), base.to_string());
    let checked = || CHECKED.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(&known) = checked().get(&key) {
        return known;
    }
    // A git call that failed isn't an answer: asked again next time.
    let Some(known) = check_content(repo, base, tip, since) else {
        return false;
    };
    let mut all = checked();
    if all.len() > 1000 {
        all.clear();
    }
    all.insert(key, known);
    known
}

fn check_content(repo: &Path, base: &str, tip: &str, since: i64) -> Option<bool> {
    let ahead = format!("{base}..{tip}");
    let Ok(commits) = run_text(repo, &["rev-list", &ahead]) else {
        return None;
    };
    let commits: Vec<&str> = commits.lines().collect();
    // Reachable from base: merged the plain way.
    if commits.is_empty() {
        return Some(true);
    }
    // Squashed, and base hasn't touched those files since: each one it changed reads the same.
    let forked = format!("{base}...{tip}");
    let names = |range: &[&str]| {
        run_text(
            repo,
            &[
                &PINS[..],
                &[
                    "diff",
                    "--name-only",
                    "--no-renames",
                    "-z",
                    "--ignore-submodules=none",
                ],
                range,
            ]
            .concat(),
        )
    };
    let (Ok(changed), Ok(differ)) = (names(&[&forked]), names(&[base, tip])) else {
        return None;
    };
    let differ: HashSet<&str> = differ.split('\0').collect();
    let changed: Vec<&str> = changed.split('\0').filter(|p| !p.is_empty()).collect();
    if changed.is_empty() {
        return Some(false);
    }
    if changed.iter().all(|p| !differ.contains(p)) {
        return Some(true);
    }
    // Base's log below names them on the command line.
    if changed.len() > 1000 {
        return Some(false);
    }
    // Rebased or picked: each commit's patch is in base. Squashed, then base moved on over the
    // same files: its whole change is one there (git-trim asks git cherry that of a scratch
    // commit; this leaves nothing in the repo). Byte for byte, with --verbatim (git 2.39+):
    // patch ids and git cherry ignore whitespace, so a fix that only reindents would pass.
    // The whole diff has no commit line, so its id comes first, against the zero id. A merge
    // on the branch counts by what it brought in.
    let whole = [&PINS[..], &["diff"], &PATCH, &[&forked]].concat();
    let own = [
        &PINS[..],
        &["log", "-p", "--diff-merges=first-parent", FORMAT],
        &PATCH,
        &[&ahead],
    ]
    .concat();
    // Base's side since the branch's last commit (a day's slack for a clock ahead of the host's),
    // and only on the files it changed.
    let after = format!("--since={}", since - 86_400);
    let since_fork = format!("{tip}..{base}");
    let log = [
        &PINS[..],
        &["log", "-p", "--no-merges", FORMAT, &after],
        &PATCH,
        &[&since_fork, "--"],
        &changed,
    ]
    .concat();
    let (Some(ours), Some(theirs)) = (patch_ids(repo, &[whole, own]), patch_ids(repo, &[log]))
    else {
        return None;
    };
    // A zero id past the whole diff's is a patch patch-id split apart: part of it went unread.
    let zero = |(_, sha): &&(String, String)| sha.bytes().all(|b| b == b'0');
    let (wholes, own): (Vec<_>, Vec<_>) = ours.iter().partition(zero);
    if wholes.len() > 1 || theirs.iter().any(|t| zero(&t)) {
        return Some(false);
    }
    let theirs: HashSet<&str> = theirs.iter().map(|(id, _)| id.as_str()).collect();
    Some(
        wholes.first().is_some_and(|(id, _)| theirs.contains(id.as_str()))
        // A commit with no id (binary only) never matches.
        || commits
            .iter()
            .all(|c| own.iter().any(|(id, sha)| sha == c && theirs.contains(id.as_str()))),
    )
}

// The user's diff settings are pinned (PINS): patch-id reads the patches as text.
const PATCH: [&str; 8] = [
    "--ignore-submodules=none",
    "--submodule=short",
    "-U3",
    "--no-color",
    "--no-ext-diff",
    "--no-textconv",
    "--no-renames",
    "--full-index",
];
const FORMAT: &str = "--format=commit %H";

/// `git <source>; …| git patch-id --verbatim`: (id, commit) pairs, each source streamed through
/// one pipe in turn, as a branch's and its base's history can be long.
fn patch_ids(repo: &Path, sources: &[Vec<&str>]) -> Option<Vec<(String, String)>> {
    let (reader, writer) = spawning(std::io::pipe).ok()?;
    let mut ids = command(repo, &["patch-id", "--verbatim"]);
    ids.stdin(reader);
    std::thread::scope(|s| {
        let out = s.spawn(move || exec(ids, "git patch-id", &[], None, None));
        let mut written = true;
        for args in sources {
            let mut cmd = command(repo, args);
            cmd.stderr(Stdio::null());
            written &= writer
                .try_clone()
                .and_then(|w| spawn(cmd.stdout(w))?.wait())
                .is_ok_and(|s| s.success());
        }
        drop(writer);
        let out = out.join().ok()?.ok().filter(|_| written)?;
        Some(
            String::from_utf8_lossy(&out)
                .lines()
                .filter_map(|l| l.split_once(' '))
                .map(|(id, sha)| (id.to_string(), sha.to_string()))
                .collect(),
        )
    })
}

/// The branch checked out in the repo `dir` is in; None when detached or outside a repo.
pub fn current_branch(dir: &Path) -> Option<String> {
    run_text(dir, &["symbolic-ref", "--short", "-q", "HEAD"])
        .ok()
        .map(|b| b.trim().to_string())
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
/// Deletes branch `name` only while it's still at `sha`, and its settings with it, as
/// `git branch -D` does. update-ref has no checked-out guard: the caller checks that.
pub(super) fn delete_branch_at(repo: &Path, name: &str, sha: &str) -> Result<(), String> {
    run(
        repo,
        &["update-ref", "-d", &format!("refs/heads/{name}"), sha],
    )?;
    let _ = run(
        repo,
        &["config", "--remove-section", &format!("branch.{name}")],
    );
    Ok(())
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_branches_from_reflog_subjects() {
        let subjects = "checkout: moving from feat-b to main
commit: tweak
checkout: moving from main to feat-b
Branch: renamed refs/heads/old-name to refs/heads/feat-c
checkout: moving from old-name to main
checkout: moving from main to 1a2b3c4d5e6f
checkout: moving from feat-a to old-name
reset: moving to HEAD~1
checkout: moving from main to feat-a
";
        // Newest first, each once; a renamed-away name never comes back.
        assert_eq!(
            recent_from_reflog(subjects, 10),
            ["main", "feat-b", "feat-c", "1a2b3c4d5e6f", "feat-a"]
        );
        assert_eq!(recent_from_reflog(subjects, 2), ["main", "feat-b"]);
        assert!(recent_from_reflog("", 5).is_empty());
    }

    #[test]
    fn upstream_track_counts() {
        assert_eq!(track(""), (0, 0, false));
        assert_eq!(track("ahead 3"), (3, 0, false));
        assert_eq!(track("behind 12"), (0, 12, false));
        assert_eq!(track("ahead 1, behind 40"), (1, 40, false));
        assert_eq!(track("gone"), (0, 0, true));
        // Anything else counts as nothing rather than a guess.
        assert_eq!(track("aheadish 3, behind x"), (0, 0, false));
    }
}
