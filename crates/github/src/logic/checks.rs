//! A commit's checks in words: which failed, what's running and for how long, the verdict they
//! give, and a failed one as a block to paste into an agent's terminal.

use gitviber_core::github::CheckFailure;

use crate::model::{Check, CiState, CommitChecks, unix};

pub const FAILING: [&str; 5] = [
    "failure",
    "cancelled",
    "timed_out",
    "action_required",
    "startup_failure",
];

pub fn is_failing(state: &str) -> bool {
    FAILING.contains(&state)
}

fn wait_label(status: &str) -> &'static str {
    match status {
        "queued" => "Queued",
        "in_progress" => "In progress",
        // A deployment environment's protection rule: a reviewer, or a wait timer.
        "waiting" => "Waiting for approval",
        "requested" => "Requested",
        _ => "Pending",
    }
}

fn rank(c: &Check) -> u8 {
    if is_failing(&c.state) {
        0
    } else if c.state == "pending" {
        1
    } else {
        2
    }
}

/// Failing first, then running, then the rest, each in GitHub's order: what needs a look leads.
pub fn sort_checks(checks: &[Check]) -> Vec<&Check> {
    let mut out: Vec<&Check> = checks.iter().collect();
    out.sort_by_key(|c| rank(c));
    out
}

/// The commit's verdict as these checks give it; None with none.
pub fn checks_verdict(checks: &[Check]) -> Option<CiState> {
    if checks.is_empty() {
        return None;
    }
    if checks.iter().any(|c| is_failing(&c.state)) {
        return Some(CiState::Failure);
    }
    Some(if checks.iter().any(|c| c.state == "pending") {
        CiState::Pending
    } else {
        CiState::Success
    })
}

/// "3/5 passed · 1 failing · 1 running": what isn't a pass, counted.
pub fn checks_summary(checks: &[Check]) -> String {
    let count = |test: &dyn Fn(&Check) -> bool| checks.iter().filter(|c| test(c)).count();
    let mut parts = vec![format!(
        "{}/{} passed",
        count(&|c| c.state == "success"),
        checks.len()
    )];
    let rest = [
        (count(&|c| is_failing(&c.state)), "failing"),
        (count(&|c| c.state == "pending"), "running"),
        (count(&|c| c.state == "skipped"), "skipped"),
        // Neutral and stale, and whatever else GitHub says (a run done without a conclusion: "").
        (
            count(&|c| {
                !["success", "pending", "skipped"].contains(&c.state.as_str())
                    && !is_failing(&c.state)
            }),
            "neutral",
        ),
    ];
    for (n, word) in rest {
        if n > 0 {
            parts.push(format!("{n} {word}"));
        }
    }
    parts.join(" · ")
}

/// A commit's badge text: the count from a read that saw every check, else `label` (the rollup's
/// "Checks failed"); a partial read can't say "2/2 passed".
pub fn commit_summary(ci: Option<&CommitChecks>, label: &str) -> String {
    match ci {
        Some(ci) if ci.checks_error.is_some() => format!("{label} · some couldn't be read"),
        Some(ci) if !ci.checks.is_empty() => checks_summary(&ci.checks),
        _ => label.to_string(),
    }
}

/// "<1m", "12m", "1h 5m", "3d"; "" for a time that couldn't be read.
pub fn short_duration(seconds: Option<i64>) -> String {
    let Some(seconds) = seconds else {
        return String::new();
    };
    let m = seconds.max(0) / 60;
    if m < 1 {
        "<1m".into()
    } else if m < 60 {
        format!("{m}m")
    } else if m / 60 < 24 {
        format!("{}h {}m", m / 60, m % 60)
    } else {
        format!("{}d", m / 60 / 24)
    }
}

