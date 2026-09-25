//! Shells for the terminal panel. Each session is a login shell on a pseudo-terminal; its
//! output streams to the UI as raw bytes (xterm.js decodes UTF-8 split across chunks).

use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use tauri::ipc::{Channel, Response};

struct Session {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
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

impl Ptys {
    /// Starts the user's login shell in `cwd`. `exit` gets the exit code once it's gone.
    pub fn spawn(
        &self,
        cwd: &Path,
        cols: u16,
        rows: u16,
        output: Channel<Response>,
        exit: Channel<Option<u32>>,
    ) -> Result<u32, String> {
        if !cwd.is_dir() {
            return Err(format!("folder not found: {}", cwd.display()));
        }
        let pair = native_pty_system()
            .openpty(size(cols, rows))
            .map_err(|e| e.to_string())?;
        // The user's login shell, like Terminal.app: a Finder-launched app has a bare PATH.
        let mut cmd = CommandBuilder::new_default_prog();
        cmd.cwd(cwd);
        // Not our environment: with npm_config_prefix from `pnpm tauri dev`, pnpm went missing.
        cmd.env_clear();
        for (key, value) in crate::shell::clean_env() {
            cmd.env(key, value);
        }
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
                writer,
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
            let code = child.wait().ok().map(|s| s.exit_code());
            sessions.lock().unwrap().remove(&id);
            let _ = exit.send(code);
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

    pub fn write(&self, id: u32, data: &str) -> Result<(), String> {
        self.with(id, |s| {
            s.writer
                .write_all(data.as_bytes())
                .and_then(|_| s.writer.flush())
                .map_err(|e| e.to_string())
        })
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

    /// Sessions whose foreground job isn't the shell itself: a command is running there.
    #[cfg(unix)]
    pub fn busy(&self) -> usize {
        let sessions = self.sessions.lock().unwrap();
        let running = |s: &&Session| {
            let leader = s.master.process_group_leader();
            leader.is_some_and(|pid| Some(pid as u32) != s.shell)
        };
        sessions.values().filter(running).count()
    }

    #[cfg(not(unix))]
    pub fn busy(&self) -> usize {
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
