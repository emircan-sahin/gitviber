//! Answers what git and ssh ask during a network command (a password, an SSH key's passphrase,
//! a new host's key) from a dialog on the page. They run this app's own binary as GIT_ASKPASS /
//! SSH_ASKPASS; started that way it only relays the prompt to the running app over a private
//! Unix socket and prints the answer. Nothing typed is stored or logged: an answer goes to git
//! once, and the user's credential helper decides what to keep, as in a terminal.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

const SOCKET_ENV: &str = "GITVIBER_ASKPASS_SOCKET";
const TOKEN_ENV: &str = "GITVIBER_ASKPASS_TOKEN";
/// A forgotten dialog mustn't hold its command forever; long enough to go and create a token.
const PROMPT_TIMEOUT: Duration = Duration::from_secs(300);

/// What the page shows; it answers with `answer(id, …)`.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Prompt {
    pub id: u64,
    /// git's or ssh's own words: "Password for 'https://me@github.com': ", "Enter passphrase
    /// for key '/Users/me/.ssh/id_ed25519': ", "Are you sure you want to continue connecting
    /// (yes/no/[fingerprint])?" after the host's fingerprint.
    pub text: String,
    /// ssh's SSH_ASKPASS_PROMPT: "confirm" (a yes/no) or "none" (a notice ssh closes itself).
    pub kind: Option<String>,
    /// The command asking: "git push", "git clone", …
    pub label: String,
    /// The page's id for that command, if it gave one.
    pub op: Option<String>,
    /// The hosts that command talks to, so a prompt naming another one stands out. Empty when
    /// unknown (a submodule update).
    pub hosts: Vec<String>,
}

pub enum Event {
    Ask(Prompt),
    /// Answered, timed out, or the command stopped waiting: the dialog goes.
    Done(u64),
}

