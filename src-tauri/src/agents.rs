//! Coding agents running in the terminal's panes, told apart by a table (agents.json) rather than
//! code per agent: how to know one from its command line, where it keeps the id of the
//! conversation it's in, and the command that resumes that conversation. A restored terminal
//! offers that command, and an agent whose state file says it stopped working marks its pane.
//!
//! Only programs in the table are offered: running any other command line again could repeat a
//! deploy or a publish.

use crate::procinfo::{self, Process};
use notify::{recommended_watcher, RecommendedWatcher, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::UNIX_EPOCH;

/// How many words a flag takes after it (`--flag=value` takes its own). A flag the table
/// doesn't name takes none, so a prompt after an unknown switch is dropped, never kept.
#[derive(Deserialize, Clone, Copy, Debug, PartialEq)]
pub enum Arity {
    #[serde(rename = "1")]
    One,
    /// The next word unless it's a flag (`--resume [id]`), as commander and yargs read it.
    #[serde(rename = "?")]
    Optional,
    /// Every word up to the next flag (`--add-dir <dirs...>`).
    #[serde(rename = "*")]
    Many,
}

/// Where an agent reports what it's doing: a field of its pid file, by value.
#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct Status {
    field: String,
    working: Vec<String>,
    waiting: Vec<String>,
    idle: Vec<String>,
}

impl Status {
    /// None for a value the table doesn't know (a newer version's): the last state stands.
    fn state(&self, value: &str) -> Option<&'static str> {
        let is = |list: &[String]| list.iter().any(|v| v == value);
        [
            (&self.working, "working"),
            (&self.waiting, "waiting"),
            (&self.idle, "idle"),
        ]
        .into_iter()
        .find_map(|(list, state)| is(list).then_some(state))
    }
}

/// Where the id of the conversation an agent is in can be read. Paths start at the home folder
/// (`~/`) and may hold `{pid}` (the agent's process) and `{cwdSha256}` (its folder, hashed).
/// `uuid`: the id is one, and anything else read there isn't taken.
#[derive(Deserialize, Debug)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Session {
    /// A JSON file per running process, with the id in `id` and maybe its state.
    PidFile {
        path: String,
        id: String,
        #[serde(default)]
        uuid: bool,
        status: Option<Status>,
    },
    /// The newest `<prefix>*.json` in `dir` written since the process started, the id in `id`
    /// near its top.
    NewestFile {
        dir: String,
        prefix: String,
        id: String,
        #[serde(default)]
        uuid: bool,
    },
    /// None to read: the resume command continues the folder's last conversation.
    // Braced: a unit variant would take fields it doesn't know.
    Continue {},
}

/// Where an agent keeps the conversations it had, for Resume a conversation (conversations.rs).
#[derive(Deserialize, Debug)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum History {
    /// Claude Code's: in `dir`, a folder per project named after its path, a JSON-lines file
    /// per conversation named by its id.
    ClaudeProjects { dir: String },
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Adapter {
    /// What the user calls it ("Claude Code").
    pub name: String,
    /// The versions it was written against, and what doesn't hold.
    #[allow(dead_code)]
    note: String,
    /// argv[0]'s file name, or a script's that a runtime (node, bun) runs.
    programs: Vec<String>,
    /// Script paths that end with one of these (an npm install's `cli.js`).
    #[serde(default)]
    scripts: Vec<String>,
    session: Session,
    /// The resume command's words: `{id}` the session's (before the flags, so a flag the table
    /// doesn't know can't take it for its value), `{flags}` the flags kept from the run.
    resume: Vec<String>,
    /// The flags that take words after them.
    #[serde(default)]
    flags: HashMap<String, Arity>,
    /// Flags left out of the resume (with their words): ones that pick or start a conversation.
    #[serde(default)]
    drop: Vec<String>,
    /// Flags, or a first word (a subcommand), that mean the run wasn't a conversation to resume.
    #[serde(default)]
    skip: Vec<String>,
    /// Where its past conversations are, when they can be listed.
    #[serde(default)]
    history: Option<History>,
}

fn adapters() -> &'static [Adapter] {
    static TABLE: OnceLock<Vec<Adapter>> = OnceLock::new();
    TABLE.get_or_init(|| {
        serde_json::from_str(include_str!("agents.json")).expect("agents.json is a valid table")
    })
}

