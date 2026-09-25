//! Checks on names and revisions from the page, so none can be read as an option.

use super::run;
use std::path::Path;

/// A remote name git accepts, that can't be read as an option.
pub(super) fn validate_remote_name(repo: &Path, name: &str) -> Result<(), String> {
    let ok = !name.is_empty()
        && !name.starts_with('-')
        && !name.contains('/')
        && run(
            repo,
            &["check-ref-format", &format!("refs/remotes/{name}/x")],
        )
        .is_ok();
    ok.then_some(())
        .ok_or_else(|| format!("invalid remote name: {name}"))
}

pub(super) fn validate_url(url: &str) -> Result<(), String> {
    if url.trim().is_empty() || url.starts_with('-') || url.contains(['\n', '\r']) {
        return Err(format!("invalid remote URL: {url}"));
    }
    Ok(())
}

/// A config value typed into a form: one line, not empty.
pub(super) fn validate_one_line(key: &str, value: &str) -> Result<(), String> {
    if value.is_empty() || value.contains(['\n', '\r']) {
        return Err(format!("{key} must be one line of text"));
    }
    Ok(())
}

pub(super) fn validate_rev(rev: &str) -> Result<(), String> {
    if rev.len() >= 4 && rev.len() <= 64 && rev.chars().all(|c| c.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(format!("invalid commit id: {rev}"))
    }
}

pub(crate) fn validate_branch(repo: &Path, name: &str) -> Result<(), String> {
    // check-ref-format also rejects a leading '-', so the name can't be read as a flag.
    // "@" means HEAD wherever a revision is read, so a branch by that name is a trap.
    if name == "@" {
        return Err(format!("invalid branch name: {name}"));
    }
    run(repo, &["check-ref-format", "--branch", name])
        .map(|_| ())
        .map_err(|_| format!("invalid branch name: {name}"))
}

pub(super) const REF_KINDS: [(&str, &str); 3] = [
    ("refs/heads/", "--branches"),
    ("refs/remotes/", "--remotes"),
    ("refs/tags/", "--tags"),
];

/// A full branch, remote-tracking branch or tag name, never an option or a range.
pub(super) fn validate_full_ref(repo: &Path, name: &str) -> Result<(), String> {
    let kind = REF_KINDS.iter().any(|(prefix, _)| name.starts_with(prefix));
    if !kind || run(repo, &["check-ref-format", name]).is_err() {
        return Err(format!("not a branch or tag: {name}"));
    }
    Ok(())
}

/// A commit id, or one with a trailing `^` for its first parent: the old side of a commit's diff.
pub(crate) fn validate_tree_rev(rev: &str) -> Result<(), String> {
    validate_rev(rev.strip_suffix('^').unwrap_or(rev))
}

/// Accepts a local or remote-tracking branch (or any commit-ish) that isn't an option.
pub(super) fn validate_ref(repo: &Path, name: &str) -> Result<(), String> {
    let spec = format!("{name}^{{commit}}");
    if name.starts_with('-') || run(repo, &["rev-parse", "--verify", "-q", &spec]).is_err() {
        return Err(format!("unknown branch or commit: {name}"));
    }
    Ok(())
}

pub(super) fn validate_tag(repo: &Path, name: &str) -> Result<(), String> {
    let full = format!("refs/tags/{name}");
    // "@" alone is valid in a full ref but means HEAD wherever a revision is read.
    if name == "@" || name.starts_with('-') || run(repo, &["check-ref-format", &full]).is_err() {
        return Err(format!("invalid tag name: {name}"));
    }
    Ok(())
}

/// What a new branch may start at: HEAD, a full ref to a local or remote branch or a tag,
/// or a commit's full id (SHA-1 or SHA-256).
pub(super) fn validate_base(repo: &Path, base: &str) -> Result<(), String> {
    let sha = matches!(base.len(), 40 | 64) && base.chars().all(|c| c.is_ascii_hexdigit());
    if base != "HEAD"
        && !sha
        && !["refs/heads/", "refs/remotes/", "refs/tags/"]
            .iter()
            .any(|p| base.starts_with(p))
    {
        return Err(format!("not a branch or tag: {base}"));
    }
    validate_ref(repo, base)
}
