//! Shells for the terminal panel. Each session is a login shell on a pseudo-terminal; its
//! output streams to the UI as raw bytes (xterm.js decodes UTF-8 split across chunks).

use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};
use std::borrow::Cow;
use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};
use tauri::ipc::{Channel, Response};

/// Its own lock: a program that isn't reading blocks the write once the pty's buffer fills, and
/// the other panes' input, resizes and kills must not wait behind it.
pub type Writer = Arc<Mutex<Box<dyn Write + Send>>>;

/// Blocks until the program reads the input; once its shell is gone, the pty fails the write.
pub fn write(writer: &Writer, data: &[u8]) -> Result<(), String> {
    let mut w = writer.lock().unwrap_or_else(|e| e.into_inner());
    w.write_all(data)
        .and_then(|_| w.flush())
        .map_err(|e| e.to_string())
}

/// The bytes typed: text as UTF-8, or `binary`, xterm.js's char-per-byte string (mouse reports in
/// the default encoding, where a coordinate past 95 is a byte past 127).
pub fn input_bytes(data: &str, binary: bool) -> Cow<'_, [u8]> {
    // ConPTY reads its input as UTF-8: a lone byte past 127 isn't one.
    if binary && !cfg!(windows) {
        Cow::Owned(data.chars().map(|c| c as u8).collect())
    } else {
        Cow::Borrowed(data.as_bytes())
    }
}

/// How a shell ended: its exit code, or the signal that ended it ("Segmentation fault: 11").
#[derive(serde::Serialize)]
pub struct Exit {
    code: Option<u32>,
    signal: Option<String>,
}

/// A started shell, and whether it was started with the shell integration (shell_integration.rs).
#[derive(serde::Serialize)]
pub struct Spawned {
    id: u32,
    integrated: bool,
}

/// Output sent to the page that xterm.js hasn't parsed yet, past which the reader stops reading.
/// The page parses on its one thread for every pane: unthrottled, a flood (`cat` of a huge file,
/// `yes`) queued tens of MB, stalled every pane, and past 50 MB xterm.js drops data. Stopped, the
/// pty's own buffer fills and the program waits on its write. At 512 KiB no more than that is
/// drawn ahead of a ⌃C's effect, and eight 64 KiB reads fit before the reader waits.
const HIGH_WATER: usize = 512 * 1024;
/// Reading resumes below this, not at the first ack, so a flood moves in large steps.
const LOW_WATER: usize = HIGH_WATER / 2;
/// How long the reader waits for acks before it reads on regardless, until the page catches up.
/// A hidden window's WebKit throttles xterm.js's parsing: waiting on it froze an agent working in
/// the background (Node writes to a tty synchronously). A page in view parses the 512 KiB it holds
/// in far less, so only a page that isn't running gets past it.
const ACK_WAIT: Duration = Duration::from_secs(1);

#[derive(Default)]
struct Flow {
    state: Mutex<FlowState>,
    resumed: Condvar,
}

#[derive(Default)]
struct FlowState {
    unacked: usize,
    closed: bool,
    /// The page let ACK_WAIT pass without catching up: output flows unchecked until it does.
    unheard: bool,
}

impl Flow {
    /// Counts `n` bytes sent; past the high water, waits for the page's acks (or the session's
    /// end) for ACK_WAIT at most.
    fn sent(&self, n: usize) {
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        s.unacked += n;
        if s.unacked < HIGH_WATER || s.unheard {
            return;
        }
        let until = Instant::now() + ACK_WAIT;
        while s.unacked > LOW_WATER && !s.closed {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                s.unheard = true;
                return;
            }
            s = match self.resumed.wait_timeout(s, left) {
                Ok((s, _)) => s,
                Err(e) => e.into_inner().0,
            };
        }
    }

    /// `n` bytes the page has parsed.
    fn ack(&self, n: usize) {
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        s.unacked = s.unacked.saturating_sub(n);
        if s.unacked <= LOW_WATER {
            s.unheard = false;
            self.resumed.notify_all();
        }
    }

    /// Lets a waiting reader go, to see the pty close.
    fn close(&self) {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).closed = true;
        self.resumed.notify_all();
    }
}

