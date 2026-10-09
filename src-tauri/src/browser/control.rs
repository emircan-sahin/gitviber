//! `gitviber browser …` from an agent in a GitViber terminal: the request it sends over the
//! socket (server.rs), what it may ask, and the tab each terminal pane's agent gets. The page's
//! part is scripts/agent.js; apart from AppKit, so it can be tested.

use super::console::Entry;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri::Url;

/// What a terminal's shell gets (pty.rs), and the CLI reads back.
pub const EXE_ENV: &str = "GITVIBER_EXE";
pub const SOCKET_ENV: &str = "GITVIBER_BROWSER_SOCKET";
pub const TOKEN_ENV: &str = "GITVIBER_BROWSER_TOKEN";
pub const PTY_ENV: &str = "GITVIBER_PTY";

/// The CLI's exit status when control is off or GitViber isn't there to answer.
pub const OFF: i32 = 2;
pub const OFF_TEXT: &str = "Browser control is off (Settings → Browser).";

/// How long `wait` waits unless told.
pub const WAIT: Duration = Duration::from_secs(10);

pub const HELP: &str = "\
gitviber browser <command> [args]: this terminal pane's own browser tab in GitViber.
The pane's first command opens it, in the background; every agent in the pane shares it.

  open <url>                   Load a page (http or https; localhost:3000 works) and wait for it
  back | forward | reload      Through its history, waiting for the page
  url                          The page's address and title
  snapshot [-i] [-s <css>] [-d <n>]
                               The page as a tree, with refs (e1, e2, ...) to act on: -i only
                               what can be clicked or typed in, -s under one element, -d so many
                               levels deep. Refs last until the page changes.
  click <ref|css>              Click an element
  fill <ref|css> <text>        Replace a field's text
  type <ref|css> <text>        Type into a field, key by key
  select <ref|css> <value>...  Choose a <select>'s options, by value or label
  hover <ref|css>              The mouse over an element (no CSS :hover)
  press <key>                  A key to the focused element: Enter, Tab, Escape, ArrowDown, Meta+a
  scroll <up|down|ref|css> [px]
  wait <css> | --text <text> | --load | --ms <n> [--timeout <ms>]
                               Until an element shows, the text does, the page has loaded, or
                               the time is up; 10 s at most unless --timeout says
  eval <js>                    Runs in the page; prints the result as JSON
  screenshot [path] [--ref <ref|css>]
                               A PNG of the page, or of one element; prints its path
  console [--errors] [--clear] What the page logged
  device <name|WxH|off>        Show the page as a device; `device --list` names them
  help

  --json                       The reply as JSON

Exit status: 0 done; 1 failed, why on stderr; 2 browser control is off (Settings → Browser).
";

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct Request {
    pub token: String,
    /// The pane it came from (PTY_ENV); its tab is this pane's.
    pub pty: Option<u32>,
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
    Device(Option<AgentScreen>),
    Devices,
    Help,
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
        "open" | "goto" | "navigate" => target("open <url>").map(Command::Open),
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
        "press" | "key" => target("press <key>").map(Command::Press),
        "scroll" => scroll(args),
        "wait" => wait(args),
        "eval" if !args.is_empty() => Ok(Command::Eval(rest())),
        "eval" => Err("Usage: gitviber browser eval <js>".into()),
        "screenshot" => screenshot(args),
        "console" => console(args),
        "device" => match args {
            [list] if list == "--list" => Ok(Command::Devices),
            [] => Err("Usage: gitviber browser device <name|WxH|off>, or device --list".into()),
            _ => device(&rest()).map(Command::Device),
        },
        "help" | "--help" | "-h" => Ok(Command::Help),
        _ => Err(format!(
            "No command `{cmd}`: `gitviber browser help` lists them"
        )),
    }
}

/// The value after a flag.
fn value<'a>(it: &mut impl Iterator<Item = &'a String>, flag: &str) -> Result<&'a String, String> {
    it.next().ok_or_else(|| format!("{flag} needs a value"))
}

