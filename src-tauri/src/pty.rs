//! Shells for the terminal panel. Each session is a login shell on a pseudo-terminal; its
//! output streams to the UI as raw bytes (xterm.js decodes UTF-8 split across chunks).

use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
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

struct Session {
    master: Box<dyn MasterPty + Send>,
    writer: Writer,
    killer: Box<dyn ChildKiller + Send + Sync>,
    shell: Option<u32>,
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
    /// Starts the user's login shell in `cwd`. `exit` gets how it ended once it's gone.
    pub fn spawn(
        &self,
        cwd: &Path,
        cols: u16,
        rows: u16,
        output: Channel<Response>,
        exit: Channel<Option<Exit>>,
    ) -> Result<u32, String> {
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
        // The user's login shell, like Terminal.app: a Finder-launched app has a bare PATH.
        let mut cmd = CommandBuilder::new_default_prog();
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
        let mut child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let writer = pair.master.take_writer().map_err(|e| e.to_string())?;

        let id = self.next.fetch_add(1, Ordering::Relaxed);
        self.sessions.lock().unwrap().insert(
            id,
            Session {
                master: pair.master,
                writer: Arc::new(Mutex::new(writer)),
                killer: child.clone_killer(),
                shell: child.process_id(),
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
                    }
                }
            }
            let status = child.wait().ok().map(|s| match s.signal() {
                Some(signal) => Exit {
                    code: None,
                    signal: Some(signal.to_string()),
                },
                None => Exit {
                    code: Some(s.exit_code()),
                    signal: None,
                },
            });
            sessions.lock().unwrap().remove(&id);
            let _ = exit.send(status);
        });
        Ok(id)
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

    pub fn resize(&self, id: u32, cols: u16, rows: u16) -> Result<(), String> {
        self.with(id, |s| {
            s.master.resize(size(cols, rows)).map_err(|e| e.to_string())
        })
    }

    /// Hangs up the shell. Its reader thread then sees EOF and reports the exit.
    pub fn kill(&self, id: u32) {
        let session = self.sessions.lock().unwrap().remove(&id);
        if let Some(mut s) = session {
            let _ = s.killer.kill();
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

    /// A reloaded page has lost every terminal it had; without this their shells run on unseen.
    pub fn kill_all(&self) {
        let sessions: Vec<Session> = self
            .sessions
            .lock()
            .unwrap()
            .drain()
            .map(|(_, s)| s)
            .collect();
        for mut s in sessions {
            let _ = s.killer.kill();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::start_dir;

    #[test]
    fn a_gone_folder_starts_in_the_nearest_one_left() {
        let tmp = std::env::temp_dir();
        assert_eq!(start_dir(&tmp), Some(tmp.clone()));
        let gone = tmp.join("gitviber-gone-worktree").join("sub");
        assert_eq!(start_dir(&gone), Some(tmp));
    }
}
