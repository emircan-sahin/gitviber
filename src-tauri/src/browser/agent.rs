//! `gitviber browser` commands (control.rs), each run on its request's connection thread
//! (server.rs) as a few steps on the main thread, where the views are, waiting in between.

use super::control::{
    self, AgentScreen, Click, Command, Found, Listed, NewTab, Pane, Reply, Request, Scroll,
    Sessions, Tabs, Target, Wait,
};
use super::downloads;
use super::input::{self, Half, Keys, Pointer};
use super::marks;
use super::network;
use super::notes;
use super::output;
use super::picks::{self, Bounds};
use super::sessions::{self, Session, Shelf};
use super::{Go, PageState};
use crate::state::{AppState, Res};
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, LazyLock, Mutex, MutexGuard, PoisonError, TryLockError};
use std::time::{Duration, Instant, SystemTime};
use tauri::{AppHandle, Manager, Url};

/// The agent script, its parts joined in order (scripts/agent/prelude.js says what each holds);
/// the tests join the same list (src/lib/browser/agentSource.test.ts).
const AGENT: &str = concat!(
    include_str!("scripts/agent/prelude.js"),
    include_str!("scripts/agent/read.js"),
    include_str!("scripts/agent/act.js"),
    include_str!("scripts/agent/watch.js"),
    include_str!("scripts/agent/run.js"),
);
/// A step on the main thread answers at once: past this the app is stuck.
const STEP: Duration = Duration::from_secs(10);
/// What a script or a picture may take.
const SCRIPT: Duration = Duration::from_secs(30);
/// How long a page gets to load after open, a click or a key before the reply goes anyway.
const LOAD: Duration = Duration::from_secs(15);
const POLL: Duration = Duration::from_millis(100);
/// How long an action's requests and redraws get to land before its reply says what changed: a
/// reply sooner would miss the answer to the request a click sent. A load it starts gets LOAD.
const SETTLE: Duration = Duration::from_millis(500);
/// The page still this long counts as done redrawing.
const QUIET: Duration = Duration::from_millis(100);
/// An eval's result, cut: an agent reads it.
const EVAL_MAX: usize = 20_000;
/// What `eval`'s script gives: `=` and its value however it came, or `!` and what it threw.
const SERIALIZE: &str = include_str!("scripts/serialize.js");
/// What `wait --fn`'s script gives.
const TRUTHY: &str = include_str!("scripts/truthy.js");
/// The page's storage for a session; in GitViber's world, where the page can't stand in for it.
const STORAGE_READ: &str = "return JSON.stringify({ origin: location.origin, local: Object.entries(localStorage), session: Object.entries(sessionStorage) });";
/// Written only on the session's origin; elsewhere, where the page is instead.
const STORAGE_WRITE: &str = "const s = JSON.parse(input); if (location.origin !== s.origin) return location.origin; localStorage.clear(); sessionStorage.clear(); for (const [k, v] of s.local) localStorage.setItem(k, v); for (const [k, v] of s.session) sessionStorage.setItem(k, v); return '';";
const NO_INDEXED_DB: &str = "IndexedDB isn't carried.";
/// How long a page has to open its file chooser after the click that asks for it.
const CHOOSER: Duration = Duration::from_secs(3);
/// The page's height and how far down it is, for the wheel: its window's, not a panel's in it.
const SCROLLED: &str = "return JSON.stringify([innerHeight, Math.round(scrollY), Math.max(0, Math.round(document.documentElement.scrollHeight - innerHeight))]);";
/// How long a page gets to scroll after the wheel turns: WebKit scrolls on a thread of its own.
const SCROLLING: Duration = Duration::from_millis(100);

pub fn run(app: &AppHandle, req: Request) -> Reply {
    command(app, req).unwrap_or_else(Reply::error)
}

fn command(app: &AppHandle, req: Request) -> Res<Reply> {
    if let Some(op) = control::parse_pane(&req.cmd, &req.args) {
        let op = op?;
        let (pty, root) = pane(app, &req)?;
        // A page shown the user is the one command allowed with control off: its reply is a
        // fixed text, with none of the pane's notes in it.
        let shows = matches!(op, Pane::Tab(Tabs::Show { .. }));
        // A tab command names pages and their titles; a session's, origins and counts only.
        let (reply, marked) = match op {
            Pane::Tab(op) => (tabs(app, pty, root, req.tab, op, &req.cwd), !shows),
            Pane::Session(op) => (sessions(app, pty, root, req.tab, op), false),
        };
        // What happened to the pane's tabs meanwhile, marked and cleaned as the rest (notes.rs).
        let told = if shows {
            vec![]
        } else {
            notes::take(pty, None)
        };
        let reply = notes::with(reply.unwrap_or_else(Reply::error), &told);
        let nonce = output::nonce().ok_or(NO_NONCE)?;
        let ctx = output::Ctx {
            marked: marked || !told.is_empty(),
            origin: None,
            nonce: &nonce,
            reveal: req.reveal,
            secrets: &secrets(),
            dialog: None,
            looped: false,
        };
        return Ok(output::finish(reply, &ctx));
    }
    let cmd = control::parse(&req.cmd, &req.args)?;
    let (pty, root) = pane(app, &req)?;
    let name = target(req.tab)?;
    let first = match &cmd {
        Command::Open(url) => Some(url.clone()),
        _ => None,
    };
    let (id, found) = main(app, move |app| {
        super::agent_tab(app, pty, &root, name.as_deref(), first.as_deref())
    })?;
    // Made: just made, at the address an open asks for.
    let made = found == Found::Made;
    if let Command::FillSecret(_, name) = &cmd {
        let value = req
            .secret
            .clone()
            .ok_or("No value came with --secret: run it as `gitviber browser`.")?;
        if value.chars().count() < output::SHORTEST_SECRET {
            return Err(format!(
                "${name} is shorter than {} characters, too short to hide in replies: fill it \
                 as text if it isn't secret.",
                output::SHORTEST_SECRET
            ));
        }
        remember(&id, |m| {
            m.secrets.retain(|(n, _)| n != name);
            m.secrets.push((name.clone(), value));
        });
    }
    let said = format!("{} {}", req.cmd, req.args.join(" "));
    let acts = cmd.acts();
    drive(app, id, found, |tab| {
        let ran = if acts {
            tab.yours().and_then(|()| {
                let ran = tab.run(cmd, made, &req.cwd, req.reveal);
                // Taken while it ran: whatever it did or didn't do, the page isn't what it saw.
                match tab.users()? {
                    Some(name) => Err(control::interrupted(&name)),
                    None => ran,
                }
            })
        } else {
            tab.run(cmd, made, &req.cwd, req.reveal)
        };
        tab.finish(ran, &said, req.reveal, pty)
    })
}

/// The pane the request came from (its token says so: server.rs), and its own folder as its
/// workspace knows it: its tabs go to that worktree.
fn pane(app: &AppHandle, req: &Request) -> Res<(u32, String)> {
    let ptys = &app.state::<AppState>().ptys;
    req.pty
        .and_then(|pty| Some((pty, ptys.pane(pty)?.to_string_lossy().into_owned())))
        .ok_or_else(|| "This terminal isn't one of GitViber's open panes.".into())
}

/// The tab named by `--tab` or GITVIBER_BROWSER_TAB, read only where a command goes to a tab.
fn target(name: Option<String>) -> Res<Option<String>> {
    match name.as_deref() {
        Some("") => Err("--tab needs the tab's name".into()),
        Some(name) => control::tab_name(name).map(Some),
        None => Ok(None),
    }
}

/// `steps` on tab `id` in its turn, as one command; on a tab just loaded again from parking,
/// once its page has, as reading or acting on it before then finds nothing.
fn drive<T>(
    app: &AppHandle,
    id: String,
    found: Found,
    steps: impl FnOnce(&Tab) -> Res<T>,
) -> Res<T> {
    let queue = queue(&id);
    let _turn = take_turn(&queue, STEP)?;
    // What a `wait --download` after it looks back to.
    downloads::command(&id);
    let tab = Tab {
        app: app.clone(),
        id,
        command: NEXT_COMMAND.fetch_add(1, Ordering::Relaxed),
    };
    tab.start()?;
    if found == Found::Restored {
        tab.loaded(LOAD)?;
    }
    steps(&tab)
}

