//! GitHub pull requests and issues over the REST API. The token comes from the GitHub CLI if it is
//! installed and logged in, otherwise from git's own credential store; it lives only in
//! memory and is never written anywhere.

use crate::git;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const API: &str = "https://api.github.com";

#[derive(Clone)]
pub struct Token {
    value: String,
    /// "gh" or "git"
    source: &'static str,
}

#[derive(Default)]
pub struct Session {
    token: Mutex<Option<Token>>,
    etags: Mutex<Etags>,
}

fn gh_token() -> Option<String> {
    let mut cmd = Command::new("gh");
    cmd.args(["auth", "token", "--hostname", "github.com"])
        .env("PATH", git::search_path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let out = git::exec(
        cmd,
        "gh auth token",
        &[],
        None,
        Some(Duration::from_secs(10)),
    )
    .ok()?;
    let token = String::from_utf8_lossy(&out).trim().to_string();
    (!token.is_empty()).then_some(token)
}

impl Session {
    fn token(&self, repo: &Path) -> Result<Token, String> {
        if let Some(t) = self.token.lock().unwrap().clone() {
            return Ok(t);
        }
        let token = gh_token()
            .map(|value| Token {
                value,
                source: "gh",
            })
            .or_else(|| {
                git::credential_token(repo).map(|value| Token {
                    value,
                    source: "git",
                })
            })
            .ok_or_else(|| NOT_CONNECTED.to_string())?;
        *self.token.lock().unwrap() = Some(token.clone());
        Ok(token)
    }

    fn forget(&self) {
        *self.token.lock().unwrap() = None;
        // Responses seen with the old token are not the next token's to reuse.
        *self.etags.lock().unwrap() = Etags::default();
    }
}

/// Last good GET responses by path, revalidated with `If-None-Match`. GitHub answers 304
/// when nothing changed, and a 304 doesn't count against the rate limit.
#[derive(Default)]
struct Etags {
    entries: HashMap<String, Cached>,
    /// Bumped on every store; the entry stored longest ago is evicted first.
    clock: u64,
}

struct Cached {
    tag: String,
    body: Value,
    stored: u64,
}

/// A few dozen PR views' worth of requests.
const ETAG_CAP: usize = 256;
/// A body this large is rare and costly to keep around; it's simply fetched again.
const ETAG_MAX_BODY: usize = 1 << 20;

impl Etags {
    fn tag(&self, path: &str) -> Option<String> {
        self.entries.get(path).map(|c| c.tag.clone())
    }

    /// The body a 304 stands for, if it's still kept.
    fn cached(&self, path: &str) -> Option<Value> {
        self.entries.get(path).map(|c| c.body.clone())
    }

    /// Remembers a fresh 2xx response that has an ETag and fits; otherwise forgets the path.
    fn store(&mut self, path: &str, etag: Option<&str>, body: &Value, size: usize) {
        let Some(tag) = etag.filter(|_| size <= ETAG_MAX_BODY) else {
            self.entries.remove(path);
            return;
        };
        if self.entries.len() >= ETAG_CAP && !self.entries.contains_key(path) {
            let oldest = self
                .entries
                .iter()
                .min_by_key(|(_, c)| c.stored)
                .map(|(k, _)| k.clone());
            if let Some(k) = oldest {
                self.entries.remove(&k);
            }
        }
        self.clock += 1;
        self.entries.insert(
            path.to_string(),
            Cached {
                tag: tag.to_string(),
                body: body.clone(),
                stored: self.clock,
            },
        );
    }
}

/// Recognized by the UI to show setup steps instead of an error.
pub const NOT_CONNECTED: &str = "github:not-connected";

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(25)))
        .http_status_as_error(false)
        .build()
        .into()
}

enum Method {
    Get,
    Post(Value),
    Put(Value),
    Patch(Value),
}

const JSON: &str = "application/vnd.github+json";

fn call(session: &Session, repo: &Path, method: Method, path: &str) -> Result<Value, String> {
    request(session, repo, method, path, JSON)
}

const PER_PAGE: usize = 100;
/// Caps a read of every page, so a runaway thread can't stall.
const MAX_PAGES: usize = 30;

/// Every page of a list endpoint. GitHub lists comments and reviews oldest first, so one
/// page of a long thread would drop the newest ones.
fn all_pages(
    session: &Session,
    repo: &Path,
    path: &str,
    accept: &str,
) -> Result<Vec<Value>, String> {
    pages(session, repo, path, accept, None, MAX_PAGES)
}

/// Pages 1..=`max` of a list endpoint, up to the first short one. `field`: where an endpoint
/// that wraps its list in an object (check runs, statuses) keeps it.
fn pages(
    session: &Session,
    repo: &Path,
    path: &str,
    accept: &str,
    field: Option<&str>,
    max: usize,
) -> Result<Vec<Value>, String> {
    let sep = if path.contains('?') { '&' } else { '?' };
    let mut out = vec![];
    for page in 1..=max {
        let v = request(
            session,
            repo,
            Method::Get,
            &format!("{path}{sep}per_page={PER_PAGE}&page={page}"),
            accept,
        )?;
        let list = match field {
            Some(f) => &v[f],
            None => &v,
        };
        let items = list.as_array().cloned().unwrap_or_default();
        let n = items.len();
        out.extend(items);
        if n < PER_PAGE {
            break;
        }
    }
    Ok(out)
}

