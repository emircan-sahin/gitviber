//! Why a check failed: its output, its annotations and, for a GitHub Actions job, its log's tail
//! (the job log `gh run view --log-failed` falls back to). Asked for one check at a time.

use super::{call, pages, string, target, text_tail, Method, Session, JSON};
use serde::Serialize;
use std::path::Path;

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
    let annotations = if output["annotations_count"].as_u64().unwrap_or(0) > 0 {
        let path = r.api(&format!("/check-runs/{id}/annotations"));
        pages(session, repo, &path, JSON, None, 1)?
            .iter()
            .map(|a| Annotation {
                path: string(&a["path"]),
                line: a["start_line"].as_u64().unwrap_or(0),
                level: string(&a["annotation_level"]),
                title: string(&a["title"]),
                message: string(&a["message"]),
            })
            .collect()
    } else {
        vec![]
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
        log,
        log_error,
    })
}

/// The last `lines` lines up to the log's last `##[error]` (post-job cleanup comes after it),
/// without the timestamps and colors. `cut`: the text starts mid-line, so its first goes.
pub fn log_tail(text: &str, cut: bool, lines: usize) -> String {
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
    // Each line starts "2024-05-01T12:00:00.1234567Z ".
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
    let mut out = String::with_capacity(line.len());
    let mut kept = 0;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if kept >= LINE_CHARS {
            out.push('…');
            break;
        }
        // CSI sequences (colors, cursor moves): ESC [ params, then a final byte in @..~.
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for f in chars.by_ref() {
                    if ('@'..='~').contains(&f) {
                        break;
                    }
                }
            }
            continue;
        }
        if c != '\r' {
            out.push(c);
            kept += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn a_long_line_is_cut() {
        let line = "x".repeat(2000);
        assert_eq!(log_tail(&line, false, 1).chars().count(), LINE_CHARS + 1);
    }
}
