//! Network commands (fetch, pull, push, clone). git's progress streams to the page as it
//! comes, and the user can stop one while it transfers. A transfer that keeps reporting runs
//! as long as it needs; only silence times out.

use crate::{askpass, process};
use serde::Serialize;
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The error of a command the user stopped; the page shows it as neutral, not as a failure.
pub const CANCELLED: &str = "git:cancelled";

/// A dead connection or a hanging proxy goes quiet; a live transfer reports every second.
const SILENCE_TIMEOUT: Duration = Duration::from_secs(300);
/// Stopping sends SIGTERM first and waits this long, so git can remove its lock files and a
/// half-made clone.
const STOP_GRACE: Duration = Duration::from_secs(3);

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Progress {
    /// git's own words: "Receiving objects", "Resolving deltas", …
    pub phase: String,
    /// None for phases git only counts ("Enumerating objects: 1234").
    pub percent: Option<u8>,
    /// False once git changes local files: stopped then, it would leave them half updated.
    pub cancellable: bool,
}

const TRANSFER_PHASES: &[&str] = &[
    "Enumerating objects",
    "Counting objects",
    "Compressing objects",
    "Receiving objects",
    "Resolving deltas",
    "Writing objects",
    "Unpacking objects",
];
/// The checkout after a clone or a pull's merge.
const LOCAL_PHASE: &str = "Updating files";

/// One `\r`- or `\n`-ended piece of git's stderr under `--progress`, such as
/// "Receiving objects:  45% (450/1000), 1.20 MiB | 800.00 KiB/s" or the server's
/// "remote: Counting objects: 100% (5/5), done.". Anything else is None.
pub fn parse_progress(line: &str) -> Option<Progress> {
    let line = line.trim();
    let line = line.strip_prefix("remote:").map_or(line, str::trim_start);
    let (phase, rest) = line.split_once(':')?;
    let cancellable = TRANSFER_PHASES.contains(&phase);
    if !cancellable && phase != LOCAL_PHASE {
        return None;
    }
    let percent = rest
        .split_once('%')
        .and_then(|(n, _)| n.trim().parse::<u8>().ok())
        .filter(|p| *p <= 100);
    Some(Progress {
        phase: phase.to_string(),
        percent,
        cancellable,
    })
}

// A command's state: running and stoppable, stopped by the user, or past the point where
// stopping it is safe. Only a running one moves to either of the others.
const RUNNING: u8 = 0;
const CANCELLED_STATE: u8 = 1;
const SETTLED: u8 = 2;

type Registry = Arc<Mutex<HashMap<String, Arc<AtomicU8>>>>;

/// How a network command is watched: where its progress goes and the flag that stops it.
/// `Net::default()` just runs the command. A registered one leaves the registry when dropped.
#[derive(Default)]
pub struct Net {
    state: Arc<AtomicU8>,
    progress: Option<Box<dyn Fn(Progress) + Send + Sync>>,
    registered: Option<(Registry, String)>,
}

impl Net {
    /// The page's id for this command, if it registered one.
    fn op(&self) -> Option<&str> {
        self.registered.as_ref().map(|(_, id)| id.as_str())
    }

    pub fn cancelled(&self) -> bool {
        self.state.load(Ordering::Relaxed) == CANCELLED_STATE
    }

    /// From here on Cancel is ignored. True only for the call that settled it.
    fn settle(&self) -> bool {
        self.state
            .compare_exchange(RUNNING, SETTLED, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
    }

    fn report(&self, p: Progress) {
        if let Some(f) = &self.progress {
            f(p);
        }
    }
}

impl Drop for Net {
    fn drop(&mut self) {
        if let Some((map, id)) = &self.registered {
            let mut map = map.lock().unwrap();
            // A later command may have taken the same id (a force push after a rejected push).
            if map.get(id).is_some_and(|c| Arc::ptr_eq(c, &self.state)) {
                map.remove(id);
            }
        }
    }
}

/// Network commands in flight, by the id the page gave each, so Cancel stops exactly that one.
#[derive(Default)]
pub struct Running(Registry);

impl Running {
    pub fn start(&self, id: String, progress: impl Fn(Progress) + Send + Sync + 'static) -> Net {
        let state = Arc::new(AtomicU8::new(RUNNING));
        self.0.lock().unwrap().insert(id.clone(), state.clone());
        Net {
            state,
            progress: Some(Box::new(progress)),
            registered: Some((self.0.clone(), id)),
        }
    }

