//! A commit's checks, for its PR and its History entry, and why one failed: its output, its
//! annotations and, for a GitHub Actions job, its log's tail (the job log `gh run view
//! --log-failed` falls back to). Asked for one check at a time.

use super::{call, pages, string, target, text_tail, Method, Session, JSON, MAX_PAGES};
use serde::Serialize;
use serde_json::Value;
use std::path::Path;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Check {
    pub name: String,
    /// "success" | "failure" | "pending" | "neutral" | "skipped" | "cancelled" | ...
    pub state: String,
    /// What a pending one waits on: "queued" | "in_progress" | "waiting" | "requested" |
    /// "pending"; "" once done.
    pub status: String,
    /// A check run's output title, a status's description: often why it failed.
    pub description: String,
    /// The app that ran a check run ("GitHub Actions"); "" for a status.
    pub app: String,
    /// A status's is when it was posted, so only a check run has a duration.
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub url: Option<String>,
    /// A check run's id, for why it failed (check_failure); a commit status has none.
    pub id: Option<u64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitChecks {
    pub checks: Vec<Check>,
    /// Checks that couldn't be read (a token without access to them, say): not "no checks".
    pub checks_error: Option<String>,
}

/// Commit `sha`'s check runs and statuses, the latest of each, as GitHub lists them on it.
pub fn commit_checks(
    session: &Session,
    repo: &Path,
    to: Option<&str>,
    sha: &str,
) -> Result<CommitChecks, String> {
    if sha.len() != 40 || !sha.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("{sha} is not a commit id."));
    }
    let r = target(session, repo, to)?;
    Ok(read_checks(session, repo, &r.api(""), sha))
}

/// `base`: the repository's API path. Unchanged lists come back as 304s from the ETag cache,
/// which is what lets a running commit be asked again every half minute.
pub(super) fn read_checks(session: &Session, repo: &Path, base: &str, sha: &str) -> CommitChecks {
    let mut checks = vec![];
    let mut checks_error = None;
    let runs = format!("{base}/commits/{sha}/check-runs");
    match pages(session, repo, &runs, JSON, Some("check_runs"), MAX_PAGES) {
        Ok(runs) => checks.extend(runs.iter().map(check_run)),
        Err(e) => checks_error = Some(e),
    }
    let statuses = format!("{base}/commits/{sha}/status");
    match pages(session, repo, &statuses, JSON, Some("statuses"), MAX_PAGES) {
        Ok(statuses) => checks.extend(statuses.iter().map(status)),
        Err(e) => {
            checks_error.get_or_insert(e);
        }
    }
    CommitChecks {
        checks,
        checks_error,
    }
}

fn check_run(c: &Value) -> Check {
    let done = c["status"] == "completed";
    Check {
        name: string(&c["name"]),
        state: if done {
            string(&c["conclusion"])
        } else {
            "pending".to_string()
        },
        status: if done {
            String::new()
        } else {
            string(&c["status"])
        },
        description: string(&c["output"]["title"]),
        app: string(&c["app"]["name"]),
        started_at: c["started_at"].as_str().map(str::to_string),
        completed_at: c["completed_at"].as_str().map(str::to_string),
        url: c["html_url"].as_str().map(str::to_string),
        id: c["id"].as_u64(),
    }
}

/// A commit status: "pending", "success", "failure" or "error", which reads as a failure.
fn status(c: &Value) -> Check {
    let state = match c["state"].as_str() {
        Some("error") => "failure",
        other => other.unwrap_or("pending"),
    };
    Check {
        name: string(&c["context"]),
        state: state.to_string(),
        status: if state == "pending" { state } else { "" }.to_string(),
        description: string(&c["description"]),
        app: String::new(),
        started_at: c["created_at"].as_str().map(str::to_string),
        completed_at: None,
        url: c["target_url"].as_str().map(str::to_string),
        id: None,
    }
}

