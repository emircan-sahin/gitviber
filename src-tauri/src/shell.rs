//! The environment the user's login shell builds. An app launched from Finder gets launchd's
//! bare PATH, so hooks that call node, npx or pnpm (from nvm, fnm, Volta, mise, …) failed
//! with exit 127 while the same commit worked in a terminal.

use std::ffi::OsString;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Condvar, Mutex, MutexGuard};
use std::time::Duration;

/// Where the terminal and the shell probe start: the environment launchd gives any app, not
/// ours. Run from `pnpm tauri dev` we carry npm_config_prefix, which makes nvm refuse to load.
pub fn clean_env() -> Vec<(OsString, OsString)> {
    let mut env: Vec<(OsString, OsString)> = KEEP
        .iter()
        .chain(DESKTOP)
        .filter_map(|k| std::env::var_os(k).map(|v| (k.into(), v)))
        .collect();
    if cfg!(target_os = "linux") {
        env.extend(std::env::vars_os().filter(|(k, _)| k.to_string_lossy().starts_with("LC_")));
        if let Some(i) = env.iter().position(|(k, _)| k == "XDG_DATA_DIRS") {
            match without_appimage(&env[i].1, std::env::var_os("APPDIR")) {
                Some(dirs) => env[i].1 = dirs,
                None => drop(env.remove(i)),
            }
        }
    }
    if cfg!(unix) {
        env.push(("PATH".into(), "/usr/bin:/bin:/usr/sbin:/sbin".into()));
    }
    env
}

/// `dirs` without the ones an AppImage's launcher put first, inside its own mount; None if
/// nothing else is left.
fn without_appimage(dirs: &OsString, app: Option<OsString>) -> Option<OsString> {
    let Some(app) = app.filter(|a| !a.is_empty()) else {
        return Some(dirs.clone());
    };
    let app = Path::new(&app);
    let kept: Vec<_> = std::env::split_paths(dirs)
        .filter(|d| !d.starts_with(app))
        .collect();
    if kept.is_empty() {
        return None;
    }
    std::env::join_paths(kept).ok()
}

/// A Linux desktop session's: without them `xdg-open`, `code .`, a browser login, a keyring
/// or the clipboard tools an AI CLI pastes images with can't reach the display or D-Bus.
/// macOS apps get none of these from launchd.
#[cfg(target_os = "linux")]
const DESKTOP: &[&str] = &[
    "DISPLAY",
    "WAYLAND_DISPLAY",
    "XAUTHORITY",
    "XDG_RUNTIME_DIR",
    "DBUS_SESSION_BUS_ADDRESS",
    "XDG_SESSION_TYPE",
    "XDG_CURRENT_DESKTOP",
    "XDG_SESSION_DESKTOP",
    "DESKTOP_SESSION",
    "XDG_DATA_DIRS",
    "XDG_CONFIG_DIRS",
    "XDG_CONFIG_HOME",
    "XDG_DATA_HOME",
    "XDG_CACHE_HOME",
    "XDG_STATE_HOME",
    "LANGUAGE",
    "GTK_IM_MODULE",
    "QT_IM_MODULE",
    "XMODIFIERS",
];

#[cfg(not(target_os = "linux"))]
const DESKTOP: &[&str] = &[];

#[cfg(not(windows))]
const KEEP: &[&str] = &[
    "HOME",
    "USER",
    "LOGNAME",
    "TMPDIR",
    "SSH_AUTH_SOCK",
    "LANG",
    "LC_ALL",
];

/// Nothing starts without SystemRoot, and COMSPEC is the shell. There's no login shell to
/// rebuild PATH on Windows, so the user's own is kept.
#[cfg(windows)]
const KEEP: &[&str] = &[
    "SystemRoot",
    "SystemDrive",
    "windir",
    "COMSPEC",
    "PATHEXT",
    "PATH",
    "USERNAME",
    "USERPROFILE",
    "HOMEDRIVE",
    "HOMEPATH",
    "APPDATA",
    "LOCALAPPDATA",
    "ProgramData",
    "ProgramFiles",
    "ProgramFiles(x86)",
    "TEMP",
    "TMP",
    "SSH_AUTH_SOCK",
    "LANG",
];

/// Shells that print a banner or run a slow plugin manager still answer well within this.
const TIMEOUT: Duration = Duration::from_secs(3);
/// The background retry has nobody waiting on it, so it may take longer (a busy login).
const RETRY_AFTER: Duration = Duration::from_secs(20);
const RETRY_TIMEOUT: Duration = Duration::from_secs(10);

