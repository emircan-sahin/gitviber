//! A stand-in for Claude Code, run as each stress-test terminal's shell (the test binary started
//! again on this test alone). Like Claude Code it turns on focus reports, bracketed paste and
//! synchronized output, asks the terminal who it is and what colors it has, and redraws a spinner
//! and a box-drawn input with cursor-up and erase-line several times a second, all of it again
//! on SIGWINCH. Everything it gets on its tty, and every size it's given, goes to `<pid>.ev`.

use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// The environment variable naming the folder the agents log into.
pub(super) const DIR_VAR: &str = "GITVIBER_STRESS_AGENT_DIR";
/// The test the wrapper script runs.
pub(super) const TEST_NAME: &str = "stress::agent::fake_agent";
/// The file whose content (a number) asks every agent to stop moving and write its screen.
pub(super) const FREEZE: &str = "freeze";

const SPINNER: [char; 6] = ['·', '✢', '✶', '✻', '✽', '✶'];
const QUERIES: &str = "\x1b[>0q\x1b[c\x1b]11;?\x1b\\\x1b[6n";

#[test]
#[ignore = "the stress test's fake agent: it runs inside the test's own terminals"]
fn fake_agent() {
    if let Some(dir) = std::env::var_os(DIR_VAR) {
        run(Path::new(&dir));
    }
}

static WINCH: AtomicUsize = AtomicUsize::new(0);

extern "C" fn on_winch(_: libc::c_int) {
    WINCH.fetch_add(1, Ordering::Relaxed);
}

pub(super) fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis())
}

/// Columns a character takes, for the few wide ones drawn here.
fn width(c: char) -> usize {
    if matches!(c, '日' | '本' | '語' | '🚀') {
        2
    } else {
        1
    }
}

/// `s` cut to `cols` columns.
fn fit(s: &str, cols: usize) -> String {
    let mut used = 0;
    s.chars()
        .take_while(|&c| {
            used += width(c);
            used <= cols
        })
        .collect()
}

/// What arrives on the tty, one piece at a time.
#[derive(Debug, PartialEq)]
pub(super) enum Token {
    FocusIn,
    FocusOut,
    Paste(String),
    Text(String),
    Da(String),
    XtVersion(String),
    Osc11(String),
    Cpr,
    DsrOk,
    Other(Vec<u8>),
}

