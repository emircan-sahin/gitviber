//! Committing, and what the commit form reads: template, recent authors, details.

use super::{command, git_dir, has_head, run, run_text, run_with, validate_rev};
use crate::lfs;
use crate::network::{self, Net, CANCELLED};
use crate::process::exec;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Duration;

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct CommitOptions {
    pub amend: bool,
    /// `--signoff`: a Signed-off-by trailer for the committer.
    pub sign_off: bool,
    /// `--no-verify`: skips the pre-commit and commit-msg hooks.
    pub no_verify: bool,
    /// "Name <email>" each, added as Co-authored-by trailers.
    pub co_authors: Vec<String>,
}

/// Ends a failed commit's message when a hook `--no-verify` skips is set up and stopped it. git
/// says nothing of its own when one fails, so this is how the page knows to offer committing
/// without them.
pub const HOOKS_HINT: &str = "hint: Commit hooks set up here: ";

/// A commit cancelled after its hooks stashed something (lint-staged's backup), which they may
/// not have put back.
pub const CANCELLED_STASHED: &str = "git:cancelled-stashed";

/// How git starts the lines it ends a commit with, exit code 1, before or after the hooks ran.
const OWN_REFUSALS: [&str; 4] = [
    "nothing to commit",
    "nothing added to commit",
    "no changes added to commit",
    "Aborting commit due to empty commit message",
];

