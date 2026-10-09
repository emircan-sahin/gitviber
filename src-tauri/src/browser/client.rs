//! `gitviber browser …`: resources/gitviber runs this binary with `--browser`, which sends one
//! request to the running app (server.rs), prints the reply and exits with its status. Never
//! starts the app itself.

use super::control::{self, Reply, Request, HELP, OFF, OFF_TEXT};
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
    /// Nothing listening: control is off, or GitViber isn't running.
    Off,
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
    let (Ok(socket), Ok(token)) = (
        std::env::var(control::SOCKET_ENV),
        std::env::var(control::TOKEN_ENV),
    ) else {
        eprintln!("gitviber browser works in GitViber's own terminals.");
        return OFF;
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
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;
    let mut stream = UnixStream::connect(socket).map_err(|_| Failure::Off)?;
    // A wait or a slow page takes a while; a stuck app doesn't keep the agent forever.
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(300)));
    let mut line = serde_json::to_string(request).map_err(|_| Failure::NoAnswer)?;
    line.push('\n');
    stream
        .write_all(line.as_bytes())
        .map_err(|_| Failure::NoAnswer)?;
    let mut reply = String::new();
    BufReader::new(&stream)
        .read_line(&mut reply)
        .map_err(|_| Failure::NoAnswer)?;
    serde_json::from_str(&reply).map_err(|_| Failure::NoAnswer)
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
}
