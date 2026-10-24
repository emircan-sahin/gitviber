//! The Guided Review agent: the CLI command Settings names, the model and effort flags a known
//! one takes, and the flags that make it start fast.

use std::collections::BTreeMap;

use gitviber_core::suggest::{Agent, Reads};

/// How a known CLI takes a model and an effort, and the flags that make it start fast: the one
/// table Settings and the commit box read too.
pub use gitviber_ui::settings::{AGENT_PRESETS as PRESETS, AgentPreset as Preset};

pub type Models = BTreeMap<String, String>;

pub fn preset_of(command: &str) -> Option<&'static Preset> {
    Preset::of(command)
}

/// The model a preset runs with: the user's id, or the default until they type one.
pub fn model_of(p: &Preset, models: &Models) -> String {
    p.model_in(models)
}

/// The effort a preset runs with: the user's, or its default until they pick one.
pub fn effort_of(p: &Preset, efforts: &Models) -> String {
    p.effort_in(efforts)
}

/// The command as run: a preset's with its lean (`reads`: a guided review's), model and effort
/// flags; a custom one carries its own.
pub fn command_line(
    command: &str,
    models: &Models,
    efforts: &Models,
    reads: bool,
    lean: bool,
) -> String {
    gitviber_ui::settings::command_line(command, models, efforts, reads, lean)
}

/// What a guided review runs: its own command, models and efforts, or Commit Messages' where it
/// has none.
#[derive(Clone, Debug, PartialEq)]
pub struct ReviewAgent {
    pub command: String,
    pub models: Models,
    pub efforts: Models,
    pub language: String,
}

impl ReviewAgent {
    pub fn from_settings(s: &gitviber_ui::Settings) -> Self {
        let mut models = s.suggest_models.clone();
        models.extend(s.review_models.clone());
        let mut efforts = s.suggest_efforts.clone();
        efforts.extend(s.review_efforts.clone());
        ReviewAgent {
            command: s
                .review_command
                .clone()
                .unwrap_or_else(|| s.suggest_command.clone()),
            models,
            efforts,
            language: s.review_language.clone(),
        }
    }

    /// The line a run starts with; `lean` false for a Claude Code too old for the lean flags.
    pub fn line(&self, lean: bool) -> String {
        command_line(&self.command, &self.models, &self.efforts, true, lean)
    }

    pub fn program(&self) -> String {
        program_of(&self.command)
    }

    pub fn is_claude_preset(&self) -> bool {
        preset_of(&self.command).is_some_and(|p| p.key == "claude")
    }

    /// Claude Code checks its answer against the run's schema; it and opencode are let read the
    /// patch file. Others get the shape in the prompt: codex exec's --output-schema takes a file
    /// and OpenAI's strict mode, which wants every field required, so it isn't used.
    pub fn agent(&self, lean: bool, schema: &serde_json::Value) -> Agent {
        match preset_of(&self.command).map(|p| p.key) {
            // --json-schema is as new as the lean flags: an older Claude Code goes without both.
            Some("claude") => Agent {
                args: if lean {
                    vec!["--json-schema".into(), schema.to_string()]
                } else {
                    vec![]
                },
                reads: Some(Reads::Claude),
            },
            Some("opencode") => Agent {
                args: vec![],
                reads: Some(Reads::Opencode),
            },
            _ => Agent::default(),
        }
    }
}

/// A Claude Code too old for a flag says so as commander does.
pub fn is_unknown_option(error: &str) -> bool {
    error.contains("unknown option '--")
}

/// The program a template runs, by name ("claude" from "~/.local/bin/claude -p").
pub fn program_of(command: &str) -> String {
    command
        .split_whitespace()
        .next()
        .and_then(|w| w.rsplit('/').next())
        .unwrap_or_default()
        .to_string()
}

