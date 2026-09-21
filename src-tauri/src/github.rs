//! GitHub pull requests over the REST API. The token comes from the GitHub CLI if it is
//! installed and logged in, otherwise from git's own credential store; it lives only in
//! memory and is never written anywhere.

use crate::git;
use serde::Serialize;
use serde_json::{json, Value};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::Duration;

const API: &str = "https://api.github.com";

#[derive(Clone)]
pub struct Token {
    value: String,
    /// "gh" or "git"
    source: &'static str,
}

#[derive(Default)]
pub struct Session(Mutex<Option<Token>>);

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
        if let Some(t) = self.0.lock().unwrap().clone() {
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
        *self.0.lock().unwrap() = Some(token.clone());
        Ok(token)
    }

    fn forget(&self) {
        *self.0.lock().unwrap() = None;
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
}

fn call(session: &Session, repo: &Path, method: Method, path: &str) -> Result<Value, String> {
    let token = session.token(repo)?;
    let url = format!("{API}{path}");
    let auth = format!("Bearer {}", token.value);
    let agent = agent();
    macro_rules! headers {
        ($req:expr) => {
            $req.header("Authorization", &auth)
                .header("Accept", "application/vnd.github+json")
                .header("X-GitHub-Api-Version", "2022-11-28")
                .header("User-Agent", "GitViber")
        };
    }
    let result = match method {
        Method::Get => headers!(agent.get(&url)).call(),
        Method::Post(body) => headers!(agent.post(&url)).send_json(body),
        Method::Put(body) => headers!(agent.put(&url)).send_json(body),
    };
    let mut resp = result.map_err(|e| format!("GitHub request failed: {e}"))?;
    let status = resp.status().as_u16();
    let body: Value = resp.body_mut().read_json().unwrap_or(Value::Null);
    if status == 401 {
        // Expired or revoked; look for a fresh one next time.
        session.forget();
        return Err(NOT_CONNECTED.to_string());
    }
    if status >= 400 {
        let msg = body["message"].as_str().unwrap_or("request failed");
        let detail = body["errors"][0]["message"]
            .as_str()
            .map(|d| format!(": {d}"))
            .unwrap_or_default();
        return Err(format!("GitHub {status}: {msg}{detail}"));
    }
    Ok(body)
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

fn repo_ref(repo: &Path) -> Result<RepoRef, String> {
    let url = git::remote_url(repo, "origin").ok_or("This repository has no 'origin' remote.")?;
    parse_remote(&url)
        .ok_or_else(|| "The 'origin' remote is not a github.com repository.".to_string())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub login: String,
    /// "gh" (GitHub CLI) or "git" (git credential store)
    pub source: String,
    pub repo: Option<RepoRef>,
    pub default_branch: Option<String>,
}

pub fn account(session: &Session, repo: &Path) -> Result<Account, String> {
    let user = call(session, repo, Method::Get, "/user")?;
    let source = session.token(repo)?.source.to_string();
    let r = repo_ref(repo).ok();
    let default_branch = match &r {
        Some(r) => call(
            session,
            repo,
            Method::Get,
            &format!("/repos/{}/{}", r.owner, r.name),
        )
        .ok()
        .and_then(|v| v["default_branch"].as_str().map(str::to_string)),
        None => None,
    };
    Ok(Account {
        login: user["login"].as_str().unwrap_or_default().into(),
        source,
        repo: r,
        default_branch,
    })
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

/// `state`: "open" | "closed" | "all"
pub fn list(session: &Session, repo: &Path, state: &str) -> Result<Vec<Pull>, String> {
    let r = repo_ref(repo)?;
    let state = if matches!(state, "open" | "closed" | "all") {
        state
    } else {
        "open"
    };
    let v = call(
        session,
        repo,
        Method::Get,
        &format!(
            "/repos/{}/{}/pulls?state={state}&sort=updated&direction=desc&per_page=50",
            r.owner, r.name
        ),
    )?;
    Ok(v.as_array()
        .map(|a| a.iter().map(pull_from).collect())
        .unwrap_or_default())
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
    pub comments: Vec<Comment>,
}

pub fn detail(session: &Session, repo: &Path, number: u64) -> Result<PullDetail, String> {
    let r = repo_ref(repo)?;
    let base = format!("/repos/{}/{}", r.owner, r.name);
    let v = call(
        session,
        repo,
        Method::Get,
        &format!("{base}/pulls/{number}"),
    )?;
    let pull = pull_from(&v);
    let s = |x: &Value| x.as_str().unwrap_or_default().to_string();

    let mut checks = vec![];
    if let Ok(runs) = call(
        session,
        repo,
        Method::Get,
        &format!("{base}/commits/{}/check-runs?per_page=100", pull.head_sha),
    ) {
        for c in runs["check_runs"].as_array().into_iter().flatten() {
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
    if let Ok(st) = call(
        session,
        repo,
        Method::Get,
        &format!("{base}/commits/{}/status", pull.head_sha),
    ) {
        for c in st["statuses"].as_array().into_iter().flatten() {
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

    let mut comments = vec![];
    if let Ok(list) = call(
        session,
        repo,
        Method::Get,
        &format!("{base}/issues/{number}/comments?per_page=100"),
    ) {
        for c in list.as_array().into_iter().flatten() {
            comments.push(Comment {
                author: s(&c["user"]["login"]),
                body: s(&c["body"]),
                created_at: s(&c["created_at"]),
                review: None,
            });
        }
    }
    if let Ok(list) = call(
        session,
        repo,
        Method::Get,
        &format!("{base}/pulls/{number}/reviews?per_page=100"),
    ) {
        for c in list.as_array().into_iter().flatten() {
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
    }
    comments.sort_by(|a, b| a.created_at.cmp(&b.created_at));

    Ok(PullDetail {
        body: s(&v["body"]),
        additions: v["additions"].as_u64().unwrap_or_default(),
        deletions: v["deletions"].as_u64().unwrap_or_default(),
        changed_files: v["changed_files"].as_u64().unwrap_or_default(),
        commits: v["commits"].as_u64().unwrap_or_default(),
        mergeable: v["mergeable"].as_bool(),
        mergeable_state: s(&v["mergeable_state"]),
        checks,
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
pub fn files(
    repo: &Path,
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
            "origin",
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

pub fn create(
    session: &Session,
    repo: &Path,
    title: &str,
    body: &str,
    head: &str,
    base: &str,
    draft: bool,
) -> Result<Pull, String> {
    let r = repo_ref(repo)?;
    let v = call(
        session,
        repo,
        Method::Post(
            json!({ "title": title, "body": body, "head": head, "base": base, "draft": draft }),
        ),
        &format!("/repos/{}/{}/pulls", r.owner, r.name),
    )?;
    Ok(pull_from(&v))
}

/// `method`: "merge" | "squash" | "rebase"
pub fn merge(session: &Session, repo: &Path, number: u64, method: &str) -> Result<(), String> {
    let r = repo_ref(repo)?;
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

/// Switches to the PR's branch: the real branch for same-repo PRs, `pr/<n>` for forks.
/// An existing local branch is only fast-forwarded, never reset: unpushed work on it is
/// kept, and a branch that diverged from the PR is reported instead of silently used.
pub fn checkout(repo: &Path, number: u64, head_ref: &str, same_repo: bool) -> Result<(), String> {
    let local = if same_repo {
        head_ref.to_string()
    } else {
        format!("pr/{number}")
    };
    git::run(repo, &["check-ref-format", "--branch", &local])
        .map_err(|_| format!("invalid branch: {local}"))?;
    let source = if same_repo {
        head_ref.to_string()
    } else {
        format!("pull/{number}/head")
    };
    // FETCH_HEAD is the PR head either way; never force-update a local branch.
    git::run(repo, &["fetch", "--quiet", "origin", &source])?;
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
        // DWIM: creates a local branch tracking origin/<head_ref>.
        git::switch_branch(repo, &local, false)
    } else {
        git::run(repo, &["switch", "-c", &local, "FETCH_HEAD"]).map(|_| ())
    }
}

/// Opens a github.com page in the default browser. The URL comes from API data, so it's
/// restricted to plain URL characters: no quotes, spaces or shell metacharacters.
pub fn open_url(url: &str) -> Result<(), String> {
    let plain = url
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "-._~/:?=#%+".contains(c));
    if !url.starts_with("https://github.com/") || !plain {
        return Err("refusing to open this URL".into());
    }
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
    cmd.arg(url).spawn().map(|_| ()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

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
            acct.login, acct.source, acct.default_branch
        );

        let open = list(&session, repo, "open").unwrap();
        let number = match open.iter().find(|p| p.head_ref == "feature/review") {
            Some(p) => p.number,
            None => {
                create(
                    &session,
                    repo,
                    "Review: newest commit opens by default",
                    "Opened by GitViber's live test.",
                    "feature/review",
                    "main",
                    false,
                )
                .unwrap()
                .number
            }
        };
        let d = detail(&session, repo, number).unwrap();
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
            repo,
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
        let d = detail(&session, repo, 2).unwrap();
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
        checkout(repo, 2, &d.pull.head_ref, same_repo).unwrap();
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
}