/// `tab …`; `to`: the tab a command would go to, which `tab new` takes as its name and `tab
/// close` closes, unless they name one.
fn tabs(
    app: &AppHandle,
    pty: u32,
    root: String,
    to: Option<String>,
    op: Tabs,
    cwd: &str,
) -> Res<Reply> {
    match op {
        Tabs::New {
            name,
            url,
            shared,
            session,
            idle,
        } => {
            let name = name.map_or_else(|| target(to), |n| Ok(Some(n)))?;
            // Read first: a session that isn't there makes no tab.
            let saved = match session {
                Some(s) => Some((shelf(app, &root)?.load(&s)?, s)),
                None => None,
            };
            let idle = idle.map_or(control::IDLE, |m| Duration::from_secs(u64::from(m) * 60));
            // Loaded with a session, it goes to the session's origin first.
            let first = url.clone().filter(|_| saved.is_none());
            let at = root.clone();
            let (id, name) = main(app, move |app| {
                let new = NewTab {
                    name: name.as_deref(),
                    default: false,
                    shared,
                    idle,
                    show: false,
                };
                super::agent_new_tab(app, pty, &at, new, first.as_deref())
            })?;
            let mut said = vec![name.clone()];
            // What was done so far, then what went wrong.
            let more = |said: &mut Vec<String>, step: Res<String>| -> Res<()> {
                let out = step.map_err(|e| format!("{}\n{e}", said.join("\n")))?;
                said.push(out);
                Ok(())
            };
            if let Some((session, s)) = &saved {
                more(&mut said, load(app, (id.clone(), Found::Made), s, session))?;
            }
            if let Some(url) = url {
                let made = saved.is_none();
                let opened = drive(app, id, Found::Made, |tab| {
                    tab.run(Command::Open(url), made, cwd, false)
                });
                more(&mut said, opened.map(|(r, _)| r.out.unwrap_or_default()))?;
            }
            Ok(Reply::out(said.join("\n")))
        }
        Tabs::Show { name, url } => {
            let name = name.map_or_else(|| target(to), |n| Ok(Some(n)))?;
            let (_, name) = main(app, move |app| {
                // Private like any agent tab: the user's logins stay out of reach of a page an
                // agent chose. Never sleeping or closing unused, as it is theirs.
                let new = NewTab {
                    name: name.as_deref(),
                    default: false,
                    shared: false,
                    idle: Duration::ZERO,
                    show: true,
                };
                super::agent_new_tab(app, pty, &root, new, Some(&url))
            })?;
            Ok(Reply::out(format!(
                "{name}\nOpened for the user, in a private session (not their logins). It's \
                 theirs: no command reaches it but `tab list` until they give it back."
            )))
        }
        Tabs::List => {
            let tabs = main(app, move |_| super::agent_tabs(pty))?;
            let data = serde_json::to_value(&tabs).map_err(|e| e.to_string())?;
            Ok(Reply::data(tabs_text(&tabs), json!({ "tabs": data })))
        }
        Tabs::Release(named) => {
            let name = named.map_or_else(|| target(to), |n| Ok(Some(n)))?;
            let id = main(app, move |_| super::agent_find_tab(pty, name.as_deref()))?;
            // Only the user's own button hands back a page opened for them.
            let shown = id.clone();
            if let Some(name) = main(app, move |_| super::agent_shown(&shown))? {
                return Err(control::shown_tab(&name));
            }
            let given = main(app, move |app| super::agent_give_back(app, &id))?;
            Ok(Reply::out(match given {
                Some(name) => format!("Gave tab {name} back to the pane's agents"),
                None => "The tab is the agents' already".into(),
            }))
        }
        Tabs::Close(named) => {
            let name = named.map_or_else(|| target(to), |n| Ok(Some(n)))?;
            let id = main(app, move |_| super::agent_find_tab(pty, name.as_deref()))?;
            let users = id.clone();
            if let Some(name) = main(app, move |_| super::agent_users(&users))? {
                return Err(control::users_tab(&name));
            }
            // After a command running on it, not through it.
            let queue = queue(&id);
            let _turn = take_turn(&queue, STEP)?;
            let gone = id.clone();
            let closed = main(app, move |app| super::agent_close_tab(app, &gone))?;
            Ok(Reply::out(format!("Closed tab {closed}")))
        }
    }
}

/// `session …`, saved with the pane's repository; `to`: the tab the command goes to.
fn sessions(
    app: &AppHandle,
    pty: u32,
    root: String,
    to: Option<String>,
    op: Sessions,
) -> Res<Reply> {
    let shelf_here = || shelf(app, &root);
    let tab = || {
        let (root, name) = (root.clone(), target(to)?);
        main(app, move |app| {
            super::agent_tab(app, pty, &root, name.as_deref(), None)
        })
    };
    match op {
        Sessions::List => Ok(Reply::out(sessions_text(&shelf_here()?.list(), now()))),
        Sessions::Delete(s) => {
            shelf_here()?.delete(&s)?;
            Ok(Reply::out(format!("Deleted session {s}")))
        }
        Sessions::Save(s, force) => {
            let shelf = shelf_here()?;
            let (id, found) = tab()?;
            // The user's tab holds the user's sign-ins: not saved for the agents.
            let session = drive(app, id, found, |tab| {
                tab.yours()?;
                tab.session()
            })?;
            shelf.save(&s, &session, force)?;
            Ok(Reply::out(format!(
                "Saved session {s}: {}. {NO_INDEXED_DB}",
                summary(&session)
            )))
        }
        Sessions::Load(s) => {
            let session = shelf_here()?.load(&s)?;
            let (id, found) = tab()?;
            drive(app, id, found, |tab| {
                tab.yours()?;
                tab.load_session(&s, &session)
            })
            .map(Reply::out)
        }
        Sessions::Clear => {
            let (id, found) = tab()?;
            drive(app, id, found, |tab| {
                tab.yours()?;
                tab.clear_session()
            })
            .map(Reply::out)
        }
    }
}

/// Session `name` into the tab, for `session load` and `tab new --session` alike.
fn load(
    app: &AppHandle,
    (id, found): (String, Found),
    name: &str,
    session: &Session,
) -> Res<String> {
    drive(app, id, found, |tab| tab.load_session(name, session))
}

/// Agent tab `id` closed as `tab close` closes it, once the command running on it is done: its
/// page closed itself (window.close()), maybe in that very command.
pub(super) fn close_soon(app: &AppHandle, id: String) {
    let app = app.clone();
    std::thread::spawn(move || {
        let queue = queue(&id);
        let Ok(_turn) = take_turn(&queue, control::CLI_WAIT) else {
            return;
        };
        let _ = main(&app, move |app| super::agent_close_tab(app, &id));
    });
}

