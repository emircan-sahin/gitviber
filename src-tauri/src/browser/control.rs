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
gitviber browser <command> [args]: this terminal pane's own browser tab in GitViber.
The pane's first command opens it, in the background; every agent in the pane shares it.

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
  wait <css> | --text <text> | --load | --ms <n> [--timeout <ms>]
                               Until an element shows, the text does, the page has loaded, or
                               the time is up; 10 s at most unless --timeout says (120 s at most)
  eval <js>                    Runs in the page; prints the result as JSON
  screenshot [path] [--ref <ref|css>]
                               A PNG of the page, or of one element; prints its path
  console [--errors] [--clear] What the page logged
  device <name|WxH|off>        Show the page as a device; `device --list` names them
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
    /// Whether the page's dialogs are accepted (OK) rather than dismissed.
    Dialogs(bool),
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
        "device" if args.is_empty() => {
            Err("Usage: gitviber browser device <name|WxH|off>, or device --list".into())
        }
        "device" => device(&rest()).map(Command::Device),
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
        _ => {
            return Err(
                "Usage: gitviber browser wait <css> | --text <text> | --load | --ms <n>".into(),
            )
        }
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
        assert_eq!(parse("device", &args("off")), Ok(Command::Device(None)));
        assert_eq!(parse("dialogs", &[]), Ok(Command::Dialogs(false)));
        assert_eq!(
            parse("dialogs", &args("accept")),
            Ok(Command::Dialogs(true))
        );
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
    fn a_closed_tab_routes_anew_and_other_panes_keep_theirs() {
        let mut routes = Routes::default();
        let route = |id: &str| Route {
            id: id.into(),
            root: "/w".into(),
        };
        routes.set(1, route("a"));
        routes.set(2, route("b"));
        assert_eq!(routes.tab(1, |_| true), Some(route("a")));
        assert_eq!(routes.tab(1, |id| id != "a"), None, "closed");
        assert_eq!(routes.tab(1, |_| true), None, "and forgotten");
        assert_eq!(routes.tab(2, |_| true), Some(route("b")));
        assert_eq!(routes.tab(99, |_| true), None);
        routes.clear();
        assert_eq!(routes.tab(2, |_| true), None);
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