/// Only default-JSON GETs go through the ETag cache: it's keyed by path alone.
fn request(
    session: &Session,
    repo: &Path,
    method: Method,
    path: &str,
    accept: &str,
) -> Result<Value, String> {
    let token = session.token(repo)?;
    let url = format!("{API}{path}");
    let auth = format!("Bearer {}", token.value);
    let agent = agent();
    let cacheable = matches!(method, Method::Get) && accept == JSON;
    let mut etag = if cacheable {
        session.etags.lock().unwrap().tag(path)
    } else {
        None
    };
    macro_rules! headers {
        ($req:expr) => {
            $req.header("Authorization", &auth)
                .header("Accept", accept)
                .header("X-GitHub-Api-Version", "2022-11-28")
                .header("User-Agent", "GitViber")
        };
    }
    loop {
        let result = match &method {
            Method::Get => {
                let mut req = headers!(agent.get(&url));
                if let Some(tag) = &etag {
                    req = req.header("If-None-Match", tag);
                }
                req.call()
            }
            Method::Post(body) => headers!(agent.post(&url)).send_json(body),
            Method::Put(body) => headers!(agent.put(&url)).send_json(body),
            Method::Patch(body) => headers!(agent.patch(&url)).send_json(body),
        };
        let mut resp = result.map_err(|e| format!("GitHub request failed: {e}"))?;
        let status = resp.status().as_u16();
        if status == 304 {
            if let Some(body) = session.etags.lock().unwrap().cached(path) {
                return Ok(body);
            }
            // Evicted while the request was in flight: ask again, unconditionally.
            if etag.take().is_some() {
                continue;
            }
            return Err("GitHub sent no data (304).".into());
        }
        let header = |name: &str| {
            resp.headers()
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string)
        };
        let new_etag = header("etag");
        let limited = rate_limit_error(
            status,
            header("x-ratelimit-remaining").as_deref(),
            header("x-ratelimit-reset").as_deref(),
            header("retry-after").as_deref(),
            now(),
        );
        let text = resp.body_mut().read_to_string().unwrap_or_default();
        let body: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        if status == 401 {
            // Expired or revoked; look for a fresh one next time.
            session.forget();
            return Err(NOT_CONNECTED.to_string());
        }
        if let Some(msg) = limited {
            return Err(msg);
        }
        if status >= 400 {
            let msg = body["message"].as_str().unwrap_or("request failed");
            let detail = body["errors"][0]["message"]
                .as_str()
                .map(|d| format!(": {d}"))
                .unwrap_or_default();
            return Err(format!("GitHub {status}: {msg}{detail}"));
        }
        if cacheable {
            session
                .etags
                .lock()
                .unwrap()
                .store(path, new_etag.as_deref(), &body, text.len());
        }
        return Ok(body);
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

/// GitHub signals the primary limit with `x-ratelimit-remaining: 0` (reset time in
/// `x-ratelimit-reset`) and secondary limits with `retry-after`, on a 403 or 429.
fn rate_limit_error(
    status: u16,
    remaining: Option<&str>,
    reset: Option<&str>,
    retry_after: Option<&str>,
    now: u64,
) -> Option<String> {
    if status != 403 && status != 429 {
        return None;
    }
    let wait = |secs: u64| {
        if secs < 60 {
            format!("{secs}s")
        } else {
            format!("{} min", secs.div_ceil(60))
        }
    };
    if let Some(secs) = retry_after.and_then(|r| r.trim().parse::<u64>().ok()) {
        return Some(format!(
            "GitHub is throttling requests (secondary rate limit). Try again in {}.",
            wait(secs)
        ));
    }
    if remaining.map(str::trim) == Some("0") {
        let when = reset
            .and_then(|r| r.trim().parse::<u64>().ok())
            .map(|r| format!(" It resets in {}.", wait(r.saturating_sub(now))))
            .unwrap_or_default();
        return Some(format!("GitHub API rate limit reached.{when}"));
    }
    (status == 429).then(|| "GitHub rate limit reached. Try again in a minute.".to_string())
}

// ---------------------------------------------------------------- repo identity

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
    fn full(&self) -> String {
        format!("{}/{}", self.owner, self.name)
    }

    /// GitHub names are case-insensitive; a remote URL may spell them differently.
    fn is(&self, full: &str) -> bool {
        self.full().eq_ignore_ascii_case(full)
    }
}