/// The agents whose past conversations can be listed, and where those are.
pub fn histories() -> impl Iterator<Item = (&'static Adapter, &'static History)> {
    adapters()
        .iter()
        .filter_map(|a| Some((a, a.history.as_ref()?)))
}

/// A table path, `~/` the home folder.
pub fn from_home(path: &str, home: &Path) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None => PathBuf::from(path),
    }
}

/// Runtimes that run an agent's script: its first word that isn't a flag of theirs.
const RUNTIMES: [&str; 3] = ["node", "bun", "deno"];

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

impl Adapter {
    /// The command that resumes conversation `id`, read from a file: a plain one only.
    pub fn resume_command(&self, id: &str) -> Option<String> {
        let uuid = matches!(
            self.session,
            Session::PidFile { uuid: true, .. } | Session::NewestFile { uuid: true, .. }
        );
        rewrite(self, &[], Some(&plain_id(id, uuid)?))
    }

    /// Where the agent's own arguments start in `argv`, if it's this agent's.
    fn args_at(&self, argv: &[String]) -> Option<usize> {
        let program = file_name(argv.first()?);
        if self.programs.iter().any(|p| p == program) {
            return Some(1);
        }
        if !RUNTIMES.contains(&program) {
            return None;
        }
        let at = 1 + argv[1..].iter().position(|a| !a.starts_with('-'))?;
        let script = &argv[at];
        let known = self.programs.iter().any(|p| p == file_name(script))
            || self.scripts.iter().any(|s| script.ends_with(s.as_str()));
        known.then_some(at + 1)
    }
}

/// An agent found running, and the process that is it.
#[derive(Clone)]
pub struct Found {
    adapter: &'static Adapter,
    process: Process,
    args_at: usize,
}

fn found(table: &'static [Adapter], process: Process) -> Option<Found> {
    table.iter().find_map(|adapter| {
        let args_at = adapter.args_at(&process.argv)?;
        Some(Found {
            adapter,
            process: process.clone(),
            args_at,
        })
    })
}

/// The agent a pane's foreground job runs: its leader, or a process a launcher (npx) started in it.
fn detect(leader: Process) -> Option<Found> {
    let pgid = leader.pid;
    found(adapters(), leader).or_else(|| {
        procinfo::job(pgid)
            .into_iter()
            .filter(|&pid| pid != pgid)
            .filter_map(procinfo::process)
            .find_map(|p| found(adapters(), p))
    })
}

/// `word` as zsh, bash and fish all read it back: bare when it's plain, else in single quotes,
/// with `'` and `\` outside them (fish reads `\'` and `\\` inside quotes as escapes).
fn quote(word: &str) -> String {
    let plain = |c: char| c.is_ascii_alphanumeric() || "_-.,/:@%+=".contains(c);
    // zsh expands a word that starts with `=` to a command's path.
    if !word.is_empty() && !word.starts_with('=') && word.chars().all(plain) {
        return word.to_string();
    }
    let mut out = String::new();
    let mut open = false;
    for c in word.chars() {
        if c == '\'' || c == '\\' {
            if open {
                out.push('\'');
                open = false;
            }
            out.push('\\');
            out.push(c);
        } else {
            if !open {
                out.push('\'');
                open = true;
            }
            out.push(c);
        }
    }
    if open {
        out.push('\'');
    }
    if word.is_empty() {
        out.push_str("''");
    }
    out
}

