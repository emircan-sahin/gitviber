//! `gitviber browser` commands (control.rs), each run on its request's connection thread
//! (server.rs) as a few steps on the main thread, where the views are, waiting in between.

use super::control::{
    self, AgentScreen, Command, Found, Listed, NewTab, Pane, Reply, Request, Scroll, Sessions,
    Tabs, Wait,
};
use super::marks;
use super::network;
use super::output;
use super::picks::{self, Bounds};
use super::sessions::{self, Session, Shelf};
use super::{Go, PageState};
use crate::state::{AppState, Res};
use serde_json::{json, Value};
use std::cell::Cell;
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, LazyLock, Mutex, MutexGuard, PoisonError, TryLockError};
use std::time::{Duration, Instant, SystemTime};
use tauri::{AppHandle, Manager, Url};

const AGENT: &str = include_str!("scripts/agent.js");
/// A step on the main thread answers at once: past this the app is stuck.
const STEP: Duration = Duration::from_secs(10);
/// What a script or a picture may take.
const SCRIPT: Duration = Duration::from_secs(30);
/// How long a page gets to load after open, a click or a key before the reply goes anyway.
const LOAD: Duration = Duration::from_secs(15);
const POLL: Duration = Duration::from_millis(100);
/// How long an action's requests and redraws get to land before its reply says what changed,
/// as Playwright lets a click's effects settle; a load it starts gets LOAD.
const SETTLE: Duration = Duration::from_millis(500);
/// The page still this long counts as done redrawing.
const QUIET: Duration = Duration::from_millis(100);
/// An eval's result, cut: an agent reads it.
const EVAL_MAX: usize = 20_000;
/// What `eval`'s script gives: `=` and its value as JSON however it came, or `!` and what it threw.
const SERIALIZE: &str = "try { const s = JSON.stringify(value); return `=${s === undefined ? String(value) : s}`; } catch { return `=${String(value)}`; }";
/// The page's storage for a session; in GitViber's world, where the page can't stand in for it.
const STORAGE_READ: &str = "return JSON.stringify({ origin: location.origin, local: Object.entries(localStorage), session: Object.entries(sessionStorage) });";
/// Written only on the session's origin; elsewhere, where the page is instead.
const STORAGE_WRITE: &str = "const s = JSON.parse(input); if (location.origin !== s.origin) return location.origin; localStorage.clear(); sessionStorage.clear(); for (const [k, v] of s.local) localStorage.setItem(k, v); for (const [k, v] of s.session) sessionStorage.setItem(k, v); return '';";
const NO_INDEXED_DB: &str = "IndexedDB isn't carried.";

pub fn run(app: &AppHandle, req: Request) -> Reply {
    command(app, req).unwrap_or_else(Reply::error)
}