#[derive(Serialize, Deserialize)]
struct Request {
    token: String,
    prompt: String,
    kind: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct Reply {
    answer: Option<String>,
}

/// A command allowed to ask, by the token it was given.
#[derive(Clone)]
struct Session {
    label: String,
    op: Option<String>,
    repo: Option<PathBuf>,
    args: Vec<String>,
    /// Its prompts on screen right now.
    open: Arc<AtomicUsize>,
}

struct Server {
    socket: PathBuf,
    helper: PathBuf,
    sessions: Mutex<HashMap<String, Session>>,
    waiting: Mutex<HashMap<u64, Sender<Option<String>>>>,
    next: AtomicU64,
    /// False when the page couldn't be told: the prompt is declined at once.
    emit: Box<dyn Fn(Event) -> bool + Send + Sync>,
    timeout: Duration,
}

static SERVER: OnceLock<Arc<Server>> = OnceLock::new();

/// Registered for the command it was attached to; that command can't ask once this drops.
pub struct Asking {
    server: Arc<Server>,
    token: String,
    open: Arc<AtomicUsize>,
}

impl Asking {
    /// Waiting for the user, so the command's silence isn't the remote's.
    pub fn prompting(&self) -> bool {
        self.open.load(Ordering::Relaxed) > 0
    }
}

impl Drop for Asking {
    fn drop(&mut self) {
        lock(&self.server.sessions).remove(&self.token);
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Lets `cmd` (git, and the ssh it starts) ask the page. None where the server isn't running
/// (it failed to start, or not on unix): the command then fails as it did before.
pub fn attach(cmd: &mut Command, label: &str, op: Option<&str>) -> Option<Asking> {
    let server = SERVER.get()?.clone();
    let repo = cmd.get_current_dir().map(Path::to_path_buf);
    // An askpass the user set up keeps answering. git asks GIT_ASKPASS, then core.askPass,
    // then SSH_ASKPASS; ssh only SSH_ASKPASS.
    let set = |var| std::env::var_os(var).is_some_and(|v| !v.is_empty());
    let own_ssh = set("SSH_ASKPASS");
    let own_git = own_ssh
        || set("GIT_ASKPASS")
        || repo
            .as_deref()
            .is_some_and(|r| crate::git::config_value(r, None, "core.askPass").is_some());
    if own_git && own_ssh {
        return None;
    }
    let token = random_hex(16)?;
    let open = Arc::new(AtomicUsize::new(0));
    let session = Session {
        label: label.to_string(),
        op: op.map(str::to_string),
        repo,
        args: cmd
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect(),
        open: open.clone(),
    };
    lock(&server.sessions).insert(token.clone(), session);
    // git still tries the credential helpers first; only what they can't answer reaches us.
    // GIT_TERMINAL_PROMPT=0 (cmd.rs) stays: git asks askpass before it would try a terminal.
    cmd.env(SOCKET_ENV, &server.socket).env(TOKEN_ENV, &token);
    if !own_git {
        cmd.env("GIT_ASKPASS", &server.helper);
    }
    // OpenSSH before 8.4 ignores SSH_ASKPASS_REQUIRE and asks only with a DISPLAY, which X11
    // and XWayland sessions set; none is made up for Wayland alone. macOS 13+ has a newer ssh.
    if !own_ssh {
        cmd.env("SSH_ASKPASS", &server.helper)
            .env("SSH_ASKPASS_REQUIRE", "force");
    }
    Some(Asking {
        server,
        token,
        open,
    })
}

/// The page's answer; None is Cancel. Unknown ids (already timed out) are ignored.
pub fn answer(id: u64, answer: Option<String>) {
    if let Some(server) = SERVER.get() {
        server.answer(id, answer);
    }
}

/// Whether the page listens for prompts; until it does, they're declined at once.
static PAGE_READY: AtomicBool = AtomicBool::new(false);

pub fn page_ready() {
    PAGE_READY.store(true, Ordering::Relaxed);
}

/// The page is (re)loading: it no longer shows the open prompts, nor sees new ones yet.
pub fn decline_all() {
    PAGE_READY.store(false, Ordering::Relaxed);
    if let Some(server) = SERVER.get() {
        let waiting = std::mem::take(&mut *lock(&server.waiting));
        for tx in waiting.into_values() {
            let _ = tx.send(None);
        }
    }
}

impl Server {
    fn answer(&self, id: u64, answer: Option<String>) {
        if let Some(tx) = lock(&self.waiting).remove(&id) {
            let _ = tx.send(answer);
        }
    }
}

/// Serves prompts to the page for as long as the app runs, with this binary as the helper.
pub fn serve(app: tauri::AppHandle) {
    use tauri::Emitter;
    let Ok(helper) = std::env::current_exe() else {
        return;
    };
    start(helper, move |event| match event {
        Event::Ask(prompt) => {
            PAGE_READY.load(Ordering::Relaxed) && app.emit("askpass", prompt).is_ok()
        }
        Event::Done(id) => app.emit("askpass-done", id).is_ok(),
    });
}

/// Starts the one server, with `helper` as the program git and ssh run to ask. A failure only
/// leaves prompts unanswered, as before.
pub fn start(helper: PathBuf, emit: impl Fn(Event) -> bool + Send + Sync + 'static) {
    #[cfg(unix)]
    if let Some(server) = unix::listen(helper, Box::new(emit), PROMPT_TIMEOUT) {
        if SERVER.set(server).is_ok() {
            unix::remove_at_exit();
        }
    }
    #[cfg(not(unix))]
    let _ = (helper, emit);
}

/// When git or ssh started this binary to ask something: relays the question to the running
/// app, prints the answer, and returns the exit code (1 for Cancel). None for a normal launch.
/// Never starts the app itself, whatever goes wrong.
pub fn helper() -> Option<i32> {
    let socket = std::env::var_os(SOCKET_ENV)?;
    let token = std::env::var(TOKEN_ENV).unwrap_or_default();
    let prompt = std::env::args_os()
        .nth(1)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let request = Request {
        token,
        prompt,
        kind: std::env::var("SSH_ASKPASS_PROMPT").ok(),
    };
    #[cfg(unix)]
    let answer = unix::ask(std::path::Path::new(&socket), &request);
    #[cfg(not(unix))]
    let answer: Option<String> = {
        let _ = (socket, request);
        None
    };
    Some(match answer {
        Some(a) => {
            use std::io::Write;
            let mut out = std::io::stdout().lock();
            let _ = writeln!(out, "{a}").and_then(|_| out.flush());
            0
        }
        None => 1,
    })
}

/// The hosts a network command talks to, from its arguments and the repo's remotes.
fn remote_hosts(repo: &Path, args: &[String]) -> Vec<String> {
    use crate::git;
    let Some(sub) = args.first().map(String::as_str) else {
        return vec![];
    };
    // Each submodule has remotes of its own.
    if sub == "submodule" {
        return vec![];
    }
    let remotes = git::remotes(repo);
    let named = if args.iter().any(|a| a == "--all") {
        remotes.clone()
    } else {
        // `lfs pull`'s first word is its own subcommand; it uses the default remote.
        let first = args[1..]
            .iter()
            .find(|a| !a.starts_with('-'))
            .filter(|_| sub != "lfs");
        match first {
            Some(r) if remotes.contains(r) => vec![r.clone()],
            Some(url) if host_of(url).is_some() => return host_of(url).into_iter().collect(),
            _ => default_remote(repo, sub == "push", &remotes)
                .into_iter()
                .collect(),
        }
    };
    let mut hosts = vec![];
    for name in named {
        let mut get = vec!["remote", "get-url"];
        if sub == "push" {
            get.push("--push");
        }
        get.push(&name);
        let host = git::run_text(repo, &get)
            .ok()
            .and_then(|u| host_of(u.trim()));
        if let Some(h) = host.filter(|h| !hosts.contains(h)) {
            hosts.push(h);
        }
    }
    hosts
}

/// The remote git picks when none is named: the branch's (for a push its pushRemote, or
/// remote.pushDefault, first), else origin, else the only one.
fn default_remote(repo: &Path, push: bool, remotes: &[String]) -> Option<String> {
    use crate::git::{config_value, run_text};
    let branch = run_text(repo, &["symbolic-ref", "--short", "-q", "HEAD"]).ok();
    let key = |k: &str| {
        let b = branch.as_deref()?.trim();
        config_value(repo, None, &format!("branch.{b}.{k}")).filter(|r| remotes.contains(r))
    };
    push.then(|| {
        key("pushRemote").or_else(|| {
            config_value(repo, None, "remote.pushDefault").filter(|r| remotes.contains(r))
        })
    })
    .flatten()
    .or_else(|| key("remote"))
    .or_else(|| remotes.iter().find(|r| *r == "origin").cloned())
    .or_else(|| (remotes.len() == 1).then(|| remotes[0].clone()))
}

/// `https://me:secret@host:8443/x`, `ssh://git@[::1]/x` and `git@host:owner/x` → the host.
/// None for a local path.
fn host_of(url: &str) -> Option<String> {
    let rest = match url.split_once("://") {
        Some(("file", _)) => return None,
        Some((_, rest)) => rest,
        // scp-like `host:path`; a local path has a slash before any colon.
        None => url
            .find(':')
            .filter(|&i| !url[..i].contains('/'))
            .map(|_| url)?,
    };
    let authority = rest.split(['/', '?', '#']).next()?;
    let host = authority.rsplit('@').next()?;
    let host = match host.strip_prefix('[') {
        Some(v6) => v6.split(']').next()?,
        None => host.split(':').next()?,
    };
    (!host.is_empty()).then(|| host.to_string())
}

/// `n` random bytes, in hex.
fn random_hex(n: usize) -> Option<String> {
    use std::io::Read;
    let mut bytes = vec![0u8; n];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .ok()?;
    Some(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

#[cfg(unix)]
mod unix {
    use super::*;
    use std::io::{BufRead, BufReader, ErrorKind, Read, Write};
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::Path;
    use std::sync::mpsc::{channel, RecvTimeoutError};
    use std::time::Instant;

    /// A request is a token and a prompt; anything longer isn't one of our helpers.
    const MAX_REQUEST: u64 = 64 * 1024;

    /// The socket sits in a new directory only this user can enter, and every request must
    /// carry the token of a command still running.
    pub(super) fn listen(
        helper: PathBuf,
        emit: Box<dyn Fn(Event) -> bool + Send + Sync>,
        timeout: Duration,
    ) -> Option<Arc<Server>> {
        let dir = std::env::temp_dir().join(format!("gitviber-{}", random_hex(6)?));
        // Fails if anything already sits at that path, so nobody can plant one for us.
        std::fs::DirBuilder::new().mode(0o700).create(&dir).ok()?;
        let socket = dir.join("askpass");
        let listener = UnixListener::bind(&socket).ok()?;
        let _ = std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600));
        let server = Arc::new(Server {
            socket,
            helper,
            sessions: Mutex::default(),
            waiting: Mutex::default(),
            next: AtomicU64::new(1),
            emit,
            timeout,
        });
        let s = server.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let s = s.clone();
                std::thread::spawn(move || s.handle(stream));
            }
        });
        Some(server)
    }