fn shelf(app: &AppHandle, root: &str) -> Res<Shelf> {
    let data = app.path().app_data_dir().map_err(|e| e.to_string())?;
    Ok(Shelf::of(&data, &sessions::repository(root)?))
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

static NEXT_COMMAND: AtomicU64 = AtomicU64::new(1);

/// What a tab's commands leave for the next, by tab id; gone with the tab (control.rs `gone`).
#[derive(Default)]
struct Memory {
    /// The highest ref the tab handed out, on any of its pages.
    last_ref: u64,
    history: History,
    /// Filled with `fill --secret`, by name: hidden from every reply after.
    secrets: Vec<(String, String)>,
}

/// The tab's last few commands with the page as each left it, for the nudge out of a loop.
#[derive(Default)]
struct History(VecDeque<(u64, u64)>);

/// How many commands alike in a row are a loop: all History keeps.
const LOOP: usize = 3;

impl History {
    /// `command` (its name and arguments) run, the page then in `state`: whether it's the
    /// LOOPth alike in a row, the page the same after each.
    fn again(&mut self, command: &str, state: &str) -> bool {
        if self.0.len() == LOOP {
            self.0.pop_front();
        }
        self.0.push_back((hash(command), hash(state)));
        let last = self.0.back().copied();
        self.0.len() == LOOP && self.0.iter().all(|c| Some(*c) == last)
    }
}

fn hash(s: &str) -> u64 {
    let mut h = DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

/// What a command leaves besides its reply: the page as it left it, for the loop nudge.
#[derive(Default)]
struct Meta {
    state: Option<String>,
}

/// A reply with no page state to compare.
fn plain(reply: Reply) -> (Reply, Meta) {
    (reply, Meta::default())
}

/// A reply, and the page as the agent read it in it.
fn read(reply: Reply, state: &str) -> (Reply, Meta) {
    let state = Some(state.to_string());
    (reply, Meta { state })
}

/// No nonce, no reply: page text unmarked, or marked by one the page could guess, is worse.
const NO_NONCE: &str = "No randomness to mark the page's text with: nothing of it is shown.";

/// Every tab's secrets: what a reply hides, whichever tab it reads.
fn secrets() -> Vec<(String, String)> {
    memories()
        .values()
        .flat_map(|m| m.secrets.clone())
        .collect()
}

/// A tab gone: what its commands kept goes too, its secrets with it.
pub(super) fn forget(id: &str) {
    memories().remove(id);
}

fn memories() -> MutexGuard<'static, HashMap<String, Memory>> {
    static MEMORY: LazyLock<Mutex<HashMap<String, Memory>>> = LazyLock::new(Mutex::default);
    MEMORY.lock().unwrap_or_else(PoisonError::into_inner)
}

fn remember<T>(id: &str, f: impl FnOnce(&mut Memory) -> T) -> T {
    f(memories().entry(id.into()).or_default())
}

/// Each tab's turn: agents in a pane share its tabs, and two commands at once would interleave
/// their steps (a click between another's fill and its Enter). Kept only while a command holds
/// or waits for one, so a closed tab's goes with its last command.
fn queue(id: &str) -> Arc<Mutex<()>> {
    static QUEUES: LazyLock<Mutex<HashMap<String, Arc<Mutex<()>>>>> = LazyLock::new(Mutex::default);
    let mut queues = QUEUES.lock().unwrap_or_else(PoisonError::into_inner);
    queues.retain(|_, q| Arc::strong_count(q) > 1);
    queues.entry(id.into()).or_default().clone()
}

fn take_turn(queue: &Mutex<()>, wait: Duration) -> Res<MutexGuard<'_, ()>> {
    let until = Instant::now() + wait;
    loop {
        match queue.try_lock() {
            Ok(turn) => return Ok(turn),
            Err(TryLockError::Poisoned(turn)) => return Ok(turn.into_inner()),
            Err(TryLockError::WouldBlock) if Instant::now() < until => std::thread::sleep(POLL / 5),
            Err(TryLockError::WouldBlock) => {
                return Err(
                    "Another command is still running on this tab: try again once it's done."
                        .into(),
                )
            }
        }
    }
}

/// `f` on the main thread, its answer back here.
fn main<T: Send + 'static>(
    app: &AppHandle,
    f: impl FnOnce(&AppHandle) -> Res<T> + Send + 'static,
) -> Res<T> {
    let (tx, rx) = mpsc::channel();
    let handle = app.clone();
    app.run_on_main_thread(move || {
        let _ = tx.send(f(&handle));
    })
    .map_err(|e| e.to_string())?;
    rx.recv_timeout(STEP)
        .map_err(|_| "GitViber didn't answer in time.".to_string())?
}

/// `f` on the main thread, its answer whenever it hands it to `done` (a script's, a picture's).
fn later<T: Send + 'static>(
    app: &AppHandle,
    f: impl FnOnce(&AppHandle, Box<dyn FnOnce(T)>) -> Res<()> + Send + 'static,
) -> Res<T> {
    later_for(app, SCRIPT, f)
}

/// As `later`, waiting `wait` at most.
fn later_for<T: Send + 'static>(
    app: &AppHandle,
    wait: Duration,
    f: impl FnOnce(&AppHandle, Box<dyn FnOnce(T)>) -> Res<()> + Send + 'static,
) -> Res<T> {
    let (tx, rx) = mpsc::channel::<Res<T>>();
    let handle = app.clone();
    app.run_on_main_thread(move || {
        let failed = tx.clone();
        let done: Box<dyn FnOnce(T)> = Box::new(move |v| {
            let _ = tx.send(Ok(v));
        });
        if let Err(e) = f(&handle, done) {
            let _ = failed.send(Err(e));
        }
    })
    .map_err(|e| e.to_string())?;
    rx.recv_timeout(wait)
        .map_err(|_| "The page didn't answer in time.".to_string())?
}

/// What's where a pointer command acts (scripts/agent/act.js `aim`).
#[derive(serde::Deserialize)]
struct Spot {
    x: f64,
    y: f64,
    label: String,
    #[serde(default)]
    picker: bool,
    #[serde(default)]
    drag: bool,
    #[serde(default)]
    scrolled: bool,
}

/// A point as a reply shows it: whole CSS px.
fn xy((x, y): (f64, f64)) -> String {
    format!("{},{}", x.round(), y.round())
}

/// The pane's tab for one command, running it from `start` for as long as this lives. Its end
/// is queued after the start on the main thread even when the start timed out here, so the tab
/// can't stay answering its dialogs.
struct Tab {
    app: AppHandle,
    id: String,
    command: u64,
}

impl Drop for Tab {
    fn drop(&mut self) {
        let (id, command) = (std::mem::take(&mut self.id), self.command);
        let _ = self
            .app
            .run_on_main_thread(move || super::agent_running(&id, command, false));
    }
}

impl Tab {
    fn start(&self) -> Res<()> {
        let (id, command) = (self.id.clone(), self.command);
        main(&self.app, move |_| {
            super::agent_running(&id, command, true);
            Ok(())
        })
    }

    /// The tab's name while the user has it.
    fn users(&self) -> Res<Option<String>> {
        let id = self.id.clone();
        main(&self.app, move |_| super::agent_users(&id))
    }

    /// Refused, at once, on a tab the user has.
    fn yours(&self) -> Res<()> {
        match self.users()? {
            Some(name) => Err(control::users_tab(&name)),
            None => Ok(()),
        }
    }

