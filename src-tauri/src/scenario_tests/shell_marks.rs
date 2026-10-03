//! The zsh integration's command mark (OSC 133;C;cmdline_url=…) from a real `zsh -i` on a pty,
//! loaded as a pane loads it (ZDOTDIR), with a made-up HOME so no one's own rc files run.
//! The mark must reach xterm.js whole: its parser ends or drops an OSC at BEL, ESC, CAN, SUB
//! and every C1 control (U+0080..U+009F), and whatever follows is then printed or run.

use super::*;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::{Read, Write};
use std::sync::Arc;
use std::time::{Duration, Instant};

const A: &str = "\x1b]133;A\x07";

struct Zsh {
    out: Arc<Mutex<Vec<u8>>>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
    _master: Box<dyn portable_pty::MasterPty + Send>,
    _sb: Sandbox,
}

impl Zsh {
    /// None where there's no zsh (a Linux runner without it).
    fn start(name: &str, zshrc: &str, env: &[(&str, &str)]) -> Option<Zsh> {
        let zsh = ["/bin/zsh", "/usr/bin/zsh"]
            .into_iter()
            .find(|p| Path::new(p).exists())?;
        let sb = Sandbox::new(name);
        let home = sb.path("home");
        fs::create_dir_all(&home).unwrap();
        fs::write(
            home.join(".zshrc"),
            format!("PS1='%% '\nHISTFILE=\n{zshrc}\n"),
        )
        .unwrap();
        let inject = crate::shell_integration::injection(Path::new(zsh), &sb.path("si")).unwrap();
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: 24,
                cols: 200,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let mut cmd = CommandBuilder::new(zsh);
        cmd.arg("-i");
        cmd.cwd(&home);
        cmd.env_clear();
        cmd.env("PATH", "/usr/bin:/bin");
        cmd.env("TERM", "xterm-256color");
        cmd.env("LANG", "en_US.UTF-8");
        cmd.env("HOME", &home);
        for (k, v) in &inject.env {
            cmd.env(k, v);
        }
        for (k, v) in env {
            cmd.env(k, v);
        }
        let child = pair.slave.spawn_command(cmd).unwrap();
        drop(pair.slave);
        let out = Arc::new(Mutex::new(Vec::new()));
        let mut reader = pair.master.try_clone_reader().unwrap();
        let sink = out.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 {
                    break;
                }
                sink.lock().unwrap().extend_from_slice(&buf[..n]);
            }
        });
        let writer = pair.master.take_writer().unwrap();
        let mut z = Zsh {
            out,
            writer,
            child,
            _master: pair.master,
            _sb: sb,
        };
        z.prompts(1);
        Some(z)
    }

    fn text(&self) -> String {
        String::from_utf8_lossy(&self.out.lock().unwrap()).into_owned()
    }

    /// Waits for the `n`th prompt mark.
    fn prompts(&mut self, n: usize) {
        let until = Instant::now() + Duration::from_secs(30);
        while self.text().matches(A).count() < n {
            assert!(Instant::now() < until, "no prompt {n}: {:?}", self.text());
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Types `line` (Enter included) and returns what the C mark carried, up to the BEL that
    /// should end it.
    fn run(&mut self, line: &[u8]) -> String {
        let before = self.text().matches(A).count();
        for chunk in line.chunks(256) {
            self.writer.write_all(chunk).unwrap();
        }
        self.prompts(before + 1);
        let text = self.text();
        let from = text.rfind("\x1b]133;C").expect("a C mark") + "\x1b]133;C".len();
        let len = text[from..].find('\x07').expect("a BEL");
        text[from..from + len].to_string()
    }
}

impl Drop for Zsh {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The command line a mark's `;cmdline_url=…` carries, percent-decoded; None without one.
fn decoded(mark: &str) -> Option<String> {
    let url = mark.strip_prefix(";cmdline_url=")?.as_bytes();
    let mut out = vec![];
    let mut i = 0;
    while i < url.len() {
        if url[i] == b'%' {
            let hex = std::str::from_utf8(&url[i + 1..i + 3]).unwrap();
            out.push(u8::from_str_radix(hex, 16).unwrap());
            i += 3;
        } else {
            out.push(url[i]);
            i += 1;
        }
    }
    Some(String::from_utf8_lossy(&out).into_owned())
}

/// What would end or drop the OSC in xterm.js before its BEL.
fn breaks_osc(c: char) -> bool {
    matches!(c as u32, 0x07 | 0x18 | 0x1a | 0x1b | 0x80..=0x9f)
}

fn assert_whole(mark: &str, want: &str) {
    assert!(
        !mark.chars().any(breaks_osc),
        "the mark would be cut short: {mark:?}"
    );
    assert_eq!(decoded(mark).as_deref(), Some(want));
}

/// Control characters typed into the line (^V ESC, ^V BEL, ^V ^X, a newline in a loop) are
/// spaces; `;`, `\`, `%` and quotes go as typed, percent-encoded.
#[test]
fn a_command_line_with_controls_in_it_reaches_the_terminal_whole() {
    let Some(mut z) = Zsh::start("marks-controls", "", &[]) else {
        return;
    };
    assert_whole(&z.run(b"echo hi\r"), "echo hi");
    // Kept out of the history, so out of the mark.
    assert_eq!(z.run(b" echo secret\r"), "");
    assert_whole(&z.run(b"echo 'a;b=c' ; true\r"), "echo 'a;b=c' ; true");
    assert_whole(
        &z.run(b"echo '\x16\x1b]0;title\x16\x07' tail\r"),
        "echo ' ]0;title ' tail",
    );
    assert_whole(&z.run(b": 'a\x16\x18b\x16\x1ac' tail\r"), ": 'a b c' tail");
    assert_whole(
        &z.run(b"for i in 1 2\rdo echo $i\rdone\r"),
        "for i in 1 2 do echo $i done",
    );
    assert_whole(
        &z.run(b"echo \\e\\a %F{red} 100%d\r"),
        "echo \\e\\a %F{red} 100%d",
    );
    // A C1 control pasted in: zsh in a UTF-8 locale counts it a control too.
    assert_whole(
        &z.run(b": '\xc2\x9b2J\xc2\x9c\xc2\x9d0;x' tail\r"),
        ": ' 2J  0;x' tail",
    );
    // UTF-8 whose continuation bytes are in 0x80..0x9F (日 本 are E6 97 A5, E6 9C AC) stays whole.
    assert_whole(&z.run("echo 日本 ş\r".as_bytes()), "echo 日本 ş");
    let long = z.run(&[b"echo ".as_slice(), &[b'x'; 5000], b"\r"].concat());
    assert_whole(&long, &format!("echo {}", "x".repeat(195)));
    let wide = z.run(&[b"echo ".as_slice(), "ş".repeat(300).as_bytes(), b"\r"].concat());
    assert_whole(&wide, &format!("echo {}", "ş".repeat(195)));
}

/// With LC_ALL=C (set by some for sort order or old tools) zsh reads bytes, and a C1 control
/// pasted into a line (U+009B, U+009C, U+009D) went out raw: xterm.js took U+009C as the mark's
/// end and printed the rest with a bell (the tab's dot), U+009B ran the rest as CSI (`2J` cleared
/// the screen), U+009D set the window title, and the command had no mark at all.
#[test]
fn a_c1_control_in_the_line_never_breaks_the_mark_in_the_c_locale() {
    let Some(mut z) = Zsh::start("marks-c-locale", "", &[("LC_ALL", "C")]) else {
        return;
    };
    for (line, want) in [
        (b": 'x\xc2\x9cy' tail\r".as_slice(), ": 'x y' tail"),
        (b": '\xc2\x9b2J' tail\r", ": ' 2J' tail"),
        (b": '\xc2\x9d0;title' tail\r", ": ' 0;title' tail"),
    ] {
        let mark = z.run(line);
        assert!(
            !mark.chars().any(breaks_osc),
            "{want:?} came out as {mark:?}"
        );
    }
}

/// `setopt ksh_arrays` (kept by some from ksh, or set by a plugin) makes subscripts start at 0:
/// `${1[1,200]}` dropped the command's first character ("cho hi").
#[test]
fn ksh_arrays_keeps_the_whole_command() {
    let Some(mut z) = Zsh::start("marks-ksh", "setopt ksh_arrays", &[]) else {
        return;
    };
    assert_whole(&z.run(b"echo hi\r"), "echo hi");
}