/// The command that resumes `args` (the agent's own, after its program): its flags kept but for
/// the `drop`ped ones, its words (a first prompt) left out so they aren't sent again. None for a
/// run that wasn't a conversation (`skip`), or one whose id is needed and unknown.
fn rewrite(adapter: &Adapter, args: &[String], id: Option<&str>) -> Option<String> {
    let mut kept: Vec<&str> = Vec::new();
    let mut first_word = true;
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        i += 1;
        if arg == "--" {
            break;
        }
        if !arg.starts_with('-') || arg == "-" {
            if first_word && adapter.skip.iter().any(|s| s == arg) {
                return None;
            }
            first_word = false;
            continue;
        }
        let (name, inline) = match arg.split_once('=') {
            Some((name, _)) if arg.starts_with("--") => (name, true),
            _ => (arg, false),
        };
        if adapter.skip.iter().any(|s| s == name) {
            return None;
        }
        let from = i - 1;
        if !inline {
            let is_word = |a: &String| !a.starts_with('-');
            match adapter.flags.get(name) {
                Some(Arity::One) => i = (i + 1).min(args.len()),
                Some(Arity::Optional) if args.get(i).is_some_and(is_word) => i += 1,
                Some(Arity::Many) => {
                    while args.get(i).is_some_and(is_word) {
                        i += 1;
                    }
                }
                _ => {}
            }
        }
        if !adapter.drop.iter().any(|d| d == name) {
            kept.extend(args[from..i].iter().map(String::as_str));
        }
    }
    let mut words = Vec::new();
    for part in &adapter.resume {
        match part.as_str() {
            "{flags}" => words.extend(kept.iter().copied()),
            "{id}" => words.push(id?),
            word => words.push(word),
        }
    }
    // Typed into the shell, a newline or escape would act as a key.
    if words.iter().any(|w| w.chars().any(char::is_control)) {
        return None;
    }
    Some(words.iter().map(|w| quote(w)).collect::<Vec<_>>().join(" "))
}

/// A conversation id is typed into a shell: only plain ones, never one a program would read as
/// a flag, and a UUID where the agent uses those.
fn plain_id(id: &str, uuid: bool) -> Option<String> {
    let plain = !id.is_empty()
        && id.len() <= 128
        && !id.starts_with('-')
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    let shaped = !uuid || is_uuid(id);
    (plain && shaped).then(|| id.to_string())
}

fn is_uuid(id: &str) -> bool {
    let parts: Vec<&str> = id.split('-').collect();
    parts.iter().map(|p| p.len()).eq([8, 4, 4, 4, 12])
        && parts
            .iter()
            .all(|p| p.chars().all(|c| c.is_ascii_hexdigit()))
}

fn expand(template: &str, process: &Process, home: &Path) -> Option<PathBuf> {
    use sha2::{Digest, Sha256};
    let mut path = template.replace("{pid}", &process.pid.to_string());
    if path.contains("{cwdSha256}") {
        let cwd = process.cwd.as_ref()?.to_str()?;
        path = path.replace("{cwdSha256}", &format!("{:x}", Sha256::digest(cwd)));
    }
    Some(from_home(&path, home))
}

/// Written since the process started (to the second, with a second's slack): a file left by a
/// process that had the same pid before isn't this one's.
fn since_start(meta: &std::fs::Metadata, process: &Process) -> bool {
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs());
    modified + 1 >= process.started
}

/// The string `field` holds near the top of a JSON file too big to parse whole for it.
fn head_field(head: &str, field: &str) -> Option<String> {
    let key = format!("\"{field}\"");
    let rest = head[head.find(&key)? + key.len()..].trim_start();
    let rest = rest.strip_prefix(':')?.trim_start().strip_prefix('"')?;
    Some(rest[..rest.find('"')?].to_string())
}

/// A pid file's id, when it's this process's.
fn pid_file_id(file: &Path, process: &Process, id: &str, uuid: bool) -> Option<String> {
    if !since_start(&std::fs::metadata(file).ok()?, process) {
        return None;
    }
    let json: serde_json::Value = serde_json::from_slice(&std::fs::read(file).ok()?).ok()?;
    plain_id(json.get(id)?.as_str()?, uuid)
}

fn newest_file(
    dir: &Path,
    prefix: &str,
    id: &str,
    uuid: bool,
    process: &Process,
) -> Option<String> {
    use std::io::Read;
    let (_, path) = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| {
            let e = e.ok()?;
            let name = e.file_name();
            let name = name.to_str()?;
            if !name.starts_with(prefix) || !name.ends_with(".json") {
                return None;
            }
            let meta = e.metadata().ok()?;
            if !since_start(&meta, process) {
                return None;
            }
            Some((meta.modified().ok()?, e.path()))
        })
        .max()?;
    // A chat holds the whole conversation, megabytes; its id is written first.
    let mut head = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(64 * 1024)
        .read_to_end(&mut head)
        .ok()?;
    plain_id(&head_field(&String::from_utf8_lossy(&head), id)?, uuid)
}