struct Probe {
    /// The last PATH a probe read; a failed probe later keeps it.
    path: Option<OsString>,
    running: bool,
    /// The first probe has finished, well or not.
    answered: bool,
}

static PROBE: Mutex<Probe> = Mutex::new(Probe {
    path: None,
    running: false,
    answered: false,
});
static FINISHED: Condvar = Condvar::new();
/// Bumped whenever `path` changes, so process.rs can cache its merged PATH until then.
static GENERATION: AtomicU64 = AtomicU64::new(0);

fn probe() -> MutexGuard<'static, Probe> {
    PROBE.lock().unwrap_or_else(|e| e.into_inner())
}

/// PATH from the login shell, once one has answered.
pub fn login_path() -> Option<OsString> {
    probe().path.clone()
}

pub fn generation() -> u64 {
    GENERATION.load(Ordering::Acquire)
}

/// Asks the login shell for its PATH on a thread of its own, so the window never waits.
/// Commands that run before it answers use the fallback PATH. A failed probe (a slow
/// login, say) is tried once more a little later.
pub fn resolve_in_background() {
    if cfg!(windows) {
        probe().answered = true;
        return;
    }
    std::thread::spawn(|| {
        if run(TIMEOUT).is_err() {
            std::thread::sleep(RETRY_AFTER);
            let _ = run(RETRY_TIMEOUT);
        }
    });
}

/// Probes again now (or joins a probe already running), for "Check again" after the user
/// installed something into a folder only the shell's PATH has.
pub fn reprobe() {
    if !cfg!(windows) {
        let _ = run(TIMEOUT);
    }
}

/// Waits for the first probe to finish, for a caller that found nothing on the fallback PATH.
pub fn wait_for_first_answer() {
    let wait = TIMEOUT + Duration::from_secs(1);
    drop(FINISHED.wait_timeout_while(probe(), wait, |p| !p.answered));
}

fn run(timeout: Duration) -> Result<(), String> {
    {
        let p = probe();
        if p.running {
            drop(FINISHED.wait_while(p, |p| p.running));
            return Ok(());
        }
        let mut p = p;
        p.running = true;
    }
    let found = probe_path(Path::new(&login_shell()), timeout);
    record(&found);
    found.map(|_| ())
}

/// $SHELL, else the one passwd(5) records for the user (the terminal's pty does the same),
/// else /bin/sh: many Linux systems have no zsh.
fn login_shell() -> OsString {
    std::env::var_os("SHELL")
        .filter(|s| !s.is_empty())
        .or_else(passwd_shell)
        .unwrap_or_else(|| "/bin/sh".into())
}

#[cfg(unix)]
fn passwd_shell() -> Option<OsString> {
    use std::os::unix::ffi::OsStrExt;
    let mut buf = vec![0 as libc::c_char; 4096];
    let mut pwd: libc::passwd = unsafe { std::mem::zeroed() };
    let mut found: *mut libc::passwd = std::ptr::null_mut();
    // The _r form: getpwuid's static buffer isn't safe beside other threads.
    let rc = unsafe {
        libc::getpwuid_r(
            libc::getuid(),
            &mut pwd,
            buf.as_mut_ptr(),
            buf.len(),
            &mut found,
        )
    };
    if rc != 0 || found.is_null() || pwd.pw_shell.is_null() {
        return None;
    }
    let shell = unsafe { std::ffi::CStr::from_ptr(pwd.pw_shell) }.to_bytes();
    // An empty field means /bin/sh, which the caller falls back to.
    (!shell.is_empty()).then(|| std::ffi::OsStr::from_bytes(shell).to_os_string())
}

#[cfg(not(unix))]
fn passwd_shell() -> Option<OsString> {
    None
}

fn record(found: &Result<OsString, String>) {
    let mut p = probe();
    p.running = false;
    p.answered = true;
    match found {
        Ok(path) if p.path.as_ref() != Some(path) => {
            p.path = Some(path.clone());
            GENERATION.fetch_add(1, Ordering::Release);
        }
        Ok(_) => {}
        Err(e) => eprintln!("login shell PATH: {e}; keeping the PATH we had"),
    }
    FINISHED.notify_all();
}