    /// Takes the socket's directory with the app; a crash leaves an empty-ish folder in temp.
    pub(super) fn remove_at_exit() {
        extern "C" fn remove() {
            if let Some(dir) = SERVER.get().and_then(|s| s.socket.parent()) {
                let _ = std::fs::remove_dir_all(dir);
            }
        }
        unsafe {
            libc::atexit(remove);
        }
    }

    impl Server {
        fn handle(&self, stream: UnixStream) {
            let answer = self.request(&stream);
            let _ = stream.set_nonblocking(false);
            let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
            if let Ok(reply) = serde_json::to_string(&Reply { answer }) {
                let _ = (&stream).write_all(reply.as_bytes());
            }
        }

        fn request(&self, stream: &UnixStream) -> Option<String> {
            stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
            let mut line = String::new();
            BufReader::new(Read::take(stream, MAX_REQUEST))
                .read_line(&mut line)
                .ok()?;
            let request: Request = serde_json::from_str(&line).ok()?;
            let session = lock(&self.sessions).get(&request.token).cloned()?;
            let id = self.next.fetch_add(1, Ordering::Relaxed);
            let (tx, rx) = channel();
            lock(&self.waiting).insert(id, tx);
            session.open.fetch_add(1, Ordering::Relaxed);
            let hosts = session
                .repo
                .as_deref()
                .map_or_else(Vec::new, |r| remote_hosts(r, &session.args));
            let shown = (self.emit)(Event::Ask(Prompt {
                id,
                text: request.prompt,
                kind: request.kind,
                label: session.label,
                op: session.op,
                hosts,
            }));
            let answer = shown
                .then(|| stream.set_nonblocking(true).ok())
                .flatten()
                .and_then(|_| self.wait(&rx, stream));
            lock(&self.waiting).remove(&id);
            session.open.fetch_sub(1, Ordering::Relaxed);
            (self.emit)(Event::Done(id));
            answer
        }

