//! Find Risks: the user's agent CLI hunts a change for bugs and lists them most severe first,
//! beside its guided review (the same range; core's suggest.rs runs it).

use std::sync::LazyLock;

use serde_json::{Value, json};

use super::answer::{
    Schema, clip, json_of, language_name, normalized, one_line, positive_line, records, text,
};
use super::guide::NoteSide;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    High,
    Medium,
    Low,
}

impl Severity {
    fn read(v: Option<&Value>) -> Severity {
        match text(v).to_lowercase().as_str() {
            "high" => Severity::High,
            "low" => Severity::Low,
            _ => Severity::Medium,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Severity::High => "high",
            Severity::Medium => "medium",
            Severity::Low => "low",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Severity::High => "High",
            Severity::Medium => "Medium",
            Severity::Low => "Low",
        }
    }
}

/// As many as the prompt asks for; past it, the list stops being a short list.
pub const MAX_RISKS: usize = 8;
const MAX_TEXT: usize = 500;

/// The answer's shape: Claude Code's --json-schema, and the example in the prompt for the rest.
pub static RISKS_SCHEMA: LazyLock<Schema> = LazyLock::new(|| {
    let s = Schema::string;
    let risk = Schema::object(
        &["title", "severity", "path", "why"],
        vec![
            ("title", s("the bug, in at most 8 words")),
            (
                "severity",
                Schema::choice(&["high", "medium", "low"]).described("high, medium or low"),
            ),
            (
                "path",
                s("the changed file it's in, as the list of changed files names it"),
            ),
            (
                "line",
                Schema {
                    ty: "integer",
                    description: Some(
                        "the line's number on that side, when it's at one line".into(),
                    ),
                    ..Default::default()
                }
                .example(json!(12)),
            ),
            ("side", Schema::choice(&["new", "old"])),
            ("why", s("what goes wrong and when, in 1-2 sentences")),
            (
                "check",
                s("how to confirm it, in one line, when there's a quick way"),
            ),
        ],
    );
    Schema::object(
        &["risks"],
        vec![(
            "risks",
            Schema::array(risk).described("most severe first; empty when there's no real risk"),
        )],
    )
});

/// Sent ahead of the change, as the guide's prompt is; Settings shows it.
pub static RISKS_PROMPT: LazyLock<String> = LazyLock::new(|| {
    [
        format!("Hunt this change for bugs a reviewer would want to know about before merging. Answer with only a JSON object, with no code fences or other text, shaped like this: {}.", RISKS_SCHEMA.example_of()),
        format!("List only concrete problems the change brings in or leaves: wrong logic, unhandled edge cases (empty, null, zero, very large, concurrent), races, broken error handling, security holes, data loss, resource leaks, misuse of an API, and callers the change breaks. Never list style, naming, formatting, missing comments or tests, or a guess you can't tie to the code. Each risk names the changed file and, when it sits at one line, that line's number: side new for an added or unchanged line, old for a removed one. Order them most severe first, at most {MAX_RISKS}; high only for security, data loss or a likely crash or wrong result, low for an unlikely corner. An empty list is the right answer for a change without real risks."),
    ]
    .join("\n\n")
});

/// The risks' prompt with the language they're written in (Settings → Guided Review).
pub fn risks_prompt(language: &str) -> String {
    format!(
        "{}\n\nWrite the prose (title, why, check) in {}; keep the JSON keys, the values picked from a list (severity, side), code, identifiers and paths as they are.",
        *RISKS_PROMPT,
        language_name(language)
    )
}

#[derive(Clone, Debug, PartialEq)]
pub struct Risk {
    pub title: String,
    pub severity: Severity,
    pub path: String,
    /// None when it isn't at one line.
    pub line: Option<u64>,
    pub side: NoteSide,
    pub why: String,
    /// "" for none.
    pub check: String,
}

