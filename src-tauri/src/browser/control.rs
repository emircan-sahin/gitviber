//! `gitviber browser …` from an agent in a GitViber terminal: the request it sends over the
//! socket (server.rs), what it may ask, and the tabs each terminal pane's agents get. The page's
//! part is scripts/agent.js; apart from AppKit, so it can be tested.

use super::console::Entry;
use super::network;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tauri::Url;

/// What a terminal's shell gets (pty.rs), and the CLI reads back.
pub const EXE_ENV: &str = "GITVIBER_EXE";
pub const SOCKET_ENV: &str = "GITVIBER_BROWSER_SOCKET";
pub const TOKEN_ENV: &str = "GITVIBER_BROWSER_TOKEN";
pub const PTY_ENV: &str = "GITVIBER_PTY";
/// The tab a command goes to when it names none (`--tab`): set by the agent, not the app.
pub const TAB_ENV: &str = "GITVIBER_BROWSER_TAB";

/// The CLI's exit status when control is off.
pub const OFF: i32 = 2;
pub const OFF_TEXT: &str = "Browser control is off (Settings → Browser).";
/// The CLI's exit status outside GitViber's terminals, or with GitViber gone.
pub const NOT_HERE: i32 = 3;

/// How long `wait` waits unless told.
pub const WAIT: Duration = Duration::from_secs(10);
/// The longest a command waits, inside the CLI's own wait for any reply.
pub const LONGEST_WAIT: Duration = Duration::from_secs(120);
pub const CLI_WAIT: Duration = Duration::from_secs(300);

pub const HELP: &str = "\
gitviber browser <command> [args]: this terminal pane's own browser tabs in GitViber, opened in
the background and drawn while its agents use them. A command goes to the tab --tab names, else
the one $GITVIBER_BROWSER_TAB names, else the pane's default tab, which its first command opens;
a name no open tab of the pane has fails. Agents in one pane share its tabs; the user's own tabs
are out of their reach. Each tab, the default one too, has cookies and storage of its own, gone
when it closes, unless --shared. A tab no command touched for 10 minutes sleeps: slowed, and in
time parked to save memory, its next command loading it again. Unused for an hour, it closes,
unless the user has it on show. A popup (window.open, a target=_blank link) opens as another tab
named for its opener (t1-popup1), in the opener's cookies; a popup that closes itself closes its
tab. A page whose process ends (a crash, memory) loads again; ending again within 30 s, it's left
stopped. Replies end with what happened meanwhile, each line starting with →.

  tab new [name] [url] [--shared] [--session <name>] [--idle <minutes>]
                               Another tab, at url if given; prints its name: the one --tab or
                               $GITVIBER_BROWSER_TAB names, else t1, t2, ... (letters, digits, -
                               and _). --shared: in the user's own cookies and logins.
                               --session: a saved session loaded first. --idle: minutes before
                               it sleeps, 0 never while the pane is open (120 at most); it closes
                               unused after an hour, or six times its idle time, if longer
  tab list                     The pane's tabs: name, awake, sleeping or parked, last command,
                               idle minutes, private or shared, origin and title
  tab close [name]             Close a tab once its running command is done: the one a command
                               would go to, unless named
  --tab <name>                 With any command: the tab it goes to

  session save <name> [--force]
                               The tab's cookies, and its page's localStorage and sessionStorage
                               (not IndexedDB), saved for this repository and its worktrees; over
                               a session of that name only with --force. Not from a --shared tab
  session load <name>          Into the tab, on the page's origin the session was saved at (a
                               blank tab goes there first): the tab's cookies and storage
                               cleared, then the saved ones set. Not into a --shared tab
  session clear                The tab's own cookies and storage, gone
  session list                 Each saved session's name, origin, cookie count and age
  session delete <name>        A saved session gone. A session's name is one whatever its case

  open <url>                   Load a page (http or https; localhost:3000 works) and wait for it
  back | forward | reload      Through its history, waiting for the page
  url                          The page's address and title
  snapshot [-i] [-s <css>] [-d <n>]
                               The page as a tree, with refs (e1, e2, ...) to act on: -i only
                               what can be clicked or typed in, -s under one element, -d so many
                               levels deep. Refs (also @e1 or ref=e1) last until the page changes.
  click <ref|css>              Click an element
  fill <ref|css> <text>        Replace a field's text
  type <ref|css> <text>        Type into a field, key by key
  select <ref|css> <value>...  Choose a <select>'s options, by value or label
  hover <ref|css>              The mouse over an element (no CSS :hover)
  press <key>                  A key to the focused element: Enter, Tab, Escape, ArrowDown, Meta+a
  scroll <up|down|ref|css> [px]
  wait <css> | --text <text> | --load | --download | --ms <n> [--timeout <ms>]
                               Until an element shows, the text does, the page has loaded, a
                               download ends (one that ended in the minute before counts; prints
                               its path, size and type), or the time is up; 10 s at most unless
                               --timeout says (120 s at most)
  eval <js>                    Runs in the page; prints the result as JSON
  screenshot [path] [--ref <ref|css>]
                               A PNG of the page, or of one element; prints its path
  console [--errors] [--clear] What the page logged
  downloads [--clear]          The tab's downloaded files, newest first, with their paths: kept
                               in a folder of the tab's own (its newest 20, a day at most), gone
                               when it closes; one that ends is told in the next reply
  network [--failed] [--filter <text>] [--type fetch|xhr|ws|resource] [--clear]
                               What the page requested and loaded, one line each with an id;
                               the newest 300 requests and 500 other loads
  network <id>                 One in full: headers, bodies, WebSocket frames
  device <name|WxH|off>        Show the page as a device; `device --list` names them
  appearance light|dark|auto   The page's prefers-color-scheme; auto is the app's
  dialogs accept|dismiss       How the page's alert, confirm and prompt are answered while a
                               command runs on it; dismiss unless told
  help

  --json                       The reply as JSON

Exit status: 0 done; 1 failed, why on stderr; 2 browser control is off (Settings → Browser);
3 not in a GitViber terminal, or GitViber isn't running.
";

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct Request {
    pub token: String,
    /// The pane it came from (PTY_ENV); its tabs are this pane's.
    pub pty: Option<u32>,
    /// The tab it goes to (`--tab`, else TAB_ENV); the pane's default one if none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab: Option<String>,
    pub cwd: String,
    pub cmd: String,
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Default, PartialEq)]
pub struct Reply {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub out: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// The reply as JSON, for `--json`, where a command has more to say than its text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
    /// The exit status when not 0 or 1 (OFF).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<i32>,
}

impl Reply {
    pub fn out(text: impl Into<String>) -> Self {
        Self {
            ok: true,
            out: Some(text.into()),
            ..Self::default()
        }
    }

    pub fn data(text: impl Into<String>, data: serde_json::Value) -> Self {
        Self {
            data: Some(data),
            ..Self::out(text)
        }
    }

    pub fn error(text: impl Into<String>) -> Self {
        Self {
            error: Some(text.into()),
            ..Self::default()
        }
    }

    pub fn exit_code(&self) -> i32 {
        self.code.unwrap_or(if self.ok { 0 } else { 1 })
    }
}

/// Pane `pty`'s own token from the launch's `secret`: GITVIBER_PTY is the agent's to change, so
/// a pane's token reaches its own tabs and sessions only.
pub fn pane_token(secret: &str, pty: u32) -> String {
    hmac(secret.as_bytes(), pty.to_string().as_bytes())
}

