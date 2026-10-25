//! The socket `gitviber browser` talks to (client.rs): in a folder only this user can enter,
//! answering only requests with a token from this launch, which each of the app's terminals is
//! given for its own pane. While Settings → Browser doesn't let agents use the browser, it answers
//! only a page shown to the user (`tab new --show`).

use super::control::{self, Reply, Request};
use crate::Handle;
use std::path::PathBuf;
use std::sync::OnceLock;

/// This launch's secret, each pane's token made from it (control.rs `pane_token`): 32 random
/// bytes, hex. Empty when none could be had, which no request matches.
pub fn token() -> &'static str {
    static TOKEN: OnceLock<String> = OnceLock::new();
    TOKEN.get_or_init(|| gitviber_core::local_socket::random_hex(32).unwrap_or_default())
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
        env.push((control::TOKEN_ENV, control::pane_token(token(), pty)));
    }
    env
}

/// The answer to a request: none without its pane's token from this launch; while control is
/// off, only to one that shows the user a page.
fn answer(app: &Handle, req: Request, on: bool) -> Reply {
    let ours = token();
    let pane = req.pty.map(|pty| control::pane_token(ours, pty));
    if ours.is_empty() || !pane.is_some_and(|pane| control::same_token(&req.token, &pane)) {
        return Reply::error(
            "This terminal is from another launch of GitViber: open a new one to use the browser.",
        );
    }
    if !on && !control::allowed_when_off(&req.cmd, &req.args) {
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
pub fn set_enabled(_: &Handle, _: bool) {}

#[cfg(unix)]
mod unix {
    use super::*;
    use gitviber_core::local_socket::{private_dir, serve_line};
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::UnixListener;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex, PoisonError};
    use std::time::Duration;

    /// A command and its arguments: a page of text to fill in, at most.
    const MAX_REQUEST: u64 = 1024 * 1024;

    struct Listening {
        /// Settings → Browser lets agents use the browser.
        on: Arc<AtomicBool>,
    }

    static LISTENING: Mutex<Option<Listening>> = Mutex::new(None);

    /// Settings → Browser's switch: the socket is made once and stays, so the CLI can still show
    /// the user a page while agents may not use the browser; the switch says what it answers.
    pub fn set_enabled(app: &Handle, on: bool) {
        let mut listening = LISTENING.lock().unwrap_or_else(PoisonError::into_inner);
        if listening.is_none() {
            *listening = listen(app.clone());
        }
        if let Some(l) = listening.as_ref() {
            l.on.store(on, Ordering::SeqCst);
        }
    }

    fn listen(app: Handle) -> Option<Listening> {
        let path = socket_path();
        private_dir(path.parent()?)?;
        // One left by a launch that crashed.
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path).ok()?;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        let on = Arc::new(AtomicBool::new(false));
        let control = on.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let (app, control) = (app.clone(), control.clone());
                std::thread::spawn(move || {
                    serve_line(
                        &stream,
                        MAX_REQUEST,
                        Duration::from_secs(5),
                        |req| match req {
                            Some(req) => answer(&app, req, control.load(Ordering::SeqCst)),
                            None => Reply::error("Not a request."),
                        },
                    )
                });
            }
        });
        Some(Listening { on })
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
            let pane = super::super::control::pane_token(token, 7);
            assert_eq!(get(super::super::control::TOKEN_ENV), Some(pane.as_str()));
            assert_eq!(get(super::super::control::SOCKET_ENV), path.to_str());
            assert!(get(super::super::control::EXE_ENV).is_some());
        }
    }
}