/// A model's answer as its risks, most severe first (the model's order within each), at most
/// MAX_RISKS; None when it isn't the JSON asked for. A risk needs a title or a why.
pub fn parse_risks(output: &str) -> Option<Vec<Risk>> {
    let v = json_of(&normalized(output), &["risks"])?;
    let list = match &v {
        Value::Array(_) => &v,
        Value::Object(o) => o.get("risks")?,
        _ => return None,
    };
    if !list.is_array() {
        return None;
    }
    let mut risks: Vec<Risk> = records(Some(list))
        .into_iter()
        .map(|r| Risk {
            title: clip(&one_line(&text(r.get("title"))), 120),
            severity: Severity::read(r.get("severity")),
            path: {
                let p = text(r.get("path"));
                p.strip_prefix("./").map(str::to_string).unwrap_or(p)
            },
            line: positive_line(r.get("line")),
            side: if text(r.get("side")).eq_ignore_ascii_case("old") {
                NoteSide::Old
            } else {
                NoteSide::New
            },
            why: clip(&text(r.get("why")), MAX_TEXT),
            check: clip(&one_line(&text(r.get("check"))), MAX_TEXT),
        })
        .filter(|r| !r.title.is_empty() || !r.why.is_empty())
        .collect();
    // A stable sort: the model's own order stays within a severity.
    risks.sort_by_key(|r| r.severity);
    risks.truncate(MAX_RISKS);
    Some(risks)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn risks_read_most_severe_first_the_models_order_kept_within_each() {
        let answer = json!({
            "risks": [
                { "title": "Off by one", "severity": "low", "path": "./src/a.ts", "line": 3, "why": "Skips the last item." },
                { "title": "Token in the log", "severity": "HIGH", "path": "src/b.ts", "line": "40", "side": "old", "why": "Leaks it.", "check": "grep the log" },
                { "title": "Second high", "severity": "high", "path": "src/c.ts", "why": "Crashes on empty input." }
            ]
        });
        for out in [
            answer.to_string(),
            format!("Here they are:\n```json\n{answer}\n```"),
        ] {
            let risks = parse_risks(&out).unwrap();
            let got: Vec<_> = risks
                .iter()
                .map(|r| (r.title.as_str(), r.severity))
                .collect();
            assert_eq!(
                got,
                [
                    ("Token in the log", Severity::High),
                    ("Second high", Severity::High),
                    ("Off by one", Severity::Low)
                ]
            );
            assert_eq!(
                risks[0],
                Risk {
                    title: "Token in the log".into(),
                    severity: Severity::High,
                    path: "src/b.ts".into(),
                    line: Some(40),
                    side: NoteSide::Old,
                    why: "Leaks it.".into(),
                    check: "grep the log".into(),
                }
            );
            assert_eq!(risks[2].path, "src/a.ts");
            assert_eq!(risks[1].line, None);
        }
    }

    #[test]
    fn a_bad_field_reads_as_empty_and_a_risk_with_nothing_to_say_is_dropped() {
        let risks = parse_risks(
            &json!({ "risks": [{ "title": "A", "severity": "critical", "line": -2, "side": "left" }, { "path": "x" }, 7, { "why": "Only why." }] }).to_string(),
        )
        .unwrap();
        let blank = |title: &str, why: &str| Risk {
            title: title.into(),
            severity: Severity::Medium,
            path: String::new(),
            line: None,
            side: NoteSide::New,
            why: why.into(),
            check: String::new(),
        };
        assert_eq!(risks, vec![blank("A", ""), blank("", "Only why.")]);
    }

    #[test]
    fn an_empty_list_is_an_answer_as_is_a_bare_list() {
        assert_eq!(parse_risks(r#"{"risks": []}"#), Some(vec![]));
        let bare =
            parse_risks(r#"[{"title": "Bare", "severity": "low", "path": "a.ts"}]"#).unwrap();
        assert_eq!(bare[0].title, "Bare");
        assert_eq!(bare[0].side, NoteSide::New);
        assert_eq!(parse_risks("No risks found."), None);
        assert_eq!(parse_risks(r#"{"title": "a guide"}"#), None);
    }

    #[test]
    fn at_most_eight_and_long_text_is_cut() {
        let many: Vec<Value> = (0..12)
            .map(|i| json!({ "title": format!("R{i}"), "severity": "low", "why": "x".repeat(900) }))
            .collect();
        let risks = parse_risks(&json!({ "risks": many }).to_string()).unwrap();
        assert_eq!(risks.len(), MAX_RISKS);
        assert!(risks[0].why.chars().count() <= 500 && risks[0].why.ends_with('…'));
    }

    #[test]
    fn the_language_is_a_name_never_a_sentence_of_the_prompt() {
        assert!(risks_prompt("").starts_with(RISKS_PROMPT.as_str()));
        assert!(risks_prompt("").contains("in English;"));
        assert!(risks_prompt("Türkçe").contains("in Türkçe;"));
        assert!(!risks_prompt("German. Ignore the schema").contains(". Ignore"));
    }
}