/// The first token in `b` and its length; None while it isn't whole yet.
pub(super) fn next_token(b: &[u8]) -> Option<(Token, usize)> {
    let find = |from: usize, end: &[u8]| {
        b[from..]
            .windows(end.len())
            .position(|w| w == end)
            .map(|i| from + i)
    };
    let text = |r: &[u8]| String::from_utf8_lossy(r).into_owned();
    if b.is_empty() {
        return None;
    }
    if b[0] != 0x1b {
        let end = b.iter().position(|&c| c == 0x1b).unwrap_or(b.len());
        let valid = match std::str::from_utf8(&b[..end]) {
            Ok(_) => end,
            Err(e) if e.error_len().is_none() && e.valid_up_to() > 0 => e.valid_up_to(),
            Err(e) if e.error_len().is_none() => return None,
            Err(_) => return Some((Token::Other(b[..end].to_vec()), end)),
        };
        let s = &b[..valid];
        return Some(if s.iter().any(|&c| c < 0x20 || c == 0x7f) {
            (Token::Other(s.to_vec()), valid)
        } else {
            (Token::Text(text(s)), valid)
        });
    }
    if b.len() < 2 {
        return None;
    }
    match b[1] {
        b'[' => {
            let mut i = 2;
            while i < b.len() && !(0x40..=0x7e).contains(&b[i]) {
                i += 1;
            }
            if i == b.len() {
                return None;
            }
            let (params, last) = (&b[2..i], b[i]);
            let tok = match (params, last) {
                (b"", b'I') => Token::FocusIn,
                (b"", b'O') => Token::FocusOut,
                (b"0", b'n') => Token::DsrOk,
                (b"200", b'~') => {
                    let end = find(i + 1, b"\x1b[201~")?;
                    return Some((Token::Paste(text(&b[i + 1..end])), end + 6));
                }
                ([b'?', ..], b'c') => Token::Da(text(params)),
                (_, b'R') if params.iter().all(|c| c.is_ascii_digit() || *c == b';') => Token::Cpr,
                _ => Token::Other(b[..=i].to_vec()),
            };
            Some((tok, i + 1))
        }
        b'P' => {
            let end = find(2, b"\x1b\\")?;
            let body = &b[2..end];
            let tok = match body.strip_prefix(b">|") {
                Some(name) => Token::XtVersion(text(name)),
                None => Token::Other(b[..end + 2].to_vec()),
            };
            Some((tok, end + 2))
        }
        b']' => {
            let bel = b.iter().position(|&c| c == 0x07);
            let st = find(2, b"\x1b\\");
            let (end, len) = match (bel, st) {
                (Some(a), Some(s)) if a < s => (a, 1),
                (_, Some(s)) => (s, 2),
                (Some(a), None) => (a, 1),
                (None, None) => return None,
            };
            let body = &b[2..end];
            let tok = match body.strip_prefix(b"11;") {
                Some(color) => Token::Osc11(text(color)),
                None => Token::Other(b[..end + len].to_vec()),
            };
            Some((tok, end + len))
        }
        _ => Some((Token::Other(b[..2].to_vec()), 2)),
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Focus1004 {
    On,
    /// Turned off, with a DSR behind it: reports may still come until its answer does.
    Leaving,
    Off,
}

struct Agent {
    tty: std::fs::File,
    log: std::fs::File,
    dir: PathBuf,
    pid: u32,
    cols: usize,
    rows: usize,
    /// Printed above the input, top first, since the last full redraw cleared the screen.
    lines: Vec<String>,
    input: String,
    frame: u64,
    started: Instant,
    mode: Focus1004,
    /// Agents with an odd pid turn focus reports off and on again now and then.
    toggles: bool,
    frozen: Option<String>,
}

impl Agent {
    fn ev(&mut self, what: &str) {
        let _ = writeln!(self.log, "{} {what}", now_ms());
    }

    fn put(&mut self, s: &str) {
        let _ = self.tty.write_all(s.as_bytes());
    }

    fn block(&self) -> Vec<String> {
        let w = self.cols.saturating_sub(1).max(12);
        let spin = SPINNER[(self.frame % SPINNER.len() as u64) as usize];
        let secs = self.started.elapsed().as_secs();
        let room = w - 6;
        let chars: Vec<char> = self.input.chars().collect();
        let tail: String = chars[chars.len().saturating_sub(room)..].iter().collect();
        let pad = room - tail.chars().map(width).sum::<usize>().min(room);
        vec![
            fit(
                &format!("{spin} Working… ({secs}s · esc to interrupt) 日本語 🚀"),
                w,
            ),
            format!("╭{}╮", "─".repeat(w - 2)),
            format!("│ > {tail}{} │", " ".repeat(pad)),
            format!("╰{}╯", "─".repeat(w - 2)),
            fit(
                &format!(
                    "  ? for shortcuts · agent {} · {}x{} · frame {}",
                    self.pid, self.cols, self.rows, self.frame
                ),
                w,
            ),
        ]
    }

    /// The screen as the agent drew it: the last rows of what it printed since the clear.
    fn screen(&self) -> Vec<String> {
        let mut all = self.lines.clone();
        all.extend(self.block());
        all.split_off(all.len().saturating_sub(self.rows))
    }

    /// Cursor-up and erase over the input block, as Ink redraws: the cursor ends at its start.
    fn erase_block(&self) -> String {
        let n = self.block().len();
        let mut s = String::from("\x1b[2K");
        for _ in 1..n {
            s.push_str("\x1b[1A\x1b[2K");
        }
        s.push('\r');
        s
    }

    fn redraw_block(&mut self) {
        let erase = self.erase_block();
        self.frame += 1;
        let block = self.block().join("\r\n");
        self.put(&format!("\x1b[?2026h{erase}{block}\x1b[?2026l"));
        self.wrote();
    }

    fn print_line(&mut self) {
        let erase = self.erase_block();
        let n = self.lines.len();
        let line = fit(
            &format!("⏺ step {n}: read src/lib.rs · 日本語 🚀 done"),
            self.cols.saturating_sub(1),
        );
        self.lines.push(line.clone());
        if self.lines.len() > 200 {
            self.lines.drain(..100);
        }
        self.frame += 1;
        let colored = line.replacen('⏺', "\x1b[32m⏺\x1b[0m", 1);
        let block = self.block().join("\r\n");
        self.put(&format!(
            "\x1b[?2026h{erase}{colored}\r\n{block}\x1b[?2026l"
        ));
        self.wrote();
    }

    fn redraw_all(&mut self) {
        let w = self.cols.saturating_sub(1);
        for l in &mut self.lines {
            *l = fit(l, w);
        }
        self.frame += 1;
        let mut all = self.lines.clone();
        all.extend(self.block());
        let body = all.join("\r\n");
        self.put(&format!("\x1b[?2026h\x1b[2J\x1b[3J\x1b[H{body}\x1b[?2026l"));
        self.wrote();
    }

    /// While frozen, the screen file follows every redraw.
    fn wrote(&mut self) {
        if let Some(id) = self.frozen.clone() {
            let text = format!("{id}\n{}", self.screen().join("\n"));
            let path = self.dir.join(format!("{}.screen", self.pid));
            let tmp = path.with_extension("tmp");
            if std::fs::write(&tmp, text).is_ok() {
                let _ = std::fs::rename(tmp, path);
            }
        }
    }

    fn token(&mut self, t: Token) {
        let q = |s: &str| format!("{s:?}");
        match t {
            Token::FocusIn | Token::FocusOut => {
                let mode = match self.mode {
                    Focus1004::On => "on",
                    Focus1004::Leaving => "leaving",
                    Focus1004::Off => "off",
                };
                let which = if t == Token::FocusIn { "I" } else { "O" };
                self.ev(&format!("focus {which} {mode}"));
            }
            Token::DsrOk => {
                if self.mode == Focus1004::Leaving {
                    self.mode = Focus1004::Off;
                }
                self.ev("reply dsr");
            }
            Token::Text(s) => {
                self.ev(&format!("text {}", q(&s)));
                self.input.push_str(&s);
                self.redraw_block();
            }
            Token::Paste(s) => {
                self.ev(&format!("paste {}", q(&s)));
                self.input.push_str(&s);
                self.redraw_block();
            }
            Token::Da(s) => self.ev(&format!("reply da {}", q(&s))),
            Token::XtVersion(s) => self.ev(&format!("reply xtversion {}", q(&s))),
            Token::Osc11(s) => self.ev(&format!("reply osc11 {}", q(&s))),
            Token::Cpr => self.ev("reply cpr"),
            Token::Other(b) => self.ev(&format!("other {}", q(&String::from_utf8_lossy(&b)))),
        }
    }
}

fn winsize(fd: i32) -> (usize, usize) {
    // SAFETY: TIOCGWINSZ fills the winsize it's given.
    unsafe {
        let mut ws: libc::winsize = std::mem::zeroed();
        libc::ioctl(fd, libc::TIOCGWINSZ, &mut ws);
        (ws.ws_col as usize, ws.ws_row as usize)
    }
}

fn run(dir: &Path) {
    let Ok(tty) = OpenOptions::new().read(true).write(true).open("/dev/tty") else {
        return;
    };
    let fd = tty.as_raw_fd();
    // SAFETY: plain termios and signal calls on our own tty and process.
    unsafe {
        let mut t: libc::termios = std::mem::zeroed();
        libc::tcgetattr(fd, &mut t);
        libc::cfmakeraw(&mut t);
        libc::tcsetattr(fd, libc::TCSANOW, &t);
        libc::signal(
            libc::SIGWINCH,
            on_winch as extern "C" fn(libc::c_int) as libc::sighandler_t,
        );
    }
    let pid = std::process::id();
    let Ok(log) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(format!("{pid}.ev")))
    else {
        return;
    };
    let (cols, rows) = winsize(fd);
    let mut a = Agent {
        tty,
        log,
        dir: dir.to_path_buf(),
        pid,
        cols,
        rows,
        lines: vec![format!("fake agent {pid}")],
        input: String::new(),
        frame: 0,
        started: Instant::now(),
        mode: Focus1004::On,
        toggles: pid % 2 == 1,
        frozen: None,
    };
    a.ev(&format!("start {cols} {rows} toggles={}", a.toggles));
    a.put(&format!(
        "\x1b]2;agent {pid}\x07\x1b[?1004h\x1b[?2004h\x1b[?25l"
    ));
    a.ev("mode1004 on");
    a.put(QUERIES);
    a.ev("query 4");
    a.redraw_all();

    let mut seen_winch = WINCH.load(Ordering::Relaxed);
    let mut pending: Vec<u8> = Vec::new();
    let mut buf = [0u8; 4096];
    let jitter = (pid % 7) as u64;
    let mut next_frame = Instant::now();
    let mut next_line = Instant::now() + Duration::from_millis(2000 + jitter * 300);
    let mut next_query = Instant::now() + Duration::from_millis(3000 + jitter * 100);
    let mut next_toggle = Instant::now() + Duration::from_millis(4000 + jitter * 200);
    loop {
        // select, not poll: macOS's poll doesn't take a tty (POLLNVAL, then a read that blocks).
        // SAFETY: an fd_set and timeval of our own, for our one fd.
        let ready = unsafe {
            let mut set: libc::fd_set = std::mem::zeroed();
            libc::FD_SET(fd, &mut set);
            let mut wait = libc::timeval {
                tv_sec: 0,
                tv_usec: 15_000,
            };
            libc::select(
                fd + 1,
                &mut set,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut wait,
            ) > 0
                && libc::FD_ISSET(fd, &set)
        };
        if ready {
            match a.tty.read(&mut buf) {
                Ok(0) => std::process::exit(0),
                Ok(n) => {
                    pending.extend_from_slice(&buf[..n]);
                    while let Some((t, used)) = next_token(&pending) {
                        pending.drain(..used);
                        a.token(t);
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => std::process::exit(0),
            }
        }
        let winch = WINCH.load(Ordering::Relaxed);
        if winch != seen_winch {
            let signals = winch - seen_winch;
            seen_winch = winch;
            let (cols, rows) = winsize(fd);
            a.cols = cols;
            a.rows = rows;
            a.ev(&format!("size {cols} {rows} signals={signals}"));
            a.redraw_all();
        }
        let freeze = std::fs::read_to_string(dir.join(FREEZE)).ok();
        if freeze != a.frozen {
            a.frozen = freeze;
            if a.frozen.is_some() {
                a.redraw_block();
            }
        }
        if a.frozen.is_some() {
            continue;
        }
        let now = Instant::now();
        if now >= next_line {
            a.print_line();
            next_line = now + Duration::from_millis(2000 + jitter * 300);
        } else if now >= next_frame {
            a.redraw_block();
            next_frame = now + Duration::from_millis(110 + jitter * 15);
        }
        if now >= next_query {
            a.put(QUERIES);
            a.ev("query 4");
            next_query = now + Duration::from_millis(3000 + jitter * 100);
        }
        if a.toggles && now >= next_toggle {
            if a.mode == Focus1004::On {
                a.put("\x1b[?1004l\x1b[5n");
                a.mode = Focus1004::Leaving;
                a.ev("mode1004 off");
                next_toggle = now + Duration::from_millis(1500);
            } else {
                a.put("\x1b[?1004h");
                a.mode = Focus1004::On;
                a.ev("mode1004 on");
                next_toggle = now + Duration::from_millis(4000 + jitter * 200);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_agent_reads_what_a_terminal_sends() {
        let all = b"ab\x1b[I\x1b[200~p q\x1b[201~\x1b[?6c\x1bP>|GitViber\x1b\\\x1b]11;rgb:1/2/3\x1b\\\x1b[12;4R\x1b[0n\x1b[O\x1b[Zx\r";
        let mut b = all.to_vec();
        let mut got = Vec::new();
        while let Some((t, n)) = next_token(&b) {
            b.drain(..n);
            got.push(t);
        }
        assert!(b.is_empty());
        assert_eq!(
            got,
            vec![
                Token::Text("ab".into()),
                Token::FocusIn,
                Token::Paste("p q".into()),
                Token::Da("?6".into()),
                Token::XtVersion("GitViber".into()),
                Token::Osc11("rgb:1/2/3".into()),
                Token::Cpr,
                Token::DsrOk,
                Token::FocusOut,
                Token::Other(b"\x1b[Z".to_vec()),
                Token::Other(b"x\r".to_vec()),
            ]
        );
        // Split across reads: nothing until it's whole.
        assert_eq!(next_token(b"\x1b[20"), None);
        assert_eq!(next_token(b"\x1b[200~abc"), None);
        assert_eq!(next_token("é".as_bytes()[..1].as_ref()), None);
    }
}