struct Session {
    master: Box<dyn MasterPty + Send>,
    writer: Writer,
    killer: Box<dyn ChildKiller + Send + Sync>,
    shell: Option<u32>,
    flow: Arc<Flow>,
}

impl Session {
    #[cfg(unix)]
    fn job_leader(&self) -> Option<u32> {
        crate::procinfo::job_leader(self.master.process_group_leader(), self.shell)
    }

    #[cfg(not(unix))]
    fn job_leader(&self) -> Option<u32> {
        None
    }

    fn kill(mut self) {
        let _ = self.killer.kill();
        self.flow.close();
    }
}

#[derive(Default)]
pub struct Ptys {
    sessions: Arc<Mutex<HashMap<u32, Session>>>,
    next: AtomicU32,
}

fn size(cols: u16, rows: u16) -> PtySize {
    PtySize {
        rows: rows.max(1),
        cols: cols.max(1),
        pixel_width: 0,
        pixel_height: 0,
    }
}

/// `cwd`, or the nearest folder above it that's left, else home.
fn start_dir(cwd: &Path) -> Option<PathBuf> {
    cwd.ancestors()
        .find(|dir| dir.is_dir())
        .map(Path::to_path_buf)
        .or_else(|| {
            ["HOME", "USERPROFILE"]
                .into_iter()
                .filter_map(std::env::var_os)
                .map(PathBuf::from)
                .find(|home| home.is_dir())
        })
}

/// `path` with `dir` last, unless it's on it already: a `gitviber` of the user's own comes first.
/// `dir` has no `:` in it (cli::bin_dir).
fn with_last(path: &OsStr, dir: &Path) -> OsString {
    if std::env::split_paths(path).any(|d| d == dir) {
        return path.to_owned();
    }
    // Not after an empty PATH: ":dir" would put the current folder on it.
    let mut out = path.to_owned();
    if !out.is_empty() {
        out.push(":");
    }
    out.push(dir);
    out
}

