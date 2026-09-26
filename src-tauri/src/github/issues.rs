//! Issues: listing, counting, details, editing, labels and comments.

use super::{all_pages, call, graphql, list_state, string, target, Comment, Method, Session, JSON};
use serde::Serialize;
use serde_json::{json, Value};
use std::path::Path;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Label {
    pub name: String,
    /// Hex without the '#'
    pub color: String,
    pub description: String,
}

fn label_from(v: &Value) -> Label {
    Label {
        name: string(&v["name"]),
        color: string(&v["color"]),
        description: string(&v["description"]),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    pub number: u64,
    pub title: String,
    /// "open" | "closed"
    pub state: String,
    /// "completed" | "not_planned" | "reopened", when GitHub recorded one
    pub state_reason: Option<String>,
    pub author: String,
    pub labels: Vec<Label>,
    pub assignees: Vec<String>,
    pub comments: u64,
    pub created_at: String,
    pub updated_at: String,
    pub url: String,
}

fn issue_from(v: &Value) -> Issue {
    Issue {
        number: v["number"].as_u64().unwrap_or_default(),
        title: string(&v["title"]),
        state: string(&v["state"]),
        state_reason: v["state_reason"].as_str().map(str::to_string),
        author: string(&v["user"]["login"]),
        labels: v["labels"]
            .as_array()
            .into_iter()
            .flatten()
            .map(label_from)
            .collect(),
        assignees: v["assignees"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|a| string(&a["login"]))
            .collect(),
        comments: v["comments"].as_u64().unwrap_or_default(),
        created_at: string(&v["created_at"]),
        updated_at: string(&v["updated_at"]),
        url: string(&v["html_url"]),
    }
}

/// `state`: "open" | "closed" | "all"; `labels`: only issues carrying all of them. GitHub
/// lists PRs as issues too; they're left out.
pub fn issues(
    session: &Session,
    repo: &Path,
    to: Option<&str>,
    state: &str,
    labels: &[String],
) -> Result<Vec<Issue>, String> {
    let r = target(session, repo, to)?;
    let state = list_state(state);
    // The endpoint lists PRs too, and they're dropped here: one page of recent activity can
    // be nearly all PRs (a repo's "all" showed 3 issues). Read on until there are enough.
    const WANT: usize = 50;
    const PAGE: usize = 100;
    // GitHub splits the list on commas after decoding, so a name with a comma can't be asked for.
    let labels = if labels.is_empty() {
        String::new()
    } else {
        let names: Vec<String> = labels.iter().map(|l| query_value(l)).collect();
        format!("&labels={}", names.join(","))
    };
    let mut out = vec![];
    for page in 1..=5 {
        let v = call(
            session,
            repo,
            Method::Get,
            &r.api(&format!("/issues?state={state}{labels}&sort=updated&direction=desc&per_page={PAGE}&page={page}")),
        )?;
        let items = v.as_array().cloned().unwrap_or_default();
        out.extend(
            items
                .iter()
                .filter(|i| i.get("pull_request").is_none())
                .map(issue_from),
        );
        if items.len() < PAGE || out.len() >= WANT {
            break;
        }
    }
    out.truncate(WANT);
    Ok(out)
}

/// Every label defined in the repository, for filtering by one it has no recent issue with.
pub fn issue_labels(
    session: &Session,
    repo: &Path,
    to: Option<&str>,
) -> Result<Vec<Label>, String> {
    let r = target(session, repo, to)?;
    let path = r.api("/labels");
    Ok(all_pages(session, repo, &path, JSON)?
        .iter()
        .map(label_from)
        .collect())
}

/// Percent-encodes a query value: everything but unreserved ASCII (label names have spaces,
/// colons, emoji).
fn query_value(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IssueDetail {
    #[serde(flatten)]
    pub issue: Issue,
    pub body: String,
    /// The conversation; `issue.comments` is only the count
    pub thread: Vec<Comment>,
    /// Who closed it, if closed: an author may reopen only what they closed themselves
    pub closed_by: Option<String>,
}

pub fn issue_detail(
    session: &Session,
    repo: &Path,
    to: Option<&str>,
    number: u64,
) -> Result<IssueDetail, String> {
    let r = target(session, repo, to)?;
    let path = r.api(&format!("/issues/{number}"));
    let v = call(session, repo, Method::Get, &path)?;
    let thread = all_pages(session, repo, &format!("{path}/comments"), JSON)?
        .iter()
        .map(|c| Comment {
            author: string(&c["user"]["login"]),
            body: string(&c["body"]),
            created_at: string(&c["created_at"]),
            review: None,
        })
        .collect();
    Ok(IssueDetail {
        closed_by: v["closed_by"]["login"].as_str().map(str::to_string),
        body: string(&v["body"]),
        issue: issue_from(&v),
        thread,
    })
}

pub fn issue_create(
    session: &Session,
    repo: &Path,
    to: Option<&str>,
    title: &str,
    body: &str,
) -> Result<Issue, String> {
    let r = target(session, repo, to)?;
    let v = call(
        session,
        repo,
        Method::Post(json!({ "title": title, "body": body })),
        &r.api("/issues"),
    )?;
    Ok(issue_from(&v))
}

/// Edits the title and description.
pub fn issue_edit(
    session: &Session,
    repo: &Path,
    to: Option<&str>,
    number: u64,
    title: &str,
    body: &str,
) -> Result<Issue, String> {
    let r = target(session, repo, to)?;
    let v = call(
        session,
        repo,
        Method::Patch(json!({ "title": title, "body": body })),
        &r.api(&format!("/issues/{number}")),
    )?;
    Ok(issue_from(&v))
}

/// `reason` when closing: "completed" | "not_planned". Reopening records "reopened" itself.
pub fn issue_set_open(
    session: &Session,
    repo: &Path,
    to: Option<&str>,
    number: u64,
    open: bool,
    reason: &str,
) -> Result<Issue, String> {
    let r = target(session, repo, to)?;
    let patch = if open {
        json!({ "state": "open" })
    } else {
        let reason = if reason == "not_planned" {
            "not_planned"
        } else {
            "completed"
        };
        json!({ "state": "closed", "state_reason": reason })
    };
    let v = call(
        session,
        repo,
        Method::Patch(patch),
        &r.api(&format!("/issues/{number}")),
    )?;
    Ok(issue_from(&v))
}

/// Replaces an issue's labels with `labels`; returns the ones it carries now.
pub fn issue_set_labels(
    session: &Session,
    repo: &Path,
    to: Option<&str>,
    number: u64,
    labels: &[String],
) -> Result<Vec<Label>, String> {
    let r = target(session, repo, to)?;
    let v = call(
        session,
        repo,
        Method::Put(json!({ "labels": labels })),
        &r.api(&format!("/issues/{number}/labels")),
    )?;
    Ok(v.as_array().into_iter().flatten().map(label_from).collect())
}

/// Deletes an issue for good. REST has no endpoint for it, only GraphQL's `deleteIssue`,
/// and GitHub allows it to repository admins alone.
pub fn issue_delete(
    session: &Session,
    repo: &Path,
    to: Option<&str>,
    number: u64,
) -> Result<(), String> {
    let r = target(session, repo, to)?;
    let v = call(
        session,
        repo,
        Method::Get,
        &r.api(&format!("/issues/{number}")),
    )?;
    if v.get("pull_request").is_some() {
        return Err(format!("#{number} is a pull request, not an issue."));
    }
    let id = v["node_id"]
        .as_str()
        .ok_or("GitHub sent no id for this issue.")?;
    graphql(
        session,
        repo,
        "mutation($id: ID!) { deleteIssue(input: { issueId: $id }) { clientMutationId } }",
        json!({ "id": id }),
    )?;
    Ok(())
}

/// How many issues or pull requests are open and closed (a PR's closed counts the merged).
#[derive(Serialize)]
pub struct StateCounts {
    pub open: u64,
    pub closed: u64,
}

/// How many issues are open and closed, carrying all of `labels`: the list holds only the 50
/// most recent, so it can't be counted. GraphQL has both counts in one request (two with labels).
pub fn issue_counts(
    session: &Session,
    repo: &Path,
    to: Option<&str>,
    labels: &[String],
) -> Result<StateCounts, String> {
    let r = target(session, repo, to)?;
    let count = |v: &Value| v.as_u64().unwrap_or_default();
    let v = graphql(
        session,
        repo,
        "query($owner: String!, $name: String!) { repository(owner: $owner, name: $name) {
            nameWithOwner
            open: issues(states: OPEN) { totalCount }
            closed: issues(states: CLOSED) { totalCount }
        } }",
        json!({ "owner": r.owner, "name": r.name }),
    )?;
    let found = &v["repository"];
    if labels.is_empty() {
        return Ok(StateCounts {
            open: count(&found["open"]["totalCount"]),
            closed: count(&found["closed"]["totalCount"]),
        });
    }
    // `issues(labels:)` counts issues with any of them; search's `label:` qualifiers need all,
    // as the list does. Search finds nothing under a renamed repository's old name.
    let full = found["nameWithOwner"]
        .as_str()
        .map_or_else(|| r.full(), str::to_string);
    let labels: String = labels.iter().map(|l| format!(" label:\"{l}\"")).collect();
    let q = |state: &str| format!("repo:{full} is:issue is:{state}{labels}");
    let v = graphql(
        session,
        repo,
        "query($open: String!, $closed: String!) {
            open: search(query: $open, type: ISSUE) { issueCount }
            closed: search(query: $closed, type: ISSUE) { issueCount }
        }",
        json!({ "open": q("open"), "closed": q("closed") }),
    )?;
    Ok(StateCounts {
        open: count(&v["open"]["issueCount"]),
        closed: count(&v["closed"]["issueCount"]),
    })
}

/// A comment in an issue's conversation (a PR's works the same way).
pub fn issue_comment(
    session: &Session,
    repo: &Path,
    to: Option<&str>,
    number: u64,
    body: &str,
) -> Result<(), String> {
    let r = target(session, repo, to)?;
    call(
        session,
        repo,
        Method::Post(json!({ "body": body })),
        &r.api(&format!("/issues/{number}/comments")),
    )
    .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_values_are_percent_encoded() {
        assert_eq!(query_value("difficulty: easy"), "difficulty%3A%20easy");
        assert_eq!(query_value("a,b&c=d"), "a%2Cb%26c%3Dd");
        assert_eq!(query_value("good-first_issue.~"), "good-first_issue.~");
        assert_eq!(query_value("ü"), "%C3%BC");
    }
}
