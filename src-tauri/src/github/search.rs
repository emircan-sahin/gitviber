//! Narrowed lists of pull requests and issues. REST has no author, assignee or label to ask a
//! PR list for, and the matches may lie ten pages down, so a narrowed list is GitHub's search
//! (as the pull request extension for VS Code does). "Me" is the login of the token's account,
//! never `@me`, so it follows the account picked for the repository.

use super::{
    call, graphql, repo_info, string, Issue, Label, Method, Pull, RepoRef, Session, StateCounts,
    PER_PAGE,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::Path;

/// What a list is narrowed to, on top of its open/closed state.
#[derive(Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ListFilter {
    pub scope: Option<Scope>,
    /// Pull requests only: Some(true) drafts, Some(false) those ready for review.
    pub draft: Option<bool>,
    /// Carrying all of them.
    pub labels: Vec<String>,
}

#[derive(Deserialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub enum Scope {
    Created,
    Assigned,
    Mentioned,
    /// Pull requests only.
    ReviewRequested,
}

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Kind {
    Pull,
    Issue,
}

impl ListFilter {
    /// Nothing narrows the list: the plain endpoints answer, and the ETag cache with them.
    pub(super) fn is_empty(&self, kind: Kind) -> bool {
        self.scope.is_none()
            && self.labels.is_empty()
            && (kind == Kind::Issue || self.draft.is_none())
    }

    fn needs_login(&self) -> bool {
        self.scope.is_some()
    }
}

/// GitHub's search syntax for `filter`; `state` is "open", "closed" or "all". `repo` is the
/// repository's current name (search finds nothing under an old one).
pub(super) fn search_query(
    repo: &str,
    kind: Kind,
    state: &str,
    filter: &ListFilter,
    login: &str,
) -> String {
    let mut q = format!(
        "repo:{repo} is:{}",
        if kind == Kind::Pull { "pr" } else { "issue" }
    );
    if matches!(state, "open" | "closed") {
        q += &format!(" is:{state}");
    }
    match (filter.scope, kind) {
        (Some(Scope::Created), _) => q += &format!(" author:{login}"),
        (Some(Scope::Assigned), _) => q += &format!(" assignee:{login}"),
        (Some(Scope::Mentioned), _) => q += &format!(" mentions:{login}"),
        (Some(Scope::ReviewRequested), Kind::Pull) => q += &format!(" review-requested:{login}"),
        (Some(Scope::ReviewRequested), Kind::Issue) | (None, _) => {}
    }
    if let (Some(draft), Kind::Pull) = (filter.draft, kind) {
        q += &format!(" draft:{draft}");
    }
    for label in &filter.labels {
        q += &format!(
            " label:\"{}\"",
            label.replace('\\', "\\\\").replace('"', "\\\"")
        );
    }
    q + " sort:updated-desc"
}

/// The signed-in account's login. A 304 when it hasn't changed, so it costs nothing.
fn viewer_login(session: &Session, repo: &Path) -> Result<String, String> {
    let user = call(session, repo, Method::Get, "/user")?;
    Some(string(&user["login"]))
        .filter(|l| !l.is_empty())
        .ok_or_else(|| "GitHub didn't say which account this is.".to_string())
}

/// The query for one state of `filter`, with the repository's current name and the account.
fn query_for(
    session: &Session,
    repo: &Path,
    r: &RepoRef,
    kind: Kind,
    filter: &ListFilter,
) -> Result<impl Fn(&str) -> String, String> {
    let login = if filter.needs_login() {
        viewer_login(session, repo)?
    } else {
        String::new()
    };
    let info = repo_info(session, repo, r)?;
    let full = info["full_name"]
        .as_str()
        .map_or_else(|| r.full(), str::to_string);
    let filter = filter.clone();
    Ok(move |state: &str| search_query(&full, kind, state, &filter, &login))
}

struct Page {
    nodes: Vec<Value>,
    next: Option<String>,
}

/// Up to `max` pages, following each page's cursor until there is none.
fn read_pages(
    max: usize,
    mut fetch: impl FnMut(Option<&str>) -> Result<Page, String>,
) -> Result<Vec<Value>, String> {
    let mut out = vec![];
    let mut after: Option<String> = None;
    for _ in 0..max {
        let page = fetch(after.as_deref())?;
        out.extend(page.nodes);
        match page.next {
            Some(cursor) => after = Some(cursor),
            None => break,
        }
    }
    Ok(out)
}

