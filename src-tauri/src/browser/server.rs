//! The socket `gitviber browser` talks to (client.rs): in a folder only this user can enter,
//! there only while Settings → Browser lets agents use the browser, and answering only requests
//! with this launch's token, which the app's terminals alone are given.

use super::control::{self, Reply, Request};
use std::path::PathBuf;
use std::sync::OnceLock;
use tauri::AppHandle;

/// This launch's token: 32 random bytes, hex. Empty when none could be had, which no request
/// matches.
pub fn token() -> &'static str {
    static TOKEN: OnceLock<String> = OnceLock::new();
    TOKEN.get_or_init(|| crate::askpass::random_hex(32).unwrap_or_default())
}

/// Where the socket is: the same each launch, so a terminal from before a restart of the setting
/// still reaches it. A debug build's apart, so it can run beside the installed app.
#[cfg(unix)]
pub fn socket_path() -> PathBuf {
    let uid = unsafe { libc::getuid() };
    let name = if cfg!(debug_assertions) {
        "browser-dev.sock"
    } else {
        "browser.sock"
    };
    std::env::temp_dir()
        .join(format!("gitviber-{uid}"))
        .join(name)
}

/// What a terminal's shell gets, to run `gitviber browser` with and to say which pane it is.
pub fn env(pty: u32) -> Vec<(&'static str, String)> {
    let mut env = vec![(control::PTY_ENV, pty.to_string())];
    if let Ok(exe) = std::env::current_exe() {
        env.push((control::EXE_ENV, exe.to_string_lossy().into_owned()));
    }
    #[cfg(unix)]
    {
        env.push((
            control::SOCKET_ENV,
            socket_path().to_string_lossy().into_owned(),
        ));
        env.push((control::TOKEN_ENV, token().to_string()));
    }
    env
}

/// The answer to a request: none without this launch's token, none while control is off.
fn answer(app: &AppHandle, req: Request, on: bool) -> Reply {
    let ours = token();
    if ours.is_empty() || !control::same_token(&req.token, ours) {
        return Reply::error(
            "This terminal is from another launch of GitViber: open a new one to use the browser.",
        );
    }
    if !on {
        return Reply {
            code: Some(control::OFF),
            ..Reply::error(control::OFF_TEXT)
        };
    }
    super::agent::run(app, req)
}

#[cfg(unix)]
pub use unix::set_enabled;

#[cfg(not(unix))]
pub fn set_enabled(_: &AppHandle, _: bool) {}

#[cfg(unix)]
mod unix {
    use super::*;
    use std::io::{BufRead, BufReader, ErrorKind, Read, Write};
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex, PoisonError};
    use std::time::Duration;

    /// A command and its arguments: a page of text to fill in, at most.
    const MAX_REQUEST: u64 = 1024 * 1024;

    struct Listening {
        path: PathBuf,
        stop: Arc<AtomicBool>,
    }

    static LISTENING: Mutex<Option<Listening>> = Mutex::new(None);

    /// Settings → Browser's switch: on, the socket is made; off, it goes, and the CLI says so.
    pub fn set_enabled(app: &AppHandle, on: bool) {
        let mut listening = LISTENING.lock().unwrap_or_else(PoisonError::into_inner);
        if on && listening.is_none() {
            *listening = listen(app.clone());
        } else if !on {
            if let Some(l) = listening.take() {
                l.stop.store(true, Ordering::SeqCst);
                // Wakes the accept, which then sees it's to stop.
                let _ = UnixStream::connect(&l.path);
                let _ = std::fs::remove_file(&l.path);
            }
        }
    }

    /// A folder of this user's alone, made so or found so; anything else there and no socket.
    fn private_dir(dir: &std::path::Path) -> Option<()> {
        match std::fs::DirBuilder::new().mode(0o700).create(dir) {
            Ok(()) => {}
            Err(e) if e.kind() == ErrorKind::AlreadyExists => {}
            Err(_) => return None,
        }
        let meta = std::fs::symlink_metadata(dir).ok()?;
        if !meta.is_dir() || meta.uid() != unsafe { libc::getuid() } {
            return None;
        }
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).ok()
    }

    fn listen(app: AppHandle) -> Option<Listening> {
        let path = socket_path();
        private_dir(path.parent()?)?;
        // One left by a launch that crashed.
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path).ok()?;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                if stopped.load(Ordering::SeqCst) {
                    break;
                }
                if let Ok(stream) = stream {
                    let (app, stopped) = (app.clone(), stopped.clone());
                    std::thread::spawn(move || handle(&app, &stream, &stopped));
                }
            }
        });
        Some(Listening { path, stop })
    }

    fn handle(app: &AppHandle, stream: &UnixStream, stopped: &AtomicBool) {
        let reply = match read(stream) {
            Some(req) => answer(app, req, !stopped.load(Ordering::SeqCst)),
            None => Reply::error("Not a request."),
        };
        let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
        if let Ok(line) = serde_json::to_string(&reply) {
            let _ = (&*stream).write_all((line + "\n").as_bytes());
        }
    }

    fn read(stream: &UnixStream) -> Option<Request> {
        stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
        let mut line = String::new();
        BufReader::new(Read::take(stream, MAX_REQUEST))
            .read_line(&mut line)
            .ok()?;
        serde_json::from_str(&line).ok()
    }
}
