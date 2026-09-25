//! Commit message suggestions from the user's own agent CLI (`claude -p`, `codex exec`, …).
//! GitViber itself sends nothing anywhere: it runs the command the user picked, in the repo,
//! with the prompt and the diff on stdin. Where that goes is up to the command.

use crate::{git, process};
use serde::Deserialize;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Enough for a focused change; past it the model gets the file list and the start of the diff.
pub const MAX_DIFF: usize = 100 * 1024;
const TIMEOUT: Duration = Duration::from_secs(90);
pub const CANCELLED: &str = "cancelled";

/// What the commit would contain, as the commit button decides it.
#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    Staged,
    /// Nothing staged: "Commit all" stages every change, untracked files too.
    All,
    /// HEAD's own changes plus whatever is staged, from HEAD's parent.
    Amend,
}

/// The run in progress, so Cancel (or starting another) can stop it.
#[derive(Default)]
pub struct Suggester {
    current: Mutex<Option<Arc<AtomicBool>>>,
}

impl Suggester {
    pub fn start(&self) -> Arc<AtomicBool> {
        let flag = Arc::new(AtomicBool::new(false));
        if let Some(old) = self.current.lock().unwrap().replace(flag.clone()) {
            old.store(true, Ordering::Relaxed);
        }
        flag
    }

    pub fn finish(&self, flag: &Arc<AtomicBool>) {
        let mut cur = self.current.lock().unwrap();
        if cur.as_ref().is_some_and(|c| Arc::ptr_eq(c, flag)) {
            *cur = None;
        }
    }

    pub fn cancel(&self) {
        if let Some(flag) = self.current.lock().unwrap().take() {
            flag.store(true, Ordering::Relaxed);
        }
    }
}

/// The diff to describe, cut to MAX_DIFF at a line end; true when it was cut.
fn diff(repo: &Path, scope: Scope) -> Result<(String, bool), String> {
    let opts = ["--patch-with-stat", "--no-color", "--no-ext-diff", "-M"];
    let run = |args: &[&str]| git::run_text(repo, &[&["diff"], args, &opts].concat());
    let mut text = match scope {
        Scope::Staged => run(&["--cached"])?,
        Scope::Amend => {
            // A root commit has no parent: diff from the empty tree.
            let base = match git::run_text(repo, &["rev-parse", "--verify", "-q", "HEAD^"]) {
                Ok(p) => p,
                Err(_) => git::run_text(repo, &["hash-object", "-t", "tree", "/dev/null"])?,
            };
            run(&["--cached", base.trim()])?
        }
        Scope::All => {
            let mut text = run(&[])?;
            // `git diff` leaves out untracked files, which "Commit all" takes too.
            let list = git::run(repo, &["ls-files", "--others", "--exclude-standard", "-z"])?;
            let list = String::from_utf8_lossy(&list);
            // A trailing slash is a nested repository, which is never staged.
            for path in list
                .split('\0')
                .filter(|p| !p.is_empty() && !p.ends_with('/'))
            {
                if text.len() > MAX_DIFF {
                    break;
                }
                let args = [
                    &["diff", "--no-index"],
                    &opts[1..],
                    &["--", "/dev/null", path],
                ]
                .concat();
                let out = git::run_with(repo, &args, &[1], None)?;
                text.push_str(&String::from_utf8_lossy(&out));
            }
            text
        }
    };
    if text.trim().is_empty() {
        return Err("There are no changes to describe.".into());
    }
    if text.len() <= MAX_DIFF {
        return Ok((text, false));
    }
    let mut end = MAX_DIFF;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    end = text[..end].rfind('\n').map_or(end, |i| i + 1);
    text.truncate(end);
    Ok((text, true))
}

/// What goes to the command: the prompt as `{prompt}` in its arguments, or ahead of the diff
/// on stdin when the template has no `{prompt}` (`codex exec` reads stdin only without one).
fn prepare(template: &str, prompt: &str, diff: &str) -> Result<(Vec<String>, String), String> {
    let mut argv = process::split_command(template)?;
    if argv.is_empty() {
        return Err("No command is set. Pick one in Settings → Commit Messages.".into());
    }
    if argv.iter().any(|a| a.contains("{prompt}")) {
        for a in &mut argv {
            *a = a.replace("{prompt}", prompt);
        }
        Ok((argv, diff.to_string()))
    } else {
        Ok((argv, format!("{prompt}\n\n{diff}")))
    }
}

/// `~/bin/claude` as typed in Settings; no shell expands it for us.
fn expand_home(program: &str) -> String {
    match (program.strip_prefix("~/"), std::env::var("HOME")) {
        (Some(rest), Ok(home)) => format!("{home}/{rest}"),
        _ => program.to_string(),
    }
}

