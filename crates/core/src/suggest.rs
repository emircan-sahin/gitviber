//! Commit messages, pull request descriptions and guided reviews from the user's own agent CLI
//! (`claude -p`, `codex exec`, …).
//! GitViber itself sends nothing anywhere: it runs the command the user picked, in the repo,
//! with the prompt and the diff on stdin. Where that goes is up to the command.

mod manifest;

use crate::scratch::ScratchDir;
use crate::{git, process};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// Enough for a focused change; past it the model gets the file list and the start of the diff.
pub const MAX_DIFF: usize = 100 * 1024;
const TIMEOUT: Duration = Duration::from_secs(90);
/// A guide is a much longer answer than a commit message: a section per part of the change.
/// An agent reading a big change's patch file takes longer: a second more per 4 KB of it, up to
/// MAX_GUIDE_TIMEOUT.
const GUIDE_TIMEOUT: Duration = Duration::from_secs(300);
const MAX_GUIDE_TIMEOUT: Duration = Duration::from_secs(600);
/// What a guide's prompt gets of the change, its file list included: about 100k tokens, room
/// in any current model's context for the answer and for what an agent reads besides.
pub const MAX_GUIDE_INPUT: usize = 400 * 1024;
/// A pull request's commit subjects past this many go: the newest are kept, and room for the diff.
const MAX_SUBJECTS: usize = 200;
/// The file list kept whole past MAX_DIFF, when it alone is: about 3,000 files.
const MAX_LIST: usize = 2 * MAX_DIFF;
/// No answer asked for comes near this; past it the command is reading something back to us.
const MAX_OUTPUT: usize = 1024 * 1024;
/// How every diff sent is written: no colors or external tools, renames found.
const DIFF_OPTS: [&str; 3] = ["--no-color", "--no-ext-diff", "-M"];
/// A file list ahead of the patch, with whole paths: git shortens them to fit 80 columns.
const WITH_STAT: [&str; 3] = ["--patch-with-stat", "--stat=1000", "--stat-graph-width=20"];
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

/// Which suggestion a run is for: the commit box's, the pull request dialog's, a guided review's
/// and its risks' run side by side.
#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Message,
    Pull,
    Guide,
    Risks,
}

/// The run in progress of each kind, so Cancel (or starting another of that kind) can stop it.
#[derive(Default)]
pub struct Suggester {
    message: Mutex<Option<Arc<AtomicBool>>>,
    pull: Mutex<Option<Arc<AtomicBool>>>,
    guide: Mutex<Option<Arc<AtomicBool>>>,
    risks: Mutex<Option<Arc<AtomicBool>>>,
}

