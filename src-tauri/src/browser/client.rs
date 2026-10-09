//! `gitviber browser …`: resources/gitviber runs this binary with `--browser`, which sends one
//! request to the running app (server.rs), prints the reply and exits with its status. Never
//! starts the app itself.

use super::control::{self, Reply, Request, CLI_WAIT, HELP, NOT_HERE, OFF, OFF_TEXT};
use std::path::Path;

/// When the binary was run as the CLI: its exit status. None for a normal launch.
pub fn helper() -> Option<i32> {
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() != Some("--browser") {
        return None;
    }
    Some(run(args.collect()))
}

#[derive(Debug, PartialEq)]
pub struct Cli {
    pub json: bool,
    pub cmd: Option<String>,
    pub args: Vec<String>,
}

/// `--json` anywhere before a `--`; after one, everything is an argument, even `--json`.
pub fn parse_cli(args: Vec<String>) -> Cli {
    let mut json = false;
    let mut rest = vec![];
    let mut it = args.into_iter();
    for arg in it.by_ref() {
        match arg.as_str() {
            "--json" => json = true,
            "--" => break,
            _ => rest.push(arg),
        }
    }
    rest.extend(it);
    let mut rest = rest.into_iter();
    Cli {
        json,
        cmd: rest.next(),
        args: rest.collect(),
    }
}

enum Failure {
    /// No socket: control is off (the app takes it away then).
    Off,
    /// A socket nobody listens on: GitViber quit, or crashed.
    NotRunning,
    /// Connected, but no reply came that reads as one: GitViber went away mid-command.
    NoAnswer,
}

fn run(args: Vec<String>) -> i32 {
    let cli = parse_cli(args);
    let Some(cmd) = cli
        .cmd
        .filter(|c| !matches!(c.as_str(), "help" | "--help" | "-h"))
    else {
        print!("{HELP}");
        return 0;
    };
    // Answered here: the list is this binary's own, and needs no tab.
    if cmd == "device" && cli.args == ["--list"] {
        return print(&Reply::out(control::device_list()), cli.json);
    }
    let (Ok(socket), Ok(token)) = (
        std::env::var(control::SOCKET_ENV),
        std::env::var(control::TOKEN_ENV),
    ) else {
        eprintln!("gitviber browser works in GitViber's own terminals.");
        return NOT_HERE;
    };
    let request = Request {
        token,
        pty: std::env::var(control::PTY_ENV)
            .ok()
            .and_then(|p| p.parse().ok()),
        cwd: std::env::current_dir()
            .map(|d| d.to_string_lossy().into_owned())
            .unwrap_or_default(),
        cmd,
        args: cli.args,
    };
    match ask(Path::new(&socket), &request) {
        Ok(reply) => print(&reply, cli.json),
        Err(Failure::Off) => {
            eprintln!("{OFF_TEXT}");
            OFF
        }
        Err(Failure::NotRunning) => {
            eprintln!("GitViber isn't running.");
            NOT_HERE
        }
        Err(Failure::NoAnswer) => {
            eprintln!("GitViber didn't answer.");
            1
        }
    }
}

fn print(reply: &Reply, json: bool) -> i32 {
    if json {
        println!("{}", serde_json::to_string(reply).unwrap_or_default());
    } else if let Some(error) = &reply.error {
        eprintln!("{error}");
    } else if let Some(out) = reply.out.as_deref().filter(|o| !o.is_empty()) {
        println!("{out}");
    }
    reply.exit_code()
}

#[cfg(unix)]
fn ask(socket: &Path, request: &Request) -> Result<Reply, Failure> {
    use crate::local_socket::{ask_line, AskError};
    // A wait or a slow page takes a while; a stuck app doesn't keep the agent forever.
    ask_line(socket, request, Some(CLI_WAIT)).map_err(|e| match e {
        AskError::Connect(std::io::ErrorKind::NotFound) => Failure::Off,
        AskError::Connect(_) => Failure::NotRunning,
        AskError::NoAnswer => Failure::NoAnswer,
    })
}

