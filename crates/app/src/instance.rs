//! One app per user and profile on Linux: a second launch (`gitviber <path>`, a launcher click)
//! hands its paths to the first over a unix socket and exits, as macOS's LaunchServices does
//! there by itself. Not in development builds, which would hand their arguments to the installed
//! app and quit (GITVIBER_SINGLE_INSTANCE=1 turns it on).

#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use std::path::{Path, PathBuf};

#[derive(serde::Serialize, serde::Deserialize, Debug, PartialEq)]
pub struct Request {
    pub args: Vec<String>,
    pub cwd: String,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, PartialEq)]
pub struct Reply {
    pub ok: bool,
}

/// Off unless a release build, or asked for.
pub fn enabled() -> bool {
    cfg!(target_os = "linux")
        && (!cfg!(debug_assertions)
            || std::env::var("GITVIBER_SINGLE_INSTANCE").is_ok_and(|v| v == "1"))
}

/// The socket of this user's app with this config dir: two profiles are two apps.
pub fn socket_path() -> Option<PathBuf> {
    let config = gitviber_ui::store::config_dir();
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in config.to_string_lossy().bytes() {
        hash = (hash ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
    }
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        // Another user's folder of that name fails private_dir: no single instance then.
        .unwrap_or_else(|| std::env::temp_dir().join("gitviber-instance"));
    let dir = base.join("gitviber");
    gitviber_core::local_socket::private_dir(&base)?;
    gitviber_core::local_socket::private_dir(&dir)?;
    Some(dir.join(format!("{hash:016x}.sock")))
}

/// Hands this launch's arguments to the app already running at `socket`; true when it took them.
#[cfg(unix)]
pub fn hand_over(socket: &Path, args: Vec<String>, cwd: &Path) -> bool {
    let request = Request {
        args,
        cwd: cwd.to_string_lossy().into_owned(),
    };
    matches!(
        gitviber_core::local_socket::ask_line::<_, Reply>(
            socket,
            &request,
            Some(std::time::Duration::from_secs(5))
        ),
        Ok(Reply { ok: true })
    )
}

/// Listens at `socket` (a stale one from a crash is replaced) for later launches, giving each
/// one's paths to `opened`, on a thread of its own for as long as the app runs.
#[cfg(unix)]
pub fn serve(
    socket: PathBuf,
    opened: impl Fn(Vec<PathBuf>) + Send + 'static,
) -> std::io::Result<()> {
    use std::os::unix::net::UnixListener;
    let _ = std::fs::remove_file(&socket);
    let listener = UnixListener::bind(&socket)?;
    std::thread::Builder::new()
        .name("gitviber-instance".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                gitviber_core::local_socket::serve_line(
                    &stream,
                    64 * 1024,
                    std::time::Duration::from_secs(5),
                    |request: Option<Request>| {
                        let Some(r) = request else {
                            return Reply { ok: false };
                        };
                        let args: Vec<std::ffi::OsString> =
                            r.args.into_iter().map(Into::into).collect();
                        opened(gitviber_core::opened::from_args(&args, Path::new(&r.cwd)));
                        Reply { ok: true }
                    },
                );
            }
        })?;
    Ok(())
}

/// Before anything starts: Some(exit code) when another app took this launch.
pub fn second_launch() -> Option<i32> {
    #[cfg(unix)]
    if enabled() {
        let socket = socket_path()?;
        let args: Vec<String> = std::env::args().collect();
        let cwd = std::env::current_dir().ok()?;
        if hand_over(&socket, args, &cwd) {
            return Some(0);
        }
    }
    None
}

/// The first launch: later ones reach it from here on, their paths given to `opened` (an
/// empty list is a launcher click, which still means "show me the app").
pub fn listen(opened: impl Fn(Vec<PathBuf>) + Send + 'static) {
    #[cfg(unix)]
    if enabled()
        && let Some(socket) = socket_path()
        && let Err(err) = serve(socket, opened)
    {
        eprintln!("gitviber: couldn't listen for a second launch: {err}");
    }
    #[cfg(not(unix))]
    let _ = opened;
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn a_second_launch_hands_its_paths_over() {
        let dir = std::env::temp_dir().join(format!("gitviber-instance-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let socket = dir.join("t.sock");
        // Nobody listening: this launch is the first.
        assert!(!hand_over(&socket, vec!["gitviber".into()], &dir));
        let (tx, rx) = std::sync::mpsc::channel();
        serve(socket.clone(), move |paths| {
            let _ = tx.send(paths);
        })
        .unwrap();
        let args = vec!["gitviber".to_string(), "sub/a.rs:3".into(), "--flag".into()];
        assert!(hand_over(&socket, args, &dir));
        let got = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        assert_eq!(got, vec![dir.join("sub/a.rs:3")]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