impl Suggester {
    fn slot(&self, kind: Kind) -> MutexGuard<'_, Option<Arc<AtomicBool>>> {
        match kind {
            Kind::Message => &self.message,
            Kind::Pull => &self.pull,
            Kind::Guide => &self.guide,
            Kind::Risks => &self.risks,
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

/// `git diff` as sent: paths unquoted, as the app's own file lists have them, so the names the
/// model writes back match; none of the user's diff settings (PINS, an order file) shaping it,
/// so a guide's diffs line up with its file list.
fn pinned_diff(repo: &Path, args: &[&str]) -> Result<String, String> {
    let diff = ["-c", "core.quotePath=false", "diff", "-O/dev/null"];
    git::run_text(repo, &[&git::PINS[..], &diff, &DIFF_OPTS, args].concat())
}

/// `pinned_diff` with the file list first.
fn git_diff(repo: &Path, args: &[&str]) -> Result<String, String> {
    pinned_diff(repo, &[&WITH_STAT[..], args].concat())
}

/// The diff to describe, cut as `cut` does.
fn diff(repo: &Path, scope: Scope) -> Result<(String, Option<&'static str>), String> {
    let run = |args: &[&str]| git_diff(repo, args);
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
                    &["-c", "core.quotePath=false", "diff", "--no-index"],
                    &DIFF_OPTS[..],
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

/// `text` cut to MAX_DIFF at a line end, and when it was, what of it is whole, for the prompt to
/// say. The file list ahead of the patch stays whole past MAX_DIFF, up to MAX_LIST.
fn cut(mut text: String) -> (String, Option<&'static str>) {
    if text.len() <= MAX_DIFF {
        return (text, None);
    }
    let list = stat_end(&text).filter(|&end| end <= MAX_LIST);
    let mut end = list.map_or(MAX_DIFF, |end| end.max(MAX_DIFF));
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    end = text[..end].rfind('\n').map_or(end, |i| i + 1);
    text.truncate(end);
    let whole = match list {
        Some(_) => "everything ahead of the patch, its file list included, is whole",
        None => "the file list ahead of the patch isn't whole either",
    };
    (text, Some(whole))
}

/// Where the first `--stat` list ends: after its " 3 files changed, …" line.
fn stat_end(text: &str) -> Option<usize> {
    let mut at = 0;
    for line in text.split_inclusive('\n') {
        at += line.len();
        let mut words = line.trim_start().split(' ');
        if line.starts_with(' ')
            && words.next().is_some_and(|n| n.parse::<u32>().is_ok())
            && matches!(words.next(), Some("file" | "files"))
            && words.next().is_some_and(|w| w.starts_with("changed"))
        {
            return Some(at);
        }
    }
    None
}

/// What a pull request from HEAD into `base` (a remote-tracking branch) brings: see
/// `range_input`, with the repository's PR template if it has one.
fn pull_input(repo: &Path, base: &str) -> Result<(String, Option<&'static str>), String> {
    git::check_pull_base(repo, base)?;
    let template = pull_template(repo, base).map(|t| {
        format!(
            "The repository's pull request template; follow its sections:\n{}\n\n",
            t.trim_end()
        )
    });
    range_input(repo, base, "HEAD", &template.unwrap_or_default())
}

/// What `to` brings over `from`: its commits' subjects, `extra`, then the diff from where it
/// left `from`, cut as `cut` does.
fn range_input(
    repo: &Path,
    from: &str,
    to: &str,
    extra: &str,
) -> Result<(String, Option<&'static str>), String> {
    let mut text = format!("{}\n\n{extra}", subjects(repo, from, to)?);
    text.push_str(&git_diff(repo, &[&format!("{from}...{to}"), "--"])?);
    Ok(cut(text))
}

/// The subjects of the commits `to` has over `from`, oldest first, under a heading.
fn subjects(repo: &Path, from: &str, to: &str) -> Result<String, String> {
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
    Ok(format!("{heading}\n{}", subjects.join("\n")))
}

/// What a guided review is of: a commit, HEAD's branch since it left `base` (a full ref), a
/// pull request's commits (fetched already) since its head left `base`, whatever HEAD is, or the
/// worktree's uncommitted changes.
#[derive(Deserialize)]
#[serde(tag = "of", rename_all = "lowercase")]
pub enum Target {
    Changes,
    Commit {
        sha: String,
    },
    Branch {
        base: String,
    },
    Pull {
        base: String,
        head: String,
        title: String,
    },
}

/// A guided review as the command wrote it, and the range it read: `base..head`, a commit's
/// parent (or the empty tree) and the commit, or the merge base and HEAD.
#[derive(Serialize, Debug)]
pub struct Guided {
    pub text: String,
    pub base: String,
    pub head: String,
    /// Uncommitted changes' `git::changes_stamp` as they were read, to tell when they've moved.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stamp: Option<String>,
}

/// What a guide's agent gets besides its command line: arguments to add as they are (the
/// guide's JSON schema), and the CLI to let read the patch file outside the repository.
#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Agent {
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub reads: Option<Reads>,
}

/// CLIs that read only inside the repository until told of another folder: Claude Code (its
/// working directories) and opencode (`external_directory`, which `opencode run` rejects).
#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum Reads {
    Claude,
    Opencode,
}

/// What a guide covers: a commit from its first parent (or the empty tree), or the branch's
/// commits since it left its base (only what's committed, so `base..head` names it).
struct Range {
    /// As the view reads them back: a commit's `sha^`, or the merge base, and the head.
    base: String,
    head: String,
    /// The commit's message, or the branch's subjects.
    header: String,
    /// `base` and `head` as object ids, for the diff and the note.
    from: String,
    to: String,
    stamp: Option<String>,
}

fn guide_range(repo: &Path, target: &Target) -> Result<Range, String> {
    let oid = |rev: &str| {
        git::run_text(repo, &["rev-parse", "--verify", "-q", rev]).map(|s| s.trim().to_string())
    };
    match target {
        Target::Changes => {
            // Before the snapshot: an edit between the two marks the review outdated early, never
            // late.
            let stamp = git::changes_stamp(repo)?;
            let snap = git::reading(|| git::worktree_snapshot(repo))?;
            if snap.is_empty(repo)? {
                return Err(if snap.left_out.is_empty() {
                    "There are no uncommitted changes.".into()
                } else {
                    "The only changes are new files too big or too many to review, or nested repositories.".into()
                });
            }
            Ok(Range {
                base: snap.base.clone(),
                head: snap.tree.clone(),
                header: changes_header(repo, &snap),
                from: snap.base,
                to: snap.tree,
                stamp: Some(stamp),
            })
        }
        Target::Commit { sha } => {
            git::validate_rev(sha)?;
            let base = git::parent_or_empty(repo, sha)?;
            let message = git::run_text(repo, &["log", "-1", "--format=%B", sha, "--"])?;
            let header = format!("The commit's message:\n{}", message.trim_end());
            let (from, to) = (oid(&base)?, oid(sha)?);
            Ok(Range {
                base,
                head: sha.clone(),
                header,
                from,
                to,
                stamp: None,
            })
        }
        Target::Branch { base } => {
            let head = oid("HEAD").map_err(|_| "There are no commits yet.".to_string())?;
            let from = git::parted(repo, &head, base)?;
            let header = subjects(repo, &from, &head)?;
            Ok(Range {
                base: from.clone(),
                head: head.clone(),
                header,
                from,
                to: head,
                stamp: None,
            })
        }
        Target::Pull { base, head, title } => {
            git::validate_rev(base)?;
            git::validate_rev(head)?;
            let to = oid(head)?;
            let from = git::merge_base(repo, base, &to)?;
            let header = format!(
                "The pull request's title: {title}\n\n{}",
                subjects(repo, &from, &to)?
            );
            Ok(Range {
                base: from.clone(),
                head: to.clone(),
                header,
                from,
                to,
                stamp: None,
            })
        }
    }
}

/// What uncommitted changes are against, and what of them the review can't see.
fn changes_header(repo: &Path, snap: &git::Snapshot) -> String {
    let text = |args: &[&str]| {
        git::run_text(repo, args)
            .map(|s| s.trim().to_string())
            .unwrap_or_default()
    };
    let branch = text(&["symbolic-ref", "--short", "-q", "HEAD"]);
    let on = if branch.is_empty() {
        String::new()
    } else {
        format!(" on the branch {branch}")
    };
    let against = if snap.born {
        let subject = text(&["log", "-1", "--format=%s", "HEAD", "--"]);
        format!("HEAD {} \"{subject}\"", crate::state::short(&snap.base))
    } else {
        "no commit yet, so every file is new".to_string()
    };
    let mut header = format!("Uncommitted changes in the worktree{on}, staged and not, new files included, against {against}.");
    if git::run(repo, &["rev-parse", "-q", "--verify", "MERGE_HEAD"]).is_ok() {
        header.push_str(
            " A merge is in progress: conflicted files are as they are on disk, markers and all.",
        );
    }
    if repo.join(".gitmodules").exists() {
        header.push_str(
            " A submodule shows only when its commit moved, not its own uncommitted edits.",
        );
    }
    if !snap.left_out.is_empty() {
        let left: Vec<&str> = snap.left_out.iter().map(String::as_str).collect();
        let shown = crate::state::some_of(&left, 20);
        header.push_str(&format!(
            " Left out, as new files too big or too many to read, or nested repositories: {shown}."
        ));
    }
    header
}

/// The prompt's input for a guide: the header, the list of changed files, and the diffs that fit;
/// with the note for the prompt and, when some diff isn't inline, the whole patch to put in a file.
struct GuideInput {
    base: String,
    head: String,
    stamp: Option<String>,
    text: String,
    note: String,
    patch: Option<String>,
    timeout: Duration,
}

/// What the prompt says of a guide's input, ahead of what its diffs are (one of the next three).
const LIST_NOTE: &str = "Below is what the change is (a commit's message, the branch's or pull request's commits, or the uncommitted changes), then a list of every changed file, a line each: its status letter, lines added and removed, its path (after \"←\", the path it was renamed from), tags, and after \"@@\" the functions or sections its changes are in";
const PREFIX_NOTE: &str = "; then the start of the patch, cut at a line's end.";
const SOME_NOTE: &str = "; then the whole diffs of the files not tagged [file only], [generated] or [binary] (binary files have none).";
const ALL_NOTE: &str = "; then every file's whole diff, except for [generated] and [binary] files.";
/// After those when there's a patch file: its range, its size in KB and PATCH, its path.
const PATCH_NOTE: &str = " The whole patch, `git diff {range}` ({size} KB), is in the file {patch}. If you can read files, read the diffs you need from it and open the repository's files for context, but never modify anything. If you can't, work from the list and the diffs here.";
/// The patch file's path in the note, unknown until it's written.
const PATCH: &str = "{patch}";

fn guide_input(repo: &Path, target: &Target) -> Result<GuideInput, String> {
    let Range {
        base,
        head,
        header,
        from,
        to,
        stamp,
    } = guide_range(repo, target)?;
    let files = git::range_files(repo, &from, &to)?;
    let patch = pinned_diff(repo, &[&from, &to, "--"])?;
    let m = manifest::build(&files, &patch, MAX_GUIDE_INPUT.saturating_sub(header.len()));
    let mut note = LIST_NOTE.to_string();
    note.push_str(if m.prefix {
        PREFIX_NOTE
    } else if m.file_only > 0 {
        SOME_NOTE
    } else {
        ALL_NOTE
    });
    if m.listed < files.len() {
        note.push_str(&format!(
            " The list stops after {} of the {} files, at {} KB.",
            m.listed,
            files.len(),
            manifest::MAX_LIST / 1024
        ));
    }
    let generated = files
        .iter()
        .any(|f| f.additions.is_some() && manifest::is_generated(&f.path));
    let patch = (m.prefix || m.file_only > 0 || generated).then(|| {
        note.push_str(
            &PATCH_NOTE
                .replace("{range}", &format!("{from} {to}"))
                .replace("{size}", &patch.len().div_ceil(1024).to_string()),
        );
        patch
    });
    let extra = Duration::from_secs((m.reading / 4096) as u64);
    let text = format!(
        "{header}\n\nChanged files ({}):\n{}\n\n{}",
        files.len(),
        m.list,
        m.inline
    );
    Ok(GuideInput {
        base,
        head,
        stamp,
        text,
        note,
        patch,
        timeout: (GUIDE_TIMEOUT + extra).min(MAX_GUIDE_TIMEOUT),
    })
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
pub(crate) fn expand_home(program: &str) -> String {
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
    let (diff, whole) = diff(repo, scope)?;
    ask(
        repo,
        template,
        prompt,
        &diff,
        cancel,
        &Options::new(TIMEOUT, whole),
    )
}

/// A pull request's title and description, for HEAD into `base`: see `ask`.
pub fn run_pull(
    repo: &Path,
    template: &str,
    prompt: &str,
    base: &str,
    cancel: &AtomicBool,
) -> Result<String, String> {
    let (input, whole) = pull_input(repo, base)?;
    ask(
        repo,
        template,
        prompt,
        &input,
        cancel,
        &Options::new(TIMEOUT, whole),
    )
}

/// A guided review of `target`: see `ask`.
pub fn run_guide(
    repo: &Path,
    template: &str,
    prompt: &str,
    target: &Target,
    agent: &Agent,
    cancel: &AtomicBool,
) -> Result<Guided, String> {
    guide_within(repo, template, prompt, target, agent, cancel, None)
}

/// `run_guide`, stopped after `timeout` instead of the one the change's size gives.
pub(crate) fn guide_within(
    repo: &Path,
    template: &str,
    prompt: &str,
    target: &Target,
    agent: &Agent,
    cancel: &AtomicBool,
    timeout: Option<Duration>,
) -> Result<Guided, String> {
    let input = guide_input(repo, target)?;
    let mut options = Options::new(timeout.unwrap_or(input.timeout), None);
    options.args = agent.args.clone();
    // Removed with the patch in it however the run ends: answered, failed, cancelled or timed out.
    let mut scratch = None;
    let mut note = input.note;
    if let Some(patch) = &input.patch {
        // A run cut short by quitting left its folder: none lasts past the longest run.
        let (dir, path) = ScratchDir::fresh("guide", MAX_GUIDE_TIMEOUT + Duration::from_secs(60))?;
        let file = path.join("changes.patch");
        crate::scratch::write_private(&file, patch)
            .map_err(|e| format!("Couldn't write the patch for the agent: {e}"))?;
        let shown = file.to_string_lossy();
        note = note.replace(PATCH, &format!("`{shown}`"));
        match agent.reads {
            Some(Reads::Claude) => options
                .args
                .push(format!("--add-dir={}", path.to_string_lossy())),
            Some(Reads::Opencode) => {
                let own = std::env::var("OPENCODE_PERMISSION").ok();
                let rules = opencode_reads(own.as_deref(), &path.to_string_lossy());
                options.env.push(("OPENCODE_PERMISSION", rules));
            }
            None => {}
        }
        scratch = Some(dir);
    }
    let prompt = format!("{prompt}\n\n{note}");
    let text = ask(repo, template, &prompt, &input.text, cancel, &options);
    drop(scratch);
    Ok(Guided {
        text: text?,
        base: input.base,
        head: input.head,
        stamp: input.stamp,
    })
}

/// opencode's permission rules (`own`: the user's OPENCODE_PERMISSION, which ours would replace)
/// with reads under `dir` allowed: an `external_directory` of one action keeps it for the rest.
fn opencode_reads(own: Option<&str>, dir: &str) -> String {
    use serde_json::{json, Value};
    let mut rules = own
        .and_then(|o| serde_json::from_str::<Value>(o).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));
    let external = &mut rules["external_directory"];
    if !external.is_object() {
        *external = match external.take() {
            Value::String(action) => json!({ "*": action }),
            _ => json!({}),
        };
    }
    external[format!("{dir}/*")] = json!("allow");
    rules.to_string()
}

/// How a run goes: stopped after `timeout`; `cut`: the diff was cut, and what of it is whole;
/// `args` after the template's own, `env` added to its environment.
struct Options {
    timeout: Duration,
    cut: Option<&'static str>,
    args: Vec<String>,
    env: Vec<(&'static str, String)>,
}

impl Options {
    fn new(timeout: Duration, cut: Option<&'static str>) -> Self {
        Options {
            timeout,
            cut,
            args: vec![],
            env: vec![],
        }
    }
}

/// Runs the template with the prompt and the diff, returning what it printed. Stops on `cancel`
/// or after the timeout, killing the command and anything it started.
fn ask(
    repo: &Path,
    template: &str,
    prompt: &str,
    diff: &str,
    cancel: &AtomicBool,
    options: &Options,
) -> Result<String, String> {
    let timeout = options.timeout;
    let prompt = match options.cut {
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
        .args(&options.args)
        .envs(options.env.iter().map(|(k, v)| (k, v)))
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
    // Each output is read on a thread, kept up to MAX_OUTPUT and read past after that, so the
    // command never blocks on a full pipe; it comes back over a channel, which can be given up on.
    let drain = |r: Option<Box<dyn Read + Send>>| {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(mut r) = r {
                let _ = r.by_ref().take(MAX_OUTPUT as u64 + 1).read_to_end(&mut buf);
                let _ = std::io::copy(&mut r, &mut std::io::sink());
            }
            let _ = tx.send(buf);
        });
        rx
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
    let stop = || {
        let cancelled = cancel.load(Ordering::Relaxed);
        (cancelled || Instant::now() > deadline).then(|| {
            if cancelled {
                CANCELLED.to_string()
            } else {
                format!(
                    "\"{}\" gave no answer within {} seconds.",
                    argv[0],
                    timeout.as_secs()
                )
            }
        })
    };
    while !process::exited(&mut child) {
        if let Some(why) = stop() {
            process::kill_group(&mut child, Duration::ZERO);
            return Err(why);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    // Done: what it left running in its group goes too, or a child of it still holding stdout
    // would keep the answer from ending. Not reaped yet, so the group's id is still its own.
    process::kill_group(&mut child, Duration::ZERO);
    let status = child.wait().map_err(|e| e.to_string())?;
    // One that left the group can still hold the pipes: waiting for it stops as the run does.
    let read = |rx: mpsc::Receiver<Vec<u8>>| loop {
        match rx.recv_timeout(Duration::from_millis(20)) {
            Ok(buf) => return Ok(String::from_utf8_lossy(&buf).into_owned()),
            Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(String::new()),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if let Some(why) = stop() {
                    return Err(why);
                }
            }
        }
    };
    let stdout = read(out)?;
    let stderr = read(err)?;
    if status.success() && stdout.len() > MAX_OUTPUT {
        return Err(format!(
            "\"{}\" printed over {} MB, far more than an answer; none of it was kept.",
            argv[0],
            MAX_OUTPUT / (1024 * 1024)
        ));
    }
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
        let risks = s.start(Kind::Risks);
        assert!(!message.load(Ordering::Relaxed));
        s.cancel(Kind::Pull);
        assert!(pull.load(Ordering::Relaxed));
        assert!(!message.load(Ordering::Relaxed) && !guide.load(Ordering::Relaxed));
        // A guide's risks run beside it.
        s.cancel(Kind::Risks);
        assert!(risks.load(Ordering::Relaxed) && !guide.load(Ordering::Relaxed));
        // A second of the same kind stops the first.
        s.start(Kind::Message);
        assert!(message.load(Ordering::Relaxed));
    }

    #[test]
    fn opencode_reads_keep_the_users_own_rules() {
        use serde_json::{json, Value};
        let read = |own: Option<&str>| {
            serde_json::from_str::<Value>(&opencode_reads(own, "/t/g")).unwrap()
        };
        assert_eq!(
            read(None),
            json!({ "external_directory": { "/t/g/*": "allow" } })
        );
        assert_eq!(read(Some("not json")), read(None));
        assert_eq!(read(Some("[1]")), read(None));
        assert_eq!(
            read(Some(r#"{"bash": "deny", "external_directory": "deny"}"#)),
            json!({ "bash": "deny", "external_directory": { "*": "deny", "/t/g/*": "allow" } })
        );
        assert_eq!(
            read(Some(r#"{"external_directory": {"~/src/*": "allow"}}"#)),
            json!({ "external_directory": { "~/src/*": "allow", "/t/g/*": "allow" } })
        );
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
