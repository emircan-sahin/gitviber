//! Answers what git and ssh ask during a network command (a password, an SSH key's passphrase,
//! a new host's key) from a dialog on the page. They run this app's own binary as GIT_ASKPASS /
//! SSH_ASKPASS; started that way it only relays the prompt to the running app over a private
//! Unix socket and prints the answer. Nothing typed is stored or logged: an answer goes to git
//! once, and the user's credential helper decides what to keep, as in a terminal.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
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
    /// Its prompts on screen right now.
    open: Arc<AtomicUsize>,
}

struct Server {
    socket: PathBuf,
    helper: PathBuf,
    sessions: Mutex<HashMap<String, Session>>,
    waiting: Mutex<HashMap<u64, Sender<Option<String>>>>,
    next: AtomicU64,
    emit: Box<dyn Fn(Event) + Send + Sync>,
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
    let token = random_hex(16)?;
    let open = Arc::new(AtomicUsize::new(0));
    let session = Session {
        label: label.to_string(),
        op: op.map(str::to_string),
        open: open.clone(),
    };
    lock(&server.sessions).insert(token.clone(), session);
    // git still tries the credential helpers first; only what they can't answer reaches us.
    // GIT_TERMINAL_PROMPT=0 (cmd.rs) stays: git asks askpass before it would try a terminal.
    cmd.env("GIT_ASKPASS", &server.helper)
        .env("SSH_ASKPASS", &server.helper)
        .env("SSH_ASKPASS_REQUIRE", "force")
        .env(SOCKET_ENV, &server.socket)
        .env(TOKEN_ENV, &token);
    // OpenSSH before 8.4 ignores SSH_ASKPASS_REQUIRE and asks only with a DISPLAY; Linux
    // desktops on Wayland alone may not set one. macOS 13+ ships a newer ssh.
    if cfg!(target_os = "linux") && std::env::var_os("DISPLAY").is_none() {
        cmd.env("DISPLAY", ":0");
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

/// Declines every open prompt: a reloaded page no longer shows them.
pub fn decline_all() {
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
    start(helper, move |event| {
        let _ = match event {
            Event::Ask(prompt) => app.emit("askpass", prompt),
            Event::Done(id) => app.emit("askpass-done", id),
        };
    });
}

/// Starts the one server, with `helper` as the program git and ssh run to ask. A failure only
/// leaves prompts unanswered, as before.
pub fn start(helper: PathBuf, emit: impl Fn(Event) + Send + Sync + 'static) {
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
        emit: Box<dyn Fn(Event) + Send + Sync>,
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
            (self.emit)(Event::Ask(Prompt {
                id,
                text: request.prompt,
                kind: request.kind,
                label: session.label,
                op: session.op,
            }));
            let answer = stream
                .set_nonblocking(true)
                .ok()
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
            let done = Arc::new(Mutex::new(vec![]));
            let slot: Arc<OnceLock<std::sync::Weak<Server>>> = Arc::default();
            let (s, d) = (slot.clone(), done.clone());
            let emit = move |e: Event| match e {
                Event::Ask(p) => {
                    if let Some(a) = respond(&p) {
                        let server = s.get().and_then(|w| w.upgrade()).unwrap();
                        std::thread::spawn(move || server.answer(p.id, a));
                    }
                }
                Event::Done(id) => d.lock().unwrap().push(id),
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