    fn run(&self, cmd: Command, made: bool, cwd: &str, reveal: bool) -> Res<(Reply, Meta)> {
        let act = |input: Value| self.acted(|| self.said(input));
        Ok(match cmd {
            Command::Open(url) => {
                if !made {
                    let id = self.id.clone();
                    main(&self.app, move |_| super::navigate(&id, &url))?;
                }
                plain(self.arrived("Opened")?)
            }
            Command::Back => plain(self.go(Go::Back, "Back at")?),
            Command::Forward => plain(self.go(Go::Forward, "Forward at")?),
            Command::Reload => plain(self.go(Go::Reload, "Reloaded")?),
            Command::Url => {
                let id = self.id.clone();
                // A page that can't say whether it plays leaves the line out.
                let (page, audio) = main(&self.app, move |_| {
                    let page = super::agent_page(&id)?.0;
                    Ok((page, super::agent_playing(&id).unwrap_or(false)))
                })?;
                plain(Reply::out(url_reply(&page.url, &page.title, audio)))
            }
            Command::Snapshot(snap) => self.snapshot(snap, cwd, reveal)?,
            Command::Find(pattern) => {
                let out = self.said(json!({ "cmd": "find", "pattern": pattern }))?;
                read(Reply::out(&out), &out)
            }
            Command::Get(get) => {
                let input = json!({ "cmd": "get", "what": get.what, "names": get.names, "target": get.target });
                let found = self.script(input)?;
                let out = found["out"].as_str().unwrap_or_default();
                let reply = match found.get("box") {
                    Some(b) => Reply::data(out, json!({ "box": b })),
                    None => Reply::out(out),
                };
                read(reply, out)
            }
            Command::FillSecret(target, name) => {
                let value = remember(&self.id, |m| {
                    let found = m.secrets.iter().find(|(n, _)| *n == name);
                    found.map(|(_, v)| v.clone())
                });
                let value = value.ok_or("No value came with --secret.")?;
                let input =
                    json!({ "cmd": "fill", "target": target, "text": value, "secret": true });
                self.acted(|| Ok(format!("{} (from ${name})", self.said(input)?)))?
            }
            Command::Click(Click {
                target: Target::El(target),
                synthetic: true,
                ..
            }) => act(json!({ "cmd": "click", "target": target }))?,
            Command::Click(click) => self.acted(|| self.click(click))?,
            Command::Fill(target, text) => {
                act(json!({ "cmd": "fill", "target": target, "text": text }))?
            }
            Command::Type {
                target,
                text,
                synthetic: true,
            } => act(json!({ "cmd": "type", "target": target, "text": text }))?,
            Command::Type { target, text, .. } => self.acted(|| self.type_in(&target, &text))?,
            Command::Select(target, values) => {
                act(json!({ "cmd": "select", "target": target, "values": values }))?
            }
            Command::Check(target, on) => {
                act(json!({ "cmd": "check", "target": target, "on": on }))?
            }
            Command::Hover(target) => self.acted(|| self.hover(&target))?,
            Command::Drag(from, to) => self.acted(|| self.drag(&from, &to))?,
            Command::Upload(target, files) => {
                let files = control::upload_paths(Path::new(cwd), &files)?;
                self.acted(|| self.upload(target, files))?
            }
            Command::Press {
                chord,
                synthetic: true,
                ..
            } => act(json!({ "cmd": "press", "key": chord }))?,
            Command::Press { chord, hold, .. } => self.acted(|| self.press(&chord, hold))?,
            Command::Key(down, key) => self.acted(|| self.key(down, &key))?,
            Command::Scroll(Scroll::To(target), _) => {
                act(json!({ "cmd": "scroll", "to": target }))?
            }
            Command::Scroll(way, px) => self.acted(|| self.wheel(way == Scroll::Up, px))?,
            Command::Wait(what, timeout) => plain(self.wait(what, timeout)?),
            Command::Eval(js) => plain(self.eval(&js)?),
            Command::Screenshot(shot) => plain(self.screenshot(shot, cwd)?),
            Command::Console { level, all, clear } => {
                plain(self.console(level.as_deref(), all, clear)?)
            }
            Command::WaitDownload(timeout) => {
                plain(Reply::out(downloads::wait(&self.id, timeout)?))
            }
            Command::Downloads(clear) => {
                let cache = downloads::cache(&self.app)?;
                plain(downloads::listing(
                    &cache,
                    &self.id,
                    clear,
                    SystemTime::now(),
                ))
            }
            Command::Network(ask) => plain(self.network(ask, reveal)?),
            Command::Device(screen) => plain(self.device(screen)?),
            Command::Appearance(dark) => {
                let id = self.id.clone();
                main(&self.app, move |app| {
                    super::agent_appearance(app, &id, dark)
                })?;
                plain(Reply::out(match dark {
                    Some(true) => "The page is shown dark.",
                    Some(false) => "The page is shown light.",
                    None => "The page is shown as the app is.",
                }))
            }
            Command::Dialogs(accept) => {
                let id = self.id.clone();
                main(&self.app, move |_| super::agent_dialogs(&id, accept))?;
                plain(Reply::out(if accept {
                    "The page's dialogs are accepted while commands run on it."
                } else {
                    "The page's dialogs are dismissed while commands run on it."
                }))
            }
        })
    }

    /// The page as a tree; with --json the refs gone since too; with -o, in full in a file, as
    /// a reply would give it.
    fn snapshot(&self, snap: control::Snap, cwd: &str, reveal: bool) -> Res<(Reply, Meta)> {
        let file = snap
            .file
            .as_deref()
            .map(|f| control::text_path(Path::new(cwd), f))
            .transpose()?;
        let input = json!({ "cmd": "snapshot", "interactive": snap.interactive, "scope": snap.scope,
            "depth": snap.depth, "boxes": snap.boxes, "file": file.is_some() });
        let found = self.script(input)?;
        let out = found["out"].as_str().unwrap_or_default();
        let Some(path) = file else {
            let data = json!({ "snapshot": out, "removed": found["removed"] });
            return Ok(read(Reply::data(out, data), out));
        };
        let nonce = output::nonce().ok_or(NO_NONCE)?;
        let origin = control::origin(&self.page()?.0.url);
        let ctx = output::Ctx {
            marked: true,
            origin: Some(&origin),
            nonce: &nonce,
            reveal,
            secrets: &secrets(),
            dialog: None,
            looped: false,
        };
        let text = output::finish(Reply::out(out), &ctx)
            .out
            .unwrap_or_default();
        std::fs::write(&path, format!("{text}\n"))
            .map_err(|e| format!("Couldn't write {}: {e}", path.display()))?;
        let lines = out.lines().count();
        let wrote = Reply::out(format!("Wrote {} ({lines} lines)", path.display()));
        Ok(read(wrote, out))
    }

    /// A command's reply as the agent gets it, a failure's too (output::finish): marked as the
    /// page's, cleaned, and with the nudge when it's the same as the last two (`said`: as typed)
    /// with the page no different after each.
    /// What happened to the pane's tabs meanwhile (notes.rs: a popup, a download, a crash)
    /// joins it before it's marked and cleaned.
    fn finish(&self, ran: Res<(Reply, Meta)>, said: &str, reveal: bool, pty: u32) -> Res<Reply> {
        let (reply, meta) = ran.unwrap_or_else(|e| plain(Reply::error(e)));
        let state = match &reply.error {
            Some(error) => Some(format!("failed: {error}")),
            None => meta.state,
        };
        let looped = state.is_some_and(|s| remember(&self.id, |m| m.history.again(said, &s)));
        let reply = notes::with(reply, &notes::take(pty, Some(&self.id)));
        let nonce = output::nonce().ok_or(NO_NONCE)?;
        let origin = self.page().ok().map(|p| control::origin(&p.0.url));
        let ctx = output::Ctx {
            marked: true,
            origin: origin.as_deref(),
            nonce: &nonce,
            reveal,
            secrets: &secrets(),
            dialog: self.dialog(),
            looped,
        };
        Ok(output::finish(reply, &ctx))
    }

