//! Commit messages, pull request descriptions and guided reviews from the user's own agent CLI
//! (`claude -p`, `codex exec`, …).
//! GitViber itself sends nothing anywhere: it runs the command the user picked, in the repo,
//! with the prompt and the diff on stdin. Where that goes is up to the command.

use crate::{git, process};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// Enough for a focused change; past it the model gets the file list and the start of the diff.
pub const MAX_DIFF: usize = 100 * 1024;
const TIMEOUT: Duration = Duration::from_secs(90);
/// A guide is a much longer answer than a commit message: a section per part of the change.
const GUIDE_TIMEOUT: Duration = Duration::from_secs(300);
/// A pull request's commit subjects past this many go: the newest are kept, and room for the diff.
const MAX_SUBJECTS: usize = 200;
/// How every diff sent is written: a file list first, then the patch.
const DIFF_OPTS: [&str; 4] = ["--patch-with-stat", "--no-color", "--no-ext-diff", "-M"];
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

/// Which suggestion a run is for: the commit box's, the pull request dialog's and a guided
/// review's run side by side.
#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Message,
    Pull,
    Guide,
}

/// The run in progress of each kind, so Cancel (or starting another of that kind) can stop it.
#[derive(Default)]
pub struct Suggester {
    message: Mutex<Option<Arc<AtomicBool>>>,
    pull: Mutex<Option<Arc<AtomicBool>>>,
    guide: Mutex<Option<Arc<AtomicBool>>>,
}

impl Suggester {
    fn slot(&self, kind: Kind) -> MutexGuard<'_, Option<Arc<AtomicBool>>> {
        match kind {
            Kind::Message => &self.message,
            Kind::Pull => &self.pull,
            Kind::Guide => &self.guide,
        }
        .lock()
        .unwrap()
    }

    pub fn start(&self, kind: Kind) -> Arc<AtomicBool> {
        let flag = Arc::new(AtomicBool::new(false));
        if let Some(old) = self.slot(kind).replace(flag.clone()) {
            old.store(true, Ordering::Relaxed);
        }
        flag
    }

    pub fn finish(&self, kind: Kind, flag: &Arc<AtomicBool>) {
        let mut cur = self.slot(kind);
        if cur.as_ref().is_some_and(|c| Arc::ptr_eq(c, flag)) {
            *cur = None;
        }
    }

    pub fn cancel(&self, kind: Kind) {
        if let Some(flag) = self.slot(kind).take() {
            flag.store(true, Ordering::Relaxed);
        }
    }
}