/// Runs the template with the prompt and the diff, returning what it printed. Stops on
/// `cancel` or after TIMEOUT, killing the command and anything it started.
pub fn run(
    repo: &Path,
    template: &str,
    prompt: &str,
    scope: Scope,
    cancel: &AtomicBool,
) -> Result<String, String> {
    let (diff, cut) = diff(repo, scope)?;
    let prompt = if cut {
        format!(
            "{prompt}\n\nThe diff was cut off at {} KB; the file list at its top is complete.",
            MAX_DIFF / 1024
        )
    } else {
        prompt.to_string()
    };
    let (argv, input) = prepare(template, &prompt, &diff)?;
    let program = expand_home(&argv[0]);
    let mut cmd = Command::new(&program);
    cmd.args(&argv[1..])
        .current_dir(repo)
        // The same PATH git runs with; a Finder-launched app's own is bare.
        .env("PATH", process::search_path())
        // Set when GitViber was started from a Claude Code terminal; `claude` then refuses to
        // run, taking itself for a nested session.
        .env_remove("CLAUDECODE")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // Its own process group, so a cancel also stops what an agent CLI spawned (MCP servers,
    // tools) and nothing is left holding the output pipes open.
    process::in_own_group(&mut cmd);
    let mut child = cmd.spawn().map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => format!(
            "Couldn't find \"{}\". Put its full path in Settings → Commit Messages (`which {}` in Terminal shows it).",
            argv[0],
            argv[0].rsplit('/').next().unwrap_or(&argv[0])
        ),
        _ => format!("Couldn't run \"{}\": {e}", argv[0]),
    })?;

    // Written on a thread: a command that prints before it reads would block on a full pipe.
    // One that never reads stdin just gets a broken pipe, which is fine.
    if let Some(mut stdin) = child.stdin.take() {
        std::thread::spawn(move || {
            let _ = stdin.write_all(input.as_bytes());
        });
    }
    let drain = |r: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(mut r) = r {
                let _ = r.read_to_end(&mut buf);
            }
            buf
        })
    };
    let out = drain(
        child
            .stdout
            .take()
            .map(|s| Box::new(s) as Box<dyn Read + Send>),
    );
    let err = drain(
        child
            .stderr
            .take()
            .map(|s| Box::new(s) as Box<dyn Read + Send>),
    );

    let deadline = Instant::now() + TIMEOUT;
    let status = loop {
        if let Some(st) = child.try_wait().map_err(|e| e.to_string())? {
            break st;
        }
        let cancelled = cancel.load(Ordering::Relaxed);
        if cancelled || Instant::now() > deadline {
            process::kill_group(&mut child, Duration::ZERO);
            // The output threads aren't joined: a straggler holding the pipes would hang us.
            return Err(if cancelled {
                CANCELLED.into()
            } else {
                format!(
                    "\"{}\" gave no answer within {} seconds.",
                    argv[0],
                    TIMEOUT.as_secs()
                )
            });
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let stdout = String::from_utf8_lossy(&out.join().unwrap_or_default()).into_owned();
    let stderr = String::from_utf8_lossy(&err.join().unwrap_or_default()).into_owned();
    if status.success() {
        return Ok(stdout);
    }
    // Agent CLIs explain a missing login or an unknown flag on stderr; the end is what matters.
    let why = Some(stderr.trim())
        .filter(|e| !e.is_empty())
        .unwrap_or(stdout.trim());
    let tail: Vec<&str> = why.lines().rev().take(8).collect();
    let tail = tail.into_iter().rev().collect::<Vec<_>>().join("\n");
    let code = status
        .code()
        .map_or("a signal".to_string(), |c| format!("code {c}"));
    Err(if tail.is_empty() {
        format!("\"{}\" failed with {code}.", argv[0])
    } else {
        format!("\"{}\" failed with {code}:\n{tail}", argv[0])
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_bad_templates() {
        assert!(prepare("", "p", "d").is_err());
        assert!(prepare("   ", "p", "d").is_err());
        assert!(prepare("claude -p 'oops", "p", "d").is_err());
    }

    #[test]
    fn prompt_goes_where_the_template_says() {
        let (argv, input) = prepare("claude -p", "Write it.", "DIFF").unwrap();
        assert_eq!(argv, ["claude", "-p"]);
        assert_eq!(input, "Write it.\n\nDIFF");
        let (argv, input) =
            prepare("llm -s '{prompt} Be terse.' {prompt}", "Write it.", "DIFF").unwrap();
        assert_eq!(argv, ["llm", "-s", "Write it. Be terse.", "Write it."]);
        assert_eq!(input, "DIFF");
    }
}