    /// The alert() or confirm() open on the page, waiting for the user, if one is.
    fn dialog(&self) -> Option<&'static str> {
        let id = self.id.clone();
        main(&self.app, move |_| super::agent_dialog(&id))
            .ok()
            .flatten()
    }

    /// Fails at once while a dialog holds the page's scripts, which would wait for it.
    fn unblocked(&self) -> Res<()> {
        match self.dialog() {
            Some(kind) => Err(output::dialog_open(kind)),
            None => Ok(()),
        }
    }

    /// The page, and whether its console is kept.
    fn page(&self) -> Res<(PageState, bool)> {
        let id = self.id.clone();
        main(&self.app, move |_| super::agent_page(&id))
    }

    /// The agent script on the page, given `input`: its answer, or the error it gave. Each call tells the
    /// page the tab's highest ref so far, and learns it back (prelude.js's `floor`).
    fn script(&self, input: Value) -> Res<Value> {
        self.script_for(input, SCRIPT)
    }

    /// As `script`, waiting `wait` at most.
    fn script_for(&self, mut input: Value, wait: Duration) -> Res<Value> {
        self.unblocked()?;
        input["floor"] = json!(remember(&self.id, |m| m.last_ref));
        let (id, input) = (self.id.clone(), input.to_string());
        let out = later_for(&self.app, wait, move |_, done| {
            super::agent_js(&id, false, AGENT, &input, done)
        })??;
        let value: Value =
            serde_json::from_str(&out).map_err(|_| "The page's answer wasn't readable.")?;
        if let Some(last) = value["last"].as_u64() {
            remember(&self.id, |m| m.last_ref = m.last_ref.max(last));
        }
        match value.get("error").and_then(Value::as_str) {
            Some(error) => Err(error.into()),
            None => Ok(value),
        }
    }

    /// The agent script's `out` for `input`.
    fn said(&self, input: Value) -> Res<String> {
        let found = self.script(input)?;
        Ok(found["out"].as_str().unwrap_or_default().to_string())
    }

    /// An action, whatever acts (the page's events, or native input): what it changes counted
    /// from just before it, what it sets off waited for, then what `act` did and one line of
    /// what changed.
    fn acted(&self, act: impl FnOnce() -> Res<String>) -> Res<(Reply, Meta)> {
        let before = self.page()?.0;
        let id = self.id.clone();
        let request = main(&self.app, move |_| super::network_log(&id, |l| l.newest()))?;
        let at = super::epoch_ms();
        self.script(json!({ "cmd": "watch" }))?;
        let did = act()?;
        let (line, state) = self.changed(&before, at, request)?;
        Ok(read(Reply::out(format!("{did}\n{line}")), &state))
    }

    /// Once an action's loads, requests (past id `request`) and redraws have landed, or SETTLE
    /// has gone, a load LOAD: what changed since `at` on the page as it was `before`, and the
    /// page as it is now.
    fn changed(&self, before: &PageState, at: f64, request: u32) -> Res<(String, String)> {
        let until = Instant::now() + SETTLE;
        std::thread::sleep(POLL / 2);
        loop {
            let page = self.page()?.0;
            // A load the action started; one going on since before isn't the action's to await.
            if page.loading && (!before.loading || page.url != before.url) {
                self.loaded(LOAD)?;
                break;
            }
            let running = self.requests(request)?.iter().any(|r| r.state == "…");
            if !running || Instant::now() >= until {
                break;
            }
            std::thread::sleep(POLL / 2);
        }
        let most = until.saturating_duration_since(Instant::now()).max(QUIET);
        let input = json!({ "cmd": "changes", "quiet": QUIET.as_millis() as u64, "most": most.as_millis() as u64 });
        // Between documents there's no page to ask: it's another one by then. A dialog the user
        // has open holds this one's scripts instead.
        let seen = match self.script(input) {
            Ok(seen) => seen,
            Err(_) if self.dialog().is_some() => json!({}),
            Err(_) => json!({ "gone": true }),
        };
        let count = |k: &str| seen[k].as_u64().unwrap_or(0);
        let id = self.id.clone();
        let logged = main(&self.app, move |_| super::console_entries(&id))?;
        let logged = logged.iter().filter(|e| e.ts >= at);
        let page = self.page()?.0;
        let c = output::Changes {
            added: count("added"),
            removed: count("removed"),
            texts: count("texts"),
            attributes: count("attributes"),
            canvas: seen["canvas"].is_number(),
            dialogs: seen["dialogs"].as_i64().unwrap_or(0),
            new_page: seen["gone"] == true,
            url_before: before.url.clone(),
            errors: logged.clone().filter(|e| e.level == "error").count(),
            answered: logged
                .filter(|e| e.level == "warn")
                .filter_map(|e| output::dialog_note(&e.msg))
                .collect(),
            requests: self.requests(request)?,
            url: page.url.clone(),
        };
        let state = seen["state"].as_str().unwrap_or(&page.url).to_string();
        Ok((output::changed(&c), state))
    }

    /// The requests past id `after` in the tab's network log.
    fn requests(&self, after: u32) -> Res<Vec<output::Request>> {
        let id = self.id.clone();
        main(&self.app, move |_| {
            super::network_log(&id, |log| {
                let newer = log.entries().into_iter().filter(|e| e.id > after);
                newer.filter_map(output::Request::of).collect()
            })
        })
    }

    /// Key events from the keyboard, as `keys` asks.
    fn keys(&self, keys: Keys) -> Res<()> {
        let id = self.id.clone();
        main(&self.app, move |_| super::agent_keys(&id, keys))
    }

    /// A chord down and up, held `hold` in between if asked, on the element with focus.
    fn press(&self, chord: &str, hold: Option<Duration>) -> Res<String> {
        let on = self.focused(Some(chord))?;
        let half = |half| Keys::Press(chord.into(), half);
        match hold {
            None => self.keys(half(Half::Both))?,
            Some(hold) => {
                self.keys(half(Half::Down))?;
                std::thread::sleep(hold);
                self.keys(half(Half::Up))?;
            }
        }
        let held = hold.map_or_else(String::new, |h| format!(" for {} ms", h.as_millis()));
        Ok(format!("Pressed {chord}{held} on {on}"))
    }

    /// A key held down, or let go.
    fn key(&self, down: bool, key: &str) -> Res<String> {
        let on = self.focused(down.then_some(key))?;
        self.keys(if down {
            Keys::Down(key.into())
        } else {
            Keys::Up(key.into())
        })?;
        Ok(if down {
            format!("Holding {key} down on {on}")
        } else {
            format!("Let go of {key} on {on}")
        })
    }

    /// Focused at the end of its text, then `text` put in as an input method puts it.
    fn type_in(&self, target: &str, text: &str) -> Res<String> {
        let focused = self.script(json!({ "cmd": "focus", "target": target }))?;
        if focused["line"] == true && text.contains(['\n', '\r']) {
            return Err(
                "A single-line field takes no line breaks: `press Enter` submits it".into(),
            );
        }
        let (id, typed) = (self.id.clone(), text.to_string());
        main(&self.app, move |_| super::agent_text(&id, &typed))?;
        let label = focused["label"].as_str().unwrap_or_default();
        Ok(format!("Typed into {label}"))
    }

    /// What has the page's focus, as a reply names it. Refused: a key going `down` that opens a
    /// focused <select>'s native menu, which would hold GitViber until it closed.
    fn focused(&self, down: Option<&str>) -> Res<String> {
        let focused = self.script(json!({ "cmd": "focused" }))?;
        let label = focused["label"].as_str().unwrap_or("the page");
        if focused["menu"] == true && down.is_some_and(input::opens_menu) {
            return Err(format!(
                "{label} has focus, and {} would open its native menu: use select",
                down.unwrap_or_default()
            ));
        }
        Ok(label.to_string())
    }

    /// Where a pointer command acts once a person could act there, and what's there (act.js
    /// `aim`, which every action takes its point from).
    fn aim(&self, target: &Target) -> Res<Spot> {
        let input = match target {
            Target::El(target) => json!({ "cmd": "aim", "target": target }),
            Target::At(x, y) => json!({ "cmd": "aim", "x": x, "y": y }),
        };
        serde_json::from_value(self.script(input)?)
            .map_err(|_| "The page's answer wasn't readable.".into())
    }

    /// One native pointer event, at `at` or where the pointer is: where it went, once WebKit
    /// has it.
    fn pointer(&self, what: Pointer, at: Option<(f64, f64)>) -> Res<(f64, f64)> {
        let id = self.id.clone();
        later(&self.app, move |_, done| {
            super::agent_pointer(&id, what, at, done)
        })
    }

    /// Pointer events in turn; one failing while a button is down lets the button go, so the
    /// page isn't left mid-press.
    fn presses(&self, steps: impl IntoIterator<Item = (Pointer, Option<(f64, f64)>)>) -> Res<()> {
        let mut down = None;
        for (what, at) in steps {
            if let Err(e) = self.pointer(what, at) {
                if let Some(up) = down {
                    let _ = self.pointer(up, None);
                }
                return Err(e);
            }
            down = match what {
                Pointer::Down { right, clicks } => Some(Pointer::Up { right, clicks }),
                Pointer::Up { .. } => None,
                _ => down,
            };
        }
        Ok(())
    }

    fn click(&self, click: Click) -> Res<String> {
        let spot = self.aim(&click.target)?;
        if spot.picker {
            return Err(format!(
                "{} opens a native picker, which would hold GitViber until it closed: select or \
                 fill it instead",
                spot.label
            ));
        }
        let at = Some((spot.x, spot.y));
        let presses = input::clicks(click.count, click.right);
        self.presses(presses.into_iter().map(|p| (p, at)))?;
        let done = match (click.count, click.right) {
            (_, true) => "Right-clicked",
            (2, _) => "Double-clicked",
            _ => "Clicked",
        };
        Ok(format!("{done} {} at {}", spot.label, xy((spot.x, spot.y))))
    }

    fn hover(&self, target: &Target) -> Res<String> {
        let spot = self.aim(target)?;
        let at = self.pointer(Pointer::Move, Some((spot.x, spot.y)))?;
        Ok(format!("Hovered {} at {}", spot.label, xy(at)))
    }

    /// `files` for the file chooser a click on `target` opens: none after this command gets
    /// them, chosen or not.
    fn upload(&self, target: String, files: Vec<PathBuf>) -> Res<String> {
        let spot = self.aim(&Target::El(target))?;
        let id = self.id.clone();
        main(&self.app, move |_| super::agent_upload(&id, files))?;
        let at = Some((spot.x, spot.y));
        let clicked = format!("Clicked {} at {}", spot.label, xy((spot.x, spot.y)));
        let chosen = self
            .presses(input::clicks(1, false).into_iter().map(|p| (p, at)))
            .and_then(|()| {
                let until = Instant::now() + CHOOSER;
                loop {
                    let id = self.id.clone();
                    if let Some(chosen) = main(&self.app, move |_| super::agent_chosen(&id))? {
                        let n = chosen?;
                        let files = if n == 1 { "file" } else { "files" };
                        return Ok(format!("{clicked}\nUploaded {n} {files}"));
                    }
                    if Instant::now() >= until {
                        return Err(format!(
                            "{clicked}, but no file chooser opened: name the page's <input \
                             type=file>, its label or its upload button"
                        ));
                    }
                    std::thread::sleep(POLL);
                }
            });
        let id = self.id.clone();
        main(&self.app, move |_| super::agent_upload_done(&id))?;
        chosen
    }

    /// Pressed at `from`, moved to `to` with the button held, let go: both ends as they show
    /// together, as moving to one may scroll the other away.
    fn drag(&self, from: &Target, to: &Target) -> Res<String> {
        let mut a = self.aim(from)?;
        let b = self.aim(to)?;
        if b.scrolled {
            a = self.aim(from)?;
            if a.scrolled {
                return Err(format!(
                    "{} and {} don't show together: scroll so both do, or drag to a point",
                    a.label, b.label
                ));
            }
        }
        if a.drag {
            return Err(format!(
                "{} is draggable: HTML5 drag-and-drop follows the real mouse, so it isn't driven \
                 here. Drag from a part of it that isn't, or use eval",
                a.label
            ));
        }
        let steps = input::drag((a.x, a.y), (b.x, b.y));
        self.presses(steps.into_iter().map(|(p, at)| (p, Some(at))))?;
        Ok(format!(
            "Dragged {} at {} to {} at {}",
            a.label,
            xy((a.x, a.y)),
            b.label,
            xy((b.x, b.y))
        ))
    }

    /// The wheel where the pointer is, by `px` or most of the page's height; by script where
    /// AppKit makes no wheel event.
    fn wheel(&self, up: bool, px: Option<u32>) -> Res<String> {
        let [height, _, _] = self.scrolled()?;
        let px = px.unwrap_or((height * 0.8).round() as u32);
        let way = if up { "up" } else { "down" };
        let mut at = None;
        for turn in input::wheel(px, up) {
            match self.pointer(turn, at) {
                Ok(here) => at = Some(here),
                Err(e) if e == input::NO_WHEEL && at.is_none() => {
                    return self.said(json!({ "cmd": "scroll", "to": way, "px": px }));
                }
                Err(e) => return Err(e),
            }
        }
        std::thread::sleep(SCROLLING);
        let [_, y, most] = self.scrolled()?;
        let at = at.map_or_else(String::new, |at| format!(" at {}", xy(at)));
        Ok(format!(
            "Scrolled {way} {px} px{at}: the page at {y} of {most}"
        ))
    }

    /// The page's height, how far down it is, and how far it goes (SCROLLED).
    fn scrolled(&self) -> Res<[f64; 3]> {
        let id = self.id.clone();
        let out = later(&self.app, move |_, done| {
            super::agent_js(&id, false, SCROLLED, "", done)
        })??;
        serde_json::from_str(&out).map_err(|_| "The page's answer wasn't readable.".into())
    }

    fn go(&self, to: Go, done: &str) -> Res<Reply> {
        let id = self.id.clone();
        main(&self.app, move |_| super::go(&id, to))?;
        self.arrived(done)
    }

    /// Once the page has loaded, or LOAD has passed: where it is, or why it isn't.
    fn arrived(&self, done: &str) -> Res<Reply> {
        let (page, loading) = self.loaded(LOAD)?;
        if let Some(failed) = page.failed {
            return Err(format!("Couldn't open {}: {}", failed.url, failed.message));
        }
        let still = if loading { " (still loading)" } else { "" };
        Ok(Reply::out(format!(
            "{done} {}{still}",
            titled(&page.url, &page.title, " — ")
        )))
    }

    /// The page once it isn't loading, or as it is when `timeout` is up (true: still loading).
    fn loaded(&self, timeout: Duration) -> Res<(PageState, bool)> {
        // A load just asked for may not have started yet.
        std::thread::sleep(POLL);
        let until = Instant::now() + timeout;
        loop {
            let page = self.page()?.0;
            if !page.loading || Instant::now() >= until {
                let loading = page.loading;
                return Ok((page, loading));
            }
            std::thread::sleep(POLL);
        }
    }

    /// Until `what` is so, or `timeout` is up (one deadline for all its asking): then what was
    /// still not so, as a fact rather than a call to wait longer.
    fn wait(&self, what: Wait, timeout: Duration) -> Res<Reply> {
        if let Wait::Ms(ms) = what {
            std::thread::sleep(Duration::from_millis(ms));
            return Ok(Reply::out(format!("Waited {ms} ms")));
        }
        let wanted = match &what {
            Wait::Css(css) => format!("{css} to show"),
            Wait::Text(text) => format!("{text:?} to show"),
            Wait::Load => "the page to load".into(),
            Wait::Url(glob) => format!("the address to match {glob}"),
            Wait::Fn(js) => format!("`{}` to be true", super::cut(js, 80)),
            Wait::Ms(_) => String::new(),
        };
        let until = Instant::now() + timeout;
        loop {
            let left = until.saturating_duration_since(Instant::now());
            let still = self.not_yet(&what, left.max(POLL))?;
            let Some(still) = still else {
                return Ok(Reply::out(format!("Done waiting for {wanted}")));
            };
            if Instant::now() >= until {
                return Err(format!(
                    "Timed out after {} waiting for {wanted}: {still}",
                    duration_text(timeout)
                ));
            }
            std::thread::sleep(POLL * 2);
        }
    }

    /// What isn't so yet of `what`, asking the page for `wait` at most; None once it is.
    fn not_yet(&self, what: &Wait, wait: Duration) -> Res<Option<String>> {
        // Between pages there's no page to ask.
        let asked = |input: Value| match self.script_for(input, wait) {
            Ok(v) if v["invalid"] == true => Err("That isn't a CSS selector.".to_string()),
            Ok(v) => Ok(Some(v)),
            Err(_) => Ok(None),
        };
        let page = || self.page().map(|p| p.0);
        Ok(match what {
            Wait::Load => page()?.loading.then(|| {
                format!(
                    "{} was still loading",
                    page().map(|p| p.url).unwrap_or_default()
                )
            }),
            Wait::Url(glob) => {
                let url = page()?.url;
                (!control::url_matches(glob, &url)).then(|| format!("the page is at {url}"))
            }
            Wait::Css(css) => match asked(json!({ "cmd": "exists", "css": css }))? {
                Some(v) if v["found"] == true => None,
                Some(v) if v["count"].as_u64().unwrap_or(0) > 0 => {
                    Some(format!("{} element(s) match it, none showing", v["count"]))
                }
                Some(_) => Some("no element matches it".into()),
                None => Some("the page didn't answer".into()),
            },
            Wait::Text(text) => match asked(json!({ "cmd": "hasText", "text": text }))? {
                Some(v) if v["found"] == true => None,
                _ => Some("the page's text doesn't have it".into()),
            },
            Wait::Fn(js) => self.truthy(js, wait)?,
            Wait::Ms(_) => None,
        })
    }

    /// Expression `js` in the page's world: None once it's true, else what it was. A promise
    /// that doesn't settle in `wait` counts as not yet.
    fn truthy(&self, js: &str, wait: Duration) -> Res<Option<String>> {
        self.unblocked()?;
        let ms = wait.min(Duration::from_secs(2)).as_millis();
        let body = format!("const ms = {ms};\nconst ask = async () => ({js}\n);\n{TRUTHY}");
        let id = self.id.clone();
        let out = later_for(&self.app, wait + POLL, move |_, done| {
            super::agent_js(&id, true, &body, "", done)
        })?;
        let out = match out {
            Ok(out) => out,
            Err(e) if e.contains("SyntaxError") => {
                return Err(format!("--fn takes a JavaScript expression: {e}"))
            }
            // Between pages there's no page to ask.
            Err(_) => return Ok(Some("the page didn't answer".into())),
        };
        Ok(match out.split_at_checked(1) {
            Some(("+", _)) => None,
            Some(("-", value)) => Some(format!("it was {}", super::cut(value, 200))),
            Some(("!", thrown)) => Some(format!("it threw {}", super::cut(thrown, 200))),
            _ => Some("it hadn't settled".into()),
        })
    }

    /// `js` in the page's own world: an expression, else statements (with their own return).
    /// Only a form that doesn't compile is tried again, never one that ran: what it throws comes
    /// back as its answer. Not through the page's eval(), which its CSP may forbid.
    fn eval(&self, js: &str) -> Res<Reply> {
        self.unblocked()?;
        let run = |body: String| {
            let id = self.id.clone();
            later(&self.app, move |_, done| {
                super::agent_js(&id, true, &body, "", done)
            })?
        };
        let body = |call: String| {
            format!("let value;\ntry {{ value = await {call}; }} catch (e) {{ return `!${{e}}`; }}\n{SERIALIZE}")
        };
        let expression = body(format!("(async () => ({js}\n))()"));
        let statements = body(format!("(async () => {{\n{js}\n}})()"));
        let out = match run(expression) {
            Err(e) if e.contains("SyntaxError") => run(statements),
            other => other,
        }?;
        match out.split_at_checked(1) {
            Some(("=", value)) => Ok(Reply::out(super::cut(value, EVAL_MAX))),
            Some(("!", thrown)) => Err(format!("The page threw {}", super::cut(thrown, EVAL_MAX))),
            _ => Err("The page's answer wasn't readable.".into()),
        }
    }

    /// The viewport, one element or a clip of it as drawn, at 1 px a CSS px unless scaled, the
    /// refs drawn on it if asked; written where told, else to the app's cache.
    fn screenshot(&self, shot: control::Shot, cwd: &str) -> Res<Reply> {
        let path = shot
            .path
            .map(|p| control::picture_path(Path::new(cwd), &p))
            .transpose()?;
        let input =
            json!({ "cmd": "view", "target": shot.target, "clip": shot.clip, "refs": shot.refs });
        let found = self.script(input)?;
        let rect: Bounds = serde_json::from_value(found["rect"].clone())
            .map_err(|_| "The page gave no box to take.")?;
        let labels = found["labels"].as_array().into_iter().flatten();
        let at = |l: &Value, k: &str| l[k].as_f64().unwrap_or(0.0);
        let picture = marks::Picture {
            rect,
            scale: shot.scale,
            labels: labels
                .map(|l| marks::Label {
                    text: l["ref"].as_str().unwrap_or_default().into(),
                    x: at(l, "x"),
                    y: at(l, "y"),
                    w: at(l, "w"),
                    h: at(l, "h"),
                })
                .collect(),
            jpeg: path.as_deref().is_some_and(control::jpeg),
        };
        let id = self.id.clone();
        let taken = later(&self.app, move |_, done| {
            super::agent_picture(&id, picture, done)
        })?
        .ok_or("No picture: WebKit gave none.")?;
        let ((w, h), scale) = (taken.size, taken.scale);
        let path = match path {
            Some(path) => {
                std::fs::write(&path, &taken.bytes)
                    .map_err(|e| format!("Couldn't write {}: {e}", path.display()))?;
                path.display().to_string()
            }
            None => picks::save_png(&self.app, &taken.bytes).ok_or("Couldn't save the picture.")?,
        };
        let shrunk = if scale < shot.scale {
            ", shrunk to stay within 2000 px"
        } else {
            ""
        };
        Ok(Reply::out(format!(
            "{path} ({w}×{h} px, {scale:.2} px a CSS px{shrunk})"
        )))
    }

    fn console(&self, level: Option<&str>, all: bool, clear: bool) -> Res<Reply> {
        if !self.page()?.1 {
            return Ok(Reply::out(
                "The page's console isn't kept (Settings → Browser → Console).",
            ));
        }
        let id = self.id.clone();
        let entries = main(&self.app, move |_| super::console_entries(&id))?;
        if clear {
            let id = self.id.clone();
            main(&self.app, move |_| super::console_clear(&id))?;
        }
        Ok(Reply::out(control::console_text(&entries, level, all)))
    }

    fn network(&self, ask: network::Ask, reveal: bool) -> Res<Reply> {
        let (id, now) = (self.id.clone(), super::epoch_ms());
        let (text, data) = main(&self.app, move |_| {
            super::network_log(&id, |log| network::answer(log, &ask, now, reveal))
        })??;
        Ok(Reply::data(text, data))
    }

    fn device(&self, screen: Option<AgentScreen>) -> Res<Reply> {
        let shown = screen.as_ref().map_or_else(
            || "itself".into(),
            |s| format!("{} ({}×{})", s.name, s.w, s.h),
        );
        let id = self.id.clone();
        main(&self.app, move |app| super::agent_device(app, &id, screen))?;
        // Another user agent loads the page again.
        self.loaded(LOAD)?;
        Ok(Reply::out(format!("Showing the page as {shown}")))
    }

    /// The tab's login: the cookies of its own store, and its page's storage.
    fn session(&self) -> Res<Session> {
        #[derive(serde::Deserialize)]
        struct Storage {
            origin: String,
            local: Vec<(String, String)>,
            session: Vec<(String, String)>,
        }
        let id = self.id.clone();
        let cookies = later(&self.app, move |_, done| super::agent_cookies(&id, done))?;
        let blank = "Open the page whose login to save first: this one keeps no storage.";
        let id = self.id.clone();
        let read = later(&self.app, move |_, done| {
            super::agent_js(&id, false, STORAGE_READ, "", done)
        })?
        .map_err(|_| blank)?;
        let storage: Storage = serde_json::from_str(&read).map_err(|_| blank)?;
        if !on_web(&storage.origin) {
            return Err(blank.into());
        }
        Ok(Session {
            origin: storage.origin,
            saved: now(),
            cookies,
            local: storage.local,
            session: storage.session,
        })
    }

    /// Session `s` in place of what the tab's own store had, on the origin it was saved at (a
    /// blank tab goes there first): its cookies set, its storage written, and the page loaded
    /// again to read them. A shared tab, one on another origin, or one still loading is left as
    /// it was; a page that leaves as the session goes in keeps its cookies, not its storage.
    fn load_session(&self, name: &str, s: &Session) -> Res<String> {
        let id = self.id.clone();
        main(&self.app, move |_| super::agent_own_store(&id))?;
        let (mut page, mut loading) = self.loaded(LOAD)?;
        if !on_web(&page.url) {
            let (id, origin) = (self.id.clone(), s.origin.clone());
            main(&self.app, move |_| super::navigate(&id, &origin))?;
            (page, loading) = self.loaded(LOAD)?;
            if let Some(failed) = page.failed {
                return Err(format!("Couldn't open {}: {}", failed.url, failed.message));
            }
        }
        let here = control::origin(&page.url);
        if here != s.origin {
            return Err(format!(
                "Session {name} is for {}; this tab is on {here}.",
                s.origin
            ));
        }
        if loading {
            return Err(
                "The page is still loading: `gitviber browser wait --load`, then load the session."
                    .into(),
            );
        }
        let id = self.id.clone();
        later(&self.app, move |_, done| {
            super::agent_clear_store(&id, done)
        })?;
        let (id, cookies) = (self.id.clone(), s.cookies.clone());
        later(&self.app, move |_, done| {
            super::agent_set_cookies(&id, cookies, done)
        })?;
        let input = json!({ "origin": s.origin, "local": s.local, "session": s.session });
        let id = self.id.clone();
        let moved = later(&self.app, move |_, done| {
            super::agent_js(&id, false, STORAGE_WRITE, &input.to_string(), done)
        })??;
        if !moved.is_empty() {
            return Err(format!(
                "The page went to {moved} as session {name} loaded: its cookies are the \
                 session's, its storage isn't. Load it again."
            ));
        }
        self.reload()?;
        Ok(format!(
            "Loaded session {name}: {}. {NO_INDEXED_DB}",
            summary(s)
        ))
    }

    /// The tab's own store emptied, and its page loaded again without it.
    fn clear_session(&self) -> Res<String> {
        let id = self.id.clone();
        later(&self.app, move |_, done| {
            super::agent_clear_store(&id, done)
        })?;
        if super::loadable(&self.page()?.0.url) {
            self.reload()?;
        }
        Ok("Cleared the tab's cookies and storage.".into())
    }

    fn reload(&self) -> Res<()> {
        let id = self.id.clone();
        main(&self.app, move |_| super::go(&id, Go::Reload))?;
        self.loaded(LOAD).map(drop)
    }
}