/// The agent CLI's name for the user: Claude Code, or the program the command runs.
pub fn agent_name(command: &str) -> String {
    let program = program_of(command);
    if program.eq_ignore_ascii_case("claude") {
        "Claude Code".into()
    } else if program.is_empty() {
        "your agent".into()
    } else {
        program
    }
}

/// The model and effort a command line names, for "written by claude (x, medium)".
pub fn run_details(line: &str) -> Vec<String> {
    let words: Vec<&str> = line.split_whitespace().collect();
    let value = |flags: &[&str]| -> String {
        for (i, w) in words.iter().enumerate() {
            if flags.contains(w)
                && let Some(next) = words.get(i + 1)
                && !next.starts_with('-')
            {
                return next.to_string();
            }
            if let Some(f) = flags
                .iter()
                .find(|f| f.starts_with("--") && w.starts_with(&format!("{f}=")))
            {
                return w[f.len() + 1..].to_string();
            }
            if let Some(level) = w.strip_prefix("model_reasoning_effort=")
                && flags.contains(&"--effort")
            {
                return level.to_string();
            }
        }
        String::new()
    };
    [
        value(&["--model", "-m"]),
        value(&["--effort", "--thinking", "--variant"]),
    ]
    .into_iter()
    .filter(|v| !v.is_empty())
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(pairs: &[(&str, &str)]) -> Models {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn a_reviews_command_line() {
        let none = Models::new();
        assert_eq!(
            command_line("claude -p", &none, &none, true, true),
            "claude -p --strict-mcp-config --no-session-persistence --disable-slash-commands --tools Read,Grep,Glob --model sonnet --effort medium"
        );
        assert_eq!(
            command_line(
                "claude -p",
                &m(&[("claude", "opus")]),
                &m(&[("claude", "high")]),
                true,
                false
            ),
            "claude -p --model opus --effort high"
        );
        assert_eq!(
            command_line("codex exec", &none, &m(&[("codex", "high")]), true, true),
            "codex exec -m gpt-6-luna -c model_reasoning_effort=high"
        );
        assert_eq!(
            command_line("opencode run --agent plan", &none, &none, true, true),
            "opencode run --agent plan -m anthropic/claude-sonnet-5"
        );
        assert_eq!(
            command_line("/opt/claude -p", &none, &none, true, true),
            "/opt/claude -p"
        );
    }

    #[test]
    fn details_of_a_line() {
        assert_eq!(
            run_details("claude -p --model opus --effort high"),
            ["opus", "high"]
        );
        assert_eq!(
            run_details("codex exec -m x -c model_reasoning_effort=low"),
            ["x", "low"]
        );
        assert_eq!(run_details("claude --model=haiku"), ["haiku"]);
        assert!(run_details("my-agent").is_empty());
    }

    #[test]
    fn names_and_programs() {
        assert_eq!(program_of(" ~/.local/bin/claude -p"), "claude");
        assert_eq!(agent_name("claude -p"), "Claude Code");
        assert_eq!(agent_name("codex exec"), "codex");
        assert_eq!(agent_name(""), "your agent");
        assert!(is_unknown_option(
            "error: unknown option '--strict-mcp-config'"
        ));
    }

    #[test]
    fn the_review_agent_falls_back_to_commit_messages() {
        let mut s = gitviber_ui::Settings {
            suggest_command: "codex exec".into(),
            suggest_models: m(&[("codex", "a"), ("claude", "b")]),
            review_models: m(&[("claude", "c")]),
            ..Default::default()
        };
        let a = ReviewAgent::from_settings(&s);
        assert_eq!(a.command, "codex exec");
        assert_eq!(a.models, m(&[("codex", "a"), ("claude", "c")]));
        s.review_command = Some("claude -p".into());
        let a = ReviewAgent::from_settings(&s);
        assert!(a.is_claude_preset());
        let schema = serde_json::json!({"type": "object"});
        assert_eq!(a.agent(true, &schema).args[0], "--json-schema");
        assert!(a.agent(false, &schema).args.is_empty());
    }
}
