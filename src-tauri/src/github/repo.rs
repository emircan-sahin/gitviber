//! Which GitHub repositories this repo's remotes are, and what the account may do there.

use super::{call, pages, string, Method, Session, JSON};
use crate::git;
use crate::network::Net;
use serde::Serialize;
use serde_json::{json, Value};
use std::path::Path;

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RepoRef {
    pub owner: String,
    pub name: String,
}

/// owner/name from the origin remote, for github.com in https, ssh or scp-like form.
pub fn parse_remote(url: &str) -> Option<RepoRef> {
    let rest = url
        .strip_prefix("https://github.com/")
        .or_else(|| url.strip_prefix("http://github.com/"))
        .or_else(|| url.strip_prefix("git@github.com:"))
        .or_else(|| url.strip_prefix("ssh://git@github.com/"))?;
    let rest = rest.trim_end_matches('/').trim_end_matches(".git");
    let (owner, name) = rest.split_once('/')?;
    (!owner.is_empty() && !name.is_empty() && !name.contains('/')).then(|| RepoRef {
        owner: owner.into(),
        name: name.into(),
    })
}

impl RepoRef {
    pub(super) fn full(&self) -> String {
        format!("{}/{}", self.owner, self.name)
    }

    /// A REST path under this repository: `tail` is "" or starts with '/' or '?'.
    pub(super) fn api(&self, tail: &str) -> String {
        format!("/repos/{}/{}{tail}", self.owner, self.name)
    }

    /// GitHub names are case-insensitive; a remote URL may spell them differently.
    pub(super) fn is(&self, full: &str) -> bool {
        self.full().eq_ignore_ascii_case(full)
    }
}

pub(super) fn repo_ref(repo: &Path) -> Result<RepoRef, String> {
    let url = git::remote_url(repo, "origin").ok_or("This repository has no 'origin' remote.")?;
    parse_remote(&url)
        .ok_or_else(|| "The 'origin' remote is not a github.com repository.".to_string())
}

/// owner/name of any folder's GitHub origin, e.g. a saved project that isn't open.
pub fn origin_repo(repo: &Path) -> Option<String> {
    repo_ref(repo).ok().map(|r| r.full())
}

fn split_full(full: &str) -> Option<RepoRef> {
    let (owner, name) = full.split_once('/')?;
    Some(RepoRef {
        owner: owner.into(),
        name: name.into(),
    })
}

fn repo_info(session: &Session, repo: &Path, r: &RepoRef) -> Result<Value, String> {
    call(session, repo, Method::Get, &r.api(""))
}

/// Origin's current name and the repository it was forked from. A renamed or transferred
/// repo still answers at the name its remote has; the reply carries the current one.
fn origin_names(session: &Session, repo: &Path) -> Result<(RepoRef, Option<RepoRef>), String> {
    let origin = repo_ref(repo)?;
    let info = repo_info(session, repo, &origin)?;
    let current = info["full_name"].as_str().and_then(split_full);
    let parent = info["parent"]["full_name"].as_str().and_then(split_full);
    Ok((current.unwrap_or(origin), parent))
}

/// Where a request goes: origin (`to` = None or its name), or the repository it was forked
/// from. Nothing else, so the UI can't aim this token's writes at an arbitrary repository.
pub(super) fn target(session: &Session, repo: &Path, to: Option<&str>) -> Result<RepoRef, String> {
    let origin = repo_ref(repo)?;
    let Some(to) = to.filter(|t| !origin.is(t)) else {
        return Ok(origin);
    };
    let (current, parent) = origin_names(session, repo)?;
    if current.is(to) {
        return Ok(origin);
    }
    parent
        .filter(|p| p.is(to))
        .ok_or_else(|| format!("{to} is neither origin nor the repository it was forked from."))
}

/// What the signed-in account may do in one repository.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Access {
    /// Its current name, which a renamed repo's remote URL may not have.
    pub repo: RepoRef,
    pub default_branch: Option<String>,
    /// Write access: merge, and close or edit anyone's PRs and issues
    pub push: bool,
    /// Triage: close and reopen anyone's PRs and issues, without write access
    pub triage: bool,
    /// The only role GitHub lets delete issues
    pub admin: bool,
    /// Issues are switched on (forks start with them off)
    pub issues: bool,
}

