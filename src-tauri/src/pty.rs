//! Shells for the terminal panel. Each session is a login shell on a pseudo-terminal; its
//! output streams to the UI as raw bytes (xterm.js decodes UTF-8 split across chunks).

use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use tauri::ipc::{Channel, Response};

/// Its own lock: a program that isn't reading blocks the write once the pty's buffer fills, and
/// the other panes' input, resizes and kills must not wait behind it.
pub type Writer = Arc<Mutex<Box<dyn Write + Send>>>;

/// Blocks until the program reads the input; once its shell is gone, the pty fails the write.
pub fn write(writer: &Writer, data: &str) -> Result<(), String> {
    let mut w = writer.lock().unwrap_or_else(|e| e.into_inner());
    w.write_all(data.as_bytes())
        .and_then(|_| w.flush())
        .map_err(|e| e.to_string())
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

#[derive(Default)]
struct Flow {
    state: Mutex<FlowState>,
    resumed: Condvar,
}

#[derive(Default)]
struct FlowState {
    unacked: usize,
    closed: bool,
}

impl Flow {
    /// Counts `n` bytes sent; past the high water, waits for the page's acks (or the session's end).
    fn sent(&self, n: usize) {
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        s.unacked += n;
        if s.unacked >= HIGH_WATER {
            while s.unacked > LOW_WATER && !s.closed {
                s = self.resumed.wait(s).unwrap_or_else(|e| e.into_inner());
            }
        }
    }

    /// `n` bytes the page has parsed.
    fn ack(&self, n: usize) {
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        s.unacked = s.unacked.saturating_sub(n);
        if s.unacked <= LOW_WATER {
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
        let pair = native_pty_system()
            .openpty(size(cols, rows))
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
        let mut child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let writer = pair.master.take_writer().map_err(|e| e.to_string())?;

        let id = self.next.fetch_add(1, Ordering::Relaxed);
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
            let leader = s.master.process_group_leader();
            ids.is_none_or(|ids| ids.contains(id))
                && leader.is_some_and(|pid| Some(pid as u32) != s.shell)
        };
        sessions.iter().filter(running).count()
    }

    #[cfg(not(unix))]
    pub fn busy(&self, _ids: Option<&[u32]>) -> usize {
        0
    }

    /// The folder a session's shell is in now, asked of the process as VS Code does for a split
    /// (its own, not the foreground job's): once, on a split or a save, never polled.
    pub fn cwd(&self, id: u32) -> Option<PathBuf> {
        let pid = self.with(id, |s| Ok(s.shell)).ok()??;
        process_cwd(pid)
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

#[cfg(target_os = "macos")]
fn process_cwd(pid: u32) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStrExt;
    let mut info: libc::proc_vnodepathinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_vnodepathinfo>() as libc::c_int;
    let got = unsafe {
        libc::proc_pidinfo(
            pid as libc::c_int,
            libc::PROC_PIDVNODEPATHINFO,
            0,
            (&mut info as *mut libc::proc_vnodepathinfo).cast(),
            size,
        )
    };
    if got != size {
        return None;
    }
    // libc declares the path as 32 rows of 32 chars; it's one MAXPATHLEN buffer.
    let raw = &info.pvi_cdir.vip_path;
    let bytes: &[u8] =
        unsafe { std::slice::from_raw_parts(raw.as_ptr().cast(), std::mem::size_of_val(raw)) };
    let path = std::ffi::CStr::from_bytes_until_nul(bytes).ok()?;
    let path = Path::new(std::ffi::OsStr::from_bytes(path.to_bytes()));
    path.is_absolute().then(|| path.to_path_buf())
}

#[cfg(target_os = "linux")]
fn process_cwd(pid: u32) -> Option<PathBuf> {
    std::fs::read_link(format!("/proc/{pid}/cwd")).ok()
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn process_cwd(_pid: u32) -> Option<PathBuf> {
    None
}

#[cfg(test)]
mod tests {
    use super::{start_dir, Flow, HIGH_WATER, LOW_WATER};
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

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn a_process_folder_is_read() {
        let here = std::env::current_dir().unwrap().canonicalize().unwrap();
        let read = super::process_cwd(std::process::id()).unwrap();
        assert_eq!(read.canonicalize().unwrap(), here);
    }

    #[test]
    fn a_gone_folder_starts_in_the_nearest_one_left() {
        let tmp = std::env::temp_dir();
        assert_eq!(start_dir(&tmp), Some(tmp.clone()));
        let gone = tmp.join("gitviber-gone-worktree").join("sub");
        assert_eq!(start_dir(&gone), Some(tmp));
    }
}
