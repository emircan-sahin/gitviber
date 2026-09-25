//! Committing, and what the commit form reads: template, recent authors, details.

use super::{command, has_head, run, run_text, run_with, validate_rev};
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

pub fn commit(repo: &Path, message: &str, opts: &CommitOptions) -> Result<(), String> {
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
    if opts.amend && message.trim().is_empty() {
        // Amending with no new message keeps the old one.
        args.push("--no-edit");
        return run(repo, &args).map(|_| ());
    }
    // Message goes through stdin so it is never parsed as arguments.
    args.extend(["-F", "-"]);
    run_with(repo, &args, &[], Some(message.as_bytes())).map(|_| ())
}

/// `commit.template`'s text as git would start the message: comment lines stripped.
/// None when it isn't set, or can't be read (git reports that itself when it commits).
pub fn commit_template(repo: &Path) -> Option<String> {
    let path = run_text(repo, &["config", "--path", "--get", "commit.template"]).ok()?;
    // A relative path is relative to where git runs, the worktree root.
    let bytes = std::fs::read(repo.join(path.trim())).ok()?;
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