const PULL_FIELDS: &str = "... on PullRequest { number title state isDraft author { login }
    headRefName headRefOid headRepository { nameWithOwner } baseRefName baseRefOid
    createdAt updatedAt url }";
const ISSUE_FIELDS: &str = "... on Issue { number title state stateReason author { login }
    labels(first: 20) { nodes { name color description } } assignees(first: 10) { nodes { login } }
    comments { totalCount } createdAt updatedAt url }";

fn search_page(
    session: &Session,
    repo: &Path,
    fields: &str,
    query: &str,
    first: usize,
    after: Option<&str>,
) -> Result<Page, String> {
    let v = graphql(
        session,
        repo,
        &format!(
            "query($q: String!, $first: Int!, $after: String) {{
                search(query: $q, type: ISSUE, first: $first, after: $after) {{
                    pageInfo {{ hasNextPage endCursor }} nodes {{ {fields} }}
                }}
            }}"
        ),
        json!({ "q": query, "first": first, "after": after }),
    )?;
    let search = &v["search"];
    Ok(Page {
        // A node of the other kind comes back empty.
        nodes: search["nodes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|n| n["number"].is_u64())
            .cloned()
            .collect(),
        next: search["pageInfo"]["hasNextPage"]
            .as_bool()
            .unwrap_or(false)
            .then(|| string(&search["pageInfo"]["endCursor"])),
    })
}

/// GitHub's search lists no more than 1000 results.
pub(super) const MAX_SEARCH_PAGES: usize = 1000 / PER_PAGE;

/// `pages` of PER_PAGE pull requests, most recently updated first.
pub(super) fn pulls(
    session: &Session,
    repo: &Path,
    r: &RepoRef,
    state: &str,
    filter: &ListFilter,
    pages: usize,
) -> Result<Vec<Pull>, String> {
    let query = query_for(session, repo, r, Kind::Pull, filter)?(state);
    let nodes = read_pages(pages.clamp(1, MAX_SEARCH_PAGES), |after| {
        search_page(session, repo, PULL_FIELDS, &query, PER_PAGE, after)
    })?;
    Ok(nodes.iter().map(pull_from_node).collect())
}

/// The `first` most recently updated issues.
pub(super) fn issues(
    session: &Session,
    repo: &Path,
    r: &RepoRef,
    state: &str,
    filter: &ListFilter,
    first: usize,
) -> Result<Vec<Issue>, String> {
    let query = query_for(session, repo, r, Kind::Issue, filter)?(state);
    let page = search_page(session, repo, ISSUE_FIELDS, &query, first, None)?;
    Ok(page.nodes.iter().map(issue_from_node).collect())
}

/// How many are open and closed (a PR's closed counts the merged), in one request.
pub(super) fn counts(
    session: &Session,
    repo: &Path,
    r: &RepoRef,
    kind: Kind,
    filter: &ListFilter,
) -> Result<StateCounts, String> {
    let q = query_for(session, repo, r, kind, filter)?;
    let v = graphql(
        session,
        repo,
        "query($open: String!, $closed: String!) {
            open: search(query: $open, type: ISSUE) { issueCount }
            closed: search(query: $closed, type: ISSUE) { issueCount }
        }",
        json!({ "open": q("open"), "closed": q("closed") }),
    )?;
    let count = |v: &Value| v.as_u64().unwrap_or_default();
    Ok(StateCounts {
        open: count(&v["open"]["issueCount"]),
        closed: count(&v["closed"]["issueCount"]),
    })
}

/// A deleted account is "ghost" in REST; GraphQL has no author at all.
fn login_of(v: &Value) -> String {
    v["login"].as_str().unwrap_or("ghost").to_string()
}

fn pull_from_node(v: &Value) -> Pull {
    Pull {
        number: v["number"].as_u64().unwrap_or_default(),
        title: string(&v["title"]),
        state: string(&v["state"]).to_lowercase(),
        draft: v["isDraft"].as_bool().unwrap_or(false),
        author: login_of(&v["author"]),
        head_ref: string(&v["headRefName"]),
        head_sha: string(&v["headRefOid"]),
        head_repo: v["headRepository"]["nameWithOwner"]
            .as_str()
            .map(str::to_string),
        base_ref: string(&v["baseRefName"]),
        base_sha: string(&v["baseRefOid"]),
        created_at: string(&v["createdAt"]),
        updated_at: string(&v["updatedAt"]),
        url: string(&v["url"]),
    }
}

