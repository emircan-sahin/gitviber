//! Pull requests: listing, details, files, creating, merging, reviews and line comments.

use super::{
    all_pages, call, fetch_remote, graphql, list_state, pages, repo_ref, string, target, Method,
    Session, JSON, MAX_PAGES,
};
use crate::git;
use crate::network::Net;
use serde::Serialize;
use serde_json::{json, Value};
use std::path::Path;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Pull {
    pub number: u64,
    pub title: String,
    /// "open" | "closed" | "merged"
    pub state: String,
    pub draft: bool,
    pub author: String,
    pub head_ref: String,
    pub head_sha: String,
    pub head_repo: Option<String>,
    pub base_ref: String,
    pub base_sha: String,
    pub created_at: String,
    pub updated_at: String,
    pub url: String,
}

fn pull_from(v: &Value) -> Pull {
    let merged = !v["merged_at"].is_null();
    Pull {
        number: v["number"].as_u64().unwrap_or_default(),
        title: string(&v["title"]),
        state: if merged {
            "merged".into()
        } else {
            string(&v["state"])
        },
        draft: v["draft"].as_bool().unwrap_or(false),
        author: string(&v["user"]["login"]),
        head_ref: string(&v["head"]["ref"]),
        head_sha: string(&v["head"]["sha"]),
        head_repo: v["head"]["repo"]["full_name"].as_str().map(str::to_string),
        base_ref: string(&v["base"]["ref"]),
        base_sha: string(&v["base"]["sha"]),
        created_at: string(&v["created_at"]),
        updated_at: string(&v["updated_at"]),
        url: string(&v["html_url"]),
    }
}

/// `state`: "open" | "closed" | "all"; `pages`: how many pages of PER_PAGE, most recently
/// updated first ("Load more" asks for one more; the ones before come back as 304s).
pub fn list(
    session: &Session,
    repo: &Path,
    to: Option<&str>,
    state: &str,
    pages: usize,
) -> Result<Vec<Pull>, String> {
    let r = target(session, repo, to)?;
    let state = list_state(state);
    let list = self::pages(
        session,
        repo,
        &r.api(&format!("/pulls?state={state}&sort=updated&direction=desc")),
        JSON,
        None,
        pages.clamp(1, MAX_PAGES),
    )?;
    Ok(list.iter().map(pull_from).collect())
}