#[cfg(not(unix))]
fn ask(_: &Path, _: &Request) -> Result<Reply, Failure> {
    Err(Failure::Off)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cli(s: &str) -> Cli {
        parse_cli(s.split_whitespace().map(String::from).collect())
    }

    #[test]
    fn json_anywhere_before_a_double_dash() {
        assert_eq!(
            cli("--json snapshot -i"),
            Cli {
                json: true,
                cmd: Some("snapshot".into()),
                args: vec!["-i".into()]
            }
        );
        assert!(cli("url --json").json);
        let text = cli("fill e3 -- --json --");
        assert!(!text.json);
        assert_eq!(text.cmd.as_deref(), Some("fill"));
        assert_eq!(text.args, ["e3", "--json", "--"]);
        assert_eq!(cli("").cmd, None);
    }

    #[test]
    fn replies_print_where_they_belong_with_their_status() {
        assert_eq!(print(&Reply::out("done"), false), 0);
        assert_eq!(print(&Reply::error("no"), false), 1);
        let off = Reply {
            code: Some(OFF),
            ..Reply::error(OFF_TEXT)
        };
        assert_eq!(print(&off, true), 2);
    }

    #[cfg(unix)]
    #[test]
    fn one_request_line_out_one_reply_line_back_and_no_socket_is_off() {
        use std::io::{BufRead, BufReader, Write};
        let dir = std::env::temp_dir().join(format!("gitviber-cli-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let socket = dir.join("b.sock");
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(&stream).read_line(&mut line).unwrap();
            (&stream)
                .write_all(b"{\"ok\":true,\"out\":\"http://localhost:5173/\"}\n")
                .unwrap();
            serde_json::from_str::<Request>(&line).unwrap()
        });
        let request = Request {
            token: "t".into(),
            pty: Some(4),
            cwd: "/tmp/app".into(),
            cmd: "url".into(),
            args: vec![],
        };
        let reply = ask(&socket, &request).ok().unwrap();
        assert_eq!(reply, Reply::out("http://localhost:5173/"));
        assert_eq!(server.join().unwrap(), request);
        let _ = std::fs::remove_dir_all(&dir);
        assert!(matches!(ask(&socket, &request), Err(Failure::Off)));
    }

    #[test]
    fn help_needs_no_app_and_outside_its_terminals_the_cli_says_so() {
        let words = |s: &[&str]| s.iter().map(|w| (*w).to_string()).collect::<Vec<_>>();
        for help in [
            vec![],
            words(&["help"]),
            words(&["--help"]),
            words(&["-h"]),
            words(&["--json"]),
        ] {
            assert_eq!(run(help.clone()), 0, "{help:?}");
        }
        // Only where no GitViber terminal set these up (as here, not inside one): never a request.
        if std::env::var_os(control::SOCKET_ENV).is_none() {
            assert_eq!(run(words(&["url"])), NOT_HERE);
        }
        // A double dash before the command still finds it; one after it keeps the rest as text.
        let cli = parse_cli(words(&["--", "fill", "#q", "--json"]));
        assert_eq!((cli.json, cli.cmd.as_deref()), (false, Some("fill")));
        assert_eq!(cli.args, ["#q", "--json"]);
        let unicode = parse_cli(words(&["type", "e1", "çğış 😀", "--json"]));
        assert!(unicode.json && unicode.args == ["e1", "çğış 😀"]);
    }

    #[cfg(unix)]
    #[test]
    fn a_reply_that_isnt_one_or_a_server_that_hangs_up_is_no_answer() {
        use std::io::{BufRead, BufReader, Write};
        let dir = std::env::temp_dir().join(format!("gitviber-cli-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let socket = dir.join("b.sock");
        let _ = std::fs::remove_file(&socket);
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        let server = std::thread::spawn(move || {
            for reply in [&b"not json\n"[..], b""] {
                let (stream, _) = listener.accept().unwrap();
                let mut line = String::new();
                BufReader::new(&stream).read_line(&mut line).unwrap();
                (&stream).write_all(reply).unwrap();
            }
        });
        let request = Request {
            token: "t".into(),
            pty: None,
            cwd: String::new(),
            cmd: "url".into(),
            args: vec![],
        };
        assert!(matches!(ask(&socket, &request), Err(Failure::NoAnswer)));
        assert!(matches!(ask(&socket, &request), Err(Failure::NoAnswer)));
        server.join().unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