/// On a web page's origin, where logins and storage are.
fn on_web(url: &str) -> bool {
    Url::parse(url).is_ok_and(|u| matches!(u.scheme(), "http" | "https"))
}

/// An address with its page's title, when it has one.
fn titled(url: &str, title: &str, between: &str) -> String {
    if title.is_empty() {
        url.into()
    } else {
        format!("{url}{between}{title}")
    }
}

/// `url`: the address and title, and `audio: playing` while the page makes sound.
fn url_reply(url: &str, title: &str, audio: bool) -> String {
    let page = titled(url, title, "\n");
    if audio {
        format!("{page}\naudio: playing")
    } else {
        page
    }
}

/// `tab list`: a line a tab, the default one marked.
fn tabs_text(tabs: &[Listed]) -> String {
    if tabs.is_empty() {
        return "No tabs: the pane's next command opens its default one.".into();
    }
    let lines = tabs.iter().map(|t| {
        let default = if t.default { " (default)" } else { "" };
        let idle = match t.idle_min {
            0 => "never idles".to_string(),
            m => format!("idles after {m}m"),
        };
        let data = if t.shared { "shared" } else { "private" };
        let audio = if t.audio { "  audio: playing" } else { "" };
        let used = ago(Duration::from_secs(t.used_s));
        let page = titled(&t.origin, &t.title, "  ");
        format!(
            "{}{default}  {}  {}  used {used}  {idle}  {data}{audio}  {page}",
            t.name, t.state, t.owner
        )
    });
    lines.collect::<Vec<_>>().join("\n")
}