fn repo_ref(repo: &Path) -> Result<RepoRef, String> {
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
    call(
        session,
        repo,
        Method::Get,
        &format!("/repos/{}/{}", r.owner, r.name),
    )
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
fn target(session: &Session, repo: &Path, to: Option<&str>) -> Result<RepoRef, String> {
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

/// Origin's branches under GitHub branch protection, which the picker won't offer to delete.
pub fn protected_branches(session: &Session, repo: &Path) -> Result<Vec<String>, String> {
    let r = repo_ref(repo)?;
    let v = call(
        session,
        repo,
        Method::Get,
        &format!(
            "/repos/{}/{}/branches?protected=true&per_page=100",
            r.owner, r.name
        ),
    )?;
    Ok(v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|b| b["name"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default())
}

// ---------------------------------------------------------------- pull requests

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
    let s = |x: &Value| x.as_str().unwrap_or_default().to_string();
    let merged = !v["merged_at"].is_null();
    Pull {
        number: v["number"].as_u64().unwrap_or_default(),
        title: s(&v["title"]),
        state: if merged {
            "merged".into()
        } else {
            s(&v["state"])
        },
        draft: v["draft"].as_bool().unwrap_or(false),
        author: s(&v["user"]["login"]),
        head_ref: s(&v["head"]["ref"]),
        head_sha: s(&v["head"]["sha"]),
        head_repo: v["head"]["repo"]["full_name"].as_str().map(str::to_string),
        base_ref: s(&v["base"]["ref"]),
        base_sha: s(&v["base"]["sha"]),
        created_at: s(&v["created_at"]),
        updated_at: s(&v["updated_at"]),
        url: s(&v["html_url"]),
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
    let state = if matches!(state, "open" | "closed" | "all") {
        state
    } else {
        "open"
    };
    let list = self::pages(
        session,
        repo,
        &format!(
            "/repos/{}/{}/pulls?state={state}&sort=updated&direction=desc",
            r.owner, r.name
        ),
        JSON,
        None,
        pages.clamp(1, MAX_PAGES),
    )?;
    Ok(list.iter().map(pull_from).collect())
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
}

pub fn detail(
    session: &Session,
    repo: &Path,
    to: Option<&str>,
    number: u64,
) -> Result<PullDetail, String> {
    let r = target(session, repo, to)?;
    let base = format!("/repos/{}/{}", r.owner, r.name);
    let v = call(
        session,
        repo,
        Method::Get,
        &format!("{base}/pulls/{number}"),
    )?;
    let pull = pull_from(&v);
    let s = |x: &Value| x.as_str().unwrap_or_default().to_string();
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
                    s(&c["conclusion"])
                };
                checks.push(Check {
                    name: s(&c["name"]),
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
                    name: s(&c["context"]),
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
            author: s(&c["user"]["login"]),
            body: s(&c["body"]),
            created_at: s(&c["created_at"]),
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
        if c["state"] == "COMMENTED" && s(&c["body"]).is_empty() {
            continue;
        }
        comments.push(Comment {
            author: s(&c["user"]["login"]),
            body: s(&c["body"]),
            created_at: s(&c["submitted_at"]),
            review: c["state"].as_str().map(str::to_string),
        });
    }
    comments.sort_by(|a, b| a.created_at.cmp(&b.created_at));

    Ok(PullDetail {
        closed_by,
        body: s(&v["body"]),
        additions: v["additions"].as_u64().unwrap_or_default(),
        deletions: v["deletions"].as_u64().unwrap_or_default(),
        changed_files: v["changed_files"].as_u64().unwrap_or_default(),
        commits: v["commits"].as_u64().unwrap_or_default(),
        mergeable: v["mergeable"].as_bool(),
        mergeable_state: s(&v["mergeable_state"]),
        checks,
        checks_error,
        comments,
        pull,
    })
}

/// Signed image links for a PR's or issue's attachments, by attachment id. In a private
/// repo, `github.com/user-attachments/assets/<id>` needs a github.com login the webview
/// doesn't have; the API's rendered HTML carries short-lived signed links instead, so the
/// token itself never leaves api.github.com.
pub fn attachments(
    session: &Session,
    repo: &Path,
    to: Option<&str>,
    number: u64,
) -> Result<HashMap<String, String>, String> {
    const HTML: &str = "application/vnd.github.html+json";
    let r = target(session, repo, to)?;
    let base = format!("/repos/{}/{}", r.owner, r.name);
    let mut out = HashMap::new();
    // The issues endpoint serves PRs too, and says which one this is.
    let thread = request(
        session,
        repo,
        Method::Get,
        &format!("{base}/issues/{number}"),
        HTML,
    )?;
    signed_images(thread["body_html"].as_str().unwrap_or_default(), &mut out);
    let mut paths = vec![format!("{base}/issues/{number}/comments")];
    if thread.get("pull_request").is_some() {
        paths.push(format!("{base}/pulls/{number}/reviews"));
    }
    for path in paths {
        if let Ok(list) = all_pages(session, repo, &path, HTML) {
            for c in list {
                signed_images(c["body_html"].as_str().unwrap_or_default(), &mut out);
            }
        }
    }
    Ok(out)
}

/// `…githubusercontent.com/<user>/<n>-<id>.<ext>?jwt=…`: the id is the attachment's UUID.
fn signed_images(html: &str, out: &mut HashMap<String, String>) {
    // Only image sources: the same URL as link text or an href is anyone's to write.
    const SRC: &str = "src=\"https://private-user-images.githubusercontent.com/";
    for (i, _) in html.match_indices(SRC) {
        let url = html[i + 5..]
            .split(['"', '\'', ' ', '<', '>'])
            .next()
            .unwrap_or_default()
            .replace("&amp;", "&");
        let name = url.split('?').next().unwrap_or_default();
        let stem = name.rsplit('/').next().unwrap_or_default();
        let stem = stem.split('.').next().unwrap_or_default();
        let Some(id) = stem.len().checked_sub(36).and_then(|n| stem.get(n..)) else {
            continue;
        };
        let uuid = id.char_indices().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        });
        if uuid {
            // The description comes first, then comments in order: a later comment reusing
            // an id can't replace the link the description's image resolves to.
            out.entry(id.to_string()).or_insert(url);
        }
    }
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
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Remote {
    pub name: String,
    /// The GitHub repository it points at, owner/name; None off github.com
    pub repo: Option<String>,
}

/// This repo's remotes and the GitHub repositories behind them. Local config only.
pub fn remotes(repo: &Path) -> Vec<Remote> {
    let names = git::run(repo, &["remote"]).unwrap_or_default();
    String::from_utf8_lossy(&names)
        .lines()
        .map(|n| Remote {
            name: n.to_string(),
            repo: git::remote_url(repo, n)
                .and_then(|u| parse_remote(&u))
                .map(|r| r.full()),
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
pub fn original_remote(repo: &Path, original: &str, fetch: bool) -> Result<Option<String>, String> {
    let r = split_full(original).ok_or_else(|| format!("not a repository name: {original}"))?;
    let Some(name) = remote_for(repo, &r) else {
        return Ok(None);
    };
    if fetch {
        git::fetch_remote(repo, &name)?;
    }
    Ok(Some(name))
}

/// GitHub's "Sync fork": brings origin's `branch` up to date with the original's default
/// branch, on GitHub, then fetches origin. Returns how: "fast-forward", "merge" or "none".
/// A conflict comes back as GitHub's 409 message; that takes a local merge.
pub fn sync_fork(session: &Session, repo: &Path, branch: &str) -> Result<String, String> {
    git::run(repo, &["check-ref-format", "--branch", branch])
        .map_err(|_| format!("invalid branch: {branch}"))?;
    let r = repo_ref(repo)?;
    let v = call(
        session,
        repo,
        Method::Post(json!({ "branch": branch })),
        &format!("/repos/{}/{}/merge-upstream", r.owner, r.name),
    )?;
    git::fetch_remote(repo, "origin")?;
    Ok(v["merge_type"].as_str().unwrap_or("none").to_string())
}

/// Adds the fork's original as "upstream" (the usual name; "original" if that's taken),
/// over the same protocol as origin, and fetches it.
pub fn add_original_remote(session: &Session, repo: &Path) -> Result<String, String> {
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
    git::fetch_remote(repo, name)?;
    Ok(name.to_string())
}

pub fn files(
    session: &Session,
    repo: &Path,
    to: Option<&str>,
    number: u64,
    base_ref: &str,
    base_sha: &str,
    head_sha: &str,
) -> Result<PullFiles, String> {
    let have =
        |sha: &str| git::run(repo, &["cat-file", "-e", &format!("{sha}^{{commit}}")]).is_ok();
    if !have(head_sha) || !have(base_sha) {
        git::run(repo, &["check-ref-format", "--branch", base_ref])
            .map_err(|_| format!("invalid branch: {base_ref}"))?;
        git::fetch_objects(
            repo,
            &fetch_remote(session, repo, to)?,
            &[
                format!("pull/{number}/head"),
                format!("refs/heads/{base_ref}"),
            ],
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
    let v = call(
        session,
        repo,
        Method::Post(pr),
        &format!("/repos/{}/{}/pulls", r.owner, r.name),
    )?;
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
        &format!("/repos/{}/{}/pulls/{number}/merge", r.owner, r.name),
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
        &format!("/repos/{}/{}/pulls/{number}", r.owner, r.name),
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
        &format!("/repos/{}/{}/pulls/{number}/reviews", r.owner, r.name),
    )
    .map(|_| ())
}

// ---------------------------------------------------------------- issues

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Label {
    pub name: String,
    /// Hex without the '#'
    pub color: String,
    pub description: String,
}

fn label_from(v: &Value) -> Label {
    let s = |x: &Value| x.as_str().unwrap_or_default().to_string();
    Label {
        name: s(&v["name"]),
        color: s(&v["color"]),
        description: s(&v["description"]),
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
    let s = |x: &Value| x.as_str().unwrap_or_default().to_string();
    Issue {
        number: v["number"].as_u64().unwrap_or_default(),
        title: s(&v["title"]),
        state: s(&v["state"]),
        state_reason: v["state_reason"].as_str().map(str::to_string),
        author: s(&v["user"]["login"]),
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
            .map(|a| s(&a["login"]))
            .collect(),
        comments: v["comments"].as_u64().unwrap_or_default(),
        created_at: s(&v["created_at"]),
        updated_at: s(&v["updated_at"]),
        url: s(&v["html_url"]),
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
    let state = if matches!(state, "open" | "closed" | "all") {
        state
    } else {
        "open"
    };
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
            &format!(
                "/repos/{}/{}/issues?state={state}{labels}&sort=updated&direction=desc&per_page={PAGE}&page={page}",
                r.owner, r.name
            ),
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
    let path = format!("/repos/{}/{}/labels", r.owner, r.name);
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
    let path = format!("/repos/{}/{}/issues/{number}", r.owner, r.name);
    let v = call(session, repo, Method::Get, &path)?;
    let s = |x: &Value| x.as_str().unwrap_or_default().to_string();
    let thread = all_pages(session, repo, &format!("{path}/comments"), JSON)?
        .iter()
        .map(|c| Comment {
            author: s(&c["user"]["login"]),
            body: s(&c["body"]),
            created_at: s(&c["created_at"]),
            review: None,
        })
        .collect();
    Ok(IssueDetail {
        closed_by: v["closed_by"]["login"].as_str().map(str::to_string),
        body: s(&v["body"]),
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
        &format!("/repos/{}/{}/issues", r.owner, r.name),
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
        &format!("/repos/{}/{}/issues/{number}", r.owner, r.name),
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
        &format!("/repos/{}/{}/issues/{number}", r.owner, r.name),
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
        &format!("/repos/{}/{}/issues/{number}/labels", r.owner, r.name),
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
        &format!("/repos/{}/{}/issues/{number}", r.owner, r.name),
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

/// A GraphQL request; returns its `data`.
fn graphql(session: &Session, repo: &Path, query: &str, variables: Value) -> Result<Value, String> {
    let out = call(
        session,
        repo,
        Method::Post(json!({ "query": query, "variables": variables })),
        "/graphql",
    )?;
    // GraphQL reports failures (no permission, say) with a 200 and an `errors` list.
    match out["errors"][0]["message"].as_str() {
        Some(msg) => Err(format!("GitHub: {msg}")),
        None => Ok(out["data"].clone()),
    }
}

#[derive(Serialize)]
pub struct IssueCounts {
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
) -> Result<IssueCounts, String> {
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
        return Ok(IssueCounts {
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
    Ok(IssueCounts {
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
        &format!("/repos/{}/{}/issues/{number}/comments", r.owner, r.name),
    )
    .map(|_| ())
}

/// Switches to the PR's branch: the real branch when it lives on origin (`same_repo`),
/// otherwise `pr/<n>`, fetched from `remote` (the repository the PR is in). A fork's original
/// numbers its PRs separately, so its are `pr/<owner>/<n>` (`owner` set).
/// An existing local branch is only fast-forwarded, never reset: unpushed work on it is
/// kept, and a branch that diverged from the PR is reported instead of silently used.
pub fn checkout(
    repo: &Path,
    remote: &str,
    owner: Option<&str>,
    number: u64,
    head_ref: &str,
    same_repo: bool,
) -> Result<(), String> {
    let local = match (same_repo, owner) {
        (true, _) => head_ref.to_string(),
        (false, Some(o)) => format!("pr/{o}/{number}"),
        (false, None) => format!("pr/{number}"),
    };
    git::run(repo, &["check-ref-format", "--branch", &local])
        .map_err(|_| format!("invalid branch: {local}"))?;
    let source = if same_repo {
        head_ref.to_string()
    } else {
        format!("pull/{number}/head")
    };
    // FETCH_HEAD is the PR head either way; never force-update a local branch.
    let remote = if same_repo { "origin" } else { remote };
    git::run(repo, &["fetch", "--quiet", remote, &source])?;
    let exists = git::run(
        repo,
        &[
            "rev-parse",
            "--verify",
            "-q",
            &format!("refs/heads/{local}"),
        ],
    )
    .is_ok();
    if exists {
        git::switch_branch(repo, &local, false)?;
        git::run(repo, &["merge", "--ff-only", "--quiet", "FETCH_HEAD"])
            .map(|_| ())
            .map_err(|_| {
                format!(
                    "Local branch {local} has diverged from the pull request; reconcile it first."
                )
            })
    } else if same_repo {
        // Not `git switch <head_ref>`: in a fork, upstream often has a same-named branch and
        // git's guess then refuses ("matched multiple remote tracking branches").
        let tracked = format!("origin/{local}");
        git::run(repo, &["switch", "-c", &local, "--track", &tracked]).map(|_| ())
    } else {
        git::run(repo, &["switch", "-c", &local, "FETCH_HEAD"])?;
        // Like `gh pr checkout`: the branch follows the PR, so Pull brings its new commits,
        // and it never looks unpublished (a Publish would copy it into origin).
        let key = |k: &str| format!("branch.{local}.{k}");
        git::run(repo, &["config", &key("remote"), remote])?;
        git::run(
            repo,
            &["config", &key("merge"), &format!("refs/pull/{number}/head")],
        )
        .map(|_| ())
    }
}

/// Links in GitHub text are written by anyone, so only http(s) passes, in the canonical
/// form a browser would use: scheme and host lowercased, spaces, quotes and non-ASCII
/// percent-encoded. The launchers get it as one argument, never through a shell.
fn openable(url: &str) -> Option<String> {
    let parsed = tauri::Url::parse(url).ok()?;
    let url = String::from(parsed);
    let plain = url
        .bytes()
        .all(|b| b.is_ascii_graphic() && !b"\"<>\\`".contains(&b));
    (url.starts_with("https://") || url.starts_with("http://"))
        .then_some(url)
        .filter(|_| plain)
}

/// Opens a web page in the default browser.
pub fn open_url(url: &str) -> Result<(), String> {
    let url = openable(url).ok_or("refusing to open this URL")?;
    #[cfg(target_os = "macos")]
    let mut cmd = Command::new("open");
    // Not `cmd /C start`: cmd.exe re-parses the argument.
    #[cfg(target_os = "windows")]
    let mut cmd = {
        let mut c = Command::new("rundll32");
        c.arg("url.dll,FileProtocolHandler");
        c
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut cmd = Command::new("xdg-open");
    cmd.arg(&url).spawn().map(|_| ()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    #[test]
    fn query_values_are_percent_encoded() {
        assert_eq!(
            super::query_value("difficulty: easy"),
            "difficulty%3A%20easy"
        );
        assert_eq!(super::query_value("a,b&c=d"), "a%2Cb%26c%3Dd");
        assert_eq!(
            super::query_value("good-first_issue.~"),
            "good-first_issue.~"
        );
        assert_eq!(super::query_value("ü"), "%C3%BC");
    }

    use super::*;

    /// Read-only, against this checkout's origin: `cargo test -- --ignored live_label_filter`.
    #[test]
    #[ignore = "talks to GitHub"]
    fn live_label_filter() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let session = Session::default();
        let labels = issue_labels(&session, repo, None).unwrap();
        println!("{} labels", labels.len());
        // Names with spaces and colons are the ones that need encoding.
        let odd = labels
            .iter()
            .find(|l| l.name.contains(' '))
            .expect("a label with a space");
        let with = issues(&session, repo, None, "all", std::slice::from_ref(&odd.name)).unwrap();
        println!("{:?}: {} issues", odd.name, with.len());
        assert!(!with.is_empty());
        assert!(with
            .iter()
            .all(|i| i.labels.iter().any(|l| l.name == odd.name)));
        // The list stops at 50; the counts don't.
        let c = issue_counts(&session, repo, None, std::slice::from_ref(&odd.name)).unwrap();
        println!("counted {} open, {} closed", c.open, c.closed);
        let counted = (c.open + c.closed) as usize;
        assert!(counted >= with.len() && (with.len() == 50 || counted == with.len()));
        let all = issue_counts(&session, repo, None, &[]).unwrap();
        assert!(all.open + all.closed >= c.open + c.closed);
        // Two labels mean both: never more than either alone.
        let other = &with[0].labels.iter().find(|l| l.name != odd.name);
        if let Some(other) = other {
            let both = issues(
                &session,
                repo,
                None,
                "all",
                &[odd.name.clone(), other.name.clone()],
            )
            .unwrap();
            println!("+ {:?}: {} issues", other.name, both.len());
            assert!(!both.is_empty() && both.len() <= with.len());
            assert!(both.iter().all(|i| i.labels.len() >= 2));
        }
    }

    /// Read-only, against a clone of a fork: `GITVIBER_GH_FORK=/path/to/clone cargo test -- --ignored`.
    #[test]
    #[ignore = "talks to GitHub"]
    fn live_fork_reads_both_repositories() {
        let path = std::env::var("GITVIBER_GH_FORK").expect("GITVIBER_GH_FORK");
        let repo = Path::new(&path);
        let session = Session::default();
        let acct = account(&session, repo).unwrap();
        let origin = acct.origin.expect("origin");
        let parent = acct.parent.expect("origin should be a fork");
        println!(
            "origin {} push={} issues={} · parent {} push={} admin={} issues={} default={:?}",
            origin.repo.full(),
            origin.push,
            origin.issues,
            parent.repo.full(),
            parent.push,
            parent.admin,
            parent.issues,
            parent.default_branch
        );
        let up = parent.repo.full();
        for (to, name) in [(None, "origin"), (Some(up.as_str()), "parent")] {
            let pulls = list(&session, repo, to, "all", 1).unwrap();
            let open = issues(&session, repo, to, "open", &[]).unwrap_or_default();
            let all = issues(&session, repo, to, "all", &[]).unwrap_or_default();
            println!(
                "{name}: {} PRs, {} open / {} total issues",
                pulls.len(),
                open.len(),
                all.len()
            );
            // "all" must never list fewer than "open": PRs used to crowd issues out of the page.
            assert!(all.len() >= open.len().min(50));
            if let Some(p) = pulls.first() {
                assert!(p.url.to_lowercase().contains(&format!(
                    "/{}/pull/",
                    if to.is_some() {
                        up.to_lowercase()
                    } else {
                        origin.repo.full().to_lowercase()
                    }
                )));
                detail(&session, repo, to, p.number).unwrap();
            }
        }
        let closed = list(&session, repo, Some(&up), "closed", 1).unwrap();
        if let Some(p) = closed.iter().find(|p| p.state == "closed") {
            let d = detail(&session, repo, Some(&up), p.number).unwrap();
            println!("#{} closed by {:?}", p.number, d.closed_by);
            assert!(d.closed_by.is_some());
        }
        // Anything but origin and its parent is refused, whatever the token could reach.
        assert!(list(&session, repo, Some("torvalds/linux"), "open", 1).is_err());
        let remote = original_remote(repo, &up, false).unwrap();
        println!("original remote: {remote:?}");
        if let Some(r) = remote {
            let branch = parent.default_branch.unwrap_or_else(|| "main".into());
            let log = git::log(repo, Some(&format!("refs/remotes/{r}/{branch}")), 0, 20).unwrap();
            println!(
                "{} commits on {r}/{branch}, {} not in HEAD",
                log.len(),
                log.iter().filter(|c| c.not_in_head).count()
            );
        }
    }

    /// End to end against a real repo: `GITVIBER_GH_REPO=/path/to/clone cargo test -- --ignored`.
    /// The clone's origin needs a `feature/review` branch that differs from `main`.
    #[test]
    #[ignore = "talks to GitHub"]
    fn live_pull_request_flow() {
        let path = std::env::var("GITVIBER_GH_REPO").expect("GITVIBER_GH_REPO");
        let repo = Path::new(&path);
        let session = Session::default();
        let acct = account(&session, repo).unwrap();
        println!(
            "account: {} via {}, default branch {:?}",
            acct.login,
            acct.source,
            acct.origin.and_then(|o| o.default_branch)
        );

        let open = list(&session, repo, None, "open", 1).unwrap();
        let number = match open.iter().find(|p| p.head_ref == "feature/review") {
            Some(p) => p.number,
            None => {
                create(
                    &session,
                    repo,
                    None,
                    "Review: newest commit opens by default",
                    "Opened by GitViber's live test.",
                    "feature/review",
                    "main",
                    false,
                    false,
                )
                .unwrap()
                .number
            }
        };
        let d = detail(&session, repo, None, number).unwrap();
        println!(
            "PR #{number}: {} ({}), mergeable {:?}, {} checks, {} comments",
            d.pull.title,
            d.pull.state,
            d.mergeable,
            d.checks.len(),
            d.comments.len()
        );
        assert_eq!(d.pull.head_ref, "feature/review");

        let f = files(
            &session,
            repo,
            None,
            number,
            &d.pull.base_ref,
            &d.pull.base_sha,
            &d.pull.head_sha,
        )
        .unwrap();
        println!(
            "files: {:?}",
            f.files
                .iter()
                .map(|x| (&x.path, x.additions, x.deletions))
                .collect::<Vec<_>>()
        );
        assert_eq!(f.files.len() as u64, d.changed_files);
        let pair = git::diff_pair(
            repo,
            "range",
            &f.files[0].path,
            None,
            Some(&f.head),
            Some(&f.base),
            |_| git::FileText::default(),
        )
        .unwrap();
        assert!(pair.rows.iter().any(|r| r.k != 0));
    }

    /// The "Resolve locally" flow on a PR that conflicts with its base (diff-demo PR #2).
    /// Local only: aborts at the end and never pushes.
    #[test]
    #[ignore = "talks to GitHub"]
    fn live_resolve_locally() {
        let path = std::env::var("GITVIBER_GH_REPO").expect("GITVIBER_GH_REPO");
        let repo = Path::new(&path);
        let session = Session::default();
        let d = detail(&session, repo, None, 2).unwrap();
        println!(
            "PR #2 mergeable={:?} state={}",
            d.mergeable, d.mergeable_state
        );
        let same_repo = d.pull.head_repo.as_deref()
            == Some(
                &*d.pull
                    .url
                    .replace("https://github.com/", "")
                    .split("/pull/")
                    .next()
                    .unwrap()
                    .to_string(),
            );
        checkout(repo, "origin", None, 2, &d.pull.head_ref, same_repo).unwrap();
        assert_eq!(
            git::status(repo).unwrap().branch.as_deref(),
            Some(d.pull.head_ref.as_str())
        );
        git::fetch(repo).unwrap();
        let stopped = git::merge(repo, &format!("origin/{}", d.pull.base_ref)).unwrap();
        let st = git::status(repo).unwrap();
        println!(
            "stopped={stopped} conflicts={:?}",
            st.conflicted
                .iter()
                .map(|f| (&f.path, &f.conflict))
                .collect::<Vec<_>>()
        );
        assert!(stopped && st.conflicted.len() == 1);
        git::op_abort(repo).unwrap();
        git::switch_branch(repo, "main", false).unwrap();
    }

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

    #[test]
    fn etags_revalidate() {
        let mut c = Etags::default();
        let v = json!({ "n": 1 });
        c.store("/a", Some("W/\"1\""), &v, 10);
        assert_eq!(c.tag("/a").as_deref(), Some("W/\"1\""));
        // A 304 has no body: the remembered one stands in; none kept means ask again.
        assert_eq!(c.cached("/a"), Some(v));
        assert_eq!(c.cached("/b"), None);
    }

    #[test]
    fn rate_limits() {
        let now = 1_000;
        assert_eq!(
            rate_limit_error(403, Some("0"), Some("1600"), None, now).as_deref(),
            Some("GitHub API rate limit reached. It resets in 10 min.")
        );
        assert_eq!(
            rate_limit_error(429, Some("12"), None, Some("30"), now).as_deref(),
            Some("GitHub is throttling requests (secondary rate limit). Try again in 30s.")
        );
        // A plain permission error is GitHub's own message, not a rate limit.
        assert_eq!(
            rate_limit_error(403, Some("4999"), Some("1600"), None, now),
            None
        );
        assert_eq!(rate_limit_error(200, Some("0"), None, None, now), None);
    }

    #[test]
    fn signed_image_links() {
        let id = "012a2451-fa01-4fe5-8736-33f4a4d179f1";
        let url = format!("https://private-user-images.githubusercontent.com/23744935/650874155-{id}.png?jwt=eyJ.x&amp;y=1");
        let html = format!(
            r#"<a href="{url}"><img src="{url}" alt="x"></a> <img src="https://private-user-images.githubusercontent.com/1/2-nope.png?jwt=z">"#
        );
        let mut out = HashMap::new();
        signed_images(&html, &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[id], url.replace("&amp;", "&"));
    }

    #[test]
    fn openable_links() {
        let ok = |u: &str| openable(u).unwrap_or_else(|| panic!("{u}"));
        assert_eq!(
            ok("https://github.com/a/b/pull/1#issuecomment-2"),
            "https://github.com/a/b/pull/1#issuecomment-2"
        );
        assert_eq!(ok("HTTPS://Docs.RS/a?b=c&d"), "https://docs.rs/a?b=c&d");
        // Characters raw-HTML hrefs carry that a URL keeps as they are.
        assert_eq!(ok("https://x.com/it's/[1]|a"), "https://x.com/it's/[1]|a");
        assert_eq!(ok("https://x.com/ça va"), "https://x.com/%C3%A7a%20va");
        // Quotes and spaces can't survive into the argument.
        assert_eq!(
            ok("https://x.com/\"; rm -rf ~"),
            "https://x.com/%22;%20rm%20-rf%20~"
        );
        assert_eq!(openable("javascript:alert(1)"), None);
        assert_eq!(openable("file:///etc/passwd"), None);
        assert_eq!(openable("mailto:a@b.c"), None);
        assert_eq!(openable("not a url"), None);
    }

    #[test]
    fn etag_cache_evicts_oldest_and_skips_big_bodies() {
        let mut c = Etags::default();
        for i in 0..ETAG_CAP {
            c.store(&format!("/{i}"), Some("t"), &json!(i), 10);
        }
        // Touch /0 so /1 is now the oldest.
        c.store("/0", Some("t2"), &json!(0), 10);
        c.store("/new", Some("t"), &Value::Null, 10);
        assert_eq!(c.entries.len(), ETAG_CAP);
        assert!(c.cached("/1").is_none());
        assert_eq!(c.tag("/0").as_deref(), Some("t2"));
        assert!(c.cached("/new").is_some());
        // Too big to keep, and a stale copy must not outlive it.
        c.store("/0", Some("t3"), &json!(0), ETAG_MAX_BODY + 1);
        assert!(c.cached("/0").is_none());
        // No ETag: nothing to revalidate against.
        c.store("/2", None, &json!(2), 10);
        assert!(c.tag("/2").is_none());
    }

    #[test]
    fn signed_links_first_wins_and_only_from_images() {
        let id = "012a2451-fa01-4fe5-8736-33f4a4d179f1";
        let url = |n: u32| {
            format!("https://private-user-images.githubusercontent.com/1/{n}-{id}.png?jwt=x{n}")
        };
        let mut out = HashMap::new();
        signed_images(&format!(r#"<img src="{}">"#, url(1)), &mut out);
        signed_images(
            &format!(
                r#"<img src="{}"> <a href="{}">{}</a>"#,
                url(2),
                url(3),
                url(4)
            ),
            &mut out,
        );
        assert_eq!(out[id], url(1));
        let mut out = HashMap::new();
        signed_images(&format!(r#"<a href="{}">{}</a>"#, url(3), url(4)), &mut out);
        assert!(out.is_empty());
    }
}
