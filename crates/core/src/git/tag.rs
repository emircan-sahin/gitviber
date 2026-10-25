//! Tags, local and on remotes.

use super::{
    command, publish_remote, push_target, run, run_network, run_text, run_with, validate_rev,
    validate_tag,
};
use crate::network::{self, Net};
use serde::Serialize;
use std::path::Path;

/// A lightweight tag, or with a `message` an annotated one (which `push --follow-tags` sends).
pub fn create_tag(repo: &Path, name: &str, sha: &str, message: Option<&str>) -> Result<(), String> {
    validate_rev(sha)?;
    validate_tag(repo, name)?;
    match message.map(str::trim).filter(|m| !m.is_empty()) {
        // Through stdin, like a commit message, so it is never parsed as arguments.
        Some(m) => run_with(
            repo,
            &["tag", "-a", "-F", "-", name, sha],
            &[],
            Some(m.as_bytes()),
        ),
        None => run(repo, &["tag", name, sha]),
    }
    .map(|_| ())
}

pub fn delete_tag(repo: &Path, name: &str) -> Result<(), String> {
    validate_tag(repo, name)?;
    run(repo, &["tag", "-d", name]).map(|_| ())
}

/// Where tags are pushed: where `git push` sends this branch, else where Publish would.
fn tag_remote(repo: &Path) -> Result<String, String> {
    run_text(repo, &["symbolic-ref", "--short", "-q", "HEAD"])
        .ok()
        .and_then(|b| push_target(repo, b.trim()))
        .map(|p| p.remote)
        .map_or_else(|| publish_remote(repo), Ok)
}

/// Pushes these tags; returns the remote they went to.
pub fn push_tags(repo: &Path, names: &[String], net: &Net) -> Result<String, String> {
    let remote = tag_remote(repo)?;
    let refs: Vec<String> = names
        .iter()
        .map(|n| validate_tag(repo, n).map(|_| format!("refs/tags/{n}")))
        .collect::<Result<_, _>>()?;
    if refs.is_empty() {
        return Ok(remote);
    }
    let mut args = vec!["push", remote.as_str()];
    args.extend(refs.iter().map(String::as_str));
    run_network(repo, &args, net).map(|_| remote.clone())
}

/// Deletes a tag from the remote tags are pushed to (the local one stays); returns that remote.
pub fn delete_remote_tag(repo: &Path, name: &str, net: &Net) -> Result<String, String> {
    validate_tag(repo, name)?;
    let remote = tag_remote(repo)?;
    let full = format!("refs/tags/{name}");
    run_network(repo, &["push", &remote, "--delete", &full], net).map(|_| remote.clone())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteTags {
    pub remote: String,
    pub names: Vec<String>,
}

/// The tags on the remote tags are pushed to. A network call: asked for when a menu opens,
/// never on refresh.
pub fn remote_tags(repo: &Path, net: &Net) -> Result<RemoteTags, String> {
    let remote = tag_remote(repo)?;
    // ls-remote has no --progress; `net` is for Cancel, which the menu uses to give up quickly.
    let args = ["ls-remote", "--tags", "--refs", &remote];
    let out = network::run(command(repo, &args), "git ls-remote", net, None)?;
    let names = String::from_utf8_lossy(&out)
        .lines()
        .filter_map(|l| l.split_once("\trefs/tags/").map(|(_, n)| n.to_string()))
        .collect();
    Ok(RemoteTags { remote, names })
}

/// Tag names, newest first.
pub fn tags(repo: &Path) -> Result<Vec<String>, String> {
    let out = run_text(
        repo,
        &[
            "for-each-ref",
            "--sort=-creatordate",
            "--format=%(refname:lstrip=2)",
            "refs/tags",
        ],
    )?;
    Ok(out.lines().map(str::to_string).collect())
}
