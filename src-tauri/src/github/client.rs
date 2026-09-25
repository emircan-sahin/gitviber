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
}

#[derive(Default)]
pub struct Session {
    token: Mutex<Option<Token>>,
    etags: Mutex<Etags>,
    /// One for every request, so they share its connection pool.
    agent: OnceLock<ureq::Agent>,
}

fn gh_token() -> Option<String> {
    let mut cmd = Command::new("gh");
    cmd.args(["auth", "token", "--hostname", "github.com"])
        .env("PATH", process::search_path())
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
        if let Some(t) = self.token.lock().unwrap_or_else(|e| e.into_inner()).clone() {
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
        *self.token.lock().unwrap_or_else(|e| e.into_inner()) = Some(token.clone());
        Ok(token)
    }

    /// Whether a token is already in hand, so a call needn't ask gh or the keychain first.
    pub(super) fn has_token(&self) -> bool {
        self.token
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_some()
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

const PER_PAGE: usize = 100;
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
    let token = session.token(repo)?;
    let url = format!("{API}{path}");
    let auth = format!("Bearer {}", token.value);
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
                .unwrap_or_else(|e| e.into_inner())
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