fn access(r: RepoRef, info: &Value) -> Access {
    let can = |p: &str| info["permissions"][p].as_bool().unwrap_or(false);
    Access {
        repo: info["full_name"].as_str().and_then(split_full).unwrap_or(r),
        default_branch: info["default_branch"].as_str().map(str::to_string),
        push: can("push"),
        triage: can("triage"),
        admin: can("admin"),
        issues: info["has_issues"].as_bool().unwrap_or(true),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub login: String,
    /// "gh" (GitHub CLI) or "git" (git credential store)
    pub source: String,
    pub origin: Option<Access>,
    /// The repository origin was forked from
    pub parent: Option<Access>,
}

/// Fails as a whole if any lookup fails: a half-read account (no permissions, no parent)
/// would hide buttons and the fork panes until the next refresh. The UI keeps the last one.
pub fn account(session: &Session, repo: &Path) -> Result<Account, String> {
    let user = call(session, repo, Method::Get, "/user")?;
    let source = session.token(repo)?.source.to_string();
    let (origin, parent) = match repo_ref(repo) {
        Ok(r) => {
            let info = repo_info(session, repo, &r)?;
            let parent = match info["parent"]["full_name"].as_str().and_then(split_full) {
                Some(p) => Some(access(p.clone(), &repo_info(session, repo, &p)?)),
                None => None,
            };
            (Some(access(r, &info)), parent)
        }
        // No GitHub origin: there's an account but nothing to list.
        Err(_) => (None, None),
    };
    Ok(Account {
        login: user["login"].as_str().unwrap_or_default().into(),
        source,
        origin,
        parent,
    })
}

/// The signed-in account's name and email, to suggest as git's identity. Only when the app
/// already holds a token: prefilling a form isn't worth a gh or keychain prompt. A private
/// email becomes GitHub's noreply address, which still links commits to the account.
pub fn profile(session: &Session, repo: &Path) -> Option<git::Identity> {
    if !session.has_token() {
        return None;
    }
    let user = call(session, repo, Method::Get, "/user").ok()?;
    let email = user["email"].as_str().map(str::to_string).or_else(|| {
        let (id, login) = (user["id"].as_u64()?, user["login"].as_str()?);
        Some(format!("{id}+{login}@users.noreply.github.com"))
    });
    Some(git::Identity {
        name: user["name"].as_str().map(str::to_string),
        email,
    })
}

/// Origin's branches under GitHub branch protection, which the picker won't offer to delete.
pub fn protected_branches(session: &Session, repo: &Path) -> Result<Vec<String>, String> {
    let r = repo_ref(repo)?;
    let v = call(
        session,
        repo,
        Method::Get,
        &r.api("/branches?protected=true&per_page=100"),
    )?;
    Ok(v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|b| b["name"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default())
}

/// A repository the signed-in account can clone, for the Clone dialog's list.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnRepo {
    pub full_name: String,
    pub description: String,
    pub private: bool,
    pub clone_url: String,
    pub updated_at: String,
}

/// The account's repositories and the ones it works on (collaborator, organization member),
/// most recently updated first. `cwd`: any folder git can run in, for finding the token.
pub fn own_repos(session: &Session, cwd: &Path) -> Result<Vec<OwnRepo>, String> {
    let list = pages(
        session,
        cwd,
        "/user/repos?sort=updated&affiliation=owner,collaborator,organization_member",
        JSON,
        None,
        3,
    )?;
    Ok(list
        .iter()
        .map(|r| OwnRepo {
            full_name: string(&r["full_name"]),
            description: string(&r["description"]),
            private: r["private"].as_bool().unwrap_or(false),
            clone_url: string(&r["clone_url"]),
            updated_at: string(&r["updated_at"]),
        })
        .collect())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Remote {
    pub name: String,
    /// The GitHub repository it points at, owner/name; None off github.com
    pub repo: Option<String>,
}

/// This repo's remotes and the GitHub repositories behind them. Local config only.
pub fn remotes(repo: &Path) -> Vec<Remote> {
    git::remote_urls(repo)
        .into_iter()
        .map(|(name, url)| Remote {
            name,
            repo: url.and_then(|u| parse_remote(&u)).map(|r| r.full()),
        })
        .collect()
}

/// A remote that points at `r`, e.g. a fork's "upstream".
fn remote_for(repo: &Path, r: &RepoRef) -> Option<String> {
    remotes(repo)
        .into_iter()
        .find(|m| m.repo.as_deref().is_some_and(|full| r.is(full)))
        .map(|m| m.name)
}

/// Where a PR's commits are fetched from: origin, or for the parent a remote pointing at it,
/// else its https URL.
pub fn fetch_remote(session: &Session, repo: &Path, to: Option<&str>) -> Result<String, String> {
    let r = target(session, repo, to)?;
    if r.is(&repo_ref(repo)?.full()) {
        return Ok("origin".into());
    }
    Ok(remote_for(repo, &r).unwrap_or_else(|| format!("https://github.com/{}.git", r.full())))
}

/// The remote for `original`, the fork's parent as the UI knows it from the account, fetched
/// first if `fetch`; None when there is none yet. No GitHub call, so it works offline: it
/// only finds and fetches a remote this repo already has.
pub fn original_remote(
    repo: &Path,
    original: &str,
    fetch: bool,
    net: &Net,
) -> Result<Option<String>, String> {
    let r = split_full(original).ok_or_else(|| format!("not a repository name: {original}"))?;
    let Some(name) = remote_for(repo, &r) else {
        return Ok(None);
    };
    if fetch {
        git::fetch_remote(repo, &name, net)?;
    }
    Ok(Some(name))
}

/// GitHub's "Sync fork": brings origin's `branch` up to date with the original's default
/// branch, on GitHub, then fetches origin. Returns how: "fast-forward", "merge" or "none".
/// A conflict comes back as GitHub's 409 message; that takes a local merge.
pub fn sync_fork(
    session: &Session,
    repo: &Path,
    branch: &str,
    net: &Net,
) -> Result<String, String> {
    git::validate_branch(repo, branch)?;
    let r = repo_ref(repo)?;
    let v = call(
        session,
        repo,
        Method::Post(json!({ "branch": branch })),
        &r.api("/merge-upstream"),
    )?;
    git::fetch_remote(repo, "origin", net)?;
    Ok(v["merge_type"].as_str().unwrap_or("none").to_string())
}

/// Adds the fork's original as "upstream" (the usual name; "original" if that's taken),
/// over the same protocol as origin, and fetches it.
pub fn add_original_remote(session: &Session, repo: &Path, net: &Net) -> Result<String, String> {
    let (_, parent) = origin_names(session, repo)?;
    let r = parent.ok_or("origin is not a fork.")?;
    if let Some(name) = remote_for(repo, &r) {
        return Ok(name);
    }
    let taken = git::remote_url(repo, "upstream").is_some();
    let name = if taken { "original" } else { "upstream" };
    let origin = git::remote_url(repo, "origin").unwrap_or_default();
    let url = if origin.starts_with("https://") || origin.starts_with("http://") {
        format!("https://github.com/{}.git", r.full())
    } else {
        format!("git@github.com:{}.git", r.full())
    };
    git::run(repo, &["remote", "add", name, &url])?;
    // Branches made from upstream's then still push to the fork, not into the original.
    if git::run(repo, &["config", "--get", "remote.pushDefault"]).is_err() {
        git::set_push_default(repo, "origin")?;
    }
    git::fetch_remote(repo, name, net)?;
    Ok(name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remotes() {
        for url in [
            "https://github.com/a/b.git",
            "git@github.com:a/b.git",
            "ssh://git@github.com/a/b",
            "https://github.com/a/b/",
        ] {
            let r = parse_remote(url).unwrap_or_else(|| panic!("{url}"));
            assert_eq!((r.owner.as_str(), r.name.as_str()), ("a", "b"));
        }
        assert!(parse_remote("https://gitlab.com/a/b.git").is_none());
        assert!(parse_remote("https://github.com/a").is_none());
    }
}