/// Runs `shell -ilc` (interactive and login, so both .zprofile and .zshrc run, as in a
/// terminal) and reads PATH printed between markers, past anything the rc files print.
pub fn probe_path(shell: &Path, timeout: Duration) -> Result<OsString, String> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos());
    let mark = format!("_GITVIBER_{}_{nanos}_", std::process::id());
    let mut cmd = Command::new(shell);
    cmd.args(["-ilc", &format!("echo {mark}; printenv PATH; echo {mark}")])
        .env_clear()
        .envs(clean_env())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    detach(&mut cmd);
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("could not run {}: {e}", shell.display()))?;
    let mut out = child.stdout.take().ok_or("no stdout")?;
    let (tx, rx) = mpsc::channel();
    // Stops at the closing marker, not at EOF: a daemon the rc files start can hold the pipe.
    std::thread::spawn(move || {
        let (mut buf, mut chunk) = (Vec::new(), [0u8; 4096]);
        while let Ok(n @ 1..) = out.read(&mut chunk) {
            buf.extend_from_slice(&chunk[..n]);
            if let Some(path) = between_marks(&String::from_utf8_lossy(&buf), &mark) {
                let _ = tx.send(path);
                return;
            }
        }
    });
    let found = rx.recv_timeout(timeout);
    let _ = child.kill();
    let _ = child.wait();
    match found {
        Ok(path) if !path.is_empty() => Ok(path.into()),
        Ok(_) => Err(format!("{} printed an empty PATH", shell.display())),
        Err(mpsc::RecvTimeoutError::Timeout) => {
            Err(format!("{} took over {timeout:?}", shell.display()))
        }
        Err(_) => Err(format!("{} exited without printing PATH", shell.display())),
    }
}

/// A new session with no controlling terminal, like VS Code's `detached`. An interactive
/// shell started from a terminal (`pnpm tauri dev`) would otherwise take it over, or stop.
#[cfg(unix)]
fn detach(cmd: &mut Command) {
    use std::os::unix::process::CommandExt;
    extern "C" {
        fn setsid() -> i32;
    }
    // SAFETY: setsid is async-signal-safe, so it may run between fork and exec.
    unsafe {
        cmd.pre_exec(|| {
            setsid();
            Ok(())
        });
    }
}

#[cfg(not(unix))]
fn detach(_cmd: &mut Command) {}

fn between_marks(text: &str, mark: &str) -> Option<String> {
    let (_, rest) = text.split_once(mark)?;
    let (inner, _) = rest.split_once(mark)?;
    Some(inner.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn passwd_names_a_shell_that_exists() {
        let shell = passwd_shell().expect("the user has a passwd entry");
        assert!(Path::new(&shell).is_file(), "{shell:?}");
    }

    #[test]
    fn appimage_data_dirs_are_dropped() {
        let dirs = OsString::from("/tmp/.mount_GitVib/usr/share:/usr/share:/usr/local/share");
        let app = Some(OsString::from("/tmp/.mount_GitVib"));
        assert_eq!(
            without_appimage(&dirs, app.clone()),
            Some("/usr/share:/usr/local/share".into())
        );
        assert_eq!(without_appimage(&dirs, None), Some(dirs));
        assert_eq!(
            without_appimage(&"/tmp/.mount_GitVib/usr/share".into(), app),
            None
        );
    }

    #[test]
    fn reads_between_marks_only() {
        let out = "Last login: today\nwelcome!\nM\n/a/bin:/usr/bin\nM\nbye\n";
        assert_eq!(between_marks(out, "M").as_deref(), Some("/a/bin:/usr/bin"));
        assert_eq!(between_marks("M\n/a/bin", "M"), None, "still printing");
    }

    /// A later probe's PATH reaches git without a restart; a failed one keeps the last.
    #[test]
    fn a_new_login_path_replaces_the_cached_one() {
        let starts = |dir: &str| {
            crate::process::search_path()
                .to_string_lossy()
                .starts_with(dir)
        };
        let path = |dir: &str| {
            let mut p = OsString::from(dir);
            p.push(":/usr/bin:/bin");
            p
        };
        record(&Ok(path("/gitviber-test/first")));
        assert!(starts("/gitviber-test/first"));
        let before = generation();
        record(&Ok(path("/gitviber-test/second")));
        assert!(generation() > before);
        assert!(starts("/gitviber-test/second"));
        record(&Err("took too long".into()));
        assert!(starts("/gitviber-test/second"));
    }
}