/// What the page is told of a pane's agent.
#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Agent {
    pub name: String,
    /// What resumes its conversation; None when that can't be told (its id unread yet).
    pub command: Option<String>,
    /// The conversation's id, so two panes that read the same one don't both resume it.
    pub session: Option<String>,
    /// Where it runs: a restore resumes it only in the same folder.
    pub cwd: Option<String>,
    /// working, waiting or idle, for an agent that reports it.
    pub state: Option<&'static str>,
}

/// An agent, and the file to watch for its state if it has one.
struct Look {
    agent: Agent,
    status: Option<(PathBuf, &'static Status)>,
}

fn look(found: &Found, home: &Path) -> Look {
    let adapter: &'static Adapter = found.adapter;
    let process = &found.process;
    let args = process.argv.get(found.args_at..).unwrap_or_default();
    let (id, status) = match &adapter.session {
        Session::PidFile {
            path,
            id,
            uuid,
            status,
        } => {
            let file = expand(path, process, home);
            let id = file
                .as_deref()
                .and_then(|f| pid_file_id(f, process, id, *uuid));
            (id, file.zip(status.as_ref()))
        }
        Session::NewestFile {
            dir,
            prefix,
            id,
            uuid,
        } => {
            let id = expand(dir, process, home)
                .and_then(|d| newest_file(&d, prefix, id, *uuid, process));
            (id, None)
        }
        Session::Continue {} => (None, None),
    };
    let command = match adapter.session {
        Session::Continue {} => rewrite(adapter, args, None),
        _ => id
            .as_deref()
            .and_then(|id| rewrite(adapter, args, Some(id))),
    };
    Look {
        agent: Agent {
            name: adapter.name.clone(),
            command,
            session: id,
            cwd: process
                .cwd
                .as_ref()
                .map(|c| c.to_string_lossy().into_owned()),
            state: None,
        },
        status,
    }
}

/// Gets a pane's id and its agent's new state (None: its file is gone, the agent with it).
pub type Sink = Arc<dyn Fn(u32, Option<&'static str>) + Send + Sync>;

/// A pane's job leader as last looked up, by its start (a pid can come back as another program).
struct Seen {
    pid: u32,
    started: u64,
    /// When it was last looked up.
    checked: u64,
    found: Option<Found>,
}

/// A leader this young may be a launcher (npx) whose agent isn't started yet: looked up on
/// every call. Older and still without an agent (a launcher downloading for minutes), it's
/// looked up again this often.
const SETTLING_SECS: u64 = 30;

/// The agents in the panes: each pane's job leader read once (argv, its job) until it changes,
/// and the state files watched.
#[derive(Default)]
pub struct Agents {
    seen: Mutex<HashMap<u32, Seen>>,
    watch: Watch,
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

impl Agents {
    /// The agent pane `id` runs, its foreground job led by `leader`. `sink` (made once) hears
    /// of its state's changes from then on.
    pub fn agent(
        &self,
        id: u32,
        leader: Option<u32>,
        home: &Path,
        sink: impl FnOnce() -> Sink,
    ) -> Option<Agent> {
        let found = leader.and_then(|pid| self.found(id, pid));
        let Some(found) = found else {
            self.forget(id);
            return None;
        };
        let Look { mut agent, status } = look(&found, home);
        match status {
            // Once watched, its state is the watcher's alone: a read here can be older than an
            // event already sent.
            Some((file, status)) => agent.state = self.watch.track(id, &file, status, sink),
            None => self.watch.forget(id),
        }
        Some(agent)
    }

    fn found(&self, id: u32, pid: u32) -> Option<Found> {
        let started = procinfo::started(pid)?;
        if let Some(seen) = lock(&self.seen).get(&id) {
            let now = now();
            let settled = seen.found.is_some()
                || (now.saturating_sub(started) > SETTLING_SECS
                    && now.saturating_sub(seen.checked) <= SETTLING_SECS);
            if seen.pid == pid && seen.started == started && settled {
                return seen.found.clone();
            }
        }
        let found = procinfo::process(pid).and_then(detect);
        let seen = Seen {
            pid,
            started,
            checked: now(),
            found: found.clone(),
        };
        lock(&self.seen).insert(id, seen);
        found
    }

    pub fn forget(&self, id: u32) {
        lock(&self.seen).remove(&id);
        self.watch.forget(id);
    }

    pub fn forget_all(&self) {
        lock(&self.seen).clear();
        self.watch.forget_all();
    }
}

struct Tracked {
    id: u32,
    status: &'static Status,
    last: Option<&'static str>,
}

type Files = Mutex<HashMap<PathBuf, Tracked>>;

/// The state files of the agents in panes, watched (FSEvents on macOS) rather than polled: no
/// work between an agent's writes, and none at all without an agent.
#[derive(Default)]
struct Watch {
    files: Arc<Files>,
    /// Locked apart from `files`: watching another folder restarts the watcher's thread and
    /// waits for it, and that thread may be waiting for `files` in `changed`.
    watcher: Mutex<Option<(RecommendedWatcher, HashSet<PathBuf>)>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn read_state(file: &Path, status: &Status) -> Option<&'static str> {
    let json: serde_json::Value = serde_json::from_slice(&std::fs::read(file).ok()?).ok()?;
    status.state(json.get(&status.field)?.as_str()?)
}

impl Watch {
    /// Watches pane `id`'s agent's state `file`, in place of any it had, and gives its state as
    /// last seen. `sink` is made for the watcher, the first time there's one to make.
    fn track(
        &self,
        id: u32,
        file: &Path,
        status: &'static Status,
        sink: impl FnOnce() -> Sink,
    ) -> Option<&'static str> {
        // Events name the real path.
        let (Some(dir), Some(name)) = (file.parent(), file.file_name()) else {
            return None;
        };
        let dir = dir.canonicalize().ok()?;
        let file = dir.join(name);
        let last = {
            let mut files = lock(&self.files);
            if files.get(&file).is_none_or(|t| t.id != id) {
                files.retain(|_, t| t.id != id);
                let last = read_state(&file, status);
                files.insert(file.clone(), Tracked { id, status, last });
            }
            files.get(&file).and_then(|t| t.last)
        };
        let mut watcher = lock(&self.watcher);
        if watcher.is_none() {
            let files = Arc::downgrade(&self.files);
            let sink = sink();
            let made = recommended_watcher(move |event: notify::Result<notify::Event>| {
                if let Ok(event) = event {
                    changed(&files, &event.paths, &sink);
                }
            });
            *watcher = made.ok().map(|w| (w, HashSet::new()));
        }
        if let Some((w, dirs)) = watcher.as_mut() {
            if !dirs.contains(&dir) && w.watch(&dir, RecursiveMode::NonRecursive).is_ok() {
                dirs.insert(dir);
            }
        }
        last
    }

    /// Stops watching pane `id`'s agent; the watcher goes with the last one.
    fn forget(&self, id: u32) {
        let empty = {
            let mut files = lock(&self.files);
            files.retain(|_, t| t.id != id);
            files.is_empty()
        };
        if empty {
            self.stop();
        }
    }

    fn forget_all(&self) {
        lock(&self.files).clear();
        self.stop();
    }

    fn stop(&self) {
        // Dropped unlocked, as its thread is joined.
        let watcher = lock(&self.watcher).take();
        drop(watcher);
    }
}

fn changed(files: &Weak<Files>, paths: &[PathBuf], sink: &Sink) {
    let Some(files) = files.upgrade() else {
        return;
    };
    let mut told = Vec::new();
    {
        let mut files = lock(&files);
        for path in paths {
            let Some(t) = files.get_mut(path) else {
                continue;
            };
            if !path.exists() {
                told.push((t.id, None));
                files.remove(path);
                continue;
            }
            // Mid-write, unreadable or a state the table doesn't know: the last one stands.
            let Some(state) = read_state(path, t.status) else {
                continue;
            };
            if t.last != Some(state) {
                t.last = Some(state);
                told.push((t.id, Some(state)));
            }
        }
    }
    for (id, state) in told {
        sink(id, state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    fn claude() -> &'static Adapter {
        &adapters()[0]
    }

    fn args(words: &[&str]) -> Vec<String> {
        words.iter().map(|s| s.to_string()).collect()
    }

    const ID: &str = "7c3e9a41-5d2b-4f86-b0e1-2a9c4d6f8e16";

    fn process(pid: u32, argv: &[&str], cwd: &Path) -> Process {
        Process {
            pid,
            argv: args(argv),
            cwd: Some(cwd.to_path_buf()),
            started: now() - 60,
        }
    }

    fn found_with(p: Process) -> Found {
        found(adapters(), p).unwrap()
    }

    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("gitviber-agents-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_table_parses_and_refuses_a_field_it_doesnt_know() {
        let names: Vec<&str> = adapters().iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["Claude Code", "Gemini CLI", "opencode", "Codex"]);
        for a in adapters() {
            assert!(
                a.resume.iter().filter(|w| *w == "{flags}").count() == 1,
                "{}",
                a.name
            );
            assert!(!a.note.is_empty(), "{}", a.name);
        }
        let typo = r#"[{"name":"x","note":"","programs":["x"],"session":{"kind":"continue"},"resume":["x"],"skp":[]}]"#;
        assert!(serde_json::from_str::<Vec<Adapter>>(typo).is_err());
        let typo = r#"[{"name":"x","note":"","programs":["x"],"session":{"kind":"continue","id":"x"},"resume":["x"]}]"#;
        assert!(serde_json::from_str::<Vec<Adapter>>(typo).is_err());
    }

    #[test]
    fn the_resume_keeps_flags_and_drops_the_prompt_and_the_session_picked() {
        let id = Some("abc-123");
        let cases: &[(&[&str], Option<&str>)] = &[
            (&[], Some("claude --resume abc-123")),
            (
                &["--dangerously-skip-permissions", "fix the login bug"],
                Some("claude --resume abc-123 --dangerously-skip-permissions"),
            ),
            (
                &["--model", "opus", "--resume", "old-id", "go on"],
                Some("claude --resume abc-123 --model opus"),
            ),
            // An optional value: a flag after it isn't one.
            (
                &["-r", "--verbose"],
                Some("claude --resume abc-123 --verbose"),
            ),
            (
                &["--continue", "--fork-session"],
                Some("claude --resume abc-123"),
            ),
            (
                &["--session-id", "x", "hi"],
                Some("claude --resume abc-123"),
            ),
            (
                &["--add-dir", "../a", "../b", "--model=sonnet"],
                Some("claude --resume abc-123 --add-dir ../a ../b --model=sonnet"),
            ),
            (
                &["--append-system-prompt", "be brief, it's late"],
                Some(r"claude --resume abc-123 --append-system-prompt 'be brief, it'\''s late'"),
            ),
            (&["--", "--not-a-flag"], Some("claude --resume abc-123")),
            // npm's process.title blanks argv: nothing is kept, nothing breaks.
            (&["", "", ""], Some("claude --resume abc-123")),
            (&["-p", "explain"], None),
            (&["--print"], None),
            (&["mcp", "serve"], None),
            (&["update"], None),
            // A subcommand's name later on is a prompt's word.
            (
                &["--model", "opus", "fix", "mcp"],
                Some("claude --resume abc-123 --model opus"),
            ),
            (&["--system-prompt", "a\nb"], None),
        ];
        for (input, want) in cases {
            assert_eq!(
                rewrite(claude(), &args(input), id).as_deref(),
                *want,
                "{input:?}"
            );
        }
        assert_eq!(rewrite(claude(), &[], None), None);
    }

    #[test]
    fn the_other_agents_resume_their_own_way() {
        let [_, gemini, opencode, codex] = adapters() else {
            panic!("four agents");
        };
        let resume = |a: &Adapter, input: &[&str], id: Option<&str>| rewrite(a, &args(input), id);
        assert_eq!(
            resume(
                gemini,
                &["-y", "-m", "gemini-2.5-pro", "-i", "start here"],
                Some("u-1")
            )
            .as_deref(),
            Some("gemini --resume u-1 -y -m gemini-2.5-pro")
        );
        assert_eq!(resume(gemini, &["-p", "once"], Some("u-1")), None);
        assert_eq!(
            resume(
                opencode,
                &["-m", "anthropic/sonnet", "--continue", "."],
                None
            )
            .as_deref(),
            Some("opencode -m anthropic/sonnet --continue")
        );
        assert_eq!(resume(opencode, &["run", "hi"], None), None);
        assert_eq!(
            resume(codex, &["--full-auto", "-m", "o3", "do it"], None).as_deref(),
            Some("codex resume --last --full-auto -m o3")
        );
        assert_eq!(resume(codex, &["exec", "do it"], None), None);
    }

    #[test]
    fn words_are_quoted_for_zsh_bash_and_fish() {
        assert_eq!(quote("--model=opus"), "--model=opus");
        assert_eq!(quote("a b"), "'a b'");
        assert_eq!(quote("it's"), r"'it'\''s'");
        assert_eq!(quote(r"C:\x"), r"'C:'\\'x'");
        assert_eq!(quote("$HOME"), "'$HOME'");
        assert_eq!(quote("=ls"), "'=ls'");
        assert_eq!(quote("~"), "'~'");
        assert_eq!(quote(""), "''");
        assert_eq!(quote("'"), r"\'");
    }

    #[test]
    fn an_agent_is_told_by_its_program_or_the_script_a_runtime_runs() {
        let at = |argv: &[&str]| {
            adapters()
                .iter()
                .find_map(|a| Some((a.name.as_str(), a.args_at(&args(argv))?)))
        };
        assert_eq!(at(&["claude", "-c"]), Some(("Claude Code", 1)));
        assert_eq!(at(&["claude", "", ""]), Some(("Claude Code", 1)));
        assert_eq!(at(&["/opt/homebrew/bin/claude"]), Some(("Claude Code", 1)));
        assert_eq!(
            at(&[
                "node",
                "/usr/lib/node_modules/@anthropic-ai/claude-code/cli.js",
                "--model",
                "x"
            ]),
            Some(("Claude Code", 2))
        );
        // Gemini's relaunch: node's own flags first.
        assert_eq!(
            at(&[
                "/usr/bin/node",
                "--max-old-space-size=8192",
                "/home/u/.nvm/bin/gemini",
                "-y"
            ]),
            Some(("Gemini CLI", 3))
        );
        assert_eq!(at(&["opencode"]), Some(("opencode", 1)));
        // Claude's own background processes retitle themselves.
        assert_eq!(at(&["claude bg-spare", "--bg-spare"]), None);
        assert_eq!(at(&["node", "server.js"]), None);
        assert_eq!(at(&["vim", "claude"]), None);
        assert_eq!(at(&["npm", "run", "dev"]), None);
    }

    #[test]
    fn ids_are_plain_and_uuids_where_the_agent_uses_them() {
        assert_eq!(plain_id(ID, true).as_deref(), Some(ID));
        assert_eq!(plain_id("abc-123", false).as_deref(), Some("abc-123"));
        for bad in [
            "abc-123",
            "-7c3e9a41-5d2b-4f86-b0e1-2a9c4d6f8e16",
            "7c3e9a41-5d2b-4f86-b0e1-2a9c4d6f8e1z",
        ] {
            assert_eq!(plain_id(bad, true), None, "{bad}");
        }
        for bad in ["", "a b", "$(rm)", "--help"] {
            assert_eq!(plain_id(bad, false), None, "{bad}");
        }
    }

    #[test]
    fn a_status_the_table_doesnt_know_is_no_state() {
        let Session::PidFile {
            status: Some(s), ..
        } = &claude().session
        else {
            panic!("claude has a status");
        };
        assert_eq!(s.state("busy"), Some("working"));
        assert_eq!(s.state("idle"), Some("idle"));
        assert_eq!(s.state("compacting"), None);
    }

    #[test]
    fn claude_is_resumed_from_its_pid_file() {
        let home = temp("claude");
        let dir = home.join(".claude/sessions");
        std::fs::create_dir_all(&dir).unwrap();
        let p = process(
            4242,
            &["claude", "--dangerously-skip-permissions", "hi"],
            &home,
        );
        let run = found_with(p.clone());
        // Not written yet: an agent with no command.
        let none = look(&run, &home);
        assert_eq!(none.agent.command, None);
        assert!(none.status.is_some());
        let write = |id: &str| {
            std::fs::write(
                dir.join("4242.json"),
                format!(r#"{{"pid":4242,"sessionId":"{id}","cwd":"/x","status":"busy"}}"#),
            )
            .unwrap()
        };
        write(ID);
        let seen = look(&run, &home).agent;
        assert_eq!(
            seen.command.as_deref(),
            Some(format!("claude --resume {ID} --dangerously-skip-permissions").as_str())
        );
        assert_eq!(seen.session.as_deref(), Some(ID));
        assert_eq!(seen.cwd.as_deref(), home.to_str());
        // Not a UUID: not typed into a shell.
        write("--help");
        assert_eq!(look(&run, &home).agent.command, None);
        write(ID);
        // A file from before the process started is another's that had its pid.
        let later = Process {
            started: p.started + 3600,
            ..p
        };
        assert_eq!(look(&found_with(later), &home).agent.command, None);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn gemini_is_resumed_from_its_newest_chat_in_the_folder() {
        use sha2::{Digest, Sha256};
        let home = temp("gemini");
        let project = home.join("project");
        let p = process(7, &["node", "/x/bin/gemini", "--yolo"], &project);
        let hash = format!("{:x}", Sha256::digest(project.to_str().unwrap()));
        let chats = home.join(".gemini/tmp").join(hash).join("chats");
        std::fs::create_dir_all(&chats).unwrap();
        let chat = |name: &str, id: &str| {
            let text = format!("{{\n  \"sessionId\": \"{id}\",\n  \"messages\": []\n}}");
            std::fs::write(chats.join(name), text).unwrap();
        };
        let old = "11111111-2222-3333-4444-555555555555";
        chat("session-2026-01-01T10-00-11111111.json", old);
        std::thread::sleep(Duration::from_millis(20));
        chat("session-2026-01-01T10-01-7c3e9a41.json", ID);
        chat("logs.json", old);
        assert_eq!(
            look(&found_with(p.clone()), &home).agent.command,
            Some(format!("gemini --resume {ID} --yolo"))
        );
        // None written since it started: its chat isn't there yet.
        let later = Process {
            started: p.started + 3600,
            ..p
        };
        assert_eq!(look(&found_with(later), &home).agent.command, None);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn opencode_continues_the_folders_last_session() {
        let home = temp("opencode");
        let p = process(9, &["opencode", "--model", "x/y"], &home);
        let seen = look(&found_with(p), &home);
        assert_eq!(
            seen.agent.command.as_deref(),
            Some("opencode --model x/y --continue")
        );
        assert!(seen.status.is_none());
    }

    #[test]
    fn a_json_head_gives_up_a_field() {
        assert_eq!(
            head_field("{\n  \"sessionId\" :  \"a-b\",", "sessionId").as_deref(),
            Some("a-b")
        );
        assert_eq!(head_field("{\"sessionId\": 3}", "sessionId"), None);
    }

    #[test]
    fn a_watched_state_file_tells_each_change_and_its_removal() {
        let dir = temp("watch");
        let file = dir.join("1.json");
        let write =
            |status: &str| std::fs::write(&file, format!(r#"{{"status":"{status}"}}"#)).unwrap();
        write("busy");
        let Session::PidFile {
            status: Some(status),
            ..
        } = &claude().session
        else {
            panic!("claude has a status");
        };
        let (tx, rx) = mpsc::channel();
        let tx = Mutex::new(tx);
        let watch = Watch::default();
        let sink = move || -> Sink {
            Arc::new(move |id: u32, s: Option<&'static str>| {
                let _ = lock(&tx).send((id, s));
            })
        };
        assert_eq!(watch.track(5, &file, status, sink), Some("working"));
        // FSEvents starts with the next write after the watch.
        std::thread::sleep(Duration::from_millis(300));
        let next = || rx.recv_timeout(Duration::from_secs(5)).unwrap();
        write("idle");
        assert_eq!(next(), (5, Some("idle")));
        // Unknown to the table: no change told, the last state stands.
        write("compacting");
        write("waiting");
        assert_eq!(next(), (5, Some("waiting")));
        assert_eq!(
            watch.track(5, &file, status, || unreachable!()),
            Some("waiting")
        );
        std::fs::remove_file(&file).unwrap();
        assert_eq!(next(), (5, None));
        watch.forget(5);
        assert!(lock(&watch.watcher).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