fn number<T: std::str::FromStr>(s: &str, flag: &str) -> Result<T, String> {
    s.parse()
        .map_err(|_| format!("{flag} takes a whole number, not {s:?}"))
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
            "--ms" => what = Some(Wait::Ms(number(value(&mut it, arg)?, arg)?)),
            "--timeout" => timeout = Duration::from_millis(number(value(&mut it, arg)?, arg)?),
            _ if arg.starts_with("--") => return Err(format!("wait doesn't take {arg}")),
            _ => css.push(arg.as_str()),
        }
    }
    let what = match (what, css.is_empty()) {
        (Some(w), true) => w,
        (None, false) => Wait::Css(css.join(" ")),
        _ => {
            return Err(
                "Usage: gitviber browser wait <css> | --text <text> | --load | --ms <n>".into(),
            )
        }
    };
    Ok(Command::Wait(what, timeout))
}

fn screenshot(args: &[String]) -> Result<Command, String> {
    let (mut path, mut target) = (None, None);
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--ref" | "--selector" => target = Some(value(&mut it, arg)?.clone()),
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

/// The worktree a folder is in: the nearest one up with a `.git` (a linked worktree's is a
/// file), else the folder itself.
pub fn worktree_root(cwd: &Path) -> PathBuf {
    cwd.ancestors()
        .find(|dir| dir.join(".git").exists())
        .unwrap_or(cwd)
        .to_path_buf()
}

/// The tab of each pane's agent: made on its first command, and again once the user closed it.
#[derive(Default)]
pub struct Routes {
    tabs: HashMap<u32, Route>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Route {
    pub id: String,
    pub root: String,
}

impl Routes {
    /// The pane's tab while it's still open (`open`); None when one is to be made.
    pub fn tab(&mut self, pty: u32, open: impl Fn(&str) -> bool) -> Option<Route> {
        match self.tabs.get(&pty) {
            Some(route) if open(&route.id) => Some(route.clone()),
            Some(_) => {
                self.tabs.remove(&pty);
                None
            }
            None => None,
        }
    }

    pub fn set(&mut self, pty: u32, route: Route) {
        self.tabs.insert(pty, route);
    }

    pub fn clear(&mut self) {
        self.tabs.clear();
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

/// Page text on one line.
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
            Ok(Command::Open("localhost:3000".into()))
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
        assert_eq!(parse("device", &args("--list")), Ok(Command::Devices));
        assert_eq!(parse("device", &args("off")), Ok(Command::Device(None)));
        assert_eq!(parse("help", &[]), Ok(Command::Help));
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
            ("scroll", "down far"),
            ("screenshot", "a.png b.png"),
            ("console", "--all"),
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

    #[test]
    fn each_pane_keeps_its_own_tab_until_the_user_closes_it() {
        let mut routes = Routes::default();
        let open = |ids: &'static [&'static str]| move |id: &str| ids.contains(&id);
        assert_eq!(routes.tab(1, open(&[])), None);
        routes.set(
            1,
            Route {
                id: "t1".into(),
                root: "/w".into(),
            },
        );
        routes.set(
            2,
            Route {
                id: "t2".into(),
                root: "/w".into(),
            },
        );
        assert_eq!(routes.tab(1, open(&["t1", "t2"])).unwrap().id, "t1");
        assert_eq!(routes.tab(2, open(&["t1", "t2"])).unwrap().id, "t2");
        // Closed: the next command makes another.
        assert_eq!(routes.tab(1, open(&["t2"])), None);
        routes.set(
            1,
            Route {
                id: "t3".into(),
                root: "/w".into(),
            },
        );
        assert_eq!(routes.tab(1, open(&["t2", "t3"])).unwrap().id, "t3");
        routes.clear();
        assert_eq!(routes.tab(2, open(&["t2"])), None);
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
    fn a_folder_belongs_to_the_nearest_worktree_up() {
        let base = std::env::temp_dir().join(format!("gitviber-control-{}", std::process::id()));
        let inner = base.join("repo/.claude/worktrees/w1");
        std::fs::create_dir_all(base.join("repo/.git")).unwrap();
        std::fs::create_dir_all(inner.join("src")).unwrap();
        std::fs::write(inner.join(".git"), "gitdir: ../../../.git/worktrees/w1").unwrap();
        assert_eq!(worktree_root(&inner.join("src")), inner);
        assert_eq!(worktree_root(&base.join("repo/.claude")), base.join("repo"));
        assert_eq!(worktree_root(&base), base);
        let _ = std::fs::remove_dir_all(&base);
    }
}
