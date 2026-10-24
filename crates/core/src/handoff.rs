//! A guided review handed to the user's Claude Code in a terminal (askAgent.ts): its text, as the
//! user would paste it, goes in a private file, beside a script that starts Claude Code with that
//! text in its prompt box (sent, for a risk). Only the script's path is typed at the shell's
//! prompt: no shell reads a word of what a model wrote, and none of it lands in the shell's history.

use crate::agents::quote;
use crate::scratch::{write_private, ScratchDir};
use std::path::Path;
use std::time::Duration;

/// The script reads the text as Claude Code starts: a day covers a slow start without leaving
/// files around.
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
    /// The review's part, and what's asked.
    pub text: &'a str,
    /// Sent as the first message (a risk to look into), rather than left in the prompt box.
    pub send: bool,
}

/// Writes the text and its script, and returns the line that runs it.
pub fn command(h: &Handoff) -> Result<String, String> {
    let (dir, path) = ScratchDir::fresh("handoff", KEEP)?;
    let text = path.join("prompt.md");
    let start = path.join("start.sh");
    let fail = |e: std::io::Error| format!("Couldn't write the review for the agent: {e}");
    write_private(&text, h.text).map_err(fail)?;
    write_private(&start, &script(h, &text)).map_err(fail)?;
    dir.keep();
    // Run by sh whatever the user's shell is: fish reads `$(…)` and quotes its own way.
    Ok(format!("sh {}", quote(&start.to_string_lossy())))
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

/// One line of at most `max` characters, nothing a terminal would act on.
fn one_line(given: &str, max: usize) -> String {
    let flat: String = given
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let words = flat.split_whitespace().collect::<Vec<_>>().join(" ");
    words.chars().take(max).collect()
}

/// `exec`, so the pane's process is Claude Code itself, as agents.rs finds and resumes it.
fn script(h: &Handoff, text: &Path) -> String {
    let mut words = vec![program(h.command), "-n".into(), one_line(h.name, MAX_NAME)];
    for (flag, value) in [("--model", h.model), ("--effort", h.effort)] {
        if let Some(v) = value.map(str::trim).filter(|v| !v.is_empty()) {
            words.extend([flag.into(), v.into()]);
        }
    }
    let words = words.iter().map(|w| quote(w)).collect::<Vec<_>>().join(" ");
    // The file as one argument, its last line breaks dropped.
    let file = quote(&text.to_string_lossy());
    if h.send {
        // After `--`: never read as a flag.
        format!("exec {words} -- \"$(cat {file})\"\n")
    } else {
        // Claude Code's own (unlisted) flag for its deep links: in the prompt box, unsent. The
        // space after puts the cursor past it, for the question.
        format!("exec {words} --prefill \"$(cat {file}) \"\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn handoff(command: &str, send: bool) -> Handoff<'_> {
        Handoff {
            command,
            model: Some("opus[1m]"),
            effort: Some(" "),
            name: "Review: PR #42 · it's\nnew",
            text: "the review\n\nMy question follows.",
            send,
        }
    }

    #[test]
    fn the_script_quotes_every_word_and_leaves_out_what_isnt_set() {
        let file = Path::new("/tmp/x y/prompt.md");
        assert_eq!(
            script(&handoff("'/opt/my tools/claude' -p --model x", true), file),
            "exec '/opt/my tools/claude' -n 'Review: PR #42 · it'\\''s new' --model 'opus[1m]' -- \"$(cat '/tmp/x y/prompt.md')\"\n"
        );
        // Another CLI's program starts Claude Code; unsent, the text waits in its prompt box.
        let h = Handoff {
            model: None,
            ..handoff("codex exec", false)
        };
        assert_eq!(
            script(&h, file),
            "exec claude -n 'Review: PR #42 · it'\\''s new' --prefill \"$(cat '/tmp/x y/prompt.md') \"\n"
        );
    }

    #[test]
    fn a_home_path_is_expanded_and_a_name_is_one_short_line() {
        let home = std::env::var("HOME").unwrap();
        assert_eq!(program("~/bin/claude -p"), format!("{home}/bin/claude"));
        assert_eq!(program(" claude "), "claude");
        assert_eq!(program(""), "claude");
        assert_eq!(
            one_line(&format!("a\tb\x1b{}", "x".repeat(80)), MAX_NAME)
                .chars()
                .count(),
            MAX_NAME
        );
        assert!(one_line("a\tb\x1bc", MAX_NAME).starts_with("a b c"));
    }

    /// The script, run by sh with a stand-in for Claude Code, hands it the text whole: quotes,
    /// `$`, backticks and line breaks reach it as written, and none of them runs.
    #[cfg(unix)]
    #[test]
    fn the_text_reaches_the_agent_as_written_and_stays_private() {
        use std::os::unix::fs::PermissionsExt;
        let bin = ScratchDir::new("handoff-test").unwrap();
        let fake = bin.path().join("claude");
        let out = bin.path().join("args");
        std::fs::write(
            &fake,
            format!(
                "#!/bin/sh\nfor a in \"$@\"; do printf '%s\\0' \"$a\"; done > '{}'\n",
                out.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        let text = "it's `whoami` and $HOME\n\"quoted\"\n";
        let line = command(&Handoff {
            command: &fake.to_string_lossy(),
            text,
            ..handoff("", false)
        })
        .unwrap();
        let status = std::process::Command::new("sh")
            .args(["-c", &line])
            .status()
            .unwrap();
        assert!(status.success());
        let args = std::fs::read_to_string(&out).unwrap();
        let args: Vec<&str> = args.split('\0').filter(|a| !a.is_empty()).collect();
        assert_eq!(
            args,
            [
                "-n",
                "Review: PR #42 · it's new",
                "--model",
                "opus[1m]",
                "--prefill",
                "it's `whoami` and $HOME\n\"quoted\" "
            ]
        );
        let start = Path::new(line.trim_start_matches("sh ").trim_matches('\''));
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(start), 0o600);
        assert_eq!(mode(start.parent().unwrap()), 0o700);
        std::fs::remove_dir_all(start.parent().unwrap()).unwrap();
    }
}