    /// Ignored once the command is changing local files.
    pub fn cancel(&self, id: &str) {
        if let Some(s) = self.0.lock().unwrap().get(id) {
            let _ = s.compare_exchange(
                RUNNING,
                CANCELLED_STATE,
                Ordering::Relaxed,
                Ordering::Relaxed,
            );
        }
    }
}

/// Runs a prepared git command until it exits, is cancelled or goes silent. Progress goes to
/// `net`; the rest of stderr is kept for the error message. `settle_on`: a file whose change
/// means the transfer is over and local work began (a pull's FETCH_HEAD, written before its
/// merge or rebase), which git may not announce at all.
pub fn run(
    mut cmd: Command,
    label: &str,
    net: &Net,
    settle_on: Option<&Path>,
) -> Result<Vec<u8>, String> {
    if net.cancelled() {
        return Err(CANCELLED.into());
    }
    // Its own process group, so stopping it also stops the ssh or remote helper it started,
    // and the askpass helper either of them is waiting on, which closes the dialog.
    process::in_own_group(&mut cmd);
    let asking = askpass::attach(&mut cmd, label, net.op());
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("could not run {label}: {e}"))?;
    let (stdout, stderr) = (child.stdout.take(), child.stderr.take());
    let modified = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    let marker = settle_on.map(|p| (p, modified(p)));
    let start = Instant::now();
    // When output last arrived, in milliseconds since start.
    let heard = AtomicU64::new(0);
    let touch = || heard.store(start.elapsed().as_millis() as u64, Ordering::Relaxed);