/// HMAC-SHA256, hex.
fn hmac(key: &[u8], data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut block = [0u8; 64];
    if key.len() > block.len() {
        block[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        block[..key.len()].copy_from_slice(key);
    }
    let pad = |byte: u8| block.map(|k| k ^ byte);
    let inner = Sha256::new()
        .chain_update(pad(0x36))
        .chain_update(data)
        .finalize();
    let outer = Sha256::new()
        .chain_update(pad(0x5c))
        .chain_update(inner)
        .finalize();
    format!("{outer:x}")
}

/// The token as sent against ours, in time that doesn't depend on where they differ.
pub fn same_token(sent: &str, ours: &str) -> bool {
    let (a, b) = (sent.as_bytes(), ours.as_bytes());
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[derive(Debug, PartialEq)]
pub enum Command {
    Open(String),
    Back,
    Forward,
    Reload,
    Url,
    Snapshot {
        interactive: bool,
        scope: Option<String>,
        depth: Option<u32>,
    },
    Click(String),
    Fill(String, String),
    Type(String, String),
    Select(String, Vec<String>),
    Hover(String),
    Press(String),
    Scroll(Scroll, Option<u32>),
    Wait(Wait, Duration),
    Eval(String),
    Screenshot {
        path: Option<String>,
        target: Option<String>,
    },
    Console {
        errors: bool,
        clear: bool,
    },
    /// The tab's downloaded files, or (true) their removal.
    Downloads(bool),
    Network(network::Ask),
    Device(Option<AgentScreen>),
    /// Whether the page's dialogs are accepted (OK) rather than dismissed.
    Dialogs(bool),
    /// Dark, light, or the app's (None).
    Appearance(Option<bool>),
}

/// A command for the pane's tabs or sessions, apart from any one page's (Command).
#[derive(Debug, PartialEq)]
pub enum Pane {
    Tab(Tabs),
    Session(Sessions),
}

#[derive(Debug, PartialEq)]
pub enum Tabs {
    New {
        name: Option<String>,
        url: Option<String>,
        /// In the user's website data, not one of its own.
        shared: bool,
        /// A saved session loaded into it first.
        session: Option<String>,
        /// Minutes it may go without a command before it sleeps; 0 never.
        idle: Option<u32>,
    },
    List,
    Close(Option<String>),
}

#[derive(Debug, PartialEq)]
pub enum Sessions {
    /// Over one of the same name too (`--force`).
    Save(String, bool),
    Load(String),
    Clear,
    List,
    Delete(String),
}

#[derive(Debug, PartialEq)]
pub enum Scroll {
    Up,
    Down,
    To(String),
}

#[derive(Debug, PartialEq)]
pub enum Wait {
    Css(String),
    Text(String),
    Load,
    /// A download of the tab's to end.
    Download,
    Ms(u64),
}

/// The command a request names, or how to say it.
pub fn parse(cmd: &str, args: &[String]) -> Result<Command, String> {
    let rest = || args.join(" ");
    let target = |usage: &str| match args {
        [one] if !one.is_empty() => Ok(one.clone()),
        _ => Err(format!("Usage: gitviber browser {usage}")),
    };
    let target_and_text = |usage: &str| match args {
        [target, text @ ..] if !text.is_empty() => Ok((target.clone(), text.join(" "))),
        _ => Err(format!("Usage: gitviber browser {usage}")),
    };
    let none = |c: Command| {
        if args.is_empty() {
            Ok(c)
        } else {
            Err(format!("`{cmd}` takes nothing more"))
        }
    };
    match cmd {
        "open" => web_url(&target("open <url>")?).map(Command::Open),
        "back" => none(Command::Back),
        "forward" => none(Command::Forward),
        "reload" => none(Command::Reload),
        "url" => none(Command::Url),
        "snapshot" => snapshot(args),
        "click" => target("click <ref|css>").map(Command::Click),
        "fill" => target_and_text("fill <ref|css> <text>").map(|(t, x)| Command::Fill(t, x)),
        "type" => target_and_text("type <ref|css> <text>").map(|(t, x)| Command::Type(t, x)),
        "select" => match args {
            [target, values @ ..] if !values.is_empty() => {
                Ok(Command::Select(target.clone(), values.to_vec()))
            }
            _ => Err("Usage: gitviber browser select <ref|css> <value>...".into()),
        },
        "hover" => target("hover <ref|css>").map(Command::Hover),
        "press" => target("press <key>").map(Command::Press),
        "scroll" => scroll(args),
        "wait" => wait(args),
        "eval" if !args.is_empty() => Ok(Command::Eval(rest())),
        "eval" => Err("Usage: gitviber browser eval <js>".into()),
        "screenshot" => screenshot(args),
        "console" => console(args),
        "downloads" => match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
            [] => Ok(Command::Downloads(false)),
            ["--clear"] => Ok(Command::Downloads(true)),
            _ => Err("Usage: gitviber browser downloads [--clear]".into()),
        },
        "network" => network::ask(args).map(Command::Network),
        "device" if args.is_empty() => {
            Err("Usage: gitviber browser device <name|WxH|off>, or device --list".into())
        }
        "device" => device(&rest()).map(Command::Device),
        "appearance" => match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
            ["dark"] => Ok(Command::Appearance(Some(true))),
            ["light"] => Ok(Command::Appearance(Some(false))),
            ["auto"] => Ok(Command::Appearance(None)),
            _ => Err("Usage: gitviber browser appearance light|dark|auto".into()),
        },
        "dialogs" => match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
            [] | ["dismiss"] => Ok(Command::Dialogs(false)),
            ["accept"] => Ok(Command::Dialogs(true)),
            _ => Err("Usage: gitviber browser dialogs accept|dismiss".into()),
        },
        _ => Err(format!(
            "No command `{cmd}`: `gitviber browser help` lists them"
        )),
    }
}

fn value<'a>(it: &mut impl Iterator<Item = &'a String>, flag: &str) -> Result<&'a String, String> {
    it.next().ok_or_else(|| format!("{flag} needs a value"))
}

fn number<T: std::str::FromStr>(s: &str, flag: &str) -> Result<T, String> {
    s.parse()
        .map_err(|_| format!("{flag} takes a whole number, not {s:?}"))
}

/// A command for the pane rather than a page (`tab`, `session`); None for any other.
pub fn parse_pane(cmd: &str, args: &[String]) -> Option<Result<Pane, String>> {
    match cmd {
        "tab" => Some(tabs(args).map(Pane::Tab)),
        "session" => Some(sessions(args).map(Pane::Session)),
        _ => None,
    }
}

/// The longest `--idle`, in minutes.
const MOST_IDLE: u32 = 120;

fn tabs(args: &[String]) -> Result<Tabs, String> {
    let usage = "Usage: gitviber browser tab new [name] [url] [--shared] [--session <name>] \
                 [--idle <minutes>] | tab list | tab close [name]";
    let (mut shared, mut session, mut idle, mut words) = (false, None, None, vec![]);
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--shared" => shared = true,
            "--session" => session = Some(session_name(value(&mut it, "--session")?)?),
            "--idle" => {
                let minutes = number(value(&mut it, "--idle")?, "--idle")?;
                if minutes > MOST_IDLE {
                    return Err(format!("--idle is {MOST_IDLE} minutes at most"));
                }
                idle = Some(minutes);
            }
            _ => words.push(arg.as_str()),
        }
    }
    let new = |name: Option<&str>, url: Option<&str>| {
        if shared && session.is_some() {
            return Err("--session loads into a tab of its own: not with --shared".to_string());
        }
        Ok(Tabs::New {
            name: name.map(tab_name).transpose()?,
            url: url.map(web_url).transpose()?,
            shared,
            session: session.clone(),
            idle,
        })
    };
    let flags = shared || session.is_some() || idle.is_some();
    match words[..] {
        ["new"] => new(None, None),
        // One word is a name when it could be one; an address has a dot, colon or slash.
        ["new", one] if tab_name(one).is_ok() => new(Some(one), None),
        ["new", url] => new(None, Some(url)),
        ["new", name, url] => new(Some(name), Some(url)),
        ["list"] if !flags => Ok(Tabs::List),
        ["close"] if !flags => Ok(Tabs::Close(None)),
        ["close", name] if !flags => Ok(Tabs::Close(Some(tab_name(name)?))),
        _ => Err(usage.into()),
    }
}

