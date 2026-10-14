//! `gitviber browser` commands (control.rs), each run on its request's connection thread
//! (server.rs) as a few steps on the main thread, where the views are, waiting in between.

use super::control::{
    self, AgentScreen, Command, Listed, Reply, Request, Scroll, Sessions, Tabs, Wait,
};
use super::picks::{self, Bounds};
use super::sessions::{self, Session, Shelf};
use super::{Go, PageState};
use crate::state::{AppState, Res};
use serde_json::{json, Value};
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
/// An eval's result, cut: an agent reads it.
const EVAL_MAX: usize = 20_000;
/// What `eval`'s script gives: `=` and its value as JSON however it came, or `!` and what it threw.
const SERIALIZE: &str = "try { const s = JSON.stringify(value); return `=${s === undefined ? String(value) : s}`; } catch { return `=${String(value)}`; }";
/// The page's storage for a session; in GitViber's world, where the page can't stand in for it.
const STORAGE_READ: &str = "return JSON.stringify({ origin: location.origin, local: Object.entries(localStorage), session: Object.entries(sessionStorage) });";
const STORAGE_WRITE: &str = "const s = JSON.parse(input); localStorage.clear(); sessionStorage.clear(); for (const [k, v] of s.local) localStorage.setItem(k, v); for (const [k, v] of s.session) sessionStorage.setItem(k, v); return '';";
const NO_INDEXED_DB: &str = "IndexedDB isn't carried.";

pub fn run(app: &AppHandle, req: Request) -> Reply {
    command(app, req).unwrap_or_else(Reply::error)
}

fn command(app: &AppHandle, req: Request) -> Res<Reply> {
    let cmd = control::parse(&req.cmd, &req.args)?;
    let pty = req
        .pty
        .ok_or("This terminal started before browser control was there: open a new one.")?;
    // The pane's own folder, as its workspace knows it: the tab goes to that worktree.
    let root = app
        .state::<AppState>()
        .ptys
        .pane(pty)
        .ok_or("This terminal isn't one of GitViber's open panes.")?
        .to_string_lossy()
        .into_owned();
    let name = match req.tab.as_deref() {
        Some("") => return Err("--tab needs the tab's name".into()),
        Some(name) => Some(control::tab_name(name)?),
        None => None,
    };
    match cmd {
        Command::Tab(op) => return tabs(app, pty, root, name, op, &req.cwd),
        Command::Session(op) => return sessions(app, pty, root, name, op),
        _ => {}
    }
    let first = match &cmd {
        Command::Open(url) => Some(url.clone()),
        _ => None,
    };
    let (id, made) = main(app, move |app| {
        super::agent_tab(app, pty, &root, name.as_deref(), first.as_deref())
    })?;
    // `made`: just made, at the address an open asks for.
    drive(app, id, |tab| tab.run(cmd, made, &req.cwd))
}

/// `steps` on tab `id` in its turn, as one command.
fn drive<T>(app: &AppHandle, id: String, steps: impl FnOnce(&Tab) -> Res<T>) -> Res<T> {
    let queue = queue(&id);
    let _turn = take_turn(&queue, STEP)?;
    let tab = Tab {
        app: app.clone(),
        id,
        command: NEXT_COMMAND.fetch_add(1, Ordering::Relaxed),
    };
    tab.start()?;
    steps(&tab)
}

/// `tab …`; `name`: the tab the command would go to (`--tab`), which `tab close` closes unless
/// it names one.
fn tabs(
    app: &AppHandle,
    pty: u32,
    root: String,
    name: Option<String>,
    op: Tabs,
    cwd: &str,
) -> Res<Reply> {
    match op {
        Tabs::New {
            name,
            url,
            shared,
            session,
        } => {
            // Read first: a session that isn't there makes no tab.
            let saved = match session {
                Some(s) => Some((shelf(app, &root)?.load(&s)?, s)),
                None => None,
            };
            let first = url.clone().filter(|_| saved.is_none());
            let (name, id) = main(app, move |app| {
                super::agent_new_tab(app, pty, &root, name.as_deref(), shared, first.as_deref())
            })?;
            if saved.is_none() && url.is_none() {
                return Ok(Reply::out(name));
            }
            let said = drive(app, id, |tab| {
                let mut said = vec![name.clone()];
                if let Some((session, s)) = &saved {
                    said.push(tab.load_session(s, session)?);
                }
                if let Some(url) = url {
                    let opened = tab.run(Command::Open(url), saved.is_none(), cwd)?;
                    said.push(opened.out.unwrap_or_default());
                }
                Ok(said.join("\n"))
            })
            .map_err(|e| format!("{name}\n{e}"))?;
            Ok(Reply::out(said))
        }
        Tabs::List => {
            let tabs = main(app, move |_| super::agent_tabs(pty))?;
            Ok(Reply::out(tabs_text(&tabs)))
        }
        Tabs::Close(named) => {
            let name = named.or(name);
            let closed = main(app, move |app| {
                super::agent_close_tab(app, pty, name.as_deref())
            })?;
            Ok(Reply::out(format!("Closed tab {closed}")))
        }
    }
}