impl Ptys {
    /// Starts the user's login shell in `cwd`, with the shell integration's scripts from
    /// `integration` if given. `exit` gets how it ended once it's gone.
    pub fn spawn(
        &self,
        cwd: &Path,
        cols: u16,
        rows: u16,
        integration: Option<&Path>,
        output: Channel<Response>,
        exit: Channel<Option<Exit>>,
    ) -> Result<Spawned, String> {
        // A removed worktree's restored terminals, and their splits, still start, saying where.
        let start = start_dir(cwd).ok_or_else(|| format!("folder not found: {}", cwd.display()))?;
        if start != cwd {
            let note = format!(
                "\x1b[2mno folder at {}; started in {}\x1b[0m\r\n",
                cwd.display(),
                start.display()
            );
            let _ = output.send(Response::new(note.into_bytes()));
        }
        // Like a pipe's, the pty's fds are marked close-on-exec a step after they're made.
        let pair = crate::process::spawning(|| native_pty_system().openpty(size(cols, rows)))
            .map_err(|e| e.to_string())?;
        #[cfg(unix)]
        let inject = integration.and_then(|dir| {
            crate::shell_integration::injection(Path::new(&crate::shell::login_shell()), dir)
        });
        #[cfg(not(unix))]
        let inject: Option<crate::shell_integration::Injection> = None;
        // The user's login shell, like Terminal.app: a Finder-launched app has a bare PATH.
        let mut cmd = match &inject {
            Some(i) if !i.args.is_empty() => CommandBuilder::from_argv(i.args.clone()),
            _ => CommandBuilder::new_default_prog(),
        };
        cmd.cwd(&start);
        // Not our environment: with npm_config_prefix from `pnpm tauri dev`, pnpm went missing.
        cmd.env_clear();
        for (key, value) in crate::shell::clean_env() {
            cmd.env(key, value);
        }
        // `gitviber .` with nothing installed. The integration adds it again after the user's rc
        // files, should one of them set PATH afresh.
        if let Some(bin) = crate::cli::bin_dir() {
            let path = with_last(cmd.get_env("PATH").unwrap_or_default(), &bin);
            cmd.env("PATH", path);
            cmd.env("GITVIBER_BIN_DIR", bin);
        }
        // Else portable-pty takes passwd's shell while the PATH probe took $SHELL (shell.rs).
        #[cfg(unix)]
        cmd.env("SHELL", crate::shell::login_shell());
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        cmd.env("TERM_PROGRAM", "GitViber");
        // GUI apps get no locale; without one zsh and git print UTF-8 as escapes.
        if std::env::var_os("LANG").is_none() {
            cmd.env("LANG", "en_US.UTF-8");
        }
        for (key, value) in inject.iter().flat_map(|i| &i.env) {
            cmd.env(key, value);
        }
        // `gitviber browser` (browser/client.rs): this pane's id, and how to reach the app.
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        for (key, value) in crate::browser::server::env(id) {
            cmd.env(key, value);
        }
        let mut child = crate::process::spawning(|| pair.slave.spawn_command(cmd))
            .map_err(|e| e.to_string())?;
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let writer = pair.master.take_writer().map_err(|e| e.to_string())?;

        let flow = Arc::new(Flow::default());
        self.sessions.lock().unwrap().insert(
            id,
            Session {
                master: pair.master,
                writer: Arc::new(Mutex::new(writer)),
                killer: child.clone_killer(),
                shell: child.process_id(),
                flow: flow.clone(),
            },
        );

        let sessions = self.sessions.clone();
        std::thread::spawn(move || {
            let mut buf = vec![0u8; 64 * 1024];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if output.send(Response::new(buf[..n].to_vec())).is_err() {
                            break;
                        }
                        flow.sent(n);
                    }
                }
            }
            let status = child.wait().ok().map(|s| Exit {
                code: s.signal().is_none().then(|| s.exit_code()),
                signal: s.signal().map(str::to_string),
            });
            sessions.lock().unwrap().remove(&id);
            let _ = exit.send(status);
        });
        Ok(Spawned {
            id,
            integrated: inject.is_some(),
        })
    }

    fn with<T>(
        &self,
        id: u32,
        f: impl FnOnce(&mut Session) -> Result<T, String>,
    ) -> Result<T, String> {
        match self.sessions.lock().unwrap().get_mut(&id) {
            Some(s) => f(s),
            None => Err("terminal session has ended".into()),
        }
    }

    /// Where `write` sends a session's input, taken out so the write happens outside the sessions lock.
    pub fn writer(&self, id: u32) -> Result<Writer, String> {
        self.with(id, |s| Ok(s.writer.clone()))
    }

    /// `bytes` of a session's output that xterm.js has parsed.
    pub fn ack(&self, id: u32, bytes: usize) {
        let _ = self.with(id, |s| {
            s.flow.ack(bytes);
            Ok(())
        });
    }

    pub fn resize(&self, id: u32, cols: u16, rows: u16) -> Result<(), String> {
        self.with(id, |s| {
            s.master.resize(size(cols, rows)).map_err(|e| e.to_string())
        })
    }

    /// Hangs up the shell. Its reader thread then sees EOF and reports the exit.
    pub fn kill(&self, id: u32) {
        let session = self.sessions.lock().unwrap().remove(&id);
        if let Some(s) = session {
            s.kill();
        }
    }

    /// Sessions (of `ids`, or all) whose foreground job isn't the shell itself: a command is running there.
    #[cfg(unix)]
    pub fn busy(&self, ids: Option<&[u32]>) -> usize {
        let sessions = self.sessions.lock().unwrap();
        let running = |(id, s): &(&u32, &Session)| {
            ids.is_none_or(|ids| ids.contains(id)) && s.job_leader().is_some()
        };
        sessions.iter().filter(running).count()
    }

    #[cfg(not(unix))]
    pub fn busy(&self, _ids: Option<&[u32]>) -> usize {
        0
    }

    /// The pid of the program a session runs in the foreground (its job's leader), when that
    /// isn't the shell.
    pub fn foreground(&self, id: u32) -> Option<u32> {
        self.with(id, |s| Ok(s.job_leader())).ok()?
    }

    /// A session's shell, whose process tree the browser's ports menu looks through.
    pub fn shell(&self, id: u32) -> Option<u32> {
        self.with(id, |s| Ok(s.shell)).ok()?
    }

    /// The folder a session's shell is in now, asked of the process as VS Code does for a split
    /// (its own, not the foreground job's): once, on a split or a save, never polled.
    pub fn cwd(&self, id: u32) -> Option<PathBuf> {
        let pid = self.with(id, |s| Ok(s.shell)).ok()??;
        crate::procinfo::cwd(pid)
    }

    /// A reloaded page has lost every terminal it had; without this their shells run on unseen.
    pub fn kill_all(&self) {
        let sessions: Vec<Session> = self
            .sessions
            .lock()
            .unwrap()
            .drain()
            .map(|(_, s)| s)
            .collect();
        for s in sessions {
            s.kill();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{input_bytes, start_dir, Flow, ACK_WAIT, HIGH_WATER, LOW_WATER};
    use std::io::Write;
    use std::sync::{mpsc, Arc};
    use std::time::Duration;

    /// Runs `sent(n)` on a reader thread; the receiver hears when it returns.
    fn reader(flow: &Arc<Flow>, n: usize) -> mpsc::Receiver<()> {
        let (tx, rx) = mpsc::channel();
        let flow = flow.clone();
        std::thread::spawn(move || {
            flow.sent(n);
            let _ = tx.send(());
        });
        rx
    }

    const WAIT: Duration = Duration::from_millis(100);

    #[test]
    fn the_reader_waits_past_the_high_water_until_acks_bring_it_below_the_low() {
        let flow = Arc::new(Flow::default());
        reader(&flow, HIGH_WATER - 1).recv_timeout(WAIT).unwrap();
        let paused = reader(&flow, 1);
        assert!(paused.recv_timeout(WAIT).is_err());
        // Acked, but still above the low water.
        flow.ack(HIGH_WATER - LOW_WATER - 1);
        assert!(paused.recv_timeout(WAIT).is_err());
        flow.ack(1);
        paused.recv_timeout(WAIT).unwrap();
    }

    #[test]
    fn a_page_that_stops_acking_holds_the_reader_up_a_second_at_most() {
        let flow = Arc::new(Flow::default());
        let paused = reader(&flow, HIGH_WATER);
        assert!(paused.recv_timeout(WAIT).is_err());
        paused.recv_timeout(ACK_WAIT * 2).unwrap();
        // Then reads on without waiting, until the page catches up.
        reader(&flow, HIGH_WATER).recv_timeout(WAIT).unwrap();
        flow.ack(2 * HIGH_WATER - LOW_WATER);
        assert!(reader(&flow, HIGH_WATER).recv_timeout(WAIT).is_err());
    }

    #[test]
    fn a_closed_session_lets_its_waiting_reader_go() {
        let flow = Arc::new(Flow::default());
        let paused = reader(&flow, HIGH_WATER);
        assert!(paused.recv_timeout(WAIT).is_err());
        flow.close();
        paused.recv_timeout(WAIT).unwrap();
        // An ack for more than was counted (output sent before the count began) doesn't wrap.
        flow.ack(usize::MAX);
        reader(&flow, 1).recv_timeout(WAIT).unwrap();
    }

    /// On a pty as the panes' shells are, but `sh -i` with no rc files: a login shell's took
    /// over 15 s under load.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn the_job_leader_is_the_command_running_and_none_at_the_prompt() {
        use portable_pty::{native_pty_system, CommandBuilder};
        let pair =
            crate::process::spawning(|| native_pty_system().openpty(super::size(80, 24))).unwrap();
        let mut cmd = CommandBuilder::new("/bin/sh");
        cmd.arg("-i");
        cmd.env_clear();
        cmd.env("PATH", "/usr/bin:/bin");
        cmd.env("PS1", "$ ");
        let mut child = crate::process::spawning(|| pair.slave.spawn_command(cmd)).unwrap();
        drop(pair.slave);
        let shell = child.process_id();
        let mut writer = pair.master.take_writer().unwrap();
        // Read, so the shell's writes never block on a full pty.
        let mut reader = pair.master.try_clone_reader().unwrap();
        std::thread::spawn(move || std::io::copy(&mut reader, &mut std::io::sink()));
        let argv = || {
            let pid = crate::procinfo::job_leader(pair.master.process_group_leader(), shell)?;
            Some(crate::procinfo::process(pid)?.argv)
        };
        let wait = |want: Option<Vec<String>>| {
            let until = std::time::Instant::now() + Duration::from_secs(60);
            while argv() != want && std::time::Instant::now() < until {
                std::thread::sleep(Duration::from_millis(50));
            }
            argv()
        };
        writer.write_all(b"sleep 30\n").unwrap();
        let sleeping = Some(vec!["sleep".to_string(), "30".to_string()]);
        assert_eq!(wait(sleeping.clone()), sleeping);
        writer.write_all(b"\x03").unwrap();
        assert_eq!(wait(None), None);
        let _ = child.kill();
        let _ = child.wait();
    }

    #[test]
    fn binary_input_is_a_byte_a_char_and_text_is_utf8() {
        // A click at column 200, row 1, in X10's encoding: 32 + 200 is past 127.
        #[cfg(not(windows))]
        assert_eq!(&*input_bytes("\x1b[M \u{e8}!", true), b"\x1b[M \xe8!");
        assert_eq!(&*input_bytes("é", false), "é".as_bytes());
    }

    #[test]
    fn a_gone_folder_starts_in_the_nearest_one_left() {
        let tmp = std::env::temp_dir();
        assert_eq!(start_dir(&tmp), Some(tmp.clone()));
        let gone = tmp.join("gitviber-gone-worktree").join("sub");
        assert_eq!(start_dir(&gone), Some(tmp));
    }

    #[cfg(unix)]
    #[test]
    fn the_command_folder_goes_last_on_path_and_only_once() {
        use super::with_last;
        let bin = std::path::Path::new("/Applications/GitViber.app/Contents/Resources/bin");
        let path = with_last("/usr/bin:/bin".as_ref(), bin);
        assert_eq!(path, *format!("/usr/bin:/bin:{}", bin.display()));
        assert_eq!(with_last(&path, bin), path);
        assert_eq!(with_last("".as_ref(), bin), bin.as_os_str());
    }

    /// 5,000 entries (~200 KB) with duplicates and empty ones (the current folder) kept as they
    /// were, the folder once at the end; already on it anywhere, nothing changes.
    #[cfg(unix)]
    #[test]
    fn a_huge_path_keeps_every_entry_as_it_was() {
        use super::with_last;
        let bin = std::path::Path::new("/Applications/My Apps/GitViber.app/Contents/Resources/bin");
        let entries: Vec<String> = (0..5000)
            .map(|i| match i % 100 {
                0 => String::new(),
                1 => "/usr/bin".into(),
                _ => format!("/opt/tools/a-rather-long-folder-name/{i:05}/bin"),
            })
            .collect();
        let path = entries.join(":");
        assert!(path.len() > 200_000);
        let out = with_last(path.as_ref(), bin);
        assert_eq!(out, *format!("{path}:{}", bin.display()));
        assert_eq!(with_last(&out, bin), out);
        let middle = format!("{}:{}:{}", &path[..1000], bin.display(), &path[1000..]);
        assert_eq!(with_last(middle.as_ref(), bin), *middle);
    }
}