fn sessions(args: &[String]) -> Result<Sessions, String> {
    match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["save", name] => Ok(Sessions::Save(session_name(name)?, false)),
        ["save", name, "--force"] | ["save", "--force", name] => {
            Ok(Sessions::Save(session_name(name)?, true))
        }
        ["load", name] => Ok(Sessions::Load(session_name(name)?)),
        ["clear"] => Ok(Sessions::Clear),
        ["list"] => Ok(Sessions::List),
        ["delete", name] => Ok(Sessions::Delete(session_name(name)?)),
        _ => Err(
            "Usage: gitviber browser session save <name> [--force] | load <name> | clear | \
                  list | delete <name>"
                .into(),
        ),
    }
}

pub fn tab_name(name: &str) -> Result<String, String> {
    named("tab", name)
}

fn session_name(name: &str) -> Result<String, String> {
    named("session", name)
}

/// A name as given: kept to what a shell passes as is and a file can be called, and no flag.
fn named(what: &str, name: &str) -> Result<String, String> {
    let fits = (1..=32).contains(&name.len())
        && !name.starts_with('-')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if fits {
        Ok(name.into())
    } else {
        Err(format!(
            "A {what}'s name is letters, digits, - and _, 32 at most: not {name:?}"
        ))
    }
}

fn snapshot(args: &[String]) -> Result<Command, String> {
    let (mut interactive, mut scope, mut depth) = (false, None, None);
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-i" | "--interactive" => interactive = true,
            "-s" | "--selector" => scope = Some(value(&mut it, arg)?.clone()),
            "-d" | "--depth" => depth = Some(number(value(&mut it, arg)?, arg)?),
            _ => return Err(format!("snapshot doesn't take {arg:?}")),
        }
    }
    Ok(Command::Snapshot {
        interactive,
        scope,
        depth,
    })
}

fn scroll(args: &[String]) -> Result<Command, String> {
    let to = match args.first().map(String::as_str) {
        Some("up") => Scroll::Up,
        Some("down") => Scroll::Down,
        Some(t) => Scroll::To(t.into()),
        None => return Err("Usage: gitviber browser scroll <up|down|ref|css> [px]".into()),
    };
    let px = match args.get(1..) {
        Some([px]) => Some(number(px, "scroll")?),
        Some([]) | None => None,
        Some(_) => return Err("Usage: gitviber browser scroll <up|down|ref|css> [px]".into()),
    };
    Ok(Command::Scroll(to, px))
}

fn wait(args: &[String]) -> Result<Command, String> {
    let (mut what, mut timeout, mut css) = (None, WAIT, vec![]);
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--text" => what = Some(Wait::Text(value(&mut it, arg)?.clone())),
            "--load" => what = Some(Wait::Load),
            "--download" => what = Some(Wait::Download),
            "--ms" => what = Some(Wait::Ms(number(value(&mut it, arg)?, arg)?)),
            "--timeout" => {
                timeout =
                    Duration::from_millis(number(value(&mut it, arg)?, arg)?).min(LONGEST_WAIT)
            }
            _ if arg.starts_with("--") => return Err(format!("wait doesn't take {arg}")),
            _ => css.push(arg.as_str()),
        }
    }
    let what = match (what, css.is_empty()) {
        (Some(w), true) => w,
        (None, false) => Wait::Css(css.join(" ")),
        _ => return Err(
            "Usage: gitviber browser wait <css> | --text <text> | --load | --download | --ms <n>"
                .into(),
        ),
    };
    let what = match what {
        Wait::Ms(ms) => Wait::Ms(ms.min(LONGEST_WAIT.as_millis() as u64)),
        w => w,
    };
    Ok(Command::Wait(what, timeout))
}

fn screenshot(args: &[String]) -> Result<Command, String> {
    let (mut path, mut target) = (None, None);
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--ref" => target = Some(value(&mut it, arg)?.clone()),
            _ if arg.starts_with("--") => return Err(format!("screenshot doesn't take {arg}")),
            _ if path.is_none() => path = Some(arg.clone()),
            _ => return Err("Usage: gitviber browser screenshot [path] [--ref <ref|css>]".into()),
        }
    }
    Ok(Command::Screenshot { path, target })
}

fn console(args: &[String]) -> Result<Command, String> {
    let (mut errors, mut clear) = (false, false);
    for arg in args {
        match arg.as_str() {
            "--errors" => errors = true,
            "--clear" => clear = true,
            _ => return Err(format!("console doesn't take {arg:?}")),
        }
    }
    Ok(Command::Console { errors, clear })
}

/// Scheme, host and port: where a page's logins and storage belong. A page that isn't on the
/// web, as it is.
pub fn origin(url: &str) -> String {
    match Url::parse(url) {
        Ok(u) if matches!(u.scheme(), "http" | "https") => u.origin().ascii_serialization(),
        _ => url.into(),
    }
}

/// An address an agent typed, as the address bar takes it: a local host by http, any other
/// by https; only web pages.
pub fn web_url(typed: &str) -> Result<String, String> {
    let typed = typed.trim();
    let full = if typed.contains("://") {
        typed.to_string()
    } else {
        let authority = typed.split(['/', '?', '#']).next().unwrap_or_default();
        let host = match authority.rsplit_once(':') {
            Some((host, port)) if port.chars().all(|c| c.is_ascii_digit()) => host,
            _ => authority,
        };
        let local = super::is_loopback(host) || host.parse::<std::net::IpAddr>().is_ok();
        format!("{}://{typed}", if local { "http" } else { "https" })
    };
    let url = Url::parse(&full).map_err(|_| format!("Not a web address: {typed}"))?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(format!("Only http and https pages open here, not {typed}"));
    }
    Ok(url.to_string())
}

/// Where `screenshot <path>` writes: a .png under the CLI's folder (no `..` out of it), or at
/// an absolute path; never over anything but a PNG.
pub fn picture_path(cwd: &Path, path: &str) -> Result<PathBuf, String> {
    let given = Path::new(path);
    if !given
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("png"))
    {
        return Err(format!("A screenshot's path ends in .png, not {path}"));
    }
    let full = if given.is_absolute() {
        given.to_path_buf()
    } else if given
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(format!(
            "{path} leaves this folder: give an absolute path instead"
        ));
    } else {
        cwd.join(given)
    };
    // A link, a folder or a file of another kind there stays as it is.
    if let Ok(meta) = std::fs::symlink_metadata(&full) {
        let mut head = [0u8; 8];
        let png = meta.is_file()
            && std::fs::File::open(&full)
                .and_then(|mut f| std::io::Read::read_exact(&mut f, &mut head))
                .is_ok()
            && head == *b"\x89PNG\r\n\x1a\n";
        if !png {
            return Err(format!("{} is there and isn't a PNG", full.display()));
        }
    }
    Ok(full)
}

/// The tabs each pane's agents opened, by name. A command goes to the one it names, else to the
/// pane's default tab: agents in a pane share its environment, so none picks a tab for the
/// others. Tabs the user closed (`open` says) drop out as the pane's are asked for.
#[derive(Default)]
pub struct Routes {
    panes: HashMap<u32, PaneTabs>,
}