/// `session …`, saved with the pane's repository; `name`: the tab the command goes to.
fn sessions(
    app: &AppHandle,
    pty: u32,
    root: String,
    name: Option<String>,
    op: Sessions,
) -> Res<Reply> {
    let shelf = shelf(app, &root)?;
    let tab = move || {
        main(app, move |app| {
            super::agent_tab(app, pty, &root, name.as_deref(), None)
        })
        .map(|(id, _)| id)
    };
    match op {
        Sessions::List => Ok(Reply::out(sessions_text(&shelf.list(), now()))),
        Sessions::Delete(s) => {
            shelf.delete(&s)?;
            Ok(Reply::out(format!("Deleted session {s}")))
        }
        Sessions::Save(s) => {
            let session = drive(app, tab()?, Tab::session)?;
            shelf.save(&s, &session)?;
            Ok(Reply::out(format!(
                "Saved session {s}: {}. {NO_INDEXED_DB}",
                summary(&session)
            )))
        }
        Sessions::Load(s) => {
            let session = shelf.load(&s)?;
            drive(app, tab()?, |tab| tab.load_session(&s, &session)).map(Reply::out)
        }
        Sessions::Clear => drive(app, tab()?, Tab::clear_session).map(Reply::out),
    }
}

fn shelf(app: &AppHandle, root: &str) -> Res<Shelf> {
    let data = app.path().app_data_dir().map_err(|e| e.to_string())?;
    Ok(Shelf::of(&data, root))
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

static NEXT_COMMAND: AtomicU64 = AtomicU64::new(1);

/// Each tab's turn: agents in a pane share its tabs, and two commands at once would interleave
/// their steps (a click between another's fill and its Enter).
fn queue(id: &str) -> Arc<Mutex<()>> {
    static QUEUES: LazyLock<Mutex<HashMap<String, Arc<Mutex<()>>>>> = LazyLock::new(Mutex::default);
    let mut queues = QUEUES.lock().unwrap_or_else(PoisonError::into_inner);
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
    rx.recv_timeout(SCRIPT)
        .map_err(|_| "The page didn't answer in time.".to_string())?
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

    fn run(&self, cmd: Command, made: bool, cwd: &str) -> Res<Reply> {
        match cmd {
            Command::Open(url) => {
                if !made {
                    let id = self.id.clone();
                    main(&self.app, move |_| super::navigate(&id, &url))?;
                }
                self.arrived("Opened")
            }
            // Of the pane's tabs and sessions, not one page's: `tabs` and `sessions` answer them.
            Command::Tab(_) | Command::Session(_) => Err("Not a command for a page.".into()),
            Command::Back => self.go(Go::Back, "Back at"),
            Command::Forward => self.go(Go::Forward, "Forward at"),
            Command::Reload => self.go(Go::Reload, "Reloaded"),
            Command::Url => {
                let page = self.page()?.0;
                Ok(Reply::out(titled(&page.url, &page.title, "\n")))
            }
            Command::Snapshot {
                interactive,
                scope,
                depth,
            } => self.act(json!({ "cmd": "snapshot", "interactive": interactive, "scope": scope, "depth": depth })),
            Command::Click(target) => self.acted(json!({ "cmd": "click", "target": target })),
            Command::Fill(target, text) => {
                self.act(json!({ "cmd": "fill", "target": target, "text": text }))
            }
            Command::Type(target, text) => {
                self.act(json!({ "cmd": "type", "target": target, "text": text }))
            }
            Command::Select(target, values) => {
                self.act(json!({ "cmd": "select", "target": target, "values": values }))
            }
            Command::Hover(target) => self.act(json!({ "cmd": "hover", "target": target })),
            Command::Press(key) => self.acted(json!({ "cmd": "press", "key": key })),
            Command::Scroll(to, px) => {
                let to = match to {
                    Scroll::Up => "up".to_string(),
                    Scroll::Down => "down".to_string(),
                    Scroll::To(target) => target,
                };
                self.act(json!({ "cmd": "scroll", "to": to, "px": px }))
            }
            Command::Wait(what, timeout) => self.wait(what, timeout),
            Command::Eval(js) => self.eval(&js),
            Command::Screenshot { path, target } => self.screenshot(path, target, cwd),
            Command::Console { errors, clear } => self.console(errors, clear),
            Command::Device(screen) => self.device(screen),
            Command::Appearance(dark) => {
                let id = self.id.clone();
                main(&self.app, move |app| super::agent_appearance(app, &id, dark))?;
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

    /// The page, and whether its console is kept.
    fn page(&self) -> Res<(PageState, bool)> {
        let id = self.id.clone();
        main(&self.app, move |_| super::agent_page(&id))
    }

    /// agent.js on the page, given `input`: its answer, or the error it gave.
    fn script(&self, input: Value) -> Res<Value> {
        let (id, input) = (self.id.clone(), input.to_string());
        let out = later(&self.app, move |_, done| {
            super::agent_js(&id, false, AGENT, &input, done)
        })??;
        let value: Value =
            serde_json::from_str(&out).map_err(|_| "The page's answer wasn't readable.")?;
        match value.get("error").and_then(Value::as_str) {
            Some(error) => Err(error.into()),
            None => Ok(value),
        }
    }

    fn act(&self, input: Value) -> Res<Reply> {
        let value = self.script(input)?;
        Ok(Reply::out(value["out"].as_str().unwrap_or_default()))
    }

    /// A click or a key, which may send the page somewhere: the reply waits for it, and says.
    fn acted(&self, input: Value) -> Res<Reply> {
        let before = self.page()?.0.url;
        let did = self.script(input)?;
        let did = did["out"].as_str().unwrap_or_default();
        std::thread::sleep(POLL);
        let (page, _) = self.loaded(LOAD)?;
        Ok(Reply::out(if page.url == before {
            did.to_string()
        } else {
            format!("{did}\nNow at {}", titled(&page.url, &page.title, " — "))
        }))
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

    fn wait(&self, what: Wait, timeout: Duration) -> Res<Reply> {
        if let Wait::Ms(ms) = what {
            std::thread::sleep(Duration::from_millis(ms));
            return Ok(Reply::out(format!("Waited {ms} ms")));
        }
        let until = Instant::now() + timeout;
        let found = |input: Value| match self.script(input) {
            Ok(v) if v["invalid"] == true => Err("That isn't a CSS selector.".to_string()),
            Ok(v) => Ok(v["found"] == true),
            // Between pages there's no page to ask.
            Err(_) => Ok(false),
        };
        loop {
            let (done, said) = match &what {
                Wait::Load => (!self.page()?.0.loading, "the page to load".to_string()),
                Wait::Css(css) => (
                    found(json!({ "cmd": "exists", "css": css }))?,
                    format!("{css} to show"),
                ),
                Wait::Text(text) => (
                    found(json!({ "cmd": "hasText", "text": text }))?,
                    format!("{text:?} to show"),
                ),
                Wait::Ms(_) => (true, String::new()),
            };
            if done {
                return Ok(Reply::out(format!("Done waiting for {said}")));
            }
            if Instant::now() >= until {
                return Err(format!(
                    "Timed out after {} ms waiting for {said}",
                    timeout.as_millis()
                ));
            }
            std::thread::sleep(POLL * 2);
        }
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
        match out.split_at_checked(1) {
            Some(("=", value)) => Ok(Reply::out(super::cut(value, EVAL_MAX))),
            Some(("!", thrown)) => Err(format!("The page threw {}", super::cut(thrown, EVAL_MAX))),
            _ => Err("The page's answer wasn't readable.".into()),
        }
    }

    fn screenshot(&self, path: Option<String>, target: Option<String>, cwd: &str) -> Res<Reply> {
        let path = path
            .map(|p| control::picture_path(Path::new(cwd), &p))
            .transpose()?;
        let rect = match target {
            Some(target) => {
                let found = self.script(json!({ "cmd": "rect", "target": target }))?;
                Some(
                    serde_json::from_value::<Bounds>(found["rect"].clone())
                        .map_err(|_| "The element has no box.")?,
                )
            }
            None => None,
        };
        let id = self.id.clone();
        let png = later(&self.app, move |_, done| {
            super::agent_picture(&id, rect, done)
        })?
        .ok_or("No picture: the element doesn't show, or WebKit gave none.")?;
        let path = match path {
            Some(path) => {
                std::fs::write(&path, &png)
                    .map_err(|e| format!("Couldn't write {}: {e}", path.display()))?;
                path.display().to_string()
            }
            None => picks::save_png(&self.app, &png).ok_or("Couldn't save the picture.")?,
        };
        Ok(Reply::out(path))
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
        Ok(Reply::out(control::console_text(&entries, errors)))
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

    /// The tab's login: its cookies (in the user's own data, only those of the page's site), and
    /// its page's storage.
    fn session(&self) -> Res<Session> {
        #[derive(serde::Deserialize)]
        struct Storage {
            origin: String,
            local: Vec<(String, String)>,
            session: Vec<(String, String)>,
        }
        let blank = "Open the page whose login to save first: this one keeps no storage.";
        let id = self.id.clone();
        let read = later(&self.app, move |_, done| {
            super::agent_js(&id, false, STORAGE_READ, "", done)
        })?
        .map_err(|_| blank)?;
        let storage: Storage = serde_json::from_str(&read).map_err(|_| blank)?;
        let host = Url::parse(&storage.origin)
            .ok()
            .filter(|u| matches!(u.scheme(), "http" | "https"))
            .and_then(|u| u.host_str().map(str::to_string))
            .ok_or(blank)?;
        let id = self.id.clone();
        let shared = main(&self.app, move |_| super::agent_shared(&id))?;
        let id = self.id.clone();
        let mut cookies = later(&self.app, move |_, done| super::agent_cookies(&id, done))?;
        if shared {
            cookies.retain(|c| sessions::for_host(&host, &c.domain));
        }
        Ok(Session {
            origin: storage.origin,
            saved: now(),
            cookies,
            local: storage.local,
            session: storage.session,
        })
    }

    /// Session `s` in place of whatever the tab's own store had: its cookies set, then its
    /// storage written at its origin, which loads again to read it.
    fn load_session(&self, name: &str, s: &Session) -> Res<String> {
        let id = self.id.clone();
        later(&self.app, move |_, done| {
            super::agent_clear_store(&id, done)
        })?;
        let (id, cookies) = (self.id.clone(), s.cookies.clone());
        later(&self.app, move |_, done| {
            super::agent_set_cookies(&id, cookies, done)
        })?;
        let (id, origin) = (self.id.clone(), s.origin.clone());
        main(&self.app, move |_| super::navigate(&id, &origin))?;
        self.loaded(LOAD)?;
        if !s.local.is_empty() || !s.session.is_empty() {
            let input = json!({ "local": s.local, "session": s.session }).to_string();
            let id = self.id.clone();
            later(&self.app, move |_, done| {
                super::agent_js(&id, false, STORAGE_WRITE, &input, done)
            })??;
            self.reload()?;
        }
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
        let page = titled(&t.url, &t.title, " — ");
        let default = if t.default { "default, " } else { "" };
        let data = if t.shared { "shared" } else { "private" };
        format!(
            "{}  {page}  ({default}{data}, used {})",
            t.name,
            ago(t.idle)
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

fn ago(idle: Duration) -> String {
    match idle.as_secs() {
        s @ 0..60 => format!("{s} s ago"),
        s @ 60..3600 => format!("{} min ago", s / 60),
        s => format!("{} h ago", s / 3600),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tab_list_marks_the_default_and_says_when_each_was_used() {
        let tab = |name: &str, default: bool, url: &str, title: &str, secs: u64| Listed {
            name: name.into(),
            default,
            shared: name == "t3",
            url: url.into(),
            title: title.into(),
            idle: Duration::from_secs(secs),
        };
        assert_eq!(
            tabs_text(&[
                tab("t1", true, "http://localhost:3000/", "App", 5),
                tab("rv268", false, "about:blank", "", 125),
                tab("t3", false, "https://example.test/", "", 7300),
            ]),
            "t1  http://localhost:3000/ — App  (default, private, used 5 s ago)\n\
             rv268  about:blank  (private, used 2 min ago)\n\
             t3  https://example.test/  (shared, used 2 h ago)"
        );
        assert!(tabs_text(&[]).starts_with("No tabs"));
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
        assert_eq!(text, "mert  https://app.test  1 cookies  (saved 3 h ago)");
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
    }
}