        /// The page's answer, or None once the helper is gone (the command was stopped, or
        /// ssh closed a notice) or the prompt timed out.
        fn wait(
            &self,
            rx: &std::sync::mpsc::Receiver<Option<String>>,
            stream: &UnixStream,
        ) -> Option<String> {
            let until = Instant::now() + self.timeout;
            while Instant::now() < until {
                match rx.recv_timeout(Duration::from_millis(100)) {
                    Ok(answer) => return answer,
                    Err(RecvTimeoutError::Disconnected) => return None,
                    Err(RecvTimeoutError::Timeout) => {}
                }
                // The helper sends nothing more, so any read but "would block" means it left.
                match (&*stream).read(&mut [0u8; 1]) {
                    Err(e) if e.kind() == ErrorKind::WouldBlock => {}
                    _ => return None,
                }
            }
            None
        }
    }

    /// The helper's side: one request, one reply, then the app closes the connection.
    pub(super) fn ask(socket: &Path, request: &Request) -> Option<String> {
        let mut stream = UnixStream::connect(socket).ok()?;
        let mut line = serde_json::to_string(request).ok()?;
        line.push('\n');
        stream.write_all(line.as_bytes()).ok()?;
        serde_json::from_reader::<_, Reply>(&stream).ok()?.answer
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// A server in its own directory, answering each prompt with `respond`.
        fn server(
            timeout: Duration,
            respond: impl Fn(&Prompt) -> Option<Option<String>> + Send + Sync + 'static,
        ) -> (Arc<Server>, Arc<Mutex<Vec<u64>>>) {
            server_for(timeout, true, respond)
        }

        /// `listening`: false plays a page that isn't there to show the prompt.
        fn server_for(
            timeout: Duration,
            listening: bool,
            respond: impl Fn(&Prompt) -> Option<Option<String>> + Send + Sync + 'static,
        ) -> (Arc<Server>, Arc<Mutex<Vec<u64>>>) {
            let done = Arc::new(Mutex::new(vec![]));
            let slot: Arc<OnceLock<std::sync::Weak<Server>>> = Arc::default();
            let (s, d) = (slot.clone(), done.clone());
            let emit = move |e: Event| match e {
                Event::Ask(p) => {
                    if let Some(a) = respond(&p).filter(|_| listening) {
                        let server = s.get().and_then(|w| w.upgrade()).unwrap();
                        std::thread::spawn(move || server.answer(p.id, a));
                    }
                    listening
                }
                Event::Done(id) => {
                    d.lock().unwrap().push(id);
                    true
                }
            };
            let server = listen(PathBuf::from("/unused"), Box::new(emit), timeout).unwrap();
            let _ = slot.set(Arc::downgrade(&server));
            (server, done)
        }

        fn session(server: &Arc<Server>, token: &str) -> Arc<AtomicUsize> {
            let open = Arc::new(AtomicUsize::new(0));
            lock(&server.sessions).insert(
                token.into(),
                Session {
                    label: "git push".into(),
                    op: Some("op-1".into()),
                    repo: None,
                    args: vec![],
                    open: open.clone(),
                },
            );
            open
        }

        fn req(token: &str, prompt: &str) -> Request {
            Request {
                token: token.into(),
                prompt: prompt.into(),
                kind: None,
            }
        }

        fn cleanup(server: &Server) {
            let _ = std::fs::remove_dir_all(server.socket.parent().unwrap());
        }

        #[test]
        fn relays_prompts_and_answers() {
            let (server, done) = server(Duration::from_secs(10), |p| {
                assert_eq!(
                    (p.label.as_str(), p.op.as_deref()),
                    ("git push", Some("op-1"))
                );
                Some(match p.text.as_str() {
                    "Username for 'https://example.com': " => Some("alice".into()),
                    "Password for 'https://alice@example.com': " => Some("p ss wörd".into()),
                    _ => None,
                })
            });
            session(&server, "t1");
            let ask = |prompt| ask(&server.socket, &req("t1", prompt));
            assert_eq!(
                ask("Username for 'https://example.com': ").as_deref(),
                Some("alice")
            );
            assert_eq!(
                ask("Password for 'https://alice@example.com': ").as_deref(),
                Some("p ss wörd")
            );
            assert_eq!(ask("Something else"), None, "Cancel");
            assert_eq!(done.lock().unwrap().len(), 3);
            assert!(lock(&server.waiting).is_empty());
            cleanup(&server);
        }

        #[test]
        fn refuses_unknown_tokens_and_junk() {
            let (server, done) = server(Duration::from_secs(10), |_| Some(Some("x".into())));
            session(&server, "good");
            assert_eq!(ask(&server.socket, &req("bad", "Password: ")), None);
            assert_eq!(ask(&server.socket, &req("", "Password: ")), None);
            let mut junk = UnixStream::connect(&server.socket).unwrap();
            junk.write_all(b"not json\n").unwrap();
            let mut reply = String::new();
            junk.read_to_string(&mut reply).unwrap();
            assert_eq!(reply, r#"{"answer":null}"#);
            assert!(
                done.lock().unwrap().is_empty(),
                "no prompt reached the page"
            );
            let meta = std::fs::metadata(server.socket.parent().unwrap()).unwrap();
            assert_eq!(meta.permissions().mode() & 0o777, 0o700);
            cleanup(&server);
        }

        #[test]
        fn a_helper_that_leaves_closes_its_prompt() {
            let (server, done) = server(Duration::from_secs(10), |_| None);
            let open = session(&server, "t");
            let mut stream = UnixStream::connect(&server.socket).unwrap();
            let line = serde_json::to_string(&req("t", "Enter passphrase: ")).unwrap() + "\n";
            stream.write_all(line.as_bytes()).unwrap();
            let start = Instant::now();
            while open.load(Ordering::Relaxed) == 0 {
                assert!(start.elapsed() < Duration::from_secs(5));
                std::thread::sleep(Duration::from_millis(10));
            }
            drop(stream);
            while done.lock().unwrap().is_empty() {
                assert!(start.elapsed() < Duration::from_secs(5), "still open");
                std::thread::sleep(Duration::from_millis(10));
            }
            assert_eq!(open.load(Ordering::Relaxed), 0);
            assert!(lock(&server.waiting).is_empty());
            cleanup(&server);
        }

        #[test]
        fn a_prompt_the_page_cant_show_is_declined_at_once() {
            let (server, done) =
                server_for(Duration::from_secs(60), false, |_| Some(Some("x".into())));
            session(&server, "t");
            let start = Instant::now();
            assert_eq!(ask(&server.socket, &req("t", "Password: ")), None);
            assert!(start.elapsed() < Duration::from_secs(5));
            assert_eq!(done.lock().unwrap().len(), 1);
            cleanup(&server);
        }

        #[test]
        fn names_hosts_without_credentials() {
            for (url, host) in [
                ("https://me:s3cret@github.com/a/b.git", Some("github.com")),
                ("https://git.example.com:8443/a", Some("git.example.com")),
                ("ssh://git@[::1]:22/x", Some("::1")),
                ("git@gitlab.com:group/x.git", Some("gitlab.com")),
                ("work-github:owner/x", Some("work-github")),
                ("/srv/repos/x.git", None),
                ("../x", None),
                ("file:///srv/x", None),
            ] {
                assert_eq!(host_of(url).as_deref(), host, "{url}");
            }
        }

        #[test]
        fn finds_the_hosts_a_command_talks_to() {
            let dir = std::env::temp_dir().join(format!("gitviber-hosts-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let git = |args: &[&str]| crate::git::run(&dir, args).unwrap();
            git(&["init", "-q", "-b", "main"]);
            git(&[
                "remote",
                "add",
                "origin",
                "https://me:tok@github.com/me/x.git",
            ]);
            git(&["remote", "add", "upstream", "git@gitlab.com:them/x.git"]);
            git(&[
                "remote",
                "set-url",
                "--push",
                "upstream",
                "ssh://push.example/x",
            ]);
            let hosts = |args: &[&str]| {
                let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
                remote_hosts(&dir, &args)
            };
            assert_eq!(hosts(&["fetch", "--progress", "upstream"]), ["gitlab.com"]);
            assert_eq!(
                hosts(&["push", "--progress", "-u", "upstream", "HEAD"]),
                ["push.example"]
            );
            assert_eq!(
                hosts(&["pull", "--progress", "--no-edit", "--ff-only"]),
                ["github.com"]
            );
            assert_eq!(
                hosts(&["fetch", "--progress", "--all", "--prune"]),
                ["github.com", "gitlab.com"]
            );
            assert_eq!(
                hosts(&["clone", "--progress", "--", "https://evil.example/x", "d"]),
                ["evil.example"]
            );
            assert_eq!(
                hosts(&["lfs", "pull", "--include", "upstream", "--exclude", ""]),
                ["github.com"]
            );
            assert!(hosts(&["submodule", "update", "--init"]).is_empty());
            git(&["config", "branch.main.remote", "upstream"]);
            assert_eq!(
                hosts(&["pull", "--progress", "--no-edit", "--ff-only"]),
                ["gitlab.com"]
            );
            assert_eq!(hosts(&["push", "--progress"]), ["push.example"]);
            let _ = std::fs::remove_dir_all(&dir);
        }

        #[test]
        fn an_unanswered_prompt_times_out() {
            let (server, done) = server(Duration::from_millis(300), |_| None);
            session(&server, "t");
            let start = Instant::now();
            assert_eq!(ask(&server.socket, &req("t", "Password: ")), None);
            assert!(start.elapsed() < Duration::from_secs(5));
            assert_eq!(done.lock().unwrap().len(), 1);
            cleanup(&server);
        }
    }
}