#[derive(Default)]
struct PaneTabs {
    /// In the order made.
    tabs: Vec<Named>,
    /// Names given (t1, t2, ...) so far.
    named: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Named {
    pub name: String,
    pub route: Route,
    /// Where commands that name no tab go; the pane's first such command opened it.
    pub default: bool,
    /// In the user's website data, not its own.
    pub shared: bool,
    /// How long it may go without a command before it sleeps; zero never.
    pub idle: Duration,
    pub used: Instant,
    /// When it closes unused (`unused_after`), unless a command or the user's eye comes first.
    pub closes: Option<Instant>,
}

impl Named {
    fn used_at(&mut self, now: Instant) {
        self.used = now;
        self.closes = unused_after(self.idle).map(|after| now + after);
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Route {
    pub id: String,
    pub root: String,
}

/// How long an agent's tab goes without a command before it sleeps, unless it says.
pub const IDLE: Duration = Duration::from_secs(10 * 60);
/// How long one goes unused before it closes: agents forget to close tabs.
const UNUSED: Duration = Duration::from_secs(60 * 60);

/// When a tab that sleeps after `idle` closes unused; never for one that never sleeps.
pub fn unused_after(idle: Duration) -> Option<Duration> {
    (!idle.is_zero()).then(|| UNUSED.max(idle * 6))
}

/// A new tab as asked for: its name (else t1, t2, ...), whether it's the pane's default, in
/// the user's own data, and its idle time.
pub struct NewTab<'a> {
    pub name: Option<&'a str>,
    pub default: bool,
    pub shared: bool,
    pub idle: Duration,
}

impl Routes {
    fn pane(&mut self, pty: u32, open: impl Fn(&str) -> bool) -> &mut PaneTabs {
        let pane = self.panes.entry(pty).or_default();
        pane.tabs.retain(|t| open(&t.route.id));
        pane
    }

    /// The tab named so, else the default one, used now; None when the default is to be made.
    pub fn tab(
        &mut self,
        pty: u32,
        name: Option<&str>,
        open: impl Fn(&str) -> bool,
    ) -> Result<Option<Route>, String> {
        let pane = self.pane(pty, open);
        let found = pane.tabs.iter_mut().find(|t| match name {
            Some(name) => t.name == name,
            None => t.default,
        });
        match (found, name) {
            (Some(t), _) => {
                t.used_at(Instant::now());
                Ok(Some(t.route.clone()))
            }
            (None, Some(name)) => Err(format!(
                "No tab {name} in this pane: `gitviber browser tab list` names them"
            )),
            (None, None) => Ok(None),
        }
    }

    /// A tab made for the pane under the name asked for, while no open tab has it, else the
    /// next free t1, t2, ...: its name.
    pub fn add(
        &mut self,
        pty: u32,
        new: NewTab,
        route: Route,
        open: impl Fn(&str) -> bool,
    ) -> Result<String, String> {
        let pane = self.pane(pty, open);
        let taken = |pane: &PaneTabs, name: &str| pane.tabs.iter().any(|t| t.name == name);
        let name = match new.name {
            Some(name) if taken(pane, name) => {
                return Err(format!(
                    "Tab {name} is open already: give another name, or close it first"
                ))
            }
            Some(name) => name.to_string(),
            None => loop {
                pane.named += 1;
                let name = format!("t{}", pane.named);
                if !taken(pane, &name) {
                    break name;
                }
            },
        };
        let now = Instant::now();
        pane.tabs.push(Named {
            name: name.clone(),
            route,
            default: new.default,
            shared: new.shared,
            idle: new.idle,
            used: now,
            closes: unused_after(new.idle).map(|after| now + after),
        });
        Ok(name)
    }

    fn all(&self) -> impl Iterator<Item = &Named> {
        self.panes.values().flat_map(|p| &p.tabs)
    }

    /// The soonest a tab closes unused, for the one timer that closes them.
    pub fn due(&self) -> Option<Instant> {
        self.all().filter_map(|t| t.closes).min()
    }

    /// The tabs unused past their time at `now`.
    pub fn expired(&self, now: Instant) -> Vec<String> {
        self.all()
            .filter(|t| t.closes.is_some_and(|at| at <= now))
            .map(|t| t.route.id.clone())
            .collect()
    }

    /// Tab `id` closes unused that much later from `now`: the user is looking at it.
    pub fn postpone(&mut self, id: &str, now: Instant) {
        let mut tabs = self.panes.values_mut().flat_map(|p| &mut p.tabs);
        if let Some(t) = tabs.find(|t| t.route.id == id) {
            t.closes = unused_after(t.idle).map(|after| now + after);
        }
    }

    pub fn list(&mut self, pty: u32, open: impl Fn(&str) -> bool) -> Vec<Named> {
        self.pane(pty, open).tabs.clone()
    }

    /// A live pane's agent tab by its id: whether there's one, and what it was opened with.
    pub fn named(&self, id: &str) -> Option<&Named> {
        self.all().find(|t| t.route.id == id)
    }

    /// Agent tab `id` with the pane it's in.
    pub fn pane_of(&self, id: &str) -> Option<(u32, &Named)> {
        self.panes
            .iter()
            .find_map(|(pty, p)| Some((*pty, p.tabs.iter().find(|t| t.route.id == id)?)))
    }

    /// Tab `id` out of its pane.
    pub fn close(&mut self, id: &str) -> Option<Named> {
        self.panes.values_mut().find_map(|p| {
            let at = p.tabs.iter().position(|t| t.route.id == id)?;
            Some(p.tabs.remove(at))
        })
    }

    /// The pane closed: its tabs, the user's from now on.
    pub fn release(&mut self, pty: u32) -> Vec<Route> {
        let tabs = self.panes.remove(&pty).map(|p| p.tabs).unwrap_or_default();
        tabs.into_iter().map(|t| t.route).collect()
    }

    pub fn clear(&mut self) {
        self.panes.clear();
    }
}

/// How a command's tab came to be live: open already, just made, or loaded again from parking.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Found {
    Open,
    Made,
    Restored,
}

/// A tab as `tab list` tells it, and --json gives it.
#[derive(Serialize, Debug, PartialEq)]
pub struct Listed {
    pub name: String,
    pub default: bool,
    pub state: TabState,
    /// Seconds since a command last went to it.
    pub used_s: u64,
    /// Minutes it may go without a command before it sleeps; 0 never.
    pub idle_min: u64,
    pub shared: bool,
    pub origin: String,
    pub title: String,
}

#[derive(Serialize, Debug, PartialEq, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum TabState {
    /// Drawn and unthrottled for its agents.
    Awake,
    /// Slowed as any hidden tab, to park in time.
    Sleeping,
    /// Its view closed to save memory; its next command loads it again.
    Parked,
}

impl std::fmt::Display for TabState {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(match self {
            Self::Awake => "awake",
            Self::Sleeping => "sleeping",
            Self::Parked => "parked",
        })
    }
}

/// A device an agent's tab shows its page as: its viewport in CSS px, and what WebKit is told.
#[derive(Clone, Debug, PartialEq)]
pub struct AgentScreen {
    /// As the tab saves it (DeviceChoice): a name in devices.json, or Responsive with a size.
    pub name: String,
    pub w: f64,
    pub h: f64,
    pub dpr: Option<f64>,
    pub ua: Option<String>,
    /// Responsive's size, for the tab.
    pub size: Option<(u32, u32)>,
}

#[derive(Deserialize)]
struct Devices {
    devices: Vec<DeviceSpec>,
}

#[derive(Deserialize)]
struct DeviceSpec {
    name: String,
    w: f64,
    h: f64,
    dpr: f64,
    ua: String,
    safe: Insets,
}

#[derive(Deserialize)]
struct Insets {
    top: f64,
    bottom: f64,
}

/// The tab's device list (device mode), the one source for both.
fn devices() -> Vec<DeviceSpec> {
    serde_json::from_str::<Devices>(include_str!("../../../src/lib/browser/devices.json"))
        .map(|d| d.devices)
        .unwrap_or_default()
}

/// Responsive's sides, as devices.ts bounds them.
const SIDES: std::ops::RangeInclusive<u32> = 200..=3000;