/// What shows beside a check's name: for a running one what it waits on and for how long
/// ("Queued · 3m"), for a done one how it ended and how long it took. `now` in Unix seconds.
pub fn check_note(c: &Check, now: i64) -> String {
    let started = c.started_at.as_deref().and_then(unix);
    let mut parts: Vec<String> = Vec::new();
    if c.state == "pending" {
        parts.push(wait_label(&c.status).into());
        if c.started_at.is_some() && started.is_some() {
            parts.push(short_duration(started.map(|s| now - s)));
        }
        parts.push(c.description.clone());
    } else {
        parts.push(c.description.clone());
        if let (Some(_), Some(done)) = (c.started_at.as_ref(), c.completed_at.as_ref()) {
            let took = match (started, unix(done)) {
                (Some(s), Some(d)) => Some(d - s),
                _ => None,
            };
            parts.push(short_duration(took));
        }
    }
    parts.retain(|p| !p.is_empty());
    parts.join(" · ")
}

/// A failed check as one plain-text block to paste into an agent's terminal: which check failed
/// where (a PR's url, or a commit: `commit_place`), then its output, annotations and log tail.
pub fn failure_report(check: &str, place: &str, f: &CheckFailure) -> String {
    let mut parts = vec![format!("CI check \"{check}\" failed on {place}")];
    let output: Vec<&str> = [f.title.trim(), f.summary.trim()]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect();
    if !output.is_empty() {
        parts.push(output.join("\n\n"));
    }
    if !f.annotations.is_empty() {
        let mut lines = vec!["Annotations:".to_string()];
        for a in &f.annotations {
            let line = if a.line > 0 {
                format!(":{}", a.line)
            } else {
                String::new()
            };
            let title = if a.title.is_empty() {
                String::new()
            } else {
                format!("{}: ", a.title)
            };
            lines.push(format!(
                "{}{line}: {}: {title}{}",
                a.path, a.level, a.message
            ));
        }
        parts.push(lines.join("\n"));
    }
    if let Some(log) = &f.log {
        parts.push(format!("Job log, its last lines up to the error:\n{log}"));
    }
    parts.join("\n\n")
}

