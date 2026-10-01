//! Coding agents running in the terminal's panes, told apart by a table rather than code per
//! agent: how to know one from its command line, where it keeps the id of the conversation it's
//! in, and the command that resumes that conversation. A restored terminal offers that command,
//! and an agent whose state file says it stopped working marks its pane (Watch).
//!
//! Only programs in the table are offered: running any other command line again could repeat a
//! deploy or a publish.

use crate::pty::{self, Process};
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
#[serde(rename_all = "camelCase")]
pub struct Status {
    field: String,
    working: Vec<String>,
    waiting: Vec<String>,
}

impl Status {
    fn state(&self, value: &str) -> &'static str {
        let is = |list: &[String]| list.iter().any(|v| v == value);
        if is(&self.working) {
            "working"
        } else if is(&self.waiting) {
            "waiting"
        } else {
            "idle"
        }
    }
}

/// Where the id of the conversation an agent is in can be read. Paths start at the home folder
/// (`~/`) and may hold `{pid}` (the agent's process) and `{cwdSha256}` (its folder, hashed).
#[derive(Deserialize, Debug)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Session {
    /// A JSON file per running process, with the id in `id` and maybe its state.
    #[serde(rename_all = "camelCase")]
    PidFile {
        path: String,
        id: String,
        status: Option<Status>,
    },
    /// The newest `<prefix>*.json` in `dir` written since the process started, the id in `id`
    /// near its top.
    #[serde(rename_all = "camelCase")]
    NewestFile {
        dir: String,
        prefix: String,
        id: String,
    },
    /// None to read: the resume command continues the folder's last conversation.
    Continue,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Adapter {
    /// What the user calls it ("Claude Code").
    pub name: String,
    /// argv[0]'s file name, or a script's that a runtime (node, bun) runs.
    programs: Vec<String>,
    /// Script paths that end with one of these (an npm install's `cli.js`).
    #[serde(default)]
    scripts: Vec<String>,
    session: Session,
    /// The resume command's words: `{flags}` the flags kept from the run, `{id}` the session's.
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
}