fn command(app: &AppHandle, req: Request) -> Res<Reply> {
    if let Some(op) = control::parse_pane(&req.cmd, &req.args) {
        let op = op?;
        let (pty, root) = pane(app, &req)?;
        let reply = match op {
            Pane::Tab(op) => tabs(app, pty, root, req.tab, op, &req.cwd),
            Pane::Session(op) => sessions(app, pty, root, req.tab, op),
        };
        return reply.map(|mut r| {
            if !req.reveal {
                for text in [&mut r.out, &mut r.error].into_iter().flatten() {
                    *text = output::redact_urls(text);
                }
                if let Some(data) = &mut r.data {
                    output::each_string(data, &output::redact_urls);
                }
            }
            r
        });
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
        remember(&id, |m| {
            m.secrets.retain(|(n, _)| n != name);
            m.secrets.push((name.clone(), value));
        });
    }
    let said = format!("{} {}", req.cmd, req.args.join(" "));
    drive(app, id, found, |tab| {
        tab.reveal.set(req.reveal);
        let reply = tab.run(cmd, made, &req.cwd);
        Ok(tab.finish(reply, &said))
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
    let tab = Tab {
        app: app.clone(),
        id,
        command: NEXT_COMMAND.fetch_add(1, Ordering::Relaxed),
        reveal: Cell::new(false),
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
                    tab.run(Command::Open(url), made, cwd)
                });
                more(&mut said, opened.map(|r| r.out.unwrap_or_default()))?;
            }
            Ok(Reply::out(said.join("\n")))
        }
        Tabs::List => {
            let tabs = main(app, move |_| super::agent_tabs(pty))?;
            let data = serde_json::to_value(&tabs).map_err(|e| e.to_string())?;
            Ok(Reply::data(tabs_text(&tabs), json!({ "tabs": data })))
        }
        Tabs::Close(named) => {
            let name = named.map_or_else(|| target(to), |n| Ok(Some(n)))?;
            let id = main(app, move |_| super::agent_find_tab(pty, name.as_deref()))?;
            // After a command running on it, not through it.
            let queue = queue(&id);
            let _turn = take_turn(&queue, STEP)?;
            let gone = id.clone();
            let closed = main(app, move |app| super::agent_close_tab(app, &gone))?;
            memories().remove(&id);
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
            let session = drive(app, id, found, Tab::session)?;
            shelf.save(&s, &session, force)?;
            Ok(Reply::out(format!(
                "Saved session {s}: {}. {NO_INDEXED_DB}",
                summary(&session)
            )))
        }
        Sessions::Load(s) => {
            let session = shelf_here()?.load(&s)?;
            load(app, tab()?, &s, &session).map(Reply::out)
        }
        Sessions::Clear => {
            let (id, found) = tab()?;
            drive(app, id, found, Tab::clear_session).map(Reply::out)
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

/// What a tab's commands leave for the next, by tab id; gone when an agent closes the tab.
#[derive(Default)]
struct Memory {
    /// The highest ref the tab handed out, on any of its pages.
    last_ref: u64,
    history: output::History,
    /// Filled with `fill --secret`, by name: hidden from every reply after.
    secrets: Vec<(String, String)>,
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

/// The pane's tab for one command, running it from `start` for as long as this lives. Its end
/// is queued after the start on the main thread even when the start timed out here, so the tab
/// can't stay answering its dialogs.
struct Tab {
    app: AppHandle,
    id: String,
    command: u64,
    /// `--reveal`: tokens and credential headers left in its reply.
    reveal: Cell<bool>,
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

    fn run(&self, cmd: Command, made: bool, cwd: &str) -> Res<Reply> {
        match cmd {
            Command::Open(url) => {
                if !made {
                    let id = self.id.clone();
                    main(&self.app, move |_| super::navigate(&id, &url))?;
                }
                self.arrived("Opened")
            }
            Command::Back => self.go(Go::Back, "Back at"),
            Command::Forward => self.go(Go::Forward, "Forward at"),
            Command::Reload => self.go(Go::Reload, "Reloaded"),
            Command::Url => {
                let page = self.page()?.0;
                Ok(Reply::out(titled(&page.url, &page.title, "\n")))
            }
            Command::Snapshot(snap) => self.snapshot(snap, cwd),
            Command::Find(pattern) => {
                let found = self.script(json!({ "cmd": "find", "pattern": pattern }))?;
                let out = found["out"].as_str().unwrap_or_default();
                Ok(Reply {
                    state: Some(out.into()),
                    untrusted: true,
                    ..Reply::out(out)
                })
            }
            Command::Get(get) => {
                let input = json!({ "cmd": "get", "what": get.what, "names": get.names, "target": get.target });
                let found = self.script(input)?;
                let out = found["out"].as_str().unwrap_or_default();
                let reply = match found.get("box") {
                    Some(b) => Reply::data(out, json!({ "box": b })),
                    None => Reply::out(out),
                };
                Ok(Reply {
                    state: Some(out.into()),
                    untrusted: true,
                    ..reply
                })
            }
            Command::Click(target) => self.acted(json!({ "cmd": "click", "target": target })),
            Command::FillSecret(target, name) => {
                let value = remember(&self.id, |m| {
                    let found = m.secrets.iter().find(|(n, _)| *n == name);
                    found.map(|(_, v)| v.clone())
                });
                let value = value.ok_or("No value came with --secret.")?;
                let input =
                    json!({ "cmd": "fill", "target": target, "text": value, "secret": true });
                let reply = self.acted(input)?;
                Ok(Reply {
                    out: reply
                        .out
                        .map(|o| o.replacen('\n', &format!(" from ${name}\n"), 1)),
                    ..reply
                })
            }
            Command::Fill(target, text) => {
                self.acted(json!({ "cmd": "fill", "target": target, "text": text }))
            }
            Command::Type(target, text) => {
                self.acted(json!({ "cmd": "type", "target": target, "text": text }))
            }
            Command::Select(target, values) => {
                self.acted(json!({ "cmd": "select", "target": target, "values": values }))
            }
            Command::Check(target, on) => {
                self.acted(json!({ "cmd": "check", "target": target, "on": on }))
            }
            Command::Hover(target) => self.acted(json!({ "cmd": "hover", "target": target })),
            Command::Press(key) => self.acted(json!({ "cmd": "press", "key": key })),
            Command::Scroll(to, px) => {
                let to = match to {
                    Scroll::Up => "up".to_string(),
                    Scroll::Down => "down".to_string(),
                    Scroll::To(target) => target,
                };
                self.acted(json!({ "cmd": "scroll", "to": to, "px": px }))
            }
            Command::Wait(what, timeout) => self.wait(what, timeout),
            Command::Eval(js) => self.eval(&js),
            Command::Screenshot(shot) => self.screenshot(shot, cwd),
            Command::Console { errors, clear } => self.console(errors, clear),
            Command::Network(ask) => self.network(ask),
            Command::Device(screen) => self.device(screen),
            Command::Appearance(dark) => {
                let id = self.id.clone();
                main(&self.app, move |app| {
                    super::agent_appearance(app, &id, dark)
                })?;
                Ok(Reply::out(match dark {
                    Some(true) => "The page is shown dark.",
                    Some(false) => "The page is shown light.",
                    None => "The page is shown as the app is.",
                }))
            }
            Command::Dialogs(accept) => {
                let id = self.id.clone();
                main(&self.app, move |_| super::agent_dialogs(&id, accept))?;
                Ok(Reply::out(if accept {
                    "The page's dialogs are accepted while commands run on it."
                } else {
                    "The page's dialogs are dismissed while commands run on it."
                }))
            }
        }
    }

    /// The page as a tree; with --json the refs gone since too; with -o, in full in a file.
    fn snapshot(&self, snap: control::Snap, cwd: &str) -> Res<Reply> {
        let file = snap
            .file
            .as_deref()
            .map(|f| control::text_path(Path::new(cwd), f))
            .transpose()?;
        let input = json!({ "cmd": "snapshot", "interactive": snap.interactive, "scope": snap.scope,
            "depth": snap.depth, "boxes": snap.boxes, "file": file.is_some() });
        let found = self.script(input)?;
        let out = found["out"].as_str().unwrap_or_default();
        let reply = match file {
            Some(path) => {
                // The file is read later as page text too: marked, and kept clean, as a reply is.
                let origin = control::origin(&self.page()?.0.url);
                let marked = output::untrusted(out, &origin, &output::nonce());
                std::fs::write(&path, format!("{}\n", self.clean(&marked)))
                    .map_err(|e| format!("Couldn't write {}: {e}", path.display()))?;
                let lines = out.lines().count();
                Reply::out(format!("Wrote {} ({lines} lines)", path.display()))
            }
            None => Reply {
                untrusted: true,
                ..Reply::data(out, json!({ "snapshot": out, "removed": found["removed"] }))
            },
        };
        Ok(Reply {
            state: Some(out.into()),
            ..reply
        })
    }

    /// A command's reply (`said`: as typed) as the agent gets it, a failure too: with a nudge
    /// when it's the same as the last two, the page no different after each.
    /// Page text marked as such, tokens in addresses hidden unless --reveal, and secrets
    /// always, all here, once.
    fn finish(&self, reply: Res<Reply>, said: &str) -> Reply {
        let mut reply = reply.unwrap_or_else(Reply::error);
        if reply.untrusted {
            let origin = self
                .page()
                .map_or_else(|_| String::new(), |p| control::origin(&p.0.url));
            let nonce = output::nonce();
            for text in [&mut reply.out, &mut reply.error].into_iter().flatten() {
                *text = output::untrusted(text, &origin, &nonce);
            }
            if let Some(Value::Object(data)) = &mut reply.data {
                data.insert(
                    "untrusted".into(),
                    json!({ "origin": origin, "nonce": nonce }),
                );
            }
        }
        for text in [&mut reply.out, &mut reply.error].into_iter().flatten() {
            *text = self.clean(text);
        }
        if let Some(data) = &mut reply.data {
            output::each_string(data, &|s| self.clean(s));
        }
        let state = match (&reply.state, &reply.error) {
            (_, Some(error)) => Some(format!("failed: {error}")),
            (state, None) => state.clone(),
        };
        let looped = state.is_some_and(|s| remember(&self.id, |m| m.history.again(said, &s)));
        if looped {
            let text = reply.error.as_mut().or(reply.out.as_mut());
            if let Some(text) = text {
                text.push('\n');
                text.push_str(output::NUDGE);
            }
        }
        reply
    }

    /// Text as a reply may give it: tokens in addresses hidden unless --reveal, and the tab's
    /// secrets by their names only.
    fn clean(&self, text: &str) -> String {
        let text = if self.reveal.get() {
            text.to_string()
        } else {
            output::redact_urls(text)
        };
        let secrets = remember(&self.id, |m| m.secrets.clone());
        output::hide_secrets(&text, &secrets)
    }

    /// The page, and whether its console is kept.
    fn page(&self) -> Res<(PageState, bool)> {
        let id = self.id.clone();
        main(&self.app, move |_| super::agent_page(&id))
    }

    /// agent.js on the page, given `input`: its answer, or the error it gave. Each call tells the
    /// page the tab's highest ref so far, and learns it back (agent.js's `floor`).
    fn script(&self, input: Value) -> Res<Value> {
        self.script_for(input, SCRIPT)
    }

    /// As `script`, waiting `wait` at most.
    fn script_for(&self, mut input: Value, wait: Duration) -> Res<Value> {
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

    /// An action, which may change the page or send it somewhere: its changes counted from
    /// just before it, what it set off waited for, and a line of what changed after it.
    fn acted(&self, mut input: Value) -> Res<Reply> {
        let url = self.page()?.0.url;
        let id = self.id.clone();
        let request = main(&self.app, move |_| super::network_log(&id, |l| l.newest()))?;
        let at = super::epoch_ms();
        input["watch"] = json!(true);
        let did = self.script(input)?;
        let did = did["out"].as_str().unwrap_or_default();
        let (line, state) = self.changed(url, at, request)?;
        Ok(Reply {
            state: Some(state),
            ..Reply::out(format!("{did}\n{line}"))
        })
    }

    /// Once an action's loads, requests (past id `request`) and redraws have landed, or SETTLE
    /// has gone, a load LOAD: what changed since `at` on the page that was at `url`, and the page
    /// as it is now.
    fn changed(&self, url: String, at: f64, request: u32) -> Res<(String, String)> {
        let until = Instant::now() + SETTLE;
        std::thread::sleep(POLL / 2);
        loop {
            if self.page()?.0.loading {
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
        // Between documents there's no page to ask: it's another one by then.
        let seen = self
            .script(input)
            .unwrap_or_else(|_| json!({ "gone": true }));
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
            dialogs: seen["dialogs"].as_i64().unwrap_or(0),
            new_page: seen["gone"] == true,
            url_before: url,
            errors: logged.clone().filter(|e| e.level == "error").count(),
            answered: logged
                .filter(|e| e.level == "warn")
                .filter_map(|e| output::answered(&e.msg))
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
                (!output::url_matches(glob, &url)).then(|| format!("the page is at {url}"))
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
        let ms = wait.min(Duration::from_secs(2)).as_millis();
        let body = format!(
            "const pending = Symbol();\nlet value;\ntry {{ value = await Promise.race([(async () => ({js}\n))(), new Promise((r) => setTimeout(r, {ms}, pending))]); }} catch (e) {{ return `!${{e}}`; }}\nif (value === pending) return '~';\nif (value) return '+';\ntry {{ return `-${{JSON.stringify(value) ?? String(value)}}`; }} catch {{ return `-${{String(value)}}`; }}"
        );
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
        let reply = match out.split_at_checked(1) {
            Some(("=", value)) => Reply::out(super::cut(value, EVAL_MAX)),
            Some(("!", thrown)) => {
                Reply::error(format!("The page threw {}", super::cut(thrown, EVAL_MAX)))
            }
            _ => return Err("The page's answer wasn't readable.".into()),
        };
        Ok(Reply {
            untrusted: true,
            ..reply
        })
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
        let (w, h, scale) = marks::picture_size(rect.w, rect.h, shot.scale);
        let labels = found["labels"].as_array().into_iter().flatten();
        let at = |l: &Value, k: &str| l[k].as_f64().unwrap_or(0.0) * scale;
        let picture = marks::Picture {
            rect,
            size: (w, h),
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
        let bytes = later(&self.app, move |_, done| {
            super::agent_picture(&id, picture, done)
        })?
        .ok_or("No picture: WebKit gave none.")?;
        let path = match path {
            Some(path) => {
                std::fs::write(&path, &bytes)
                    .map_err(|e| format!("Couldn't write {}: {e}", path.display()))?;
                path.display().to_string()
            }
            None => picks::save_png(&self.app, &bytes).ok_or("Couldn't save the picture.")?,
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

    fn console(&self, errors: bool, clear: bool) -> Res<Reply> {
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
        Ok(Reply {
            untrusted: true,
            ..Reply::out(control::console_text(&entries, errors))
        })
    }

    fn network(&self, ask: network::Ask) -> Res<Reply> {
        let (id, now, reveal) = (self.id.clone(), super::epoch_ms(), self.reveal.get());
        let (text, data) = main(&self.app, move |_| {
            super::network_log(&id, |log| network::answer(log, &ask, now, reveal))
        })??;
        Ok(Reply {
            untrusted: true,
            ..Reply::data(text, data)
        })
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
        let used = ago(Duration::from_secs(t.used_s));
        let page = titled(&t.origin, &t.title, "  ");
        format!(
            "{}{default}  {}  used {used}  {idle}  {data}  {page}",
            t.name, t.state
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

fn ago(idle: Duration) -> String {
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

    #[test]
    fn the_tab_list_gives_each_tabs_state_use_idle_data_and_page_on_a_line() {
        let tab = |name: &str, state, used_s, idle_min, origin: &str, title: &str| Listed {
            name: name.into(),
            default: name == "t1",
            state,
            used_s,
            idle_min,
            shared: name == "t3",
            origin: origin.into(),
            title: title.into(),
        };
        let tabs = [
            tab("t1", TabState::Awake, 5, 10, "http://localhost:3000", "App"),
            tab("rv268", TabState::Sleeping, 125, 0, "about:blank", ""),
            tab("t3", TabState::Parked, 7300, 30, "https://example.test", ""),
        ];
        assert_eq!(
            tabs_text(&tabs),
            "t1 (default)  awake  used 5s ago  idles after 10m  private  http://localhost:3000  App\n\
             rv268  sleeping  used 2m ago  never idles  private  about:blank\n\
             t3  parked  used 2h ago  idles after 30m  shared  https://example.test"
        );
        assert!(tabs_text(&[]).starts_with("No tabs"));
        // --json: the same fields.
        let json = serde_json::to_value(&tabs[1]).unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "name": "rv268", "default": false, "state": "sleeping", "used_s": 125,
                "idle_min": 0, "shared": false, "origin": "about:blank", "title": "" })
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