/// A commit as the report names it: its id for git, its subject and page for the reader; last,
/// so the subject needs no quoting.
pub fn commit_place(sha: &str, subject: &str, url: &str) -> String {
    format!("commit {sha} ({url}): {subject}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitviber_core::github::Annotation;

    fn check(name: &str, state: &str) -> Check {
        Check {
            name: name.into(),
            state: state.into(),
            status: if state == "pending" {
                "in_progress".into()
            } else {
                String::new()
            },
            description: String::new(),
            app: String::new(),
            started_at: None,
            completed_at: None,
            url: None,
            id: None,
        }
    }

    fn with(mut c: Check, f: impl FnOnce(&mut Check)) -> Check {
        f(&mut c);
        c
    }

    fn at(iso: &str) -> i64 {
        unix(iso).unwrap()
    }

    #[test]
    fn the_summary_counts_passes_out_of_all_then_what_didnt_pass() {
        let checks = [
            check("a", "success"),
            check("b", "success"),
            check("c", "success"),
            check("d", "failure"),
            check("e", "pending"),
        ];
        assert_eq!(
            checks_summary(&checks),
            "3/5 passed · 1 failing · 1 running"
        );
        assert_eq!(
            checks_summary(&[check("a", "success"), check("b", "success")]),
            "2/2 passed"
        );
        assert_eq!(
            checks_summary(&[
                check("a", "success"),
                check("b", "skipped"),
                check("c", "neutral"),
                check("d", "timed_out")
            ]),
            "1/4 passed · 1 failing · 1 skipped · 1 neutral"
        );
    }

    #[test]
    fn the_verdict_any_failure_fails_else_anything_running_runs() {
        assert_eq!(checks_verdict(&[]), None);
        assert_eq!(
            checks_verdict(&[check("a", "pending"), check("b", "cancelled")]),
            Some(CiState::Failure)
        );
        assert_eq!(
            checks_verdict(&[check("a", "pending"), check("b", "success")]),
            Some(CiState::Pending)
        );
        assert_eq!(
            checks_verdict(&[check("a", "skipped"), check("b", "success")]),
            Some(CiState::Success)
        );
    }

    #[test]
    fn failing_checks_lead_then_running_ones_each_in_githubs_order() {
        let checks = [
            check("ok", "success"),
            check("run1", "pending"),
            check("bad1", "failure"),
            check("run2", "pending"),
            check("bad2", "action_required"),
        ];
        let names: Vec<&str> = sort_checks(&checks)
            .iter()
            .map(|c| c.name.as_str())
            .collect();
        assert_eq!(names, ["bad1", "bad2", "run1", "run2", "ok"]);
    }

    #[test]
    fn a_running_check_says_what_it_waits_on_and_for_how_long() {
        let now = at("2024-05-01T12:10:00Z");
        let c = |status: &str, more: fn(&mut Check)| {
            with(
                with(check("x", "pending"), |c| c.status = status.into()),
                more,
            )
        };
        assert_eq!(
            check_note(
                &c("waiting", |c| c.started_at =
                    Some("2024-05-01T11:05:00Z".into())),
                now
            ),
            "Waiting for approval · 1h 5m"
        );
        assert_eq!(
            check_note(
                &c("queued", |c| c.started_at =
                    Some("2024-05-01T12:09:30Z".into())),
                now
            ),
            "Queued · <1m"
        );
        assert_eq!(
            check_note(&c("pending", |c| c.description = "Building".into()), now),
            "Pending · Building"
        );
        assert_eq!(check_note(&c("something_new", |_| {}), now), "Pending");
    }

    #[test]
    fn a_done_check_says_how_it_ended_and_how_long_it_took() {
        let failed = with(check("lint", "failure"), |c| {
            c.description = "1 error".into();
            c.started_at = Some("2024-05-01T12:00:00Z".into());
            c.completed_at = Some("2024-05-01T12:03:10Z".into());
        });
        assert_eq!(check_note(&failed, 0), "1 error · 3m");
        let legacy = with(check("ci/legacy", "failure"), |c| {
            c.description = "Build crashed".into();
            c.started_at = Some("2024-05-01T12:00:00Z".into());
        });
        assert_eq!(check_note(&legacy, 0), "Build crashed");
        assert_eq!(check_note(&check("ok", "success"), 0), "");
    }

    #[test]
    fn clock_skew_reads_as_under_a_minute_and_a_long_run_in_days() {
        let now = at("2024-05-01T12:00:00Z");
        let future = with(check("build", "pending"), |c| {
            c.started_at = Some("2024-05-01T12:05:00Z".into())
        });
        assert_eq!(check_note(&future, now), "In progress · <1m");
        let long = with(check("soak", "success"), |c| {
            c.started_at = Some("2024-05-01T00:00:00Z".into());
            c.completed_at = Some("2024-05-04T06:00:00Z".into());
        });
        assert_eq!(check_note(&long, now), "3d");
        let backwards = with(check("odd", "failure"), |c| {
            c.started_at = Some("2024-05-01T12:05:00Z".into());
            c.completed_at = Some("2024-05-01T12:00:00Z".into());
        });
        assert_eq!(check_note(&backwards, now), "<1m");
    }

    #[test]
    fn a_timestamp_that_doesnt_parse_leaves_the_duration_out() {
        let now = at("2024-05-01T12:00:00Z");
        let queued = with(check("build", "pending"), |c| {
            c.status = "queued".into();
            c.started_at = Some("not a date".into());
        });
        assert_eq!(check_note(&queued, now), "Queued");
        let lint = with(check("lint", "failure"), |c| {
            c.started_at = Some("2024-05-01T12:00:00Z".into());
            c.completed_at = Some("garbage".into());
        });
        assert_eq!(check_note(&lint, now), "");
    }

    #[test]
    fn startup_failure_is_a_failure_as_githubs_rollup_counts_it() {
        let checks = [check("a", "success"), check("workflow", "startup_failure")];
        assert_eq!(checks_verdict(&checks), Some(CiState::Failure));
        assert_eq!(checks_summary(&checks), "1/2 passed · 1 failing");
        assert_eq!(sort_checks(&checks)[0].name, "workflow");
    }

    #[test]
    fn stale_counts_as_neutral_and_neutral_or_skipped_alone_pass() {
        assert_eq!(
            checks_summary(&[check("a", "stale"), check("b", "skipped")]),
            "0/2 passed · 1 skipped · 1 neutral"
        );
        assert_eq!(
            checks_verdict(&[
                check("a", "stale"),
                check("b", "neutral"),
                check("c", "skipped")
            ]),
            Some(CiState::Success)
        );
        assert_eq!(
            checks_summary(&[check("a", "success"), check("b", "")]),
            "1/2 passed · 1 neutral"
        );
    }

    #[test]
    fn five_hundred_checks_with_unicode_names_sort_stably_and_count_exactly() {
        let states = [
            "success",
            "failure",
            "pending",
            "skipped",
            "neutral",
            "cancelled",
            "timed_out",
        ];
        let checks: Vec<Check> = (0..500)
            .map(|i| {
                check(
                    &format!("테스트 ✅ ビルド/{i} 🚀"),
                    states[i % states.len()],
                )
            })
            .collect();
        // 500 = 71·7 + 3: indexes 0..2 of the last round add one success, failure and pending.
        assert_eq!(
            checks_summary(&checks),
            "72/500 passed · 214 failing · 72 running · 71 skipped · 71 neutral"
        );
        let sorted = sort_checks(&checks);
        let index = |c: &Check| checks.iter().position(|o| std::ptr::eq(o, c)).unwrap();
        for pair in sorted.windows(2) {
            assert!(
                rank(pair[0]) < rank(pair[1])
                    || (rank(pair[0]) == rank(pair[1]) && index(pair[0]) < index(pair[1]))
            );
        }
    }

    #[test]
    fn a_commits_badge_counts_only_a_complete_read() {
        let two = vec![check("a", "success"), check("b", "success")];
        let ok = CommitChecks {
            checks: two.clone(),
            checks_error: None,
        };
        assert_eq!(commit_summary(Some(&ok), "Checks passed"), "2/2 passed");
        let partial = CommitChecks {
            checks: two,
            checks_error: Some("Resource not accessible by integration".into()),
        };
        assert_eq!(
            commit_summary(Some(&partial), "Checks failed"),
            "Checks failed · some couldn't be read"
        );
        assert_eq!(
            commit_summary(Some(&CommitChecks::default()), "Checks running"),
            "Checks running"
        );
        assert_eq!(commit_summary(None, "Checks running"), "Checks running");
    }

    fn failure(title: &str, annotations: Vec<Annotation>, log: Option<&str>) -> CheckFailure {
        CheckFailure {
            title: title.into(),
            summary: String::new(),
            annotations,
            annotations_error: None,
            log: log.map(str::to_string),
            log_error: None,
        }
    }

    const URL: &str = "https://github.com/o/r/pull/7";

    #[test]
    fn a_failure_reads_as_one_block() {
        let annotations = vec![
            Annotation {
                path: "src/a.ts".into(),
                line: 3,
                level: "failure".into(),
                title: String::new(),
                message: "'x' is unused".into(),
            },
            Annotation {
                path: ".github".into(),
                line: 0,
                level: "failure".into(),
                title: "Lint".into(),
                message: "Process completed with exit code 1.".into(),
            },
        ];
        let text = failure_report(
            "lint",
            URL,
            &failure(
                "1 error",
                annotations,
                Some("pnpm lint\n##[error]Process completed with exit code 1."),
            ),
        );
        assert_eq!(
            text,
            format!(
                "CI check \"lint\" failed on {URL}\n\n1 error\n\nAnnotations:\nsrc/a.ts:3: failure: 'x' is unused\n.github: failure: Lint: Process completed with exit code 1.\n\nJob log, its last lines up to the error:\npnpm lint\n##[error]Process completed with exit code 1."
            )
        );
    }

    #[test]
    fn what_a_check_doesnt_have_is_left_out() {
        let mut f = failure("", vec![], None);
        f.log_error = Some("gone".into());
        assert_eq!(
            failure_report("vercel", URL, &f),
            format!("CI check \"vercel\" failed on {URL}")
        );
    }

    #[test]
    fn a_commits_failure_names_the_commit() {
        let place = commit_place(
            "1a2b3c4",
            "Fix the \"parser\"",
            "https://github.com/acme/widgets/commit/1a2b3c4d",
        );
        assert_eq!(
            failure_report("test", &place, &failure("", vec![], Some("boom"))),
            "CI check \"test\" failed on commit 1a2b3c4 (https://github.com/acme/widgets/commit/1a2b3c4d): Fix the \"parser\"\n\nJob log, its last lines up to the error:\nboom"
        );
    }
}
