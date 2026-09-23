//! The environment the user's login shell builds. An app launched from Finder gets launchd's
//! bare PATH, so hooks that call node, npx or pnpm (from nvm, fnm, Volta, mise, …) failed
//! with exit 127 while the same commit worked in a terminal.

use std::ffi::{OsStr, OsString};
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// Where the terminal and the shell probe start: the environment launchd gives any app, not
/// ours. Run from `pnpm tauri dev` we carry npm_config_prefix, which makes nvm refuse to load.
pub fn clean_env() -> Vec<(OsString, OsString)> {
    let mut env: Vec<(OsString, OsString)> = [
        "HOME",
        "USER",
        "LOGNAME",
        "TMPDIR",
        "SSH_AUTH_SOCK",
        "LANG",
        "LC_ALL",
    ]
    .into_iter()
    .filter_map(|k| std::env::var_os(k).map(|v| (k.into(), v)))
    .collect();
    env.push(("PATH".into(), "/usr/bin:/bin:/usr/sbin:/sbin".into()));
    env
}

/// Shells that print a banner or run a slow plugin manager still answer well within this.
const TIMEOUT: Duration = Duration::from_secs(3);

static LOGIN_PATH: OnceLock<Option<OsString>> = OnceLock::new();

/// PATH from the login shell once it has answered; None before that, or if it never did.
pub fn login_path() -> Option<&'static OsStr> {
    LOGIN_PATH.get().and_then(|p| p.as_deref())
}

/// Asks the login shell for its PATH on a thread of its own, so the window never waits.
/// Commands that run before it answers use the fallback PATH.
pub fn resolve_in_background() {
    std::thread::spawn(|| {
        let shell = std::env::var_os("SHELL")
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "/bin/zsh".into());
        let path = probe_path(Path::new(&shell))
            .inspect_err(|e| eprintln!("login shell PATH: {e}; using the default PATH"))
            .ok();
        let _ = LOGIN_PATH.set(path);
    });
}

/// Waits for the login shell's answer, for a caller that found nothing on the fallback PATH.
pub fn wait_for_login_path() -> Option<&'static OsStr> {
    let deadline = Instant::now() + TIMEOUT + Duration::from_secs(1);
    while LOGIN_PATH.get().is_none() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    login_path()
}

/// Runs `shell -ilc` (interactive and login, so both .zprofile and .zshrc run, as in a
/// terminal) and reads PATH printed between markers, past anything the rc files print.
pub fn probe_path(shell: &Path) -> Result<OsString, String> {
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
    let found = rx.recv_timeout(TIMEOUT);
    let _ = child.kill();
    let _ = child.wait();
    match found {
        Ok(path) if !path.is_empty() => Ok(path.into()),
        Ok(_) => Err(format!("{} printed an empty PATH", shell.display())),
        Err(mpsc::RecvTimeoutError::Timeout) => {
            Err(format!("{} took over {TIMEOUT:?}", shell.display()))
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

    #[test]
    fn reads_between_marks_only() {
        let out = "Last login: today\nwelcome!\nM\n/a/bin:/usr/bin\nM\nbye\n";
        assert_eq!(between_marks(out, "M").as_deref(), Some("/a/bin:/usr/bin"));
        assert_eq!(between_marks("M\n/a/bin", "M"), None, "still printing");
    }
}