/// The diff to describe, cut to MAX_DIFF at a line end; true when it was cut.
fn diff(repo: &Path, scope: Scope) -> Result<(String, bool), String> {
    let run = |args: &[&str]| git::run_text(repo, &[&["diff"], args, &DIFF_OPTS].concat());
    let text = match scope {
        Scope::Staged => run(&["--cached"])?,
        Scope::Amend => {
            let base = git::parent_or_empty(repo, "HEAD")?;
            run(&["--cached", &base])?
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
                    &DIFF_OPTS[1..],
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
    Ok(cut(text))
}

/// `text` cut to MAX_DIFF at a line end; true when it was cut.
fn cut(mut text: String) -> (String, bool) {
    if text.len() <= MAX_DIFF {
        return (text, false);
    }
    let mut end = MAX_DIFF;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    end = text[..end].rfind('\n').map_or(end, |i| i + 1);
    text.truncate(end);
    (text, true)
}

/// What a pull request from HEAD into `base` (a remote-tracking branch) brings: see
/// `range_input`, with the repository's PR template if it has one.
fn pull_input(repo: &Path, base: &str) -> Result<(String, bool), String> {
    git::check_pull_base(repo, base)?;
    let template = pull_template(repo, base).map(|t| {
        format!(
            "The repository's pull request template; follow its sections:\n{}\n\n",
            t.trim_end()
        )
    });
    range_input(repo, base, "HEAD", template.as_deref())
}

/// What `to` brings over `from`: its commits' subjects, `extra`, then the diff from where it
/// left `from`. Cut to MAX_DIFF like a commit's diff; true when it was.
fn range_input(
    repo: &Path,
    from: &str,
    to: &str,
    extra: Option<&str>,
) -> Result<(String, bool), String> {
    let range = format!("{from}..{to}");
    // One past the cap, to tell a branch of exactly MAX_SUBJECTS from a longer one.
    let n = format!("-n{}", MAX_SUBJECTS + 1);
    let subjects = git::run_text(
        repo,
        &["log", "--reverse", &n, "--format=- %s", &range, "--"],
    )?;
    let mut subjects: Vec<&str> = subjects.lines().collect();
    if subjects.is_empty() {
        return Err("This branch has no commits the base doesn't have.".into());
    }
    let heading = if subjects.len() > MAX_SUBJECTS {
        subjects.remove(0);
        format!("The newest {MAX_SUBJECTS} commits, oldest first:")
    } else {
        "Commits, oldest first:".to_string()
    };
    let mut text = format!(
        "{heading}\n{}\n\n{}",
        subjects.join("\n"),
        extra.unwrap_or("")
    );
    let merged = format!("{from}...{to}");
    text.push_str(&git::run_text(
        repo,
        &[&["diff"], &DIFF_OPTS[..], &[&merged, "--"]].concat(),
    )?);
    Ok(cut(text))
}

/// What a guided review is of: a commit, or HEAD's branch since it left `base` (a full ref).
#[derive(Deserialize)]
#[serde(tag = "of", rename_all = "lowercase")]
pub enum Target {
    Commit { sha: String },
    Branch { base: String },
}

/// A guided review as the command wrote it, and the range it read: `base..head`, a commit's
/// parent (or the empty tree) and the commit, or the merge base and HEAD.
#[derive(Serialize, Debug)]
pub struct Guided {
    pub text: String,
    pub base: String,
    pub head: String,
}

/// What `target` gets: a commit's message and diff, or the branch's commits and diff (only
/// what's committed, so the range read names it exactly).
fn guide_input(repo: &Path, target: &Target) -> Result<(String, String, (String, bool)), String> {
    match target {
        Target::Commit { sha } => {
            git::validate_rev(sha)?;
            let base = git::parent_or_empty(repo, sha)?;
            let message = git::run_text(repo, &["log", "-1", "--format=%B", sha, "--"])?;
            let diff = git::run_text(
                repo,
                &[&["diff"], &DIFF_OPTS[..], &[&base, sha, "--"]].concat(),
            )?;
            let text = format!("The commit's message:\n{}\n\n{diff}", message.trim_end());
            Ok((base, sha.clone(), cut(text)))
        }
        Target::Branch { base } => {
            let from = git::parted_at(repo, base)?;
            let head = git::run_text(repo, &["rev-parse", "HEAD"])?
                .trim()
                .to_string();
            let input = range_input(repo, &from, &head, None)?;
            Ok((from, head, input))
        }
    }
}

/// The PR template GitHub would use from `base`: in .github/, the root or docs/, any case.
fn pull_template(repo: &Path, base: &str) -> Option<String> {
    let names = git::run_text(
        repo,
        &[
            "ls-tree",
            "--name-only",
            base,
            "--",
            ".",
            ".github/",
            "docs/",
        ],
    )
    .ok()?;
    let name = [".github/", "", "docs/"].iter().find_map(|dir| {
        names
            .lines()
            .find(|n| n.eq_ignore_ascii_case(&format!("{dir}pull_request_template.md")))
    })?;
    git::run_text(repo, &["cat-file", "blob", &format!("{base}:{name}")]).ok()
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

/// A commit message for what `scope` would commit: see `ask`.
pub fn run(
    repo: &Path,
    template: &str,
    prompt: &str,
    scope: Scope,
    cancel: &AtomicBool,
) -> Result<String, String> {
    let (diff, cut) = diff(repo, scope)?;
    let note = cut.then_some("the file list at its top is complete");
    ask(repo, template, prompt, &diff, note, TIMEOUT, cancel)
}

/// A pull request's title and description, for HEAD into `base`: see `ask`.
pub fn run_pull(
    repo: &Path,
    template: &str,
    prompt: &str,
    base: &str,
    cancel: &AtomicBool,
) -> Result<String, String> {
    let (input, cut) = pull_input(repo, base)?;
    let note = cut.then_some("the commits and the diff's file list come before it, in full");
    ask(repo, template, prompt, &input, note, TIMEOUT, cancel)
}

/// A guided review of `target`: see `ask`.
pub fn run_guide(
    repo: &Path,
    template: &str,
    prompt: &str,
    target: &Target,
    cancel: &AtomicBool,
) -> Result<Guided, String> {
    let (base, head, (input, cut)) = guide_input(repo, target)?;
    let note = cut.then_some("what comes before the diff and its file list are in full");
    let text = ask(repo, template, prompt, &input, note, GUIDE_TIMEOUT, cancel)?;
    Ok(Guided { text, base, head })
}

/// Runs the template with the prompt and the diff, returning what it printed; `cut`: the diff
/// was cut, and what of it is whole. Stops on `cancel` or after `timeout`, killing the command
/// and anything it started.
fn ask(
    repo: &Path,
    template: &str,
    prompt: &str,
    diff: &str,
    cut: Option<&str>,
    timeout: Duration,
    cancel: &AtomicBool,
) -> Result<String, String> {
    let prompt = match cut {
        Some(whole) => format!(
            "{prompt}\n\nThe diff was cut off at {} KB; {whole}.",
            MAX_DIFF / 1024
        ),
        None => prompt.to_string(),
    };
    let (argv, input) = prepare(template, &prompt, diff)?;
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
    let mut child = process::spawn(&mut cmd).map_err(|e| match e.kind() {
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

    let deadline = Instant::now() + timeout;
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
                    timeout.as_secs()
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
    fn each_kind_starts_and_cancels_its_own_run() {
        let s = Suggester::default();
        let message = s.start(Kind::Message);
        let pull = s.start(Kind::Pull);
        let guide = s.start(Kind::Guide);
        assert!(!message.load(Ordering::Relaxed));
        s.cancel(Kind::Pull);
        assert!(pull.load(Ordering::Relaxed));
        assert!(!message.load(Ordering::Relaxed) && !guide.load(Ordering::Relaxed));
        // A second of the same kind stops the first.
        s.start(Kind::Message);
        assert!(message.load(Ordering::Relaxed));
    }

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