fn strings(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

fn arities(one: &[&str], optional: &[&str], many: &[&str]) -> HashMap<String, Arity> {
    [
        (one, Arity::One),
        (optional, Arity::Optional),
        (many, Arity::Many),
    ]
    .into_iter()
    .flat_map(|(names, arity)| names.iter().map(move |n| (n.to_string(), arity)))
    .collect()
}

/// The agents known. Flags as of the versions named; one added since is taken as a switch.
fn built_in() -> Vec<Adapter> {
    vec![
        // Claude Code 2.1.287, native (argv[0] "claude"); npm installs run `node …/cli.js`.
        // ~/.claude/sessions/<pid>.json holds sessionId and status (busy|shell|waiting|idle),
        // rewritten as it changes.
        Adapter {
            name: "Claude Code".into(),
            programs: strings(&["claude"]),
            scripts: strings(&["@anthropic-ai/claude-code/cli.js"]),
            session: Session::PidFile {
                path: "~/.claude/sessions/{pid}.json".into(),
                id: "sessionId".into(),
                status: Some(Status {
                    field: "status".into(),
                    working: strings(&["busy", "shell"]),
                    waiting: strings(&["waiting"]),
                }),
            },
            resume: strings(&["claude", "{flags}", "--resume", "{id}"]),
            flags: arities(
                &[
                    "--agent",
                    "--agents",
                    "--append-system-prompt",
                    "--append-system-prompt-file",
                    "--autocompact",
                    "--debug-file",
                    "--effort",
                    "--environment",
                    "--fallback-model",
                    "--input-format",
                    "--json-schema",
                    "--max-budget-usd",
                    "--model",
                    "-n",
                    "--name",
                    "--output-format",
                    "--permission-mode",
                    "--permission-prompts",
                    "--plugin-dir",
                    "--plugin-url",
                    "--remote-control-session-name-prefix",
                    "--session-id",
                    "--setting-sources",
                    "--settings",
                    "--system-prompt",
                    "--system-prompt-file",
                    "--system-prompt-snapshot",
                ],
                &[
                    "--cloud",
                    "-d",
                    "--debug",
                    "--from-pr",
                    "--prompt-suggestions",
                    "-r",
                    "--remote-control",
                    "--resume",
                    "--teleport",
                    "-w",
                    "--worktree",
                ],
                &[
                    "--add-dir",
                    "--allowedTools",
                    "--allowed-tools",
                    "--betas",
                    "--disallowedTools",
                    "--disallowed-tools",
                    "--file",
                    "--mcp-config",
                    "--tools",
                ],
            ),
            drop: strings(&[
                "-c",
                "--continue",
                "-r",
                "--resume",
                "--session-id",
                "--fork-session",
                "--from-pr",
                "--teleport",
                "--cloud",
                "-n",
                "--name",
                // A new worktree each time.
                "-w",
                "--worktree",
                "--tmux",
            ]),
            skip: strings(&[
                "-p",
                "--print",
                "--bg",
                "--background",
                "--desktop",
                "-h",
                "--help",
                "-v",
                "--version",
                "agents",
                "attach",
                "auth",
                "auto-mode",
                "doctor",
                "gateway",
                "import",
                "install",
                "kill",
                "logs",
                "mcp",
                "plugin",
                "plugins",
                "project",
                "respawn",
                "rm",
                "setup-token",
                "stop",
                "ultrareview",
                "update",
                "upgrade",
            ]),
        },
        // Gemini CLI 0.27, `node …/bin/gemini`, relaunched as a child in the same job. A chat
        // is ~/.gemini/tmp/<sha256 of its folder>/chats/session-<time>-<id8>.json; a resumed one
        // is written to again, so the newest written since the start is this run's.
        Adapter {
            name: "Gemini CLI".into(),
            programs: strings(&["gemini"]),
            scripts: strings(&["@google/gemini-cli/dist/index.js"]),
            session: Session::NewestFile {
                dir: "~/.gemini/tmp/{cwdSha256}/chats".into(),
                prefix: "session-".into(),
                id: "sessionId".into(),
            },
            resume: strings(&["gemini", "{flags}", "--resume", "{id}"]),
            flags: arities(
                &[
                    "--approval-mode",
                    "--delete-session",
                    "-e",
                    "--extensions",
                    "-i",
                    "-m",
                    "--model",
                    "-o",
                    "--output-format",
                    "-p",
                    "--prompt",
                    "--prompt-interactive",
                ],
                &["-r", "--resume"],
                &[
                    "--allowed-mcp-server-names",
                    "--allowed-tools",
                    "--include-directories",
                ],
            ),
            drop: strings(&["-r", "--resume", "-i", "--prompt-interactive"]),
            skip: strings(&[
                "-p",
                "--prompt",
                "--experimental-acp",
                "-l",
                "--list-extensions",
                "--list-sessions",
                "--delete-session",
                "-h",
                "--help",
                "-v",
                "--version",
                "mcp",
                "extensions",
                "extension",
                "skills",
                "skill",
                "hooks",
                "hook",
            ]),
        },
        // opencode 1.x, a native binary. Its sessions are in SQLite, so the folder's last one is
        // continued rather than read.
        Adapter {
            name: "opencode".into(),
            programs: strings(&["opencode"]),
            scripts: Vec::new(),
            session: Session::Continue,
            resume: strings(&["opencode", "{flags}", "--continue"]),
            flags: arities(
                &[
                    "--agent",
                    "--hostname",
                    "--log-level",
                    "-m",
                    "--mdns-domain",
                    "--model",
                    "--port",
                    "--prompt",
                    "--replay-limit",
                    "-s",
                    "--session",
                ],
                &[],
                &["--cors"],
            ),
            drop: strings(&["-c", "--continue", "-s", "--session", "--fork", "--prompt"]),
            skip: strings(&[
                "-h",
                "--help",
                "-v",
                "--version",
                "acp",
                "agent",
                "attach",
                "auth",
                "completion",
                "db",
                "debug",
                "export",
                "github",
                "import",
                "mcp",
                "models",
                "plug",
                "plugin",
                "pr",
                "providers",
                "run",
                "serve",
                "session",
                "stats",
                "uninstall",
                "upgrade",
                "web",
            ]),
        },
        // Codex: UNVERIFIED, not installed where this was written. From its docs: `codex resume
        // --last` continues the folder's last session; `exec` runs without one. npm's launcher
        // is `node …/bin/codex.js`.
        Adapter {
            name: "Codex".into(),
            programs: strings(&["codex"]),
            scripts: strings(&["@openai/codex/bin/codex.js"]),
            session: Session::Continue,
            resume: strings(&["codex", "resume", "--last", "{flags}"]),
            flags: arities(
                &[
                    "-a",
                    "--ask-for-approval",
                    "-c",
                    "--config",
                    "-C",
                    "--cd",
                    "-m",
                    "--model",
                    "-p",
                    "--profile",
                    "-s",
                    "--sandbox",
                ],
                &[],
                &["-i", "--image"],
            ),
            drop: strings(&["--last", "-i", "--image"]),
            skip: strings(&[
                "-h",
                "--help",
                "-V",
                "--version",
                "apply",
                "a",
                "app-server",
                "cloud",
                "completion",
                "debug",
                "e",
                "exec",
                "login",
                "logout",
                "mcp",
                "mcp-server",
                "sandbox",
            ]),
        },
    ]
}

fn adapters() -> &'static [Adapter] {
    static TABLE: OnceLock<Vec<Adapter>> = OnceLock::new();
    TABLE.get_or_init(built_in)
}