/// What a session holds, never its values.
fn summary(s: &Session) -> String {
    format!(
        "{} cookies, {} localStorage and {} sessionStorage items of {}",
        s.cookies.len(),
        s.local.len(),
        s.session.len(),
        s.origin
    )
}

/// `session list` at `now` (seconds since 1970).
fn sessions_text(saved: &[(String, Session)], now: u64) -> String {
    if saved.is_empty() {
        return "No sessions saved for this repository.".into();
    }
    let lines = saved.iter().map(|(name, s)| {
        let age = Duration::from_secs(now.saturating_sub(s.saved));
        format!(
            "{name}  {}  {} cookies  (saved {})",
            s.origin,
            s.cookies.len(),
            ago(age)
        )
    });
    lines.collect::<Vec<_>>().join("\n")
}

/// A wait's length as said: 10 s, 1.5 s, 300 ms.
fn duration_text(d: Duration) -> String {
    match d.as_millis() {
        ms if ms < 1000 => format!("{ms} ms"),
        ms if ms % 1000 == 0 => format!("{} s", ms / 1000),
        ms => format!("{:.1} s", ms as f64 / 1000.0),
    }
}

pub(super) fn ago(idle: Duration) -> String {
    match idle.as_secs() {
        s @ 0..60 => format!("{s}s ago"),
        s @ 60..3600 => format!("{}m ago", s / 60),
        s => format!("{}h ago", s / 3600),
    }
}

