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
    TOKEN.get_or_init(|| crate::local_socket::random_hex(32).unwrap_or_default())
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
    use crate::local_socket::{private_dir, serve_line};
    use std::os::unix::fs::PermissionsExt;
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
                    std::thread::spawn(move || {
                        serve_line(
                            &stream,
                            MAX_REQUEST,
                            Duration::from_secs(5),
                            |req| match req {
                                Some(req) => answer(&app, req, !stopped.load(Ordering::SeqCst)),
                                None => Reply::error("Not a request."),
                            },
                        )
                    });
                }
            }
        });
        Some(Listening { path, stop })
    }

    #[cfg(test)]
    mod tests {
        use super::private_dir;
        use std::os::unix::fs::{MetadataExt, PermissionsExt};

        fn mode(path: &std::path::Path) -> u32 {
            std::fs::symlink_metadata(path).unwrap().mode() & 0o777
        }

        #[test]
        fn the_sockets_folder_is_this_users_alone_or_there_is_none() {
            let base =
                std::env::temp_dir().join(format!("gitviber-server-test-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&base);
            std::fs::create_dir_all(&base).unwrap();
            // Made private.
            let fresh = base.join("fresh");
            assert_eq!(private_dir(&fresh), Some(()));
            assert_eq!(mode(&fresh), 0o700);
            // Found open to others: closed again.
            let open = base.join("open");
            std::fs::create_dir(&open).unwrap();
            std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o777)).unwrap();
            assert_eq!(private_dir(&open), Some(()));
            assert_eq!(mode(&open), 0o700);
            // A file, or a link to a folder someone placed there: no socket, and the link's target
            // keeps its mode.
            let file = base.join("file");
            std::fs::write(&file, b"x").unwrap();
            assert_eq!(private_dir(&file), None);
            let target = base.join("target");
            std::fs::create_dir(&target).unwrap();
            std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755)).unwrap();
            let link = base.join("link");
            std::os::unix::fs::symlink(&target, &link).unwrap();
            assert_eq!(private_dir(&link), None);
            assert_eq!(mode(&target), 0o755);
            // A folder that can't be made (its parent is a file): none.
            assert_eq!(private_dir(&file.join("below")), None);
            let _ = std::fs::remove_dir_all(&base);
        }

        #[test]
        fn the_socket_and_the_token_are_this_launchs_and_each_pane_gets_them() {
            let token = super::super::token();
            assert_eq!(token.len(), 64);
            assert!(token.bytes().all(|b| b.is_ascii_hexdigit()));
            assert_eq!(super::super::token(), token, "one a launch");
            let path = super::super::socket_path();
            let uid = unsafe { libc::getuid() };
            assert!(path.parent().unwrap().ends_with(format!("gitviber-{uid}")));
            let env = super::super::env(7);
            let get = |k: &str| {
                env.iter()
                    .find(|(key, _)| *key == k)
                    .map(|(_, v)| v.as_str())
            };
            assert_eq!(get(super::super::control::PTY_ENV), Some("7"));
            assert_eq!(get(super::super::control::TOKEN_ENV), Some(token));
            assert_eq!(get(super::super::control::SOCKET_ENV), path.to_str());
            assert!(get(super::super::control::EXE_ENV).is_some());
        }
    }
}