fn issue_from_node(v: &Value) -> Issue {
    Issue {
        number: v["number"].as_u64().unwrap_or_default(),
        title: string(&v["title"]),
        state: string(&v["state"]).to_lowercase(),
        state_reason: match v["stateReason"].as_str() {
            Some("COMPLETED") => Some("completed"),
            Some("NOT_PLANNED" | "DUPLICATE") => Some("not_planned"),
            Some("REOPENED") => Some("reopened"),
            _ => None,
        }
        .map(str::to_string),
        author: login_of(&v["author"]),
        labels: v["labels"]["nodes"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|l| Label {
                name: string(&l["name"]),
                color: string(&l["color"]),
                description: string(&l["description"]),
            })
            .collect(),
        assignees: v["assignees"]["nodes"]
            .as_array()
            .into_iter()
            .flatten()
            .map(login_of)
            .collect(),
        comments: v["comments"]["totalCount"].as_u64().unwrap_or_default(),
        created_at: string(&v["createdAt"]),
        updated_at: string(&v["updatedAt"]),
        url: string(&v["url"]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filter(scope: Option<Scope>, draft: Option<bool>, labels: &[&str]) -> ListFilter {
        ListFilter {
            scope,
            draft,
            labels: labels.iter().map(|l| l.to_string()).collect(),
        }
    }

    #[test]
    fn scopes_become_qualifiers_for_the_login() {
        let q = |s, kind| {
            search_query(
                "acme/app",
                kind,
                "open",
                &filter(Some(s), None, &[]),
                "mona",
            )
        };
        assert_eq!(
            q(Scope::Created, Kind::Pull),
            "repo:acme/app is:pr is:open author:mona sort:updated-desc"
        );
        assert!(q(Scope::Assigned, Kind::Issue).contains(" is:issue is:open assignee:mona "));
        assert!(q(Scope::Mentioned, Kind::Pull).contains(" mentions:mona "));
        assert!(q(Scope::ReviewRequested, Kind::Pull).contains(" review-requested:mona "));
        // The login, never `@me`: the account is the one picked for the repository.
        assert!(!q(Scope::Created, Kind::Pull).contains("@me"));
        // Nobody is asked to review an issue.
        assert!(!q(Scope::ReviewRequested, Kind::Issue).contains("review-requested"));
    }

    #[test]
    fn state_draft_and_order() {
        let none = filter(None, None, &[]);
        assert_eq!(
            search_query("acme/app", Kind::Issue, "all", &none, ""),
            "repo:acme/app is:issue sort:updated-desc"
        );
        assert!(search_query("acme/app", Kind::Pull, "closed", &none, "").contains(" is:closed "));
        let drafts = filter(None, Some(true), &[]);
        assert!(search_query("a/b", Kind::Pull, "open", &drafts, "").contains(" draft:true "));
        let ready = filter(None, Some(false), &[]);
        assert!(search_query("a/b", Kind::Pull, "open", &ready, "").contains(" draft:false "));
        // Issues have no drafts.
        assert!(!search_query("a/b", Kind::Issue, "open", &drafts, "").contains("draft"));
    }

    #[test]
    fn labels_are_quoted_and_escaped() {
        let f = filter(None, None, &["good first issue", "say \"hi\"", "a\\b"]);
        assert_eq!(
            search_query("a/b", Kind::Pull, "open", &f, ""),
            r#"repo:a/b is:pr is:open label:"good first issue" label:"say \"hi\"" label:"a\\b" sort:updated-desc"#
        );
    }

    #[test]
    fn only_a_narrowed_list_is_searched() {
        assert!(filter(None, None, &[]).is_empty(Kind::Pull));
        assert!(!filter(Some(Scope::Created), None, &[]).is_empty(Kind::Issue));
        // REST lists issues by label, not pull requests.
        assert!(!filter(None, None, &["bug"]).is_empty(Kind::Pull));
        assert!(!filter(None, Some(true), &[]).is_empty(Kind::Pull));
        assert!(filter(None, Some(true), &[]).is_empty(Kind::Issue));
    }

    #[test]
    fn scopes_come_in_as_the_ui_spells_them() {
        let f: ListFilter = serde_json::from_value(
            json!({ "scope": "reviewRequested", "draft": null, "labels": ["x"] }),
        )
        .unwrap();
        assert_eq!(f.scope, Some(Scope::ReviewRequested));
        assert_eq!(f.draft, None);
        assert!(
            serde_json::from_value::<ListFilter>(json!({ "scope": "everyone", "labels": [] }))
                .is_err()
        );
    }

    fn page(nodes: &[u64], next: Option<&str>) -> Result<Page, String> {
        Ok(Page {
            nodes: nodes.iter().map(|n| json!({ "number": n })).collect(),
            next: next.map(str::to_string),
        })
    }

    #[test]
    fn pages_follow_the_cursor_until_the_last() {
        let mut asked = vec![];
        let out = read_pages(10, |after| {
            asked.push(after.map(str::to_string));
            match after {
                None => page(&[3, 2], Some("c1")),
                Some("c1") => page(&[1], None),
                Some(other) => panic!("unexpected cursor {other}"),
            }
        })
        .unwrap();
        assert_eq!(out.len(), 3);
        assert_eq!(asked, vec![None, Some("c1".to_string())]);
    }

    #[test]
    fn pages_stop_at_the_cap_and_at_an_error() {
        let mut calls = 0;
        let out = read_pages(2, |_| {
            calls += 1;
            page(&[calls], Some("more"))
        })
        .unwrap();
        assert_eq!((out.len(), calls), (2, 2));
        let failed = read_pages(3, |after| match after {
            None => page(&[1], Some("c")),
            Some(_) => Err("rate limited".into()),
        });
        assert_eq!(failed.err().as_deref(), Some("rate limited"));
    }

    #[test]
    fn a_pull_request_node_maps_to_the_rest_shape() {
        let p = pull_from_node(&json!({
            "number": 7, "title": "Add retries", "state": "MERGED", "isDraft": false,
            "author": { "login": "mona" }, "headRefName": "retries", "headRefOid": "a".repeat(40),
            "headRepository": { "nameWithOwner": "mona/app" }, "baseRefName": "main",
            "baseRefOid": "b".repeat(40), "createdAt": "2026-01-01T00:00:00Z",
            "updatedAt": "2026-01-02T00:00:00Z", "url": "https://github.com/acme/app/pull/7"
        }));
        assert_eq!((p.number, p.state.as_str(), p.draft), (7, "merged", false));
        assert_eq!(p.head_repo.as_deref(), Some("mona/app"));
        assert_eq!(p.author, "mona");
        // A deleted fork and a deleted account leave nothing to read.
        let gone = pull_from_node(
            &json!({ "number": 8, "state": "OPEN", "isDraft": true, "author": null, "headRepository": null }),
        );
        assert_eq!(
            (gone.head_repo, gone.author.as_str(), gone.draft),
            (None, "ghost", true)
        );
    }

    #[test]
    fn an_issue_node_maps_to_the_rest_shape() {
        let i = issue_from_node(&json!({
            "number": 12, "title": "Crash on start", "state": "CLOSED", "stateReason": "NOT_PLANNED",
            "author": { "login": "hubot" },
            "labels": { "nodes": [{ "name": "bug", "color": "d73a4a", "description": "Broken" }] },
            "assignees": { "nodes": [{ "login": "mona" }] }, "comments": { "totalCount": 4 },
            "createdAt": "2026-01-01T00:00:00Z", "updatedAt": "2026-01-02T00:00:00Z",
            "url": "https://github.com/acme/app/issues/12"
        }));
        assert_eq!(
            (i.state.as_str(), i.state_reason.as_deref()),
            ("closed", Some("not_planned"))
        );
        assert_eq!(
            (i.labels.len(), i.assignees, i.comments),
            (1, vec!["mona".to_string()], 4)
        );
        assert_eq!(i.labels[0].name, "bug");
    }
    #[test]
    fn label_names_with_colons_emoji_and_spaces_stay_one_qualifier_each() {
        let f = filter(None, None, &["type: bug", "🐛 crash", "priority:high"]);
        assert_eq!(
            search_query("a/b", Kind::Issue, "open", &f, ""),
            r#"repo:a/b is:issue is:open label:"type: bug" label:"🐛 crash" label:"priority:high" sort:updated-desc"#
        );
    }

    #[test]
    fn logins_with_bot_suffixes_and_underscores_go_in_as_they_are() {
        for login in ["dependabot[bot]", "octo_cat", "mona-lisa"] {
            let q = search_query(
                "a/b",
                Kind::Pull,
                "open",
                &filter(Some(Scope::Created), None, &[]),
                login,
            );
            assert!(q.contains(&format!(" author:{login} ")), "{q}");
        }
    }

    #[test]
    fn every_narrowing_composes_in_one_query() {
        let f = filter(Some(Scope::ReviewRequested), Some(false), &["bug"]);
        assert_eq!(
            search_query("a/b", Kind::Pull, "closed", &f, "mona"),
            r#"repo:a/b is:pr is:closed review-requested:mona draft:false label:"bug" sort:updated-desc"#
        );
    }

    #[test]
    fn merged_and_closed_pull_requests_keep_their_own_state() {
        let state = |s: &str, draft: bool| {
            let p = pull_from_node(&json!({ "number": 1, "state": s, "isDraft": draft }));
            (p.state, p.draft)
        };
        assert_eq!(state("MERGED", false), ("merged".to_string(), false));
        assert_eq!(state("CLOSED", false), ("closed".to_string(), false));
        assert_eq!(state("OPEN", true), ("open".to_string(), true));
        // A draft closed unmerged stays a draft; a merged one was never a draft in REST.
        assert_eq!(state("CLOSED", true), ("closed".to_string(), true));
    }

    #[test]
    fn issue_close_reasons_map_to_the_rest_spelling() {
        let reason = |r: Value| {
            issue_from_node(&json!({ "number": 1, "state": "CLOSED", "stateReason": r }))
                .state_reason
        };
        assert_eq!(reason(json!("COMPLETED")).as_deref(), Some("completed"));
        assert_eq!(reason(json!("DUPLICATE")).as_deref(), Some("not_planned"));
        assert_eq!(reason(json!("REOPENED")).as_deref(), Some("reopened"));
        assert_eq!(reason(Value::Null), None);
    }

    #[test]
    fn an_issue_with_missing_pieces_still_maps() {
        let i = issue_from_node(&json!({
            "number": 3, "author": null,
            "labels": { "nodes": [{ "name": "x", "color": "fff", "description": null }] },
            "assignees": { "nodes": [null, { "login": "mona" }] }
        }));
        assert_eq!(i.author, "ghost");
        assert_eq!(i.labels[0].description, "");
        assert_eq!(i.comments, 0);
        // A null assignee node reads as a deleted account, not as a panic.
        assert_eq!(i.assignees, vec!["ghost".to_string(), "mona".to_string()]);
    }

    #[test]
    fn pages_stop_at_githubs_thousand_results() {
        assert_eq!(MAX_SEARCH_PAGES * PER_PAGE, 1000);
        let mut calls = 0;
        let out = read_pages(MAX_SEARCH_PAGES, |_| {
            calls += 1;
            Ok(Page {
                nodes: (0..PER_PAGE).map(|n| json!({ "number": n })).collect(),
                next: Some("more".into()),
            })
        })
        .unwrap();
        assert_eq!((calls, out.len()), (10, 1000));
    }

    #[test]
    fn an_empty_search_is_an_empty_list() {
        let mut calls = 0;
        let out = read_pages(10, |_| {
            calls += 1;
            page(&[], None)
        })
        .unwrap();
        assert_eq!((out.len(), calls), (0, 1));
    }

    #[test]
    fn a_page_of_exactly_one_hundred_ends_when_the_next_is_empty() {
        let full: Vec<u64> = (0..PER_PAGE as u64).collect();
        let mut asked = 0;
        let out = read_pages(10, |after| {
            asked += 1;
            match after {
                None => page(&full, Some("c1")),
                Some(_) => page(&[], None),
            }
        })
        .unwrap();
        assert_eq!((out.len(), asked), (100, 2));
    }
}
