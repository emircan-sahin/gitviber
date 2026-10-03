//! The REST client: the borrowed token, the ETag cache, paging and rate limits.

use crate::git;
use crate::process;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const API: &str = "https://api.github.com";

#[derive(Clone)]
pub struct Token {
    value: String,
    /// "gh" or "git"
    pub(super) source: &'static str,
    /// The account it was picked as; None: gh's active one.
    account: Option<String>,
}

#[derive(Default)]
pub struct Session {
    token: Mutex<Option<Token>>,
    /// The account picked for the open repository (accounts.rs); None: gh's active one.
    picked: Mutex<Option<String>>,
    etags: Mutex<Etags>,
    /// One for every request, so they share its connection pool.
    agent: OnceLock<ureq::Agent>,
}

/// `user`: that gh account's token (gh 2.40+), else the active account's.
fn gh_token(user: Option<&str>) -> Option<String> {
    let mut cmd = Command::new("gh");
    cmd.args(["auth", "token", "--hostname", "github.com"]);
    if let Some(user) = user {
        cmd.args(["--user", user]);
    }
    cmd.env("PATH", process::search_path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let out = process::exec(
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
    pub(super) fn token(&self, repo: &Path) -> Result<Token, String> {
        let account = self.picked();
        let held = self.token.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if let Some(t) = held.filter(|t| t.account == account) {
            return Ok(t);
        }
        let (value, source) = match &account {
            // Never another account's token in its place: that would act as someone else.
            Some(user) => gh_token(Some(user)).map(|v| (v, "gh")).ok_or_else(|| {
                format!("gh has no token for {user}, the GitHub account picked for this repository. Sign in with `gh auth login`, or pick another account in Settings > Git.")
            })?,
            None => gh_token(None)
                .map(|v| (v, "gh"))
                .or_else(|| git::credential_token(repo).map(|v| (v, "git")))
                .ok_or_else(|| NOT_CONNECTED.to_string())?,
        };
        let token = Token {
            value,
            source,
            account,
        };
        let replaced = self
            .token
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .replace(token.clone());
        if replaced.is_some_and(|old| old.account != token.account) {
            // Another account's responses are not this one's to reuse.
            *self.etags.lock().unwrap_or_else(|e| e.into_inner()) = Etags::default();
        }
        Ok(token)
    }

    fn picked(&self) -> Option<String> {
        self.picked
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub(super) fn pick(&self, account: Option<String>) {
        *self.picked.lock().unwrap_or_else(|e| e.into_inner()) = account;
    }

    /// Whether the token for the picked account is already in hand, so a call needn't ask gh or
    /// the keychain first.
    pub(super) fn has_token(&self) -> bool {
        let account = self.picked();
        self.token
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .is_some_and(|t| t.account == account)
    }

    fn forget(&self) {
        *self.token.lock().unwrap_or_else(|e| e.into_inner()) = None;
        // Responses seen with the old token are not the next token's to reuse.
        *self.etags.lock().unwrap_or_else(|e| e.into_inner()) = Etags::default();
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

fn agent(session: &Session) -> &ureq::Agent {
    session.agent.get_or_init(|| {
        ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(25)))
            .http_status_as_error(false)
            .build()
            .into()
    })
}

pub(super) enum Method {
    Get,
    Post(Value),
    Put(Value),
    Patch(Value),
}

pub(super) const JSON: &str = "application/vnd.github+json";

pub(super) fn call(
    session: &Session,
    repo: &Path,
    method: Method,
    path: &str,
) -> Result<Value, String> {
    request(session, repo, method, path, JSON)
}

pub(super) const PER_PAGE: usize = 100;
/// Caps a read of every page, so a runaway thread can't stall.
pub(super) const MAX_PAGES: usize = 30;

/// Every page of a list endpoint. GitHub lists comments and reviews oldest first, so one
/// page of a long thread would drop the newest ones.
pub(super) fn all_pages(
    session: &Session,
    repo: &Path,
    path: &str,
    accept: &str,
) -> Result<Vec<Value>, String> {
    pages(session, repo, path, accept, None, MAX_PAGES)
}

/// Pages 1..=`max` of a list endpoint, up to the first short one. `field`: where an endpoint
/// that wraps its list in an object (check runs, statuses) keeps it.
pub(super) fn pages(
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
pub(super) fn request(
    session: &Session,
    repo: &Path,
    method: Method,
    path: &str,
    accept: &str,
) -> Result<Value, String> {
    let mut token = session.token(repo)?;
    let url = format!("{API}{path}");
    let mut auth = format!("Bearer {}", token.value);
    let mut renewed = false;
    let agent = agent(session);
    let cacheable = matches!(method, Method::Get) && accept == JSON;
    let mut etag = if cacheable {
        session
            .etags
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .tag(path)
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
            if let Some(body) = session
                .etags
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .cached(path)
            {
                return Ok(body);
            }
            // Evicted while the request was in flight: ask again, unconditionally.
            if etag.take().is_some() {
                continue;
            }
            return Err("GitHub sent no data (304).".into());
        }
        if let Some(e) = response_error(session, &mut resp) {
            // A token gh replaced since it was read (a new login, the account signed out and in
            // again): asked for afresh once, so the call doesn't fail on the stale one.
            if e == NOT_CONNECTED && !renewed {
                renewed = true;
                if let Some(fresh) = session.token(repo).ok().filter(|t| t.value != token.value) {
                    token = fresh;
                    auth = format!("Bearer {}", token.value);
                    continue;
                }
            }
            return Err(e);
        }
        let new_etag = resp
            .headers()
            .get("etag")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let text = resp.body_mut().read_to_string().unwrap_or_default();
        let body: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        if cacheable {
            session
                .etags
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .store(path, new_etag.as_deref(), &body, text.len());
        }
        return Ok(body);
    }
}

/// A text response's last `keep` bytes, and whether its start was cut: a job's log, which GitHub
/// redirects to its storage (ureq follows, without the token). Read through rather than held
/// whole, as a long job's log runs to megabytes.
pub(super) fn text_tail(
    session: &Session,
    repo: &Path,
    path: &str,
    keep: usize,
) -> Result<(String, bool), String> {
    let token = session.token(repo)?;
    let mut resp = agent(session)
        .get(&format!("{API}{path}"))
        .header("Authorization", &format!("Bearer {}", token.value))
        .header("X-GitHub-Api-Version", "2022-11-28")
        .header("User-Agent", "GitViber")
        .call()
        .map_err(|e| format!("GitHub request failed: {e}"))?;
    if let Some(e) = response_error(session, &mut resp) {
        return Err(e);
    }
    let mut reader = resp.body_mut().as_reader();
    let mut chunk = vec![0; 64 * 1024];
    let mut tail = Vec::new();
    let mut cut = false;
    loop {
        let n = std::io::Read::read(&mut reader, &mut chunk)
            .map_err(|e| format!("Reading from GitHub failed: {e}"))?;
        if n == 0 {
            break;
        }
        tail.extend_from_slice(&chunk[..n]);
        if tail.len() > 2 * keep {
            tail.drain(..tail.len() - keep);
            cut = true;
        }
    }
    if tail.len() > keep {
        tail.drain(..tail.len() - keep);
        cut = true;
    }
    Ok((String::from_utf8_lossy(&tail).into_owned(), cut))
}

/// Why a response is an error, if it is: an expired or revoked token (forgotten, so the next call
/// looks for a fresh one), a rate limit, single sign-on to authorize, or GitHub's own message. A
/// 401 from where GitHub redirected (a log's storage) says nothing about the token.
fn response_error(
    session: &Session,
    resp: &mut ureq::http::Response<ureq::Body>,
) -> Option<String> {
    use ureq::ResponseExt;
    let status = resp.status().as_u16();
    if status < 400 {
        return None;
    }
    if status == 401 && resp.get_uri().host() == Some("api.github.com") {
        session.forget();
        return Some(NOT_CONNECTED.to_string());
    }
    let header = |name: &str| {
        resp.headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
    };
    let sso = header("x-github-sso");
    if let Some(msg) = rate_limit_error(
        status,
        header("x-ratelimit-remaining").as_deref(),
        header("x-ratelimit-reset").as_deref(),
        header("retry-after").as_deref(),
        now(),
    ) {
        return Some(msg);
    }
    let text = resp.body_mut().read_to_string().unwrap_or_default();
    let body: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    let msg = body["message"].as_str().unwrap_or("request failed");
    let detail = body["errors"][0]["message"]
        .as_str()
        .map(|d| format!(": {d}"))
        .unwrap_or_default();
    Some(match sso_url(status, sso.as_deref()) {
        Some(url) => format!(
            "GitHub {status}: {msg} Authorize this token for the organization's single sign-on at {url}"
        ),
        None => format!("GitHub {status}: {msg}{detail}"),
    })
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

/// A list endpoint's `state` filter: "open", "closed" or "all", else "open".
pub(super) fn list_state(state: &str) -> &str {
    if matches!(state, "open" | "closed" | "all") {
        state
    } else {
        "open"
    }
}

/// A string field of a response, "" when it's missing or null.
pub(super) fn string(v: &Value) -> String {
    v.as_str().unwrap_or_default().to_string()
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

/// Where to authorize the token for an organization that enforces SAML single sign-on, from a
/// 403's `X-GitHub-SSO: required; url=…` (as gh reads it).
fn sso_url(status: u16, header: Option<&str>) -> Option<&str> {
    if status != 403 {
        return None;
    }
    header?
        .split(';')
        .find_map(|p| p.trim().strip_prefix("url="))
        .filter(|u| u.starts_with("https://"))
}

/// A GraphQL request; returns its `data`.
pub(super) fn graphql(
    session: &Session,
    repo: &Path,
    query: &str,
    variables: Value,
) -> Result<Value, String> {
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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn sso_authorize_link() {
        let h = "required; url=https://github.com/orgs/acme/sso?authorization_request=abc";
        assert_eq!(
            sso_url(403, Some(h)),
            Some("https://github.com/orgs/acme/sso?authorization_request=abc")
        );
        // Partial results name organizations, with nowhere to go.
        assert_eq!(
            sso_url(403, Some("partial-results; organizations=1,2")),
            None
        );
        assert_eq!(sso_url(404, Some(h)), None);
        assert_eq!(sso_url(403, None), None);
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
}