/// `off`, a device by name (any case), or Responsive at `WxH`; upright, the page below its
/// status bar and above its home indicator, as device mode lays it out.
pub fn device(arg: &str) -> Result<Option<AgentScreen>, String> {
    let arg = arg.trim();
    if arg.eq_ignore_ascii_case("off") {
        return Ok(None);
    }
    if let Some((w, h)) = arg.split_once(['x', 'X', '×']) {
        if let (Ok(w), Ok(h)) = (w.trim().parse::<u32>(), h.trim().parse::<u32>()) {
            if !SIDES.contains(&w) || !SIDES.contains(&h) {
                return Err(format!(
                    "A size from {} to {} px a side",
                    SIDES.start(),
                    SIDES.end()
                ));
            }
            return Ok(Some(AgentScreen {
                name: "Responsive".into(),
                w: f64::from(w),
                h: f64::from(h),
                dpr: None,
                ua: None,
                size: Some((w, h)),
            }));
        }
    }
    let d = devices()
        .into_iter()
        .find(|d| d.name.eq_ignore_ascii_case(arg))
        .ok_or_else(|| format!("No device {arg:?}: `gitviber browser device --list` names them"))?;
    Ok(Some(AgentScreen {
        w: d.w,
        h: d.h - d.safe.top - d.safe.bottom,
        dpr: (d.dpr > 0.0).then_some(d.dpr),
        ua: (!d.ua.is_empty()).then_some(d.ua),
        name: d.name,
        size: None,
    }))
}

pub fn device_list() -> String {
    let mut out: Vec<String> = devices()
        .iter()
        .map(|d| format!("{} ({}×{})", d.name, d.w, d.h))
        .collect();
    out.push("WxH, as 1280x800: a page of that size".into());
    out.push("off: the page as itself".into());
    out.join("\n")
}