    std::thread::scope(|s| {
        let out = s.spawn(|| {
            let mut buf = Vec::new();
            if let Some(mut r) = stdout {
                let mut chunk = [0u8; 8192];
                while let Ok(n @ 1..) = r.read(&mut chunk) {
                    touch();
                    buf.extend_from_slice(&chunk[..n]);
                }
            }
            buf
        });
        let err = s.spawn(|| read_stderr(stderr, net, &touch));

        let status = loop {
            match child.try_wait() {
                Ok(Some(st)) => break Ok(st),
                Ok(None) => {}
                Err(e) => {
                    process::kill_group(&mut child, STOP_GRACE);
                    break Err(e.to_string());
                }
            }
            if net.cancelled() {
                process::kill_group(&mut child, STOP_GRACE);
                break Err(CANCELLED.to_string());
            }
            if let Some((path, before)) = marker {
                if modified(path) != before && net.settle() {
                    net.report(Progress {
                        phase: "Updating the branch".into(),
                        percent: None,
                        cancellable: false,
                    });
                }
            }
            // An open prompt waits for the user, not the remote.
            if asking.as_ref().is_some_and(askpass::Asking::prompting) {
                touch();
            }
            let quiet = start
                .elapsed()
                .saturating_sub(Duration::from_millis(heard.load(Ordering::Relaxed)));
            if quiet > SILENCE_TIMEOUT {
                process::kill_group(&mut child, STOP_GRACE);
                break Err(format!("{label} timed out: no response for 5 minutes"));
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        let (stdout, stderr) = (
            out.join().unwrap_or_default(),
            err.join().unwrap_or_default(),
        );
        let status = status?;
        if status.success() {
            return Ok(stdout);
        }
        let e = Some(stderr.trim().to_string())
            .filter(|e| !e.is_empty())
            .unwrap_or_else(|| String::from_utf8_lossy(&stdout).trim().to_string());
        Err(if e.is_empty() {
            format!("{label} failed ({})", status.code().unwrap_or(-1))
        } else {
            e
        })
    })
}

/// git redraws a progress line with `\r` and ends it with `\n`; each piece is either progress
/// (reported when it changed) or text worth keeping.
fn read_stderr(pipe: Option<impl Read>, net: &Net, touch: &dyn Fn()) -> String {
    let mut kept = String::new();
    let Some(mut r) = pipe else {
        return kept;
    };
    let mut last: Option<Progress> = None;
    let mut piece = |bytes: &[u8]| {
        let text = String::from_utf8_lossy(bytes);
        match parse_progress(&text) {
            Some(p) if last.as_ref() != Some(&p) => {
                if !p.cancellable {
                    net.settle();
                }
                last = Some(p.clone());
                net.report(p);
            }
            Some(_) => {}
            None if text.trim().is_empty() => {}
            None => {
                kept.push_str(text.trim_end());
                kept.push('\n');
            }
        }
    };
    let mut pending = Vec::new();
    let mut chunk = [0u8; 8192];
    while let Ok(n @ 1..) = r.read(&mut chunk) {
        touch();
        pending.extend_from_slice(&chunk[..n]);
        while let Some(i) = pending.iter().position(|b| *b == b'\r' || *b == b'\n') {
            piece(&pending[..i]);
            pending.drain(..=i);
        }
    }
    piece(&pending);
    kept
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(phase: &str, percent: Option<u8>) -> Option<Progress> {
        Some(Progress {
            phase: phase.into(),
            percent,
            cancellable: phase != LOCAL_PHASE,
        })
    }

    #[test]
    fn parses_git_progress_lines() {
        assert_eq!(
            parse_progress("Receiving objects:  45% (450/1000), 1.20 MiB | 800.00 KiB/s"),
            p("Receiving objects", Some(45))
        );
        assert_eq!(
            parse_progress("Resolving deltas: 100% (3/3), done."),
            p("Resolving deltas", Some(100))
        );
        assert_eq!(
            parse_progress("remote: Compressing objects:   7% (1/14)        "),
            p("Compressing objects", Some(7))
        );
        assert_eq!(
            parse_progress("Writing objects: 100% (3/3), 256 bytes | 256.00 KiB/s, done."),
            p("Writing objects", Some(100))
        );
        assert_eq!(
            parse_progress("remote: Enumerating objects: 1234, done."),
            p("Enumerating objects", None)
        );
        assert_eq!(
            parse_progress("Counting objects:   3% (3/100)"),
            p("Counting objects", Some(3))
        );
    }

    #[test]
    fn local_work_cant_be_cancelled() {
        let update = parse_progress("Updating files:  40% (4/10)").unwrap();
        assert!(!update.cancellable);
        assert!(
            parse_progress("Receiving objects: 40% (4/10)")
                .unwrap()
                .cancellable
        );

        let running = Running::default();
        let net = running.start("op".into(), |_| {});
        read_stderr(Some(&b"Updating files:  40% (4/10)\r"[..]), &net, &|| {});
        running.cancel("op");
        assert!(
            !net.cancelled(),
            "a cancel after local work began must be ignored"
        );

        let net = running.start("op2".into(), |_| {});
        running.cancel("op2");
        assert!(net.cancelled() && !net.settle());
    }

    /// The pull's marker file changing is the point of no return, even with no progress line.
    #[test]
    fn a_changed_marker_settles_the_command() {
        let dir = std::env::temp_dir().join(format!("gitviber-settle-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let marker = dir.join("FETCH_HEAD");
        let seen = Arc::new(Mutex::new(vec![]));
        let sink = seen.clone();
        let running = Running::default();
        let net = running.start("pull".into(), move |p| sink.lock().unwrap().push(p));
        let mut cmd = Command::new("sh");
        cmd.args([
            "-c",
            &format!("sleep 0.2; echo x > {}; sleep 0.3", marker.display()),
        ]);
        run(cmd, "sh", &net, Some(&marker)).unwrap();
        running.cancel("pull");
        assert!(!net.cancelled());
        assert!(seen.lock().unwrap().iter().any(|p| !p.cancellable));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn leaves_other_output_alone() {
        for line in [
            "",
            "To github.com:me/repo.git",
            " ! [rejected]        main -> main (non-fetch first)",
            "error: failed to push some refs to 'origin'",
            "remote: Total 5 (delta 0), reused 0 (delta 0), pack-reused 0",
            "hint: Updates were rejected because the remote contains work that you do",
            "fatal: Receiving objects is not a thing: 50%",
        ] {
            assert_eq!(parse_progress(line), None, "{line}");
        }
    }

    #[test]
    fn splits_redrawn_lines_and_keeps_the_rest() {
        let seen = Arc::new(Mutex::new(vec![]));
        let sink = seen.clone();
        let net = Running::default().start("t".into(), move |p| sink.lock().unwrap().push(p));
        let stderr = b"Receiving objects:  50% (1/2)\rReceiving objects:  50% (1/2)\rReceiving objects: 100% (2/2), done.\nfatal: something broke\nhint: try again";
        let kept = read_stderr(Some(&stderr[..]), &net, &|| {});
        assert_eq!(kept, "fatal: something broke\nhint: try again\n");
        assert_eq!(
            *seen.lock().unwrap(),
            vec![
                p("Receiving objects", Some(50)).unwrap(),
                p("Receiving objects", Some(100)).unwrap()
            ]
        );
    }

    #[cfg(unix)]
    #[test]
    fn cancel_stops_the_whole_process_group() {
        let dir = std::env::temp_dir().join(format!("gitviber-net-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let pidfile = dir.join("pid");
        // A child that outlives its parent unless the group is stopped, like ssh under git.
        let mut cmd = Command::new("sh");
        cmd.args([
            "-c",
            &format!("sleep 30 & echo $! > {}; wait", pidfile.display()),
        ]);
        let running = Running::default();
        let net = running.start("op".into(), |_| {});
        let started = Instant::now();
        let result = std::thread::scope(|s| {
            s.spawn(|| {
                while std::fs::read_to_string(&pidfile).map_or(true, |s| s.trim().is_empty()) {
                    std::thread::sleep(Duration::from_millis(10));
                }
                running.cancel("op");
            });
            run(cmd, "sh", &net, None)
        });
        assert_eq!(result.unwrap_err(), CANCELLED);
        assert!(started.elapsed() < Duration::from_secs(10));
        let pid: libc::pid_t = std::fs::read_to_string(&pidfile)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        // Orphaned, it lingers as a zombie until launchd reaps it.
        let gone = (0..100).any(|_| {
            std::thread::sleep(Duration::from_millis(20));
            (unsafe { libc::kill(pid, 0) }) != 0
        });
        assert!(gone, "the grandchild still runs");
        drop(net);
        assert!(running.0.lock().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
