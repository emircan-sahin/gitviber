//! A guided review handed to the user's Claude Code in a terminal (guides.ts askAgent): the
//! review's context goes in a private file the agent reads as its system prompt, and only the line
//! typed at the shell's prompt goes through the shell. The context never does, so no shell reads
//! a word of what a model wrote.

use crate::agents::quote;
use crate::scratch::{write_private, ScratchDir};
use std::path::Path;
use std::time::Duration;

/// Claude Code reads the file as it starts and keeps the prompt with the conversation (its resume
/// drops the flag, agents.json): a day covers a slow start without leaving files around.
const KEEP: Duration = Duration::from_secs(24 * 3600);
const MAX_NAME: usize = 60;

/// What starts the session: the review's program, model and effort when they're set.
pub struct Handoff<'a> {
    /// The Guided Review command (`claude -p`, maybe with a path): its program starts the session
    /// when it's Claude Code's, else plain `claude` does.
    pub command: &'a str,
    pub model: Option<&'a str>,
    pub effort: Option<&'a str>,
    /// The session's name in Claude Code.
    pub name: &'a str,
    pub context: &'a str,
    /// What the agent starts on (a risk to look into); none leaves the first word to the user.
    pub prompt: Option<&'a str>,
}

/// Writes the context to its file and returns the line that starts Claude Code with it.
pub fn command(h: &Handoff) -> Result<String, String> {
    let (dir, path) = ScratchDir::fresh("handoff", KEEP)?;
    let file = path.join("context.md");
    write_private(&file, h.context)
        .map_err(|e| format!("Couldn't write the review for the agent: {e}"))?;
    dir.keep();
    Ok(line(h, &file))
}

/// The command's program as typed: `~/` expanded, which no quoted word gets; `claude` when it's
/// another CLI's.
fn program(command: &str) -> String {
    let first = crate::process::split_command(command)
        .ok()
        .and_then(|argv| argv.into_iter().next())
        .unwrap_or_default();
    let path = crate::suggest::expand_home(&first);
    let claude = Path::new(&path)
        .file_name()
        .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case("claude"));
    if claude {
        path
    } else {
        "claude".into()
    }
}

/// One line of at most MAX_NAME characters, nothing a terminal would act on.
fn name(given: &str) -> String {
    let flat: String = given
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let words = flat.split_whitespace().collect::<Vec<_>>().join(" ");
    words.chars().take(MAX_NAME).collect()
}

fn line(h: &Handoff, file: &Path) -> String {
    let mut words = vec![
        program(h.command),
        "-n".into(),
        name(h.name),
        "--append-system-prompt-file".into(),
        file.to_string_lossy().into_owned(),
    ];
    for (flag, value) in [("--model", h.model), ("--effort", h.effort)] {
        if let Some(v) = value.map(str::trim).filter(|v| !v.is_empty()) {
            words.extend([flag.into(), v.into()]);
        }
    }
    if let Some(p) = h.prompt.map(str::trim).filter(|p| !p.is_empty()) {
        // After `--`: a prompt is never read as a flag.
        words.extend(["--".into(), p.into()]);
    }
    words.iter().map(|w| quote(w)).collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn handoff<'a>(command: &'a str, prompt: Option<&'a str>) -> Handoff<'a> {
        Handoff {
            command,
            model: Some("opus[1m]"),
            effort: Some(" "),
            name: "Review: PR #42 · it's\nnew",
            context: "the review",
            prompt,
        }
    }

    #[test]
    fn the_line_quotes_every_word_and_leaves_out_what_isnt_set() {
        let h = handoff("'/opt/my tools/claude' -p --model x", Some("Look into it"));
        assert_eq!(
            line(&h, Path::new("/tmp/x/context.md")),
            r"'/opt/my tools/claude' -n 'Review: PR #42 · it'\''s new' --append-system-prompt-file /tmp/x/context.md --model 'opus[1m]' -- 'Look into it'"
        );
        // Another CLI's program starts Claude Code; no prompt leaves the first word to the user.
        let h = Handoff {
            model: None,
            ..handoff("codex exec", None)
        };
        assert_eq!(
            line(&h, Path::new("/tmp/x/context.md")),
            "claude -n 'Review: PR #42 · it'\\''s new' --append-system-prompt-file /tmp/x/context.md"
        );
    }

    #[test]
    fn a_home_path_is_expanded_and_a_name_is_one_short_line() {
        let home = std::env::var("HOME").unwrap();
        assert_eq!(program("~/bin/claude -p"), format!("{home}/bin/claude"));
        assert_eq!(program(" claude "), "claude");
        assert_eq!(program(""), "claude");
        assert_eq!(
            name(&format!("a\tb\x1b{}", "x".repeat(80))).chars().count(),
            MAX_NAME
        );
        assert!(name("a\tb\x1bc").starts_with("a b c"));
    }

    #[cfg(unix)]
    #[test]
    fn the_context_is_written_where_only_the_user_can_read_it() {
        use std::os::unix::fs::PermissionsExt;
        let out = command(&handoff("claude -p", None)).unwrap();
        let path = out
            .split(" --append-system-prompt-file ")
            .nth(1)
            .and_then(|rest| rest.split(' ').next())
            .unwrap();
        let file = Path::new(path);
        assert_eq!(std::fs::read_to_string(file).unwrap(), "the review");
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(file), 0o600);
        assert_eq!(mode(file.parent().unwrap()), 0o700);
        std::fs::remove_dir_all(file.parent().unwrap()).unwrap();
    }
}