fn flat(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The console as an agent reads it: each line once, a little of its stack, and where each page
/// began.
pub fn console_text(entries: &[Entry], errors_only: bool) -> String {
    let mut out = vec![];
    for e in entries {
        match e.level.as_str() {
            "load" => out.push(format!("-- {}", flat(&e.url))),
            "error" => out.push(format!("[error] {}", flat(&e.msg))),
            "warn" if !errors_only => out.push(format!("[warn] {}", flat(&e.msg))),
            _ => continue,
        }
        let msg = flat(&e.msg);
        out.extend(
            e.stack
                .lines()
                .map(flat)
                .filter(|l| !l.is_empty() && !msg.contains(l.as_str()))
                .take(2)
                .map(|l| format!("    {l}")),
        );
    }
    // Pages with nothing logged on them say nothing.
    let mut kept: Vec<String> = vec![];
    for line in out {
        if line.starts_with("-- ") && kept.last().is_some_and(|l| l.starts_with("-- ")) {
            kept.pop();
        }
        kept.push(line);
    }
    if kept.last().is_some_and(|l| l.starts_with("-- ")) {
        kept.pop();
    }
    if kept.is_empty() {
        return if errors_only {
            "No errors."
        } else {
            "Nothing logged."
        }
        .into();
    }
    kept.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }

    #[test]
    fn requests_and_replies_read_as_json_lines() {
        let line = r#"{"token":"t0k","pty":3,"cwd":"/tmp/app","cmd":"click","args":["e2"]}"#;
        let req: Request = serde_json::from_str(line).unwrap();
        assert_eq!(req.pty, Some(3));
        assert_eq!(parse(&req.cmd, &req.args), Ok(Command::Click("e2".into())));
        // A pane started before the variable existed sends none.
        let bare: Request =
            serde_json::from_str(r#"{"token":"","pty":null,"cwd":"/","cmd":"url"}"#).unwrap();
        assert!(bare.args.is_empty() && bare.pty.is_none());
        assert_eq!(
            serde_json::to_string(&Reply::out("done")).unwrap(),
            r#"{"ok":true,"out":"done"}"#
        );
        let off = Reply {
            code: Some(OFF),
            ..Reply::error(OFF_TEXT)
        };
        assert_eq!(off.exit_code(), 2);
        assert_eq!(Reply::error("x").exit_code(), 1);
        assert_eq!(Reply::out("").exit_code(), 0);
    }

    #[test]
    fn each_pane_has_a_token_of_its_own_that_no_other_pane_can_pass_for() {
        let secret = "a1".repeat(32);
        let one = pane_token(&secret, 1);
        assert_eq!(one.len(), 64);
        assert_eq!(pane_token(&secret, 1), one);
        assert_ne!(pane_token(&secret, 2), one);
        assert_ne!(pane_token(&"b2".repeat(32), 1), one);
        assert!(
            !same_token(&secret, &one),
            "the launch's secret isn't a pane's token"
        );
        // RFC 4231's test cases 2 (a key shorter than a block) and 6 (longer, hashed first).
        assert_eq!(
            hmac(b"Jefe", b"what do ya want for nothing?"),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
        assert_eq!(
            hmac(
                &[0xaa; 131],
                b"Test Using Larger Than Block-Size Key - Hash Key First"
            ),
            "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54"
        );
        assert_eq!(pane_token("Jefe", 7), hmac(b"Jefe", b"7"));
    }

    #[test]
    fn only_the_exact_token_passes() {
        let ours = "a1".repeat(32);
        assert!(same_token(&ours, &ours));
        assert!(!same_token("", &ours));
        assert!(!same_token(&ours[..63], &ours));
        assert!(!same_token(&format!("{}b", &ours[..63]), &ours));
        assert!(!same_token(&format!("{ours}0"), &ours));
    }

    #[test]
    fn commands_and_their_arguments() {
        assert_eq!(
            parse("open", &args("localhost:3000")),
            Ok(Command::Open("http://localhost:3000/".into()))
        );
        assert_eq!(
            parse("snapshot", &args("-i -s main -d 3")),
            Ok(Command::Snapshot {
                interactive: true,
                scope: Some("main".into()),
                depth: Some(3)
            })
        );
        assert_eq!(
            parse("fill", &args("e3 a@b.test")),
            Ok(Command::Fill("e3".into(), "a@b.test".into()))
        );
        // Unquoted words are one text.
        assert_eq!(
            parse("type", &args("e3 hello there")),
            Ok(Command::Type("e3".into(), "hello there".into()))
        );
        assert_eq!(
            parse("select", &args("e4 red blue")),
            Ok(Command::Select(
                "e4".into(),
                vec!["red".into(), "blue".into()]
            ))
        );
        assert_eq!(
            parse("scroll", &args("down 400")),
            Ok(Command::Scroll(Scroll::Down, Some(400)))
        );
        assert_eq!(
            parse("scroll", &args("e9")),
            Ok(Command::Scroll(Scroll::To("e9".into()), None))
        );
        assert_eq!(
            parse("wait", &args("--text Saved --timeout 2000")),
            Ok(Command::Wait(
                Wait::Text("Saved".into()),
                Duration::from_secs(2)
            ))
        );
        assert_eq!(
            parse("wait", &args("div > .done")),
            Ok(Command::Wait(Wait::Css("div > .done".into()), WAIT))
        );
        assert_eq!(
            parse("wait", &args("--load")),
            Ok(Command::Wait(Wait::Load, WAIT))
        );
        assert_eq!(
            parse("wait", &args("--download --timeout 60000")),
            Ok(Command::Wait(Wait::Download, Duration::from_secs(60)))
        );
        assert_eq!(parse("downloads", &[]), Ok(Command::Downloads(false)));
        assert_eq!(
            parse("downloads", &args("--clear")),
            Ok(Command::Downloads(true))
        );
        assert_eq!(
            parse("eval", &args("document.title")),
            Ok(Command::Eval("document.title".into()))
        );
        assert_eq!(
            parse("screenshot", &args("shot.png --ref e2")),
            Ok(Command::Screenshot {
                path: Some("shot.png".into()),
                target: Some("e2".into())
            })
        );
        assert_eq!(
            parse("console", &args("--errors --clear")),
            Ok(Command::Console {
                errors: true,
                clear: true
            })
        );
        assert_eq!(
            parse("network", &args("4")),
            Ok(Command::Network(network::Ask::One(4)))
        );
        assert!(matches!(
            parse("network", &args("--failed --type fetch")),
            Ok(Command::Network(network::Ask::List { clear: false, .. }))
        ));
        assert_eq!(parse("device", &args("off")), Ok(Command::Device(None)));
        assert_eq!(parse("dialogs", &[]), Ok(Command::Dialogs(false)));
        assert_eq!(
            parse("dialogs", &args("accept")),
            Ok(Command::Dialogs(true))
        );
        assert_eq!(
            parse("appearance", &args("dark")),
            Ok(Command::Appearance(Some(true)))
        );
        assert_eq!(
            parse("appearance", &args("auto")),
            Ok(Command::Appearance(None))
        );
        assert!(parse("appearance", &args("sepia")).is_err() && parse("appearance", &[]).is_err());
        // The CLI answers these itself; the app knows no such commands.
        for (cmd, a) in [
            ("help", ""),
            ("device", "--list"),
            ("goto", "x.test"),
            ("key", "Enter"),
        ] {
            assert!(parse(cmd, &args(a)).is_err(), "{cmd}");
        }
    }

    #[test]
    fn a_wrong_command_says_how_to_say_it() {
        for (cmd, a) in [
            ("click", ""),
            ("click", "e1 e2"),
            ("fill", "e1"),
            ("back", "now"),
            ("snapshot", "-x"),
            ("snapshot", "-d many"),
            ("wait", ""),
            ("wait", "--text"),
            ("wait", "--load .x"),
            ("wait", "--download report.pdf"),
            ("downloads", "--all"),
            ("downloads", "--clear now"),
            ("scroll", "down far"),
            ("screenshot", "a.png b.png"),
            ("console", "--all"),
            ("network", "--type img"),
            ("device", ""),
            ("frobnicate", ""),
        ] {
            assert!(parse(cmd, &args(a)).is_err(), "{cmd} {a}");
        }
    }

    #[test]
    fn addresses_open_as_the_address_bar_would_and_only_web_ones() {
        assert_eq!(web_url("localhost:3000").unwrap(), "http://localhost:3000/");
        assert_eq!(
            web_url("127.0.0.1:8080/a?b").unwrap(),
            "http://127.0.0.1:8080/a?b"
        );
        assert_eq!(
            web_url("app.localhost/x").unwrap(),
            "http://app.localhost/x"
        );
        assert_eq!(web_url("example.test").unwrap(), "https://example.test/");
        assert_eq!(
            web_url(" https://example.test/a ").unwrap(),
            "https://example.test/a"
        );
        for bad in [
            "file:///etc/passwd",
            "javascript://x%0aalert(1)",
            "about:blank",
            "data:text/html,hi",
            "ftp://example.test/",
            "",
        ] {
            assert!(web_url(bad).is_err(), "{bad}");
        }
    }

    fn route(id: &str) -> Route {
        Route {
            id: id.into(),
            root: "/w".into(),
        }
    }

    fn new(name: Option<&str>, default: bool) -> NewTab<'_> {
        NewTab {
            name,
            default,
            shared: false,
            idle: IDLE,
        }
    }

    #[test]
    fn each_pane_keeps_its_own_default_tab_until_the_user_closes_it() {
        let mut routes = Routes::default();
        let open = |ids: &'static [&'static str]| move |id: &str| ids.contains(&id);
        let all = |_: &str| true;
        assert_eq!(routes.tab(1, None, open(&[])), Ok(None));
        routes.add(1, new(None, true), route("t1"), all).unwrap();
        routes.add(2, new(None, true), route("t2"), all).unwrap();
        assert_eq!(
            routes.tab(1, None, open(&["t1", "t2"])),
            Ok(Some(route("t1")))
        );
        assert_eq!(
            routes.tab(2, None, open(&["t1", "t2"])),
            Ok(Some(route("t2")))
        );
        // Closed: the next command makes another.
        assert_eq!(routes.tab(1, None, open(&["t2"])), Ok(None));
        routes.add(1, new(None, true), route("t3"), all).unwrap();
        assert_eq!(
            routes.tab(1, None, open(&["t2", "t3"])),
            Ok(Some(route("t3")))
        );
        assert_eq!(routes.tab(99, None, all), Ok(None));
        routes.clear();
        assert_eq!(routes.tab(2, None, open(&["t2"])), Ok(None));
    }

    #[test]
    fn tabs_go_by_name_and_names_stay_theirs() {
        let mut r = Routes::default();
        let all = |_: &str| true;
        let add = |r: &mut Routes, pty: u32, name: Option<&str>, id: &str, default: bool| {
            r.add(pty, new(name, default), route(id), all)
        };
        assert_eq!(add(&mut r, 1, None, "a", true), Ok("t1".into()));
        assert_eq!(
            add(&mut r, 1, Some("rv268"), "b", false),
            Ok("rv268".into())
        );
        assert_eq!(add(&mut r, 1, None, "c", false), Ok("t2".into()));
        // Another pane counts its own, and can't reach these.
        assert_eq!(add(&mut r, 2, None, "d", false), Ok("t1".into()));
        assert_eq!(
            r.tab(2, Some("rv268"), all).unwrap_err(),
            "No tab rv268 in this pane: `gitviber browser tab list` names them"
        );
        assert_eq!(r.tab(1, Some("rv268"), all), Ok(Some(route("b"))));
        // A name in use is refused, and adds nothing; one whose tab closed is free again.
        assert!(add(&mut r, 1, Some("rv268"), "x", false)
            .unwrap_err()
            .contains("open already"));
        assert!(r.named("x").is_none());
        assert_eq!(r.named("b").map(|t| t.name.as_str()), Some("rv268"));
        assert_eq!(r.close("b").unwrap().name, "rv268");
        assert!(r.named("b").is_none() && r.close("b").is_none());
        assert_eq!(
            add(&mut r, 1, Some("rv268"), "b2", false),
            Ok("rv268".into())
        );
        // Closing t1 leaves t2 as it was, and t1 isn't given again.
        assert_eq!(r.close("a").unwrap().name, "t1");
        assert_eq!(r.tab(1, Some("t2"), all), Ok(Some(route("c"))));
        assert_eq!(r.tab(1, None, all), Ok(None), "no default now");
        assert_eq!(add(&mut r, 1, None, "e", true), Ok("t3".into()));
        // A name the agent took that an auto name would have been is skipped.
        add(&mut r, 1, Some("t4"), "f", false).unwrap();
        assert_eq!(add(&mut r, 1, None, "g", false), Ok("t5".into()));
        let names: Vec<String> = r.list(1, all).into_iter().map(|t| t.name).collect();
        assert_eq!(names, ["t2", "rv268", "t3", "t4", "t5"]);
    }

    #[test]
    fn a_tab_the_user_closed_drops_out_and_a_closed_pane_lets_its_tabs_go() {
        let mut r = Routes::default();
        let all = |_: &str| true;
        r.add(1, new(None, true), route("a"), all).unwrap();
        r.add(1, new(None, false), route("b"), all).unwrap();
        r.add(2, new(None, true), route("c"), all).unwrap();
        let list = r.list(1, |id| id != "a");
        assert_eq!(list.len(), 1);
        assert_eq!((list[0].name.as_str(), list[0].default), ("t2", false));
        assert_eq!(r.tab(1, None, all), Ok(None));
        let pane_of = |r: &Routes, id| r.pane_of(id).map(|(pty, t)| (pty, t.name.clone()));
        assert_eq!(pane_of(&r, "b"), Some((1, "t2".into())));
        assert_eq!(pane_of(&r, "c"), Some((2, "t1".into())));
        assert_eq!(r.release(1), [route("b")]);
        assert_eq!(pane_of(&r, "b"), None);
        assert!(r.release(1).is_empty() && r.named("b").is_none());
        assert_eq!(r.list(1, all), []);
        assert_eq!(r.tab(2, None, all), Ok(Some(route("c"))));
    }

    #[test]
    fn using_a_tab_marks_when_and_an_unused_one_closes_in_its_time() {
        let mut r = Routes::default();
        r.add(1, new(None, true), route("a"), |_| true).unwrap();
        let before = r.list(1, |_| true)[0].used;
        std::thread::sleep(Duration::from_millis(2));
        r.tab(1, None, |_| true).unwrap();
        assert!(r.list(1, |_| true)[0].used > before);
        let min = |m: u64| Duration::from_secs(m * 60);
        assert_eq!(unused_after(IDLE), Some(min(60)));
        assert_eq!(unused_after(min(30)), Some(min(180)));
        assert_eq!(unused_after(Duration::ZERO), None, "one that never sleeps");
    }

    #[test]
    fn unused_tabs_come_due_soonest_first_and_one_on_show_waits() {
        let mut r = Routes::default();
        let min = |m: u64| Duration::from_secs(m * 60);
        let idle = |idle| NewTab {
            name: None,
            default: false,
            shared: false,
            idle,
        };
        let start = Instant::now();
        assert_eq!(r.due(), None);
        r.add(1, idle(IDLE), route("a"), |_| true).unwrap();
        r.add(1, idle(min(30)), route("b"), |_| true).unwrap();
        r.add(2, idle(Duration::ZERO), route("never"), |_| true)
            .unwrap();
        let due = r.due().unwrap();
        assert!(
            due >= start + min(60) && due <= Instant::now() + min(60),
            "a's hour"
        );
        assert!(r.expired(Instant::now()).is_empty());
        assert_eq!(r.expired(start + min(61)), ["a"]);
        let later = start + min(200);
        let mut gone = r.expired(later);
        gone.sort();
        assert_eq!(
            gone,
            ["a", "b"],
            "b's six times thirty minutes; never, never"
        );
        // On show when it came due: another hour from then.
        r.postpone("a", start + min(61));
        assert_eq!(r.expired(start + min(62)), Vec::<String>::new());
        assert!(r.due().unwrap() >= start + min(121));
        // A command restarts its time; a closed tab or pane takes its deadline with it.
        std::thread::sleep(Duration::from_millis(2));
        r.tab(1, Some("t2"), |_| true).unwrap();
        assert!(r.named("b").unwrap().closes.unwrap() > start + min(180));
        r.close("a");
        r.release(1);
        assert_eq!(r.due(), None);
        r.postpone("nope", later);
    }

    #[test]
    fn tab_commands_and_names() {
        let pane = |cmd: &str, s: &str| parse_pane(cmd, &args(s)).expect("a pane's command");
        let new = |name: Option<&str>, url: Option<&str>| {
            Ok(Pane::Tab(Tabs::New {
                name: name.map(Into::into),
                url: url.map(Into::into),
                shared: false,
                session: None,
                idle: None,
            }))
        };
        assert_eq!(parse_pane("url", &[]), None, "a page's");
        assert_eq!(pane("tab", "new"), new(None, None));
        assert_eq!(pane("tab", "new mert"), new(Some("mert"), None));
        assert_eq!(
            pane("tab", "new localhost:3000"),
            new(None, Some("http://localhost:3000/"))
        );
        assert_eq!(
            pane("tab", "new example.test"),
            new(None, Some("https://example.test/"))
        );
        assert_eq!(
            pane("tab", "new p_2 example.test/a"),
            new(Some("p_2"), Some("https://example.test/a"))
        );
        assert_eq!(pane("tab", "list"), Ok(Pane::Tab(Tabs::List)));
        assert_eq!(pane("tab", "close"), Ok(Pane::Tab(Tabs::Close(None))));
        assert_eq!(
            pane("tab", "close t2"),
            Ok(Pane::Tab(Tabs::Close(Some("t2".into()))))
        );
        for bad in [
            "",
            "2",
            "list x",
            "new a b c",
            "new a.b c",
            "close a/b",
            "new a file:///etc/passwd",
            "new a --session",
            "new a --session ../x",
            "new a --shared --session b",
            "new a --idle",
            "new a --idle 121",
            "new a --idle -1",
            "list --shared",
            "close --session a",
            "list --idle 5",
        ] {
            assert!(pane("tab", bad).is_err(), "{bad:?}");
        }
        for bad in ["", "-x", "a b", "a.b", "é", &"x".repeat(33)] {
            assert!(tab_name(bad).is_err(), "{bad:?}");
        }
        assert_eq!(
            pane("tab", "new --shared mert localhost:3000 --idle 0"),
            Ok(Pane::Tab(Tabs::New {
                name: Some("mert".into()),
                url: Some("http://localhost:3000/".into()),
                shared: true,
                session: None,
                idle: Some(0),
            }))
        );
        assert_eq!(
            pane("tab", "new p2 --session ayse --idle 120"),
            Ok(Pane::Tab(Tabs::New {
                name: Some("p2".into()),
                url: None,
                shared: false,
                session: Some("ayse".into()),
                idle: Some(120),
            }))
        );
        assert_eq!(
            pane("session", "save mert"),
            Ok(Pane::Session(Sessions::Save("mert".into(), false)))
        );
        assert_eq!(
            pane("session", "save mert --force"),
            Ok(Pane::Session(Sessions::Save("mert".into(), true)))
        );
        assert_eq!(
            pane("session", "load mert"),
            Ok(Pane::Session(Sessions::Load("mert".into())))
        );
        assert_eq!(pane("session", "clear"), Ok(Pane::Session(Sessions::Clear)));
        assert_eq!(pane("session", "list"), Ok(Pane::Session(Sessions::List)));
        assert_eq!(
            pane("session", "delete mert"),
            Ok(Pane::Session(Sessions::Delete("mert".into())))
        );
        // A name is a file's in the sessions' folder, and never a way out of it.
        for bad in [
            "",
            "save",
            "save ../x",
            "save .x",
            "load a/b",
            "clear x",
            "delete",
            "save a b",
        ] {
            assert!(pane("session", bad).is_err(), "{bad:?}");
        }
        assert_eq!(tab_name("Mert-2_b"), Ok("Mert-2_b".into()));
        assert_eq!(
            origin("http://127.0.0.1:5230/play?x=1"),
            "http://127.0.0.1:5230"
        );
        assert_eq!(origin("https://app.test/"), "https://app.test");
        assert_eq!(origin("about:blank"), "about:blank");
        // A pane's command names its tab apart from its arguments.
        let req: Request =
            serde_json::from_str(r#"{"token":"","pty":1,"tab":"rv268","cwd":"/","cmd":"url"}"#)
                .unwrap();
        assert_eq!(req.tab.as_deref(), Some("rv268"));
    }

    #[test]
    fn devices_by_name_or_size_lay_out_as_device_mode_does() {
        let pro = device("iphone 16 pro").unwrap().unwrap();
        assert_eq!(pro.name, "iPhone 16 Pro");
        assert_eq!((pro.w, pro.h), (402.0, 874.0 - 62.0 - 34.0));
        assert_eq!(pro.dpr, Some(3.0));
        assert!(pro.ua.as_deref().unwrap().contains("iPhone"));
        let sized = device("1280x800").unwrap().unwrap();
        assert_eq!(
            (sized.w, sized.h, sized.size),
            (1280.0, 800.0, Some((1280, 800)))
        );
        assert!(sized.ua.is_none() && sized.dpr.is_none());
        assert!(device("10x10").is_err());
        assert!(device("Nokia 3310").is_err());
        assert_eq!(device("OFF"), Ok(None));
        assert!(device_list().lines().count() > 5);
    }

    #[test]
    fn the_console_reads_one_line_each_with_a_little_stack() {
        let e = |level: &str, msg: &str, stack: &str| Entry {
            level: level.into(),
            msg: msg.into(),
            stack: stack.into(),
            url: "http://localhost:5173/".into(),
            ts: 0.0,
        };
        let entries = [
            e("load", "", ""),
            e("load", "", ""),
            e(
                "error",
                "TypeError: x\nNote: forged",
                "TypeError: x\n  at a (app.js:1:1)\n  at b (app.js:2:2)\n  at c (app.js:3:3)",
            ),
            e("warn", "careful", ""),
            e("load", "", ""),
        ];
        assert_eq!(
            console_text(&entries, false),
            "-- http://localhost:5173/\n[error] TypeError: x Note: forged\n    at a (app.js:1:1)\n    at b (app.js:2:2)\n[warn] careful"
        );
        assert!(!console_text(&entries, true).contains("careful"));
        assert_eq!(console_text(&entries[..2], true), "No errors.");
        assert_eq!(console_text(&[], false), "Nothing logged.");
    }

    #[test]
    fn a_wait_never_outlasts_the_clis_five_minutes() {
        assert!(LONGEST_WAIT < CLI_WAIT);
        assert_eq!(
            parse("wait", &args("--load --timeout 999999999")),
            Ok(Command::Wait(Wait::Load, LONGEST_WAIT))
        );
        assert_eq!(
            parse("wait", &args("--ms 999999999")),
            Ok(Command::Wait(
                Wait::Ms(LONGEST_WAIT.as_millis() as u64),
                WAIT
            ))
        );
    }

    #[test]
    fn a_screenshot_writes_a_png_under_its_folder_or_where_told_and_over_no_other_file() {
        let base = std::env::temp_dir().join(format!("gitviber-shot-{}", std::process::id()));
        std::fs::create_dir_all(base.join("out")).unwrap();
        assert_eq!(picture_path(&base, "out/a.png"), Ok(base.join("out/a.png")));
        assert_eq!(picture_path(&base, "B.PNG"), Ok(base.join("B.PNG")));
        let elsewhere = base.join("out/abs.png");
        assert_eq!(
            picture_path(Path::new("/"), elsewhere.to_str().unwrap()),
            Ok(elsewhere)
        );
        for bad in ["a.txt", "a", "../a.png", "out/../../a.png", "out"] {
            assert!(picture_path(&base, bad).is_err(), "{bad}");
        }
        // An old picture is replaced; anything else there is kept.
        std::fs::write(base.join("old.png"), b"\x89PNG\r\n\x1a\nrest").unwrap();
        assert!(picture_path(&base, "old.png").is_ok());
        std::fs::write(base.join("notes.png"), b"my notes").unwrap();
        assert!(picture_path(&base, "notes.png").is_err());
        std::fs::create_dir_all(base.join("dir.png")).unwrap();
        assert!(picture_path(&base, "dir.png").is_err());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(base.join("old.png"), base.join("link.png")).unwrap();
            assert!(picture_path(&base, "link.png").is_err());
        }
        let _ = std::fs::remove_dir_all(&base);
    }

    fn words(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn arguments_keep_their_spaces_quotes_and_unicode_as_the_shell_split_them() {
        assert_eq!(
            parse(
                "fill",
                &words(&["#q", "  two  spaces ", "\"quoted\"", "çğış 😀"])
            ),
            Ok(Command::Fill(
                "#q".into(),
                "  two  spaces  \"quoted\" çğış 😀".into()
            ))
        );
        assert_eq!(
            parse("eval", &words(&["document.title", "+", "'--json'"])),
            Ok(Command::Eval("document.title + '--json'".into()))
        );
        assert_eq!(
            parse("select", &words(&["e4", "Red", "Dark Blue"])),
            Ok(Command::Select("e4".into(), words(&["Red", "Dark Blue"])))
        );
        assert_eq!(
            parse("press", &words(&["Shift+Tab"])),
            Ok(Command::Press("Shift+Tab".into()))
        );
        // A css target with spaces is one argument, as quoted.
        assert_eq!(
            parse("click", &words(&["form > button.primary"])),
            Ok(Command::Click("form > button.primary".into()))
        );
        for (cmd, args) in [
            ("click", vec![]),
            ("click", words(&[""])),
            ("click", words(&["a", "b"])),
            ("fill", words(&["#q"])),
            ("select", words(&["e1"])),
            ("url", words(&["extra"])),
            ("snapshot", words(&["-d"])),
            ("snapshot", words(&["-d", "-1"])),
            ("snapshot", words(&["-x"])),
            ("scroll", words(&["down", "ten"])),
            ("scroll", words(&["down", "10", "20"])),
            ("wait", vec![]),
            ("wait", words(&["--text"])),
            ("wait", words(&["--ms", "1.5"])),
            ("wait", words(&["#a", "--text", "b"])),
            ("screenshot", words(&["a.png", "b.png"])),
            ("screenshot", words(&["--ref"])),
            ("console", words(&["--all"])),
            ("device", vec![]),
            ("", vec![]),
            ("CLICK", words(&["e1"])),
        ] {
            assert!(parse(cmd, &args).is_err(), "{cmd} {args:?}");
        }
        assert_eq!(
            parse("wait", &words(&["#list", "li", "--timeout", "500"])),
            Ok(Command::Wait(
                Wait::Css("#list li".into()),
                Duration::from_millis(500)
            ))
        );
        assert_eq!(
            parse("screenshot", &words(&["--ref", "e2", "out dir/shot.png"])),
            Ok(Command::Screenshot {
                path: Some("out dir/shot.png".into()),
                target: Some("e2".into())
            })
        );
    }

    #[test]
    fn an_agents_address_opens_only_as_a_web_page() {
        for (typed, url) in [
            ("[::1]:3000/a", "http://[::1]:3000/a"),
            ("10.0.0.5:8080", "http://10.0.0.5:8080/"),
            ("HTTP://LOCALHOST:3000", "http://localhost:3000/"),
            ("bücher.example/ä", "https://xn--bcher-kva.example/%C3%A4"),
            ("example.test:8443", "https://example.test:8443/"),
        ] {
            assert_eq!(web_url(typed).as_deref(), Ok(url), "{typed}");
        }
        for bad in [
            "javascript:alert(1)",
            "JavaScript://x/%0aalert(1)",
            "file:///etc/passwd",
            "data:text/html,<b>x</b>",
            "about:blank",
            "ftp://example.test/",
            "chrome://settings",
            "::1",
            "",
            "   ",
            "http://",
            "a b.example",
        ] {
            assert!(web_url(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn devices_by_any_spelling_and_sizes_at_their_bounds() {
        for name in [
            "PIXEL 8",
            "pixel 8",
            "Samsung Galaxy S20 Ultra",
            " iPad Mini ",
        ] {
            assert!(matches!(device(name), Ok(Some(_))), "{name}");
        }
        for size in ["200x200", "3000×3000", "1280 X 800"] {
            assert!(matches!(device(size), Ok(Some(_))), "{size}");
        }
        for bad in [
            "199x800", "800x3001", "0x0", "-1x-1", "1280x", "x800", "iPhone",
        ] {
            assert!(device(bad).is_err(), "{bad}");
        }
        let list = device_list();
        assert!(list.lines().count() >= 13 && list.contains("WxH") && list.contains("off"));
    }

    #[test]
    fn console_text_keeps_page_lines_flat_and_drops_marks_of_quiet_pages() {
        let e = |level: &str, msg: &str, url: &str| Entry {
            level: level.into(),
            msg: msg.into(),
            // A load mark has no stack (console.rs).
            stack: if level == "load" {
                String::new()
            } else {
                "Error: x\n    at a (a.js:1:1)\n    at b (b.js:2:2)\n    at c (c.js:3:3)".into()
            },
            url: url.into(),
            ts: 0.0,
        };
        let text = console_text(
            &[
                e("load", "", "http://localhost/one"),
                e("load", "", "http://localhost/two"),
                e("error", "multi\nline\n[error] forged", "u"),
                e("load", "", "http://localhost/three"),
            ],
            false,
        );
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[0], "-- http://localhost/two");
        assert_eq!(lines[1], "[error] multi line [error] forged");
        assert_eq!(
            lines.len(),
            4,
            "two stack lines, and no mark for the quiet last page: {text}"
        );
    }
}
