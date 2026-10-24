//! A pull request's title and description from the user's own agent CLI: the command as run and
//! its answer read back. The commit box's suggestions work the same way.

pub const PULL_PROMPT: &str = "Write a GitHub pull request title and description for this branch: a title under 72 characters on the first line, a blank line, then a description in Markdown of what changed and why. Output only the title and the description, with no quotes or code fences around them.";

struct Preset {
    id: &'static str,
    command: &'static str,
    model_flag: &'static str,
    model: &'static str,
    /// The reasoning effort flag and the level a preset runs at until the user picks one.
    effort: Option<(&'static str, &'static str)>,
    /// Flags every run gets, for a faster start; with no tools, as a description needs none.
    lean: &'static str,
}

const PRESETS: [Preset; 5] = [
    Preset {
        id: "claude",
        command: "claude -p",
        model_flag: "--model",
        model: "sonnet",
        effort: Some(("--effort", "medium")),
        lean: "--strict-mcp-config --no-session-persistence --disable-slash-commands --tools \"\"",
    },
    Preset {
        id: "codex",
        command: "codex exec",
        model_flag: "-m",
        model: "gpt-6-luna",
        effort: Some(("-c model_reasoning_effort=", "medium")),
        lean: "",
    },
    Preset {
        id: "opencode",
        command: "opencode run --agent plan",
        model_flag: "-m",
        model: "anthropic/claude-sonnet-5",
        effort: Some(("--variant", "")),
        lean: "",
    },
    Preset {
        id: "pi",
        command: "pi -p --no-tools --no-session",
        model_flag: "--model",
        model: "anthropic/claude-sonnet-5",
        effort: Some(("--thinking", "medium")),
        lean: "",
    },
    Preset {
        id: "llm",
        command: "llm",
        model_flag: "-m",
        model: "gpt-6-luna",
        effort: None,
        lean: "",
    },
];

/// The program a command runs, by name ("claude" from "~/.local/bin/claude -p").
pub fn program_of(command: &str) -> &str {
    command
        .split_whitespace()
        .next()
        .and_then(|p| p.rsplit('/').next())
        .unwrap_or("")
}

/// The command as run: a preset's with its lean, model and effort flags; a custom one as typed.
pub fn command_line(
    command: &str,
    models: &std::collections::BTreeMap<String, String>,
    efforts: &std::collections::BTreeMap<String, String>,
) -> String {
    let Some(p) = PRESETS.iter().find(|p| p.command == command.trim()) else {
        return command.to_string();
    };
    let model = models
        .get(p.id)
        .map(String::as_str)
        .unwrap_or(p.model)
        .trim()
        .to_string();
    let effort = p.effort.map(|(flag, default)| {
        let level = efforts.get(p.id).map(String::as_str).unwrap_or(default);
        if level.is_empty() {
            String::new()
        } else if flag.ends_with('=') {
            format!("{flag}{level}")
        } else {
            format!("{flag} {level}")
        }
    });
    let model = (!model.is_empty()).then(|| format!("{} {model}", p.model_flag));
    [
        Some(command.trim().to_string()),
        Some(p.lean.to_string()),
        model,
        effort,
    ]
    .into_iter()
    .flatten()
    .filter(|s| !s.is_empty())
    .collect::<Vec<_>>()
    .join(" ")
}

/// A Markdown answer inside a fence that wraps it: the first fence (after at most a "Here's the
/// PR:" line) to the last. One marked with a language is the description's own code.
fn unwrap_markdown(text: &str) -> String {
    let lines: Vec<&str> = text.split('\n').collect();
    let fence_of = |l: &str| {
        let c = l.chars().next()?;
        if c != '`' && c != '~' {
            return None;
        }
        let n = l.chars().take_while(|x| *x == c).count();
        (n >= 3).then(|| {
            (
                l[..n].to_string(),
                l[n..].split_whitespace().next().unwrap_or("").to_string(),
            )
        })
    };
    let Some(open) = lines.iter().position(|l| fence_of(l).is_some()) else {
        return text.to_string();
    };
    let (fence, info) = fence_of(lines[open]).expect("found above");
    let preface = lines[..open].join("\n");
    let preface = preface.trim();
    let plain = ["", "markdown", "md", "text"].contains(&info.to_lowercase().as_str());
    if !plain || (!preface.is_empty() && (preface.contains('\n') || !preface.ends_with(':'))) {
        return text.to_string();
    }
    match lines.iter().rposition(|l| l.trim() == fence) {
        Some(close) if close > open => lines[open + 1..close].join("\n").trim().to_string(),
        _ => text.to_string(),
    }
}