#[cfg(test)]
mod tests {
    use super::control::TabState;
    use super::*;
    use crate::browser::control::Owner;

    #[test]
    fn the_tab_list_gives_each_tabs_state_owner_use_idle_data_and_page_on_a_line() {
        let tab = |name: &str, state, used_s, idle_min, origin: &str, title: &str| Listed {
            name: name.into(),
            default: name == "t1",
            state,
            used_s,
            idle_min,
            shared: name == "t3",
            audio: name == "t3",
            origin: origin.into(),
            title: title.into(),
            owner: if name == "rv268" {
                Owner::User
            } else {
                Owner::Agent
            },
        };
        let tabs = [
            tab("t1", TabState::Awake, 5, 10, "http://localhost:3000", "App"),
            tab("rv268", TabState::Sleeping, 125, 0, "about:blank", ""),
            tab("t3", TabState::Parked, 7300, 30, "https://example.test", ""),
        ];
        assert_eq!(
            tabs_text(&tabs),
            "t1 (default)  awake  agent  used 5s ago  idles after 10m  private  http://localhost:3000  App\n\
             rv268  sleeping  user  used 2m ago  never idles  private  about:blank\n\
             t3  parked  agent  used 2h ago  idles after 30m  shared  audio: playing  https://example.test"
        );
        assert!(tabs_text(&[]).starts_with("No tabs"));
        // --json: the same fields.
        let json = serde_json::to_value(&tabs[1]).unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "name": "rv268", "default": false, "state": "sleeping", "used_s": 125,
                "idle_min": 0, "shared": false, "audio": false, "origin": "about:blank", "title": "", "owner": "user" })
        );
    }

    #[test]
    fn the_url_reply_adds_a_line_only_while_the_page_makes_sound() {
        assert_eq!(url_reply("http://a.test/", "A", false), "http://a.test/\nA");
        assert_eq!(url_reply("http://a.test/", "", false), "http://a.test/");
        assert_eq!(
            url_reply("http://a.test/", "A", true),
            "http://a.test/\nA\naudio: playing"
        );
    }

    #[test]
    fn sessions_are_told_by_what_they_hold_never_its_values() {
        let session = Session {
            origin: "https://app.test".into(),
            saved: 1_000,
            cookies: vec![sessions::Cookie {
                name: "sid".into(),
                value: "s3cret".into(),
                domain: "app.test".into(),
                path: "/".into(),
                expires: None,
                secure: true,
                http_only: true,
                same_site: None,
            }],
            local: vec![("token".into(), "t0ken".into())],
            session: vec![],
        };
        assert_eq!(
            summary(&session),
            "1 cookies, 1 localStorage and 0 sessionStorage items of https://app.test"
        );
        let text = sessions_text(&[("mert".into(), session)], 1_000 + 3 * 3600);
        assert_eq!(text, "mert  https://app.test  1 cookies  (saved 3h ago)");
        assert!(!text.contains("s3cret") && !text.contains("t0ken"));
        assert!(sessions_text(&[], 0).starts_with("No sessions"));
    }

    #[test]
    fn the_third_command_alike_on_an_unchanged_page_is_a_loop() {
        let mut h = History::default();
        assert!(!h.again("click e5", "page A"));
        assert!(!h.again("click e5", "page A"));
        assert!(h.again("click e5", "page A"));
        assert!(h.again("click e5", "page A"), "and each after it");
        // The page changing, or another command, starts over.
        assert!(!h.again("click e5", "page B"));
        assert!(!h.again("click e6", "page B"));
        assert!(!h.again("click e5", "page B"));
        // A toggle back and forth is no loop.
        let mut t = History::default();
        for state in ["open", "closed", "open", "closed"] {
            assert!(!t.again("click e1", state));
        }
        assert_eq!(t.0.len(), LOOP, "no more kept than read");
    }

    #[test]
    fn a_tabs_memory_and_its_secrets_go_with_it() {
        remember("tab-gone", |m| {
            m.secrets.push(("PW".into(), "hunter22".into()))
        });
        assert!(secrets().iter().any(|(n, _)| n == "PW"));
        // However it went: control.rs's routes call this on each way out.
        forget("tab-gone");
        assert!(!memories().contains_key("tab-gone"));
    }

    #[test]
    fn a_tabs_commands_take_turns_and_one_kept_waiting_too_long_is_told() {
        let queue = queue("tab-1");
        assert!(Arc::ptr_eq(&queue, &super::queue("tab-1")));
        assert!(!Arc::ptr_eq(&queue, &super::queue("tab-2")));
        let first = take_turn(&queue, POLL).unwrap();
        let (q, started) = (queue.clone(), Instant::now());
        let waiting = std::thread::spawn(move || take_turn(&q, POLL * 3).map(drop));
        assert!(waiting
            .join()
            .unwrap()
            .unwrap_err()
            .contains("still running"));
        assert!(started.elapsed() >= POLL * 3);
        // Its turn once the first is done.
        let q = queue.clone();
        let next = std::thread::spawn(move || take_turn(&q, STEP).map(drop));
        std::thread::sleep(POLL);
        drop(first);
        assert!(next.join().unwrap().is_ok());
        // Nobody holding or waiting: the next one asked for lets it go.
        let kept = Arc::downgrade(&queue);
        drop(queue);
        let _ = super::queue("tab-3");
        assert!(kept.upgrade().is_none(), "a closed tab's turn doesn't stay");
    }
}