/// CI's verdict on each commit that has one, as GitHub rolls its checks and statuses up:
/// "success", "failure" or "pending". One request for up to 100 commits.
pub fn ci_states(
    session: &Session,
    repo: &Path,
    to: Option<&str>,
    shas: &[String],
) -> Result<std::collections::HashMap<String, String>, String> {
    let r = target(session, repo, to)?;
    let shas: Vec<&String> = shas
        .iter()
        .filter(|s| s.len() == 40 && s.chars().all(|c| c.is_ascii_hexdigit()))
        .take(100)
        .collect();
    let mut out = std::collections::HashMap::new();
    if shas.is_empty() {
        return Ok(out);
    }
    let fields: String = shas
        .iter()
        .enumerate()
        .map(|(i, s)| {
            format!("c{i}: object(oid: \"{s}\") {{ ... on Commit {{ statusCheckRollup {{ state }} }} }} ")
        })
        .collect();
    let query = format!(
        "query($owner: String!, $name: String!) {{ repository(owner: $owner, name: $name) {{ {fields}}} }}"
    );
    let v = graphql(
        session,
        repo,
        &query,
        json!({ "owner": r.owner, "name": r.name }),
    )?;
    for (i, sha) in shas.iter().enumerate() {
        let state = match v["repository"][format!("c{i}")]["statusCheckRollup"]["state"].as_str() {
            Some("SUCCESS") => "success",
            Some("FAILURE" | "ERROR") => "failure",
            Some("PENDING" | "EXPECTED") => "pending",
            // Not on GitHub, or no checks.
            _ => continue,
        };
        out.insert(sha.to_string(), state.to_string());
    }
    Ok(out)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Check {
    pub name: String,
    /// "success" | "failure" | "pending" | "neutral" | "skipped" | "cancelled"
    pub state: String,
    pub url: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Comment {
    pub author: String,
    pub body: String,
    pub created_at: String,
    /// Review verdict for review entries: APPROVED, CHANGES_REQUESTED, COMMENTED
    pub review: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PullDetail {
    #[serde(flatten)]
    pub pull: Pull,
    pub body: String,
    pub additions: u64,
    pub deletions: u64,
    pub changed_files: u64,
    pub commits: u64,
    /// null while GitHub is still computing it
    pub mergeable: Option<bool>,
    pub mergeable_state: String,
    pub checks: Vec<Check>,
    /// Checks that couldn't be read (a token without access to them, say): not "no checks".
    pub checks_error: Option<String>,
    pub comments: Vec<Comment>,
    /// Who closed it, if closed: an author may reopen only what they closed themselves
    pub closed_by: Option<String>,
    /// Who merged it, if merged: often not its author.
    pub merged_by: Option<String>,
}

pub fn detail(
    session: &Session,
    repo: &Path,
    to: Option<&str>,
    number: u64,
) -> Result<PullDetail, String> {
    let r = target(session, repo, to)?;
    let base = r.api("");
    let v = call(
        session,
        repo,
        Method::Get,
        &format!("{base}/pulls/{number}"),
    )?;
    let pull = pull_from(&v);
    // The pulls endpoint doesn't say who closed it; the issue behind every PR does.
    let closed_by = if pull.state == "closed" {
        call(
            session,
            repo,
            Method::Get,
            &format!("{base}/issues/{number}"),
        )
        .ok()
        .and_then(|i| i["closed_by"]["login"].as_str().map(str::to_string))
    } else {
        None
    };

    let mut checks = vec![];
    let mut checks_error = None;
    let sha = &pull.head_sha;
    match pages(
        session,
        repo,
        &format!("{base}/commits/{sha}/check-runs"),
        JSON,
        Some("check_runs"),
        MAX_PAGES,
    ) {
        Ok(runs) => {
            for c in runs {
                let state = if c["status"] != "completed" {
                    "pending".to_string()
                } else {
                    string(&c["conclusion"])
                };
                checks.push(Check {
                    name: string(&c["name"]),
                    state,
                    url: c["html_url"].as_str().map(str::to_string),
                });
            }
        }
        Err(e) => checks_error = Some(e),
    }
    match pages(
        session,
        repo,
        &format!("{base}/commits/{sha}/status"),
        JSON,
        Some("statuses"),
        MAX_PAGES,
    ) {
        Ok(statuses) => {
            for c in statuses {
                let state = match c["state"].as_str() {
                    Some("error") => "failure".to_string(),
                    other => other.unwrap_or("pending").to_string(),
                };
                checks.push(Check {
                    name: string(&c["context"]),
                    state,
                    url: c["target_url"].as_str().map(str::to_string),
                });
            }
        }
        Err(e) => {
            checks_error.get_or_insert(e);
        }
    }

    // A failure here is an error, not "no comments": an empty thread would be a lie.
    let mut comments = vec![];
    for c in all_pages(
        session,
        repo,
        &format!("{base}/issues/{number}/comments"),
        JSON,
    )? {
        comments.push(Comment {
            author: string(&c["user"]["login"]),
            body: string(&c["body"]),
            created_at: string(&c["created_at"]),
            review: None,
        });
    }
    for c in all_pages(
        session,
        repo,
        &format!("{base}/pulls/{number}/reviews"),
        JSON,
    )? {
        // Bare "commented" reviews with no text are line comments' containers; skip the noise.
        if c["state"] == "COMMENTED" && string(&c["body"]).is_empty() {
            continue;
        }
        comments.push(Comment {
            author: string(&c["user"]["login"]),
            body: string(&c["body"]),
            created_at: string(&c["submitted_at"]),
            review: c["state"].as_str().map(str::to_string),
        });
    }
    comments.sort_by(|a, b| a.created_at.cmp(&b.created_at));

    Ok(PullDetail {
        closed_by,
        merged_by: v["merged_by"]["login"].as_str().map(str::to_string),
        body: string(&v["body"]),
        additions: v["additions"].as_u64().unwrap_or_default(),
        deletions: v["deletions"].as_u64().unwrap_or_default(),
        changed_files: v["changed_files"].as_u64().unwrap_or_default(),
        commits: v["commits"].as_u64().unwrap_or_default(),
        mergeable: v["mergeable"].as_bool(),
        mergeable_state: string(&v["mergeable_state"]),
        checks,
        checks_error,
        comments,
        pull,
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PullFiles {
    /// The merge base: what GitHub diffs the PR against.
    pub base: String,
    pub head: String,
    pub files: Vec<git::FileChange>,
}

/// Fetches the PR's commits (no refs are created) and diffs them locally, so PR files
/// open in the same full-file viewer as everything else.
#[allow(clippy::too_many_arguments)]
pub fn files(
    session: &Session,
    repo: &Path,
    to: Option<&str>,
    number: u64,
    base_ref: &str,
    base_sha: &str,
    head_sha: &str,
    net: &Net,
) -> Result<PullFiles, String> {
    let have =
        |sha: &str| git::run(repo, &["cat-file", "-e", &format!("{sha}^{{commit}}")]).is_ok();
    if !have(head_sha) || !have(base_sha) {
        git::validate_branch(repo, base_ref)?;
        git::fetch_objects(
            repo,
            &fetch_remote(session, repo, to)?,
            &[
                format!("pull/{number}/head"),
                format!("refs/heads/{base_ref}"),
            ],
            net,
        )?;
    }
    let base = git::merge_base(repo, base_sha, head_sha)?;
    Ok(PullFiles {
        files: git::range_files(repo, &base, head_sha)?,
        base,
        head: head_sha.to_string(),
    })
}

#[allow(clippy::too_many_arguments)]
pub fn create(
    session: &Session,
    repo: &Path,
    to: Option<&str>,
    title: &str,
    body: &str,
    head: &str,
    base: &str,
    draft: bool,
    maintainer_edits: bool,
) -> Result<Pull, String> {
    let r = target(session, repo, to)?;
    let origin = repo_ref(repo)?;
    let mut pr =
        json!({ "title": title, "body": body, "head": head, "base": base, "draft": draft });
    // Into the parent, the branch is named by the fork it lives in, and its maintainers may be
    // let push to it (GitHub's "Allow edits by maintainers"; only a fork's PR has it).
    if !r.is(&origin.full()) {
        pr["head"] = json!(format!("{}:{head}", origin.owner));
        pr["maintainer_can_modify"] = json!(maintainer_edits);
    }
    let v = call(session, repo, Method::Post(pr), &r.api("/pulls"))?;
    Ok(pull_from(&v))
}

/// `method`: "merge" | "squash" | "rebase"
pub fn merge(
    session: &Session,
    repo: &Path,
    to: Option<&str>,
    number: u64,
    method: &str,
) -> Result<(), String> {
    let r = target(session, repo, to)?;
    let method = if matches!(method, "merge" | "squash" | "rebase") {
        method
    } else {
        "merge"
    };
    call(
        session,
        repo,
        Method::Put(json!({ "merge_method": method })),
        &r.api(&format!("/pulls/{number}/merge")),
    )
    .map(|_| ())
}

/// Closes or reopens a PR. Close-then-reopen also makes GitHub recompute a stale diff or
/// conflict state, e.g. after the PR below it in a stack was merged.
pub fn set_open(
    session: &Session,
    repo: &Path,
    to: Option<&str>,
    number: u64,
    open: bool,
) -> Result<Pull, String> {
    let r = target(session, repo, to)?;
    let v = call(
        session,
        repo,
        Method::Patch(json!({ "state": if open { "open" } else { "closed" } })),
        &r.api(&format!("/pulls/{number}")),
    )?;
    Ok(pull_from(&v))
}

/// `event`: "APPROVE" | "REQUEST_CHANGES" | "COMMENT". GitHub requires a body for the
/// last two, and refuses the first two on your own PR; its message says so.
pub fn review(
    session: &Session,
    repo: &Path,
    to: Option<&str>,
    number: u64,
    event: &str,
    body: &str,
) -> Result<(), String> {
    let r = target(session, repo, to)?;
    if !matches!(event, "APPROVE" | "REQUEST_CHANGES" | "COMMENT") {
        return Err(format!("unknown review event: {event}"));
    }
    call(
        session,
        repo,
        Method::Post(json!({ "event": event, "body": body })),
        &r.api(&format!("/pulls/{number}/reviews")),
    )
    .map(|_| ())
}

/// A comment on a line of a PR's diff. `line` is where it sits now, on its `side` (LEFT, the
/// old file; RIGHT, the new one); none when the diff moved on past it (outdated).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewComment {
    pub id: u64,
    /// The thread's first comment, for a reply.
    pub reply_to: Option<u64>,
    pub path: String,
    pub line: Option<u64>,
    pub side: String,
    pub author: String,
    pub body: String,
    pub created_at: String,
    pub url: String,
}

fn review_comment(c: &Value) -> ReviewComment {
    ReviewComment {
        id: c["id"].as_u64().unwrap_or_default(),
        reply_to: c["in_reply_to_id"].as_u64(),
        path: string(&c["path"]),
        line: c["line"].as_u64(),
        side: c["side"].as_str().unwrap_or("RIGHT").to_string(),
        author: string(&c["user"]["login"]),
        body: string(&c["body"]),
        created_at: string(&c["created_at"]),
        url: string(&c["html_url"]),
    }
}

pub fn review_comments(
    session: &Session,
    repo: &Path,
    to: Option<&str>,
    number: u64,
) -> Result<Vec<ReviewComment>, String> {
    let r = target(session, repo, to)?;
    let path = r.api(&format!("/pulls/{number}/comments"));
    Ok(all_pages(session, repo, &path, JSON)?
        .iter()
        .map(review_comment)
        .collect())
}

/// A new comment on `line` of `path` as it is at `commit` (the PR's head), on `side`; or with
/// `reply_to`, an answer in that comment's thread.
#[allow(clippy::too_many_arguments)]
pub fn comment_line(
    session: &Session,
    repo: &Path,
    to: Option<&str>,
    number: u64,
    commit: &str,
    path: &str,
    line: u64,
    side: &str,
    reply_to: Option<u64>,
    body: &str,
) -> Result<ReviewComment, String> {
    let r = target(session, repo, to)?;
    if body.trim().is_empty() {
        return Err("The comment is empty.".into());
    }
    let base = r.api(&format!("/pulls/{number}/comments"));
    let v = match reply_to {
        Some(id) => call(
            session,
            repo,
            Method::Post(json!({ "body": body })),
            &format!("{base}/{id}/replies"),
        )?,
        None => {
            if !matches!(side, "LEFT" | "RIGHT") {
                return Err(format!("unknown side: {side}"));
            }
            call(
                session,
                repo,
                Method::Post(json!({
                    "body": body, "commit_id": commit, "path": path, "line": line, "side": side,
                })),
                &base,
            )?
        }
    };
    Ok(review_comment(&v))
}