/// The log read, from its end: enough for the lines before the error, on any job.
const LOG_BYTES: usize = 256 * 1024;
const LOG_LINES: usize = 200;
/// A minified bundle or a progress bar is one line; it's cut, not dropped.
const LINE_CHARS: usize = 500;
/// An app's summary can be a whole report.
const SUMMARY_CHARS: usize = 8000;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Annotation {
    pub path: String,
    pub line: u64,
    /// "notice" | "warning" | "failure"
    pub level: String,
    pub title: String,
    pub message: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckFailure {
    pub title: String,
    pub summary: String,
    pub annotations: Vec<Annotation>,
    /// Why the annotations couldn't be read: the output and log still show.
    pub annotations_error: Option<String>,
    /// The job log's last lines, up to its last error; None for a check that isn't an Actions job.
    pub log: Option<String>,
    /// Why the log couldn't be read (expired, no access): the rest still shows.
    pub log_error: Option<String>,
}

pub fn check_failure(
    session: &Session,
    repo: &Path,
    to: Option<&str>,
    id: u64,
) -> Result<CheckFailure, String> {
    let r = target(session, repo, to)?;
    let run = call(
        session,
        repo,
        Method::Get,
        &r.api(&format!("/check-runs/{id}")),
    )?;
    let output = &run["output"];
    let (annotations, annotations_error) = if output["annotations_count"].as_u64().unwrap_or(0) > 0
    {
        let path = r.api(&format!("/check-runs/{id}/annotations"));
        match pages(session, repo, &path, JSON, None, 1) {
            Ok(list) => (list.iter().map(annotation).collect(), None),
            Err(e) => (vec![], Some(e)),
        }
    } else {
        (vec![], None)
    };
    // An Actions check run is its job: the same id.
    let (log, log_error) = if run["app"]["slug"] == "github-actions" {
        match text_tail(
            session,
            repo,
            &r.api(&format!("/actions/jobs/{id}/logs")),
            LOG_BYTES,
        ) {
            Ok((text, cut)) => (Some(log_tail(&text, cut, LOG_LINES)), None),
            Err(e) => (None, Some(e)),
        }
    } else {
        (None, None)
    };
    Ok(CheckFailure {
        title: string(&output["title"]),
        summary: string(&output["summary"])
            .chars()
            .take(SUMMARY_CHARS)
            .collect(),
        annotations,
        annotations_error,
        log,
        log_error,
    })
}

fn annotation(a: &Value) -> Annotation {
    Annotation {
        path: string(&a["path"]),
        line: a["start_line"].as_u64().unwrap_or(0),
        level: string(&a["annotation_level"]),
        title: string(&a["title"]),
        message: string(&a["message"]),
    }
}

/// The last `lines` lines up to the log's last `##[error]` (post-job cleanup comes after it),
/// without the timestamps and colors. `cut`: the text starts mid-line, so its first goes.
fn log_tail(text: &str, cut: bool, lines: usize) -> String {
    let all: Vec<String> = text
        .lines()
        .skip(usize::from(cut))
        .map(clean_line)
        .collect();
    let end = all
        .iter()
        .rposition(|l| l.starts_with("##[error]"))
        .map_or(all.len(), |i| i + 1);
    all[end.saturating_sub(lines)..end].join("\n")
}

fn clean_line(line: &str) -> String {
    // Each line starts "2024-05-01T12:00:00.1234567Z ", the first after a BOM.
    let line = line.trim_start_matches('\u{feff}');
    let line = match line.split_once(' ') {
        Some((ts, rest))
            if ts.len() >= 20
                && ts.ends_with('Z')
                && ts.as_bytes()[10] == b'T'
                && ts.as_bytes()[..4].iter().all(u8::is_ascii_digit) =>
        {
            rest
        }
        _ => line,
    };
    // A progress bar redraws its line after a bare \r: what showed last is what's after the last.
    let line = line.trim_end_matches('\r');
    let line = line.rsplit('\r').next().unwrap_or(line);
    let mut out = String::with_capacity(line.len());
    let mut kept = 0;
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            match chars.next() {
                // CSI (colors, cursor moves): parameters, then a final byte in @..~.
                Some('[') => {
                    for f in chars.by_ref() {
                        if ('@'..='~').contains(&f) {
                            break;
                        }
                    }
                }
                // OSC (an OSC 8 link, a title): up to BEL or ESC \; a link's text stays.
                Some(']') => {
                    while let Some(f) = chars.next() {
                        if f == '\u{7}' {
                            break;
                        }
                        if f == '\u{1b}' {
                            chars.next();
                            break;
                        }
                    }
                }
                // A character set pick, ESC ( B, takes one more.
                Some('(' | ')' | '*' | '+') => {
                    chars.next();
                }
                _ => {}
            }
            continue;
        }
        if kept == LINE_CHARS {
            out.push('…');
            break;
        }
        out.push(c);
        kept += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_running_check_run_says_what_it_waits_on_and_a_done_one_how_it_ended() {
        let waiting = check_run(&serde_json::json!({
            "id": 901, "name": "deploy", "status": "waiting", "conclusion": null,
            "started_at": "2024-05-01T12:00:00Z", "completed_at": null,
            "html_url": "https://github.com/acme/widgets/runs/901",
            "app": { "name": "GitHub Actions", "slug": "github-actions" },
            "output": { "title": null }
        }));
        assert_eq!(
            (
                waiting.state.as_str(),
                waiting.status.as_str(),
                waiting.app.as_str()
            ),
            ("pending", "waiting", "GitHub Actions")
        );
        assert_eq!(waiting.started_at.as_deref(), Some("2024-05-01T12:00:00Z"));
        assert_eq!((waiting.id, waiting.description.as_str()), (Some(901), ""));

        let failed = check_run(&serde_json::json!({
            "id": 902, "name": "lint", "status": "completed", "conclusion": "failure",
            "started_at": "2024-05-01T12:00:00Z", "completed_at": "2024-05-01T12:02:10Z",
            "output": { "title": "Process completed with exit code 1." }
        }));
        assert_eq!(
            (failed.state.as_str(), failed.status.as_str()),
            ("failure", "")
        );
        assert_eq!(failed.description, "Process completed with exit code 1.");
        assert_eq!(failed.completed_at.as_deref(), Some("2024-05-01T12:02:10Z"));
        assert_eq!(failed.url, None);
    }

    #[test]
    fn a_status_error_is_a_failure_and_only_a_pending_one_waits() {
        let errored = status(&serde_json::json!({
            "context": "ci/legacy", "state": "error", "description": "Build crashed",
            "target_url": "https://ci.example.com/b/7", "created_at": "2024-05-01T12:00:00Z"
        }));
        assert_eq!(
            (errored.state.as_str(), errored.status.as_str(), errored.id),
            ("failure", "", None)
        );
        assert_eq!(errored.description, "Build crashed");
        assert_eq!(errored.url.as_deref(), Some("https://ci.example.com/b/7"));

        let pending = status(&serde_json::json!({
            "context": "deploy/preview", "state": "pending", "description": null,
            "created_at": "2024-05-01T12:05:00Z"
        }));
        assert_eq!(
            (pending.state.as_str(), pending.status.as_str()),
            ("pending", "pending")
        );
        assert_eq!(pending.started_at.as_deref(), Some("2024-05-01T12:05:00Z"));
        assert_eq!(pending.completed_at, None);
    }

    #[test]
    fn the_tail_ends_at_the_last_error_without_timestamps_or_colors() {
        let log = "2024-05-01T12:00:00.1234567Z ##[group]Run pnpm lint\n\
                   2024-05-01T12:00:01.1234567Z \u{1b}[31msrc/a.ts:3 error\u{1b}[0m no-unused-vars\n\
                   2024-05-01T12:00:02.1234567Z ##[error]Process completed with exit code 1.\n\
                   2024-05-01T12:00:03.1234567Z Post job cleanup.\n";
        assert_eq!(
            log_tail(log, false, 200),
            "##[group]Run pnpm lint\nsrc/a.ts:3 error no-unused-vars\n##[error]Process completed with exit code 1."
        );
        assert_eq!(
            log_tail(log, false, 1),
            "##[error]Process completed with exit code 1."
        );
    }

    #[test]
    fn a_cut_log_drops_its_partial_first_line_and_one_without_errors_keeps_its_end() {
        assert_eq!(
            log_tail("tial line\nwhole\nlast\n", true, 200),
            "whole\nlast"
        );
        assert_eq!(log_tail("a\nb\nc", false, 2), "b\nc");
    }

    #[test]
    fn a_long_line_is_cut_and_one_just_fitting_is_not() {
        let line = "x".repeat(2000);
        assert_eq!(log_tail(&line, false, 1).chars().count(), LINE_CHARS + 1);
        let fits = "x".repeat(LINE_CHARS);
        assert_eq!(log_tail(&fits, false, 1), fits);
    }

    #[test]
    fn a_bom_progress_redraws_and_other_escapes_go() {
        let log = "\u{feff}2024-05-01T12:00:00.1234567Z 10%\r50%\r100%\r\n\
                   \u{1b}(Bplain\u{1b}]8;;https://x.y\u{1b}\\link\u{1b}]8;;\u{7} end";
        assert_eq!(log_tail(log, false, 200), "100%\nplainlink end");
    }

    #[test]
    fn a_sha_that_is_not_forty_hex_characters_is_refused_before_any_request() {
        let session = Session::default();
        let repo = Path::new("/nonexistent/acme-widgets");
        for sha in [
            "",
            "1a2b3c4",
            &"g".repeat(40),
            // A SHA-256 repository's id: GitHub has none, so it's not asked for.
            &"ab".repeat(32),
            "1a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a0b/../x",
        ] {
            let err = commit_checks(&session, repo, None, sha).err().unwrap();
            assert!(err.ends_with("is not a commit id."), "{sha}: {err}");
        }
    }

    #[test]
    fn every_check_run_conclusion_maps_to_its_own_state() {
        for conclusion in [
            "success",
            "failure",
            "neutral",
            "cancelled",
            "skipped",
            "timed_out",
            "action_required",
            "stale",
            "startup_failure",
        ] {
            let c = check_run(&serde_json::json!({
                "id": 7001, "name": "build", "status": "completed", "conclusion": conclusion
            }));
            assert_eq!((c.state.as_str(), c.status.as_str()), (conclusion, ""));
        }
        for status in ["queued", "in_progress", "waiting", "requested", "pending"] {
            let c = check_run(&serde_json::json!({ "name": "build", "status": status }));
            assert_eq!((c.state.as_str(), c.status.as_str()), ("pending", status));
        }
    }

    #[test]
    fn a_completed_run_without_a_conclusion_and_a_bare_object_read_as_empty() {
        let c = check_run(&serde_json::json!({
            "id": 7002, "name": "odd", "status": "completed", "conclusion": null
        }));
        assert_eq!((c.state.as_str(), c.status.as_str()), ("", ""));
        let bare = check_run(&serde_json::json!({}));
        assert_eq!(
            (
                bare.name.as_str(),
                bare.state.as_str(),
                bare.status.as_str()
            ),
            ("", "pending", "")
        );
        assert_eq!((bare.id, bare.url, bare.started_at), (None, None, None));
        // A negative or fractional id isn't a check run's.
        assert_eq!(check_run(&serde_json::json!({ "id": -1 })).id, None);
        assert_eq!(check_run(&serde_json::json!({ "id": 1.5 })).id, None);
    }

    #[test]
    fn a_status_with_every_field_null_is_a_pending_one_with_no_name() {
        let s = status(&serde_json::json!({
            "context": null, "state": null, "description": null,
            "target_url": null, "created_at": null
        }));
        assert_eq!(
            (s.name.as_str(), s.state.as_str(), s.status.as_str()),
            ("", "pending", "pending")
        );
        assert_eq!(
            (s.description.as_str(), s.url, s.started_at),
            ("", None, None)
        );
        let ok = status(&serde_json::json!({ "context": "ci", "state": "success" }));
        assert_eq!((ok.state.as_str(), ok.status.as_str()), ("success", ""));
    }

    #[test]
    fn five_hundred_runs_with_unicode_names_keep_their_order_and_names() {
        let names: Vec<String> = (0..500)
            .map(|i| format!("테스트 ✅ ビルド/{i} — \u{1F680}"))
            .collect();
        let runs: Vec<Check> = names
            .iter()
            .enumerate()
            .map(|(i, n)| {
                check_run(&serde_json::json!({
                    "id": 10_000 + i as u64, "name": n, "status": "completed",
                    "conclusion": if i % 7 == 0 { "failure" } else { "success" },
                    "html_url": format!("https://github.com/acme/widgets/runs/{}", 10_000 + i)
                }))
            })
            .collect();
        assert_eq!(runs.len(), 500);
        assert!(runs.iter().zip(&names).all(|(c, n)| &c.name == n));
        assert_eq!(runs.iter().filter(|c| c.state == "failure").count(), 72);
        let json = serde_json::to_string(&CommitChecks {
            checks: runs,
            checks_error: None,
        })
        .unwrap();
        let back: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(back["checks"][499]["name"], names[499].as_str());
    }

    #[test]
    fn the_pull_detail_keeps_checks_and_checks_error_at_its_top_level() {
        use crate::github::{Pull, PullDetail};
        let d = PullDetail {
            pull: Pull {
                number: 42,
                title: "Fix the parser".into(),
                state: "open".into(),
                draft: false,
                author: "octo".into(),
                head_ref: "fix/parser".into(),
                head_sha: "1a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a0b".into(),
                head_repo: None,
                base_ref: "main".into(),
                base_sha: "0f9e8d7c6b5a4f3e2d1c0b9a8f7e6d5c4b3a2f1e".into(),
                created_at: "2024-05-01T12:00:00Z".into(),
                updated_at: "2024-05-01T12:00:00Z".into(),
                url: "https://github.com/acme/widgets/pull/42".into(),
            },
            body: String::new(),
            additions: 1,
            deletions: 2,
            changed_files: 1,
            commits: 1,
            mergeable: None,
            mergeable_state: "unknown".into(),
            checks: CommitChecks {
                checks: vec![status(&serde_json::json!({
                    "context": "ci/legacy", "state": "error", "target_url": "https://ci.example.com/b/7"
                }))],
                checks_error: Some("Resource not accessible by integration".into()),
            },
            comments: vec![],
            closed_by: None,
            merged_by: None,
        };
        let v = serde_json::to_value(&d).unwrap();
        let mut keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "additions",
                "author",
                "baseRef",
                "baseSha",
                "body",
                "changedFiles",
                "checks",
                "checksError",
                "closedBy",
                "comments",
                "commits",
                "createdAt",
                "deletions",
                "draft",
                "headRef",
                "headRepo",
                "headSha",
                "mergeable",
                "mergeableState",
                "mergedBy",
                "number",
                "state",
                "title",
                "updatedAt",
                "url"
            ]
        );
        assert_eq!(v["checksError"], "Resource not accessible by integration");
        // The fields main's PR view read are unchanged; the rest are new.
        let c = &v["checks"][0];
        assert_eq!(
            (&c["name"], &c["state"], &c["url"], &c["id"]),
            (
                &Value::from("ci/legacy"),
                &Value::from("failure"),
                &Value::from("https://ci.example.com/b/7"),
                &Value::Null
            )
        );
        let mut fields: Vec<&str> = c.as_object().unwrap().keys().map(String::as_str).collect();
        fields.sort_unstable();
        assert_eq!(
            fields,
            [
                "app",
                "completedAt",
                "description",
                "id",
                "name",
                "startedAt",
                "state",
                "status",
                "url"
            ]
        );
    }
}