/// Runtimes that run an agent's script: its first word that isn't a flag of theirs.
const RUNTIMES: [&str; 3] = ["node", "bun", "deno"];

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

impl Adapter {
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
pub fn detect(leader: Process) -> Option<Found> {
    let pgid = leader.pid;
    found(adapters(), leader).or_else(|| {
        pty::job(pgid)
            .into_iter()
            .filter(|&pid| pid != pgid)
            .filter_map(pty::process)
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
    if open || out.is_empty() {
        out.push('\'');
    }
    if word.is_empty() {
        out.push('\'');
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

/// A conversation id is typed into a shell: only plain ones.
fn plain_id(id: &str) -> Option<String> {
    let plain = !id.is_empty()
        && id.len() <= 128
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    plain.then(|| id.to_string())
}

fn expand(template: &str, process: &Process, home: &Path) -> Option<PathBuf> {
    let mut path = template.replace("{pid}", &process.pid.to_string());
    if path.contains("{cwdSha256}") {
        let cwd = process.cwd.as_ref()?.to_str()?;
        path = path.replace("{cwdSha256}", &sha256_hex(cwd.as_bytes())?);
    }
    Some(match path.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None => PathBuf::from(path),
    })
}

fn modified(meta: &std::fs::Metadata) -> u64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs())
}

/// Written since the process started (to the second, with a second's slack): a file left by a
/// process that had the same pid before isn't this one's.
fn since_start(meta: &std::fs::Metadata, process: &Process) -> bool {
    modified(meta) + 1 >= process.started
}

#[cfg(target_os = "macos")]
fn sha256_hex(data: &[u8]) -> Option<String> {
    // CommonCrypto, part of libSystem: no hashing crate for one folder name.
    extern "C" {
        fn CC_SHA256(data: *const std::ffi::c_void, len: u32, md: *mut u8) -> *mut u8;
    }
    let mut md = [0u8; 32];
    let len = u32::try_from(data.len()).ok()?;
    unsafe { CC_SHA256(data.as_ptr().cast(), len, md.as_mut_ptr()) };
    Some(md.iter().map(|b| format!("{b:02x}")).collect())
}

#[cfg(not(target_os = "macos"))]
fn sha256_hex(_data: &[u8]) -> Option<String> {
    None
}

/// The string `field` holds near the top of a JSON file too big to parse whole for it.
fn head_field(head: &str, field: &str) -> Option<String> {
    let key = format!("\"{field}\"");
    let rest = head[head.find(&key)? + key.len()..].trim_start();
    let rest = rest.strip_prefix(':')?.trim_start().strip_prefix('"')?;
    Some(rest[..rest.find('"')?].to_string())
}

/// A pid file's id and state, when it's this process's.
fn read_pid_file(
    file: &Path,
    process: &Process,
    id: &str,
    status: Option<&Status>,
) -> Option<(Option<String>, Option<&'static str>)> {
    if !since_start(&std::fs::metadata(file).ok()?, process) {
        return None;
    }
    let json: serde_json::Value = serde_json::from_slice(&std::fs::read(file).ok()?).ok()?;
    let state = status.and_then(|s| Some(s.state(json.get(&s.field)?.as_str()?)));
    Some((
        json.get(id).and_then(|v| v.as_str()).and_then(plain_id),
        state,
    ))
}

fn newest_file(dir: &Path, prefix: &str, id: &str, process: &Process) -> Option<String> {
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
    head_field(&String::from_utf8_lossy(&head), id).and_then(|v| plain_id(&v))
}

/// What the page is told of a pane's agent.
#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Agent {
    pub name: String,
    /// What resumes its conversation; None when that can't be told (its id unread yet).
    pub command: Option<String>,
    /// working, waiting or idle, for an agent that reports it.
    pub state: Option<&'static str>,
}

/// An agent, and the file to watch for its state if it has one.
pub struct Look {
    pub agent: Agent,
    pub status: Option<(PathBuf, &'static Status)>,
}

pub fn look(found: &Found, home: &Path) -> Look {
    let adapter: &'static Adapter = found.adapter;
    let process = &found.process;
    let args = process.argv.get(found.args_at..).unwrap_or_default();
    let resume = |id: Option<&str>| rewrite(adapter, args, id);
    let (command, state, status) = match &adapter.session {
        Session::PidFile { path, id, status } => {
            let file = expand(path, process, home);
            let read = file
                .as_deref()
                .and_then(|f| read_pid_file(f, process, id, status.as_ref()));
            let (id, state) = read.unwrap_or((None, None));
            let watched = file.zip(status.as_ref());
            (id.and_then(|id| resume(Some(&id))), state, watched)
        }
        Session::NewestFile { dir, prefix, id } => {
            let id = expand(dir, process, home).and_then(|d| newest_file(&d, prefix, id, process));
            (id.and_then(|id| resume(Some(&id))), None, None)
        }
        Session::Continue => (resume(None), None, None),
    };
    Look {
        agent: Agent {
            name: adapter.name.clone(),
            command,
            state,
        },
        status,
    }
}

/// Gets a pane's id and its agent's new state (None: its file is gone, the agent with it).
pub type Sink = Arc<dyn Fn(u32, Option<&'static str>) + Send + Sync>;

struct Tracked {
    id: u32,
    status: &'static Status,
    last: Option<&'static str>,
}

type Files = Mutex<HashMap<PathBuf, Tracked>>;

/// The state files of the agents in panes, watched (FSEvents on macOS) rather than polled: no
/// work between an agent's writes, and none at all without an agent.
#[derive(Default)]
pub struct Watch {
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
    Some(status.state(json.get(&status.field)?.as_str()?))
}

impl Watch {
    /// Watches pane `id`'s agent's state `file`, in place of any it had; `sink` hears of changes.
    pub fn track(&self, id: u32, file: &Path, status: &'static Status, sink: Sink) {
        // Events name the real path.
        let (Some(dir), Some(name)) = (file.parent(), file.file_name()) else {
            return;
        };
        let Ok(dir) = dir.canonicalize() else {
            return;
        };
        let file = dir.join(name);
        {
            let mut files = lock(&self.files);
            if files.get(&file).is_none_or(|t| t.id != id) {
                files.retain(|_, t| t.id != id);
                let last = read_state(&file, status);
                files.insert(file, Tracked { id, status, last });
            }
        }
        let mut watcher = lock(&self.watcher);
        if watcher.is_none() {
            let files = Arc::downgrade(&self.files);
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
    }

    /// Stops watching pane `id`'s agent; the watcher goes with the last one.
    pub fn forget(&self, id: u32) {
        let empty = {
            let mut files = lock(&self.files);
            files.retain(|_, t| t.id != id);
            files.is_empty()
        };
        if empty {
            self.stop();
        }
    }

    pub fn forget_all(&self) {
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
            // Mid-write (or unreadable): the next event has it.
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
        strings(words)
    }

    fn process(pid: u32, argv: &[&str], cwd: &Path) -> Process {
        Process {
            pid,
            argv: args(argv),
            cwd: Some(cwd.to_path_buf()),
            started: std::time::SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs()
                - 60,
        }
    }

    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("gitviber-agents-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_resume_keeps_flags_and_drops_the_prompt_and_the_session_picked() {
        let id = Some("abc-123");
        let cases: &[(&[&str], Option<&str>)] = &[
            (&[], Some("claude --resume abc-123")),
            (
                &["--dangerously-skip-permissions", "fix the login bug"],
                Some("claude --dangerously-skip-permissions --resume abc-123"),
            ),
            (
                &["--model", "opus", "--resume", "old-id", "go on"],
                Some("claude --model opus --resume abc-123"),
            ),
            // An optional value: a flag after it isn't one.
            (
                &["-r", "--verbose"],
                Some("claude --verbose --resume abc-123"),
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
                Some("claude --add-dir ../a ../b --model=sonnet --resume abc-123"),
            ),
            (
                &["--append-system-prompt", "be brief, it's late"],
                Some(r"claude --append-system-prompt 'be brief, it'\''s late' --resume abc-123"),
            ),
            (&["--", "--not-a-flag"], Some("claude --resume abc-123")),
            (&["-p", "explain"], None),
            (&["--print"], None),
            (&["mcp", "serve"], None),
            (&["update"], None),
            // A subcommand's name later on is a prompt's word.
            (
                &["--model", "opus", "fix", "mcp"],
                Some("claude --model opus --resume abc-123"),
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
            Some("gemini -y -m gemini-2.5-pro --resume u-1")
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
        assert_eq!(at(&["node", "server.js"]), None);
        assert_eq!(at(&["vim", "claude"]), None);
        assert_eq!(at(&["npm", "run", "dev"]), None);
    }

    #[test]
    fn claude_is_resumed_from_its_pid_file_with_its_state() {
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
        std::fs::write(
            dir.join("4242.json"),
            r#"{"pid":4242,"sessionId":"7c3e9a41-bf76","cwd":"/x","status":"busy"}"#,
        )
        .unwrap();
        let seen = look(&run, &home);
        assert_eq!(
            seen.agent,
            Agent {
                name: "Claude Code".into(),
                command: Some(
                    "claude --dangerously-skip-permissions --resume 7c3e9a41-bf76".into()
                ),
                state: Some("working"),
            }
        );
        // A file from before the process started is another's that had its pid.
        let later = Process {
            started: p.started + 3600,
            ..p
        };
        assert_eq!(look(&found_with(later), &home).agent.command, None);
        let _ = std::fs::remove_dir_all(&home);
    }

    fn found_with(p: Process) -> Found {
        found(adapters(), p).unwrap()
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn gemini_is_resumed_from_its_newest_chat_in_the_folder() {
        assert_eq!(
            sha256_hex(b"abc").unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let home = temp("gemini");
        let project = home.join("project");
        let p = process(7, &["node", "/x/bin/gemini", "--yolo"], &project);
        let chats = home
            .join(".gemini/tmp")
            .join(sha256_hex(project.to_str().unwrap().as_bytes()).unwrap())
            .join("chats");
        std::fs::create_dir_all(&chats).unwrap();
        let chat = |name: &str, id: &str| {
            std::fs::write(
                chats.join(name),
                format!("{{\n  \"sessionId\": \"{id}\",\n  \"messages\": []\n}}"),
            )
            .unwrap();
        };
        chat("session-2026-01-01T10-00-old.json", "old-id");
        std::thread::sleep(Duration::from_millis(20));
        chat("session-2026-01-01T10-01-new.json", "new-uuid");
        chat("logs.json", "not-a-chat");
        assert_eq!(
            look(&found_with(p.clone()), &home).agent.command.as_deref(),
            Some("gemini --yolo --resume new-uuid")
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
        assert_eq!(plain_id("a b"), None);
        assert_eq!(plain_id("$(rm)"), None);
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
        watch.track(
            5,
            &file,
            status,
            Arc::new(move |id: u32, s: Option<&'static str>| {
                let _ = tx.lock().unwrap().send((id, s));
            }),
        );
        // FSEvents starts with the next write after the watch.
        std::thread::sleep(Duration::from_millis(300));
        let next = || rx.recv_timeout(Duration::from_secs(5)).unwrap();
        write("idle");
        assert_eq!(next(), (5, Some("idle")));
        write("waiting");
        assert_eq!(next(), (5, Some("waiting")));
        std::fs::remove_file(&file).unwrap();
        assert_eq!(next(), (5, None));
        watch.forget(5);
        assert!(lock(&watch.watcher).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
