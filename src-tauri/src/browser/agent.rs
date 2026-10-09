//! `gitviber browser` commands (control.rs), each run on its request's connection thread
//! (server.rs) as a few steps on the main thread, where the views are, waiting in between.

use super::control::{self, AgentScreen, Command, Reply, Request, Scroll, Wait};
use super::picks::{self, Bounds};
use super::{Go, PageState};
use crate::state::Res;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime};
use tauri::{AppHandle, Manager};

const AGENT: &str = include_str!("scripts/agent.js");
/// A step on the main thread answers at once: past this the app is stuck.
const STEP: Duration = Duration::from_secs(10);
/// What a script or a picture may take.
const SCRIPT: Duration = Duration::from_secs(30);
/// How long a page gets to load after open, a click or a key before the reply goes anyway.
const LOAD: Duration = Duration::from_secs(15);
const POLL: Duration = Duration::from_millis(100);
/// `wait --ms` at most: the CLI waits five minutes for any reply.
const LONGEST_WAIT: Duration = Duration::from_secs(120);
/// An eval's result, cut: an agent reads it.
const EVAL_MAX: usize = 20_000;
/// The result of the agent's script, as JSON however it came.
const SERIALIZE: &str = "try { const s = JSON.stringify(value); return s === undefined ? String(value) : s; } catch { return String(value); }";

pub fn run(app: &AppHandle, req: Request) -> Reply {
    command(app, req).unwrap_or_else(Reply::error)
}

fn command(app: &AppHandle, req: Request) -> Res<Reply> {
    let cmd = control::parse(&req.cmd, &req.args)?;
    match cmd {
        Command::Help => return Ok(Reply::out(control::HELP.trim_end())),
        Command::Devices => return Ok(Reply::out(control::device_list())),
        _ => {}
    }
    let pty = req
        .pty
        .ok_or("This terminal started before browser control was there: open a new one.")?;
    let root = control::worktree_root(Path::new(&req.cwd))
        .to_string_lossy()
        .into_owned();
    let first = match &cmd {
        Command::Open(url) => Some(control::web_url(url)?),
        _ => None,
    };
    let (id, made) = main(app, move |app| {
        super::agent_tab(app, pty, &root, first.as_deref())
    })?;
    Tab::wake(app, id)?.run(cmd, made, &req.cwd)
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

/// The pane's tab, awake for as long as this lives (agent_awake).
struct Tab {
    app: AppHandle,
    id: String,
}

impl Drop for Tab {
    fn drop(&mut self) {
        let id = std::mem::take(&mut self.id);
        let _ = self
            .app
            .run_on_main_thread(move || super::agent_awake(&id, false));
    }
}

impl Tab {
    fn wake(app: &AppHandle, id: String) -> Res<Self> {
        let woken = id.clone();
        main(app, move |_| {
            super::agent_awake(&woken, true);
            Ok(())
        })?;
        Ok(Self {
            app: app.clone(),
            id,
        })
    }

    fn run(&self, cmd: Command, made: bool, cwd: &str) -> Res<Reply> {
        match cmd {
            Command::Open(url) => {
                let url = control::web_url(&url)?;
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
            Command::Help | Command::Devices => Ok(Reply::out("")),
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
            std::thread::sleep(Duration::from_millis(ms).min(LONGEST_WAIT));
            return Ok(Reply::out(format!("Waited {ms} ms")));
        }
        let until = Instant::now() + timeout;
        let found = |input: Value| match self.script(input) {
            Ok(v) => Ok(v["found"] == true),
            // Between pages there's no page to ask.
            Err(e) if !e.starts_with("Not a CSS selector") => Ok(false),
            Err(e) => Err(e),
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
    fn eval(&self, js: &str) -> Res<Reply> {
        let run = |body: String| {
            let id = self.id.clone();
            later(&self.app, move |_, done| {
                super::agent_js(&id, true, &body, "", done)
            })?
        };
        let expression = format!("const value = await (async () => ({js}\n))();\n{SERIALIZE}");
        let statements = format!("const value = await (async () => {{\n{js}\n}})();\n{SERIALIZE}");
        let out = match run(expression) {
            Err(e) if e.contains("SyntaxError") => run(statements),
            other => other,
        }
        .map_err(|e| format!("The page threw {e}"))?;
        Ok(Reply::out(super::cut(&out, EVAL_MAX)))
    }

    fn screenshot(&self, path: Option<String>, target: Option<String>, cwd: &str) -> Res<Reply> {
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
        .ok_or("WebKit gave no picture of the page.")?;
        let path = match path {
            Some(path) => {
                let path = Path::new(cwd).join(path);
                std::fs::write(&path, &png)
                    .map_err(|e| format!("Couldn't write {}: {e}", path.display()))?;
                path
            }
            None => {
                let cache = self.app.path().app_cache_dir().map_err(|e| e.to_string())?;
                picks::save(&picks::folder(&cache), &png, SystemTime::now())
                    .map_err(|e| e.to_string())?
            }
        };
        Ok(Reply::out(path.display().to_string()))
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
}

/// An address with its page's title, when it has one.
fn titled(url: &str, title: &str, between: &str) -> String {
    if title.is_empty() {
        url.into()
    } else {
        format!("{url}{between}{title}")
    }
}
