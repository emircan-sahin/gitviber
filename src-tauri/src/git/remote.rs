//! Remotes: listing and editing them, their URLs, and the github.com token git stores.

use super::{command, config_value, run, run_text, run_with, validate_remote_name, validate_url};
use crate::process::exec;
use serde::Serialize;
use std::path::Path;
use std::time::Duration;

#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Remote {
    pub name: String,
    pub url: String,
    /// Where pushes go when it isn't `url` (`remote.<name>.pushurl`).
    pub push_url: Option<String>,
}

/// Every remote, as `git remote` lists them (by name), from one read of their config rather
/// than two per remote.
pub fn remote_list(repo: &Path) -> Result<Vec<Remote>, String> {
    // 1: no remote.* key at all.
    let raw = run_with(
        repo,
        &["config", "-z", "--get-regexp", r"^remote\."],
        &[1],
        None,
    )?;
    let mut list: Vec<Remote> = vec![];
    for record in String::from_utf8_lossy(&raw).split('\0') {
        let (key, value) = record.split_once('\n').unwrap_or((record, ""));
        // remote.<name>.<key>, where the name may have dots; remote.pushDefault has none.
        let Some((name, key)) = key.strip_prefix("remote.").and_then(|k| k.rsplit_once('.')) else {
            continue;
        };
        let at = match list.iter().position(|r| r.name == name) {
            Some(i) => i,
            None => {
                list.push(Remote {
                    name: name.to_string(),
                    url: String::new(),
                    push_url: None,
                });
                list.len() - 1
            }
        };
        // Last one wins, as with `git config --get`.
        let value = value.trim();
        match key {
            "url" => list[at].url = value.to_string(),
            "pushurl" => list[at].push_url = Some(value.to_string()).filter(|v| !v.is_empty()),
            _ => {}
        }
    }
    list.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(list)
}

pub fn remote_add(repo: &Path, name: &str, url: &str) -> Result<(), String> {
    validate_remote_name(repo, name)?;
    validate_url(url)?;
    run(repo, &["remote", "add", "--", name, url.trim()]).map(|_| ())
}

/// Removes a remote, and with it its remote-tracking branches and the upstreams set to them.
pub fn remote_remove(repo: &Path, name: &str) -> Result<(), String> {
    validate_remote_name(repo, name)?;
    run(repo, &["remote", "remove", name]).map(|_| ())
}

/// Renames a remote; its remote-tracking branches and upstreams follow.
pub fn remote_rename(repo: &Path, name: &str, to: &str) -> Result<(), String> {
    validate_remote_name(repo, name)?;
    validate_remote_name(repo, to)?;
    run(repo, &["remote", "rename", name, to]).map(|_| ())
}

pub fn remote_set_url(repo: &Path, name: &str, url: &str) -> Result<(), String> {
    validate_remote_name(repo, name)?;
    validate_url(url)?;
    run(repo, &["remote", "set-url", "--", name, url.trim()]).map(|_| ())
}

pub fn remote_url(repo: &Path, remote: &str) -> Option<String> {
    run_text(repo, &["remote", "get-url", remote])
        .ok()
        .map(|s| s.trim().to_string())
}

/// The token git already stores for github.com (osxkeychain, GitHub Desktop, GCM…).
pub fn credential_token(repo: &Path) -> Option<String> {
    let mut cmd = command(repo, &["-c", "core.askPass=", "credential", "fill"]);
    // Never pop a login window from the background; no stored credential means "none".
    cmd.env("GCM_INTERACTIVE", "never")
        .env_remove("GIT_ASKPASS")
        .env_remove("SSH_ASKPASS");
    let out = exec(
        cmd,
        "git credential",
        &[],
        Some(b"protocol=https\nhost=github.com\n\n"),
        Some(Duration::from_secs(10)),
    )
    .ok()?;
    String::from_utf8_lossy(&out)
        .lines()
        .find_map(|l| l.strip_prefix("password=").map(str::to_string))
        .filter(|t| !t.is_empty())
}

pub fn remotes(repo: &Path) -> Vec<String> {
    run_text(repo, &["remote"])
        .map(|s| s.lines().map(str::to_string).collect())
        .unwrap_or_default()
}

/// Each remote with the URL it fetches from, as `remote_url` reads it (`insteadOf` applied),
/// from one `git remote -v`. None without a URL, where get-url would echo the name back.
pub fn remote_urls(repo: &Path) -> Vec<(String, Option<String>)> {
    let out = run_text(repo, &["remote", "-v"]).unwrap_or_default();
    let mut list: Vec<(String, Option<String>)> = vec![];
    for line in out.lines() {
        // `<name>\t<url> (fetch)`, a partial clone's with ` [<filter>]` after; `<name>\t` alone
        // for a remote without a URL.
        let (name, rest) = line.split_once('\t').unwrap_or((line, ""));
        let url = rest.rfind(" (fetch)").map(|i| rest[..i].to_string());
        match list.iter_mut().find(|(n, _)| n == name) {
            Some((_, u)) => {
                if u.is_none() {
                    *u = url;
                }
            }
            None => list.push((name.to_string(), url)),
        }
    }
    list
}

/// Where a branch with no upstream is first pushed: its `pushRemote`, `remote.pushDefault`,
/// the only remote, or origin among several. Anything else is the user's call.
pub fn publish_remote(repo: &Path) -> Result<String, String> {
    let branch = run_text(repo, &["symbolic-ref", "--short", "-q", "HEAD"]).ok();
    publish_remote_among(repo, &remotes(repo), branch.as_deref().map(str::trim))
}

/// `publish_remote` for a caller that has the remotes and the branch at hand already.
pub(super) fn publish_remote_among(
    repo: &Path,
    all: &[String],
    branch: Option<&str>,
) -> Result<String, String> {
    let exists = |r: &String| all.contains(r);
    let configured = branch
        .and_then(|b| config_value(repo, None, &format!("branch.{b}.pushRemote")))
        .filter(exists)
        .or_else(|| config_value(repo, None, "remote.pushDefault").filter(exists));
    if let Some(r) = configured {
        return Ok(r);
    }
    match all {
        [] => Err("This repository has no remote to publish to. Add one first, e.g. `git remote add origin <url>`.".into()),
        [only] => Ok(only.clone()),
        _ if all.iter().any(|r| r == "origin") => Ok("origin".into()),
        _ => Err(format!(
            "This repository has several remotes ({}). Choose one to publish to.",
            all.join(", ")
        )),
    }
}