/// The title and description in an agent's answer; None when it said nothing usable.
pub fn parse_suggestion(output: &str) -> Option<(String, String)> {
    let mut text = unwrap_markdown(output.replace("\r\n", "\n").replace('\r', "\n").trim());
    for q in ['"', '\'', '`'] {
        if text.len() >= 2
            && text.starts_with(q)
            && text.ends_with(q)
            && !text[1..text.len() - 1].contains(q)
        {
            text = text[1..text.len() - 1].trim().to_string();
            break;
        }
    }
    let (first, rest) = text.split_once('\n').unwrap_or((&text, ""));
    let mut summary = first.trim_start_matches('#').to_string();
    if summary.len() != first.len() {
        summary = summary.trim_start().to_string();
    }
    for wrap in ["**", "__"] {
        if summary.len() > 4 && summary.starts_with(wrap) && summary.ends_with(wrap) {
            summary = summary[2..summary.len() - 2].to_string();
        }
    }
    for label in ["summary:", "subject:", "title:", "commit message:"] {
        if summary.to_lowercase().starts_with(label) {
            summary = summary[label.len()..].trim_start().to_string();
            break;
        }
    }
    let summary = summary.trim().to_string();
    let mut body = rest.trim().to_string();
    for label in ["body:", "description:"] {
        if body.to_lowercase().starts_with(label) {
            body = body[label.len()..].to_string();
            break;
        }
    }
    (!summary.is_empty()).then_some((summary, body))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn a_preset_runs_with_its_flags_a_custom_command_as_typed() {
        let none = BTreeMap::new();
        assert_eq!(
            command_line("claude -p", &none, &none),
            "claude -p --strict-mcp-config --no-session-persistence --disable-slash-commands --tools \"\" --model sonnet --effort medium"
        );
        let models = BTreeMap::from([("codex".to_string(), "gpt-x".to_string())]);
        let efforts = BTreeMap::from([("codex".to_string(), "high".to_string())]);
        assert_eq!(
            command_line("codex exec", &models, &efforts),
            "codex exec -m gpt-x -c model_reasoning_effort=high"
        );
        assert_eq!(
            command_line("my-agent --flag", &none, &none),
            "my-agent --flag"
        );
        assert_eq!(program_of("~/.local/bin/claude -p"), "claude");
    }

    #[test]
    fn an_answer_reads_as_a_title_and_a_description() {
        assert_eq!(
            parse_suggestion("Fix login\n\nIt failed on SSO."),
            Some(("Fix login".into(), "It failed on SSO.".into()))
        );
        assert_eq!(
            parse_suggestion("# Title: Fix login\n\nBody"),
            Some(("Fix login".into(), "Body".into()))
        );
        assert_eq!(
            parse_suggestion("Here's the PR:\n```markdown\n**Fix login**\n\nBody\n```"),
            Some(("Fix login".into(), "Body".into()))
        );
        assert_eq!(
            parse_suggestion("\"Fix login\""),
            Some(("Fix login".into(), String::new()))
        );
        assert_eq!(parse_suggestion("   \n "), None);
        // A fence with a language is the description's own code.
        let code = "Add parser\n\n```rust\nfn main() {}\n```";
        assert_eq!(
            parse_suggestion(code).unwrap().1,
            "```rust\nfn main() {}\n```"
        );
    }
}