/// Watched like a network command: the hooks' output is its progress, and Cancel stops git with
/// the hooks it runs. Stopped after git moved HEAD (a post-commit hook was left), the commit
/// stands and this succeeds.
pub fn commit(repo: &Path, message: &str, opts: &CommitOptions, net: &Net) -> Result<(), String> {
    // git formats and places the trailers, next to any the message already has.
    let trailers = opts
        .co_authors
        .iter()
        .map(|a| match a.trim() {
            a if a.is_empty() || a.chars().any(char::is_control) => {
                Err(format!("invalid co-author: {a:?}"))
            }
            a => Ok(format!("--trailer=Co-authored-by: {a}")),
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut args = vec!["commit"];
    for (on, flag) in [
        (opts.amend, "--amend"),
        (opts.sign_off, "--signoff"),
        (opts.no_verify, "--no-verify"),
    ] {
        if on {
            args.push(flag);
        }
    }
    args.extend(trailers.iter().map(String::as_str));
    let input = if opts.amend && message.trim().is_empty() {
        // Amending with no new message keeps the old one.
        args.push("--no-edit");
        None
    } else {
        // Message goes through stdin so it is never parsed as arguments.
        args.extend(["-F", "-"]);
        Some(message.as_bytes())
    };
    let paths = run_text(
        repo,
        &[
            "rev-parse",
            "--git-path",
            "hooks/pre-commit",
            "--git-path",
            "hooks/commit-msg",
        ],
    )
    .unwrap_or_default();
    let paths: Vec<_> = paths.lines().map(|p| repo.join(p)).collect();
    let mut cmd = command(repo, &args);
    // It has no paths, and hooks inherit its environment: lint-staged's `git stash --keep-index`
    // restores the index with `:/`, which literal pathspecs turn into a file of that name.
    cmd.env_remove("GIT_LITERAL_PATHSPECS");
    let stash = tip(repo, "refs/stash");
    let head = tip(repo, "HEAD");
    match network::run_local(cmd, "git commit", net, input) {
        Ok(_) => Ok(()),
        // Not a file's change: a hook's own git (lint-staged's stash) writes the reflogs too.
        Err(f) if f.message == CANCELLED && made(repo, head.as_deref(), message, opts.amend) => {
            Ok(())
        }
        Err(f) if f.message == CANCELLED && tip(repo, "refs/stash") != stash => {
            Err(CANCELLED_STASHED.into())
        }
        Err(f) => {
            let hooks: Vec<_> = ["pre-commit", "commit-msg"]
                .into_iter()
                .zip(&paths)
                .filter(|(_, p)| is_hook(p))
                .map(|(name, _)| name)
                .collect();
            // A failing hook makes git exit 1 without a word; its own failures are 128 (a
            // signature that failed, a merge in the way) or 1 with one of these. A cancel is None.
            let own = f
                .message
                .lines()
                .any(|l| OWN_REFUSALS.iter().any(|r| l.starts_with(r)));
            Err(
                if f.code != Some(1) || own || opts.no_verify || hooks.is_empty() {
                    f.message
                } else {
                    format!("{}\n{HOOKS_HINT}{}.", f.message, hooks.join(", "))
                },
            )
        }
    }
}

/// Whether a cancelled commit was made first (a post-commit hook was left). An agent committing
/// in a terminal while our hooks run moves HEAD too, so ours is told by its parent (the old head,
/// or its parent for an amend) and subject. A hook that rewrites the subject makes a made commit
/// read as cancelled, the side that never takes an agent's commit for ours.
fn made(repo: &Path, head: Option<&str>, message: &str, amend: bool) -> bool {
    let now = tip(repo, "HEAD");
    if now.as_deref() == head {
        return false;
    }
    let first = |m: &str| {
        m.lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .unwrap_or("")
            .to_string()
    };
    let (parent, subject) = match (amend, head) {
        (false, head) => (head.map(str::trim).map(String::from), first(message)),
        (true, Some(head)) => {
            let Some((parent, old)) = parent_and_message(repo, head.trim()) else {
                return false;
            };
            let msg = if message.trim().is_empty() {
                &old
            } else {
                message
            };
            (parent, first(msg))
        }
        (true, None) => return false,
    };
    parent_and_message(repo, "HEAD").is_some_and(|(p, m)| p == parent && first(&m) == subject)
}

/// `rev`'s first parent (None for a root commit) and its message.
fn parent_and_message(repo: &Path, rev: &str) -> Option<(Option<String>, String)> {
    let out = run_text(
        repo,
        &[
            "log",
            "-1",
            "--no-show-signature",
            "--format=%P%n%B",
            rev,
            "--",
        ],
    )
    .ok()?;
    let (parents, message) = out.split_once('\n')?;
    let parent = parents.split(' ').next().filter(|p| !p.is_empty());
    Some((parent.map(String::from), message.to_string()))
}

/// The commit `rev` names now; None for an unborn branch or a ref that isn't there.
fn tip(repo: &Path, rev: &str) -> Option<String> {
    run_text(repo, &["rev-parse", "-q", "--verify", rev]).ok()
}

/// What git runs as a hook: a file it can execute (it skips one that isn't, with a hint).
fn is_hook(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    #[cfg(unix)]
    return meta.is_file()
        && std::os::unix::fs::PermissionsExt::mode(&meta.permissions()) & 0o111 != 0;
    #[cfg(not(unix))]
    meta.is_file()
}

/// GitHub refuses a push with a file over 100 MiB, and by then the commit has to come out of
/// history.
const LARGE_FILE_BYTES: u64 = 100 * 1024 * 1024;

#[derive(Serialize)]
pub struct LargeFile {
    pub path: String,
    /// "123.4 MB"
    pub size: String,
}

/// Staged files over GitHub's limit, by the blob the commit will hold: a file Git LFS tracks is
/// staged as its small pointer, so it never counts, while one LFS should have taken but didn't
/// (git-lfs not installed) does.
pub fn large_staged(repo: &Path) -> Result<Vec<LargeFile>, String> {
    let raw = run(
        repo,
        &[
            "diff",
            "--cached",
            "--raw",
            "-z",
            "--no-abbrev",
            "--no-renames",
            "--diff-filter=AMT",
        ],
    )?;
    let raw = String::from_utf8_lossy(&raw);
    let mut fields = raw.split('\0');
    let mut staged = vec![];
    while let (Some(meta), Some(path)) = (fields.next(), fields.next()) {
        // ":<old mode> <new mode> <old id> <new id> <status>"; a submodule (160000) is a commit.
        if let [_, mode, _, id, _] = meta.split(' ').collect::<Vec<_>>()[..] {
            if mode != "160000" {
                staged.push((id, path));
            }
        }
    }
    if staged.is_empty() {
        return Ok(vec![]);
    }
    let ids: String = staged.iter().map(|(id, _)| format!("{id}\n")).collect();
    let sizes = run_with(
        repo,
        &["cat-file", "--batch-check=%(objectsize)"],
        &[],
        Some(ids.as_bytes()),
    )?;
    // One line per id, in order; a missing object's doesn't parse and is skipped.
    Ok(staged
        .iter()
        .zip(String::from_utf8_lossy(&sizes).lines())
        .filter_map(|((_, path), size)| {
            let n: u64 = size.parse().ok()?;
            (n > LARGE_FILE_BYTES).then(|| LargeFile {
                path: path.to_string(),
                size: lfs::size_label(n),
            })
        })
        .collect())
}

/// Where git leaves a message for the next commit, in the order `git commit` joins them.
pub(super) const PREPARED: [&str; 2] = ["SQUASH_MSG", "MERGE_MSG"];

/// The message as git would start it, comment lines stripped: the one a `merge --squash` or
/// `cherry-pick -n` prepared (SQUASH_MSG then MERGE_MSG, joined as `git commit` does), else
/// `commit.template`'s. None when there's neither, or it can't be read (git reports that itself).
pub fn commit_template(repo: &Path) -> Option<String> {
    let prepared: Vec<u8> = git_dir(repo)
        .map(|d| PREPARED.map(|f| std::fs::read(d.join(f)).unwrap_or_default()))
        .unwrap_or_default()
        .concat();
    let bytes = if prepared.is_empty() {
        let path = run_text(repo, &["config", "--path", "--get", "commit.template"]).ok()?;
        // A relative path is relative to where git runs, the worktree root.
        std::fs::read(repo.join(path.trim())).ok()?
    } else {
        prepared
    };
    let text = run_with(repo, &["stripspace", "--strip-comments"], &[], Some(&bytes)).ok()?;
    let text = String::from_utf8_lossy(&text).trim().to_string();
    (!text.is_empty()).then_some(text)
}

/// Who to suggest as a co-author: recent authors and co-authors, newest first, minus the user.
pub fn recent_authors(repo: &Path) -> Result<Vec<String>, String> {
    if !has_head(repo) {
        return Ok(vec![]);
    }
    let me = run_text(repo, &["config", "user.email"]).unwrap_or_default();
    let me = format!("<{}>", me.trim().to_lowercase());
    let out = run_text(
        repo,
        &[
            "log",
            "-n500",
            "--format=%aN <%aE>%n%(trailers:key=Co-authored-by,valueonly,unfold)",
            "HEAD",
            "--",
        ],
    )?;
    let mut seen = std::collections::HashSet::new();
    Ok(out
        .lines()
        .map(str::trim)
        .filter(|a| a.ends_with('>') && !a.to_lowercase().ends_with(&me))
        .filter(|a| seen.insert(a.to_lowercase()))
        .take(200)
        .map(str::to_string)
        .collect())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitDetails {
    /// `%G?`: G good, U good but of unknown validity, X/Y good but expired signature/key,
    /// R good but revoked key, B bad, E can't be checked (e.g. missing key), N none.
    pub signature: String,
    pub signer: String,
    /// `commit.gpgSign` is on, so an unsigned commit is worth pointing out.
    pub sign_expected: bool,
    /// Key and value of each trailer (Co-authored-by, Signed-off-by…), in order.
    pub trailers: Vec<(String, String)>,
}

/// What the commit header shows beyond the log: verifying a signature runs gpg or ssh, so
/// it's asked for one commit at a time.
pub fn commit_details(repo: &Path, sha: &str) -> Result<CommitDetails, String> {
    validate_rev(sha)?;
    let out = exec(
        command(
            repo,
            &[
                "log",
                "-1",
                "--format=%G?%x1f%GS%x1f%(trailers:only,unfold)",
                sha,
                "--",
            ],
        ),
        "git log",
        &[],
        None,
        // A verifier waiting on a key server shouldn't leave the header loading for good.
        Some(Duration::from_secs(10)),
    )?;
    let out = String::from_utf8_lossy(&out);
    let mut f = out.splitn(3, '\x1f');
    let mut next = || f.next().unwrap_or_default().trim().to_string();
    let (mut signature, signer, trailers) = (next(), next(), next());
    // An ssh signature with no gpg.ssh.allowedSignersFile reads as N, as if there were none.
    if signature == "N" {
        let raw = run_text(repo, &["cat-file", "commit", sha])?;
        let header = raw.split("\n\n").next().unwrap_or_default();
        if header.lines().any(|l| l.starts_with("gpgsig")) {
            signature = "E".into();
        }
    }
    let sign_expected = run_text(repo, &["config", "--type=bool", "commit.gpgSign"])
        .is_ok_and(|v| v.trim() == "true");
    Ok(CommitDetails {
        signature,
        signer,
        sign_expected,
        trailers: trailers
            .lines()
            .filter_map(|l| l.split_once(':'))
            .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
            .collect(),
    })
}
