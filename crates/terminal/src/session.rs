//! A pane's terminal: alacritty's `Term` fed on the shell's reader thread, its writes and resizes
//! on a thread of their own, and what the UI must hear about through `Outbox`.
//!
//! Ownership runs one way so a closed pane gives everything back at once: the pane owns the
//! `Session`, which owns the `Term` (scrollback included); the reader thread holds only a `Weak`
//! to it and stops at its next read once the pane is gone. Nothing holds the pane.

use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::task::{Context, Poll, Waker};
use std::time::{Duration, Instant};

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::{Config, Term, TermMode};
use alacritty_terminal::vte::ansi::Rgb;
use gitviber_core::{Core, api::app as pty, pty::Exit};

use crate::input::{MAX_OSC52_TEXT, osc52_accepts};
use crate::marks::{self, Effect, MarkKind, Marks};
use crate::scan::{Found, Scanner, cwd_from_url};

/// Wakes the pane's task on the UI thread; wakes before it ran fold into one.
#[derive(Default)]
pub(crate) struct Signal {
    state: Mutex<SignalState>,
}

#[derive(Default)]
struct SignalState {
    set: bool,
    closed: bool,
    waker: Option<Waker>,
}

impl Signal {
    pub fn notify(&self) {
        let waker = {
            let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
            if s.set {
                return;
            }
            s.set = true;
            s.waker.take()
        };
        if let Some(w) = waker {
            w.wake();
        }
    }

    fn close(&self) {
        let waker = {
            let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
            s.closed = true;
            s.waker.take()
        };
        if let Some(w) = waker {
            w.wake();
        }
    }

    /// Resolves true at the next notify, false once closed.
    pub fn wait(&self) -> Wait<'_> {
        Wait(self)
    }
}

pub(crate) struct Wait<'a>(&'a Signal);

impl Future for Wait<'_> {
    type Output = bool;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<bool> {
        let mut s = self.0.state.lock().unwrap_or_else(|e| e.into_inner());
        if s.set {
            s.set = false;
            return Poll::Ready(true);
        }
        if s.closed {
            return Poll::Ready(false);
        }
        s.waker = Some(cx.waker().clone());
        Poll::Pending
    }
}

/// What the UI hears from a session, in order.
#[derive(Debug)]
pub(crate) enum Note {
    Started {
        integrated: bool,
    },
    /// The shell couldn't start; said in the pane.
    Failed,
    Title(Option<String>),
    Bell,
    /// A program's OSC 52 copy.
    Copy(String),
    Prompt,
    Ended(marks::CommandEnd),
    Cwd(String),
    /// A program's notification escape.
    Attention(crate::agent::Note),
    /// The state the program in the pane reported (OSC 7501).
    Status(crate::agent::AgentState),
    Exited {
        exit: Option<Exit>,
        lived: Duration,
    },
}

enum Io {
    Write(String, bool),
    Resize(u16, u16),
}

/// The colors and sizes a program may ask for (OSC 4/10/11/12, CSI 14 t), set from the UI.
#[derive(Clone, Copy)]
pub(crate) struct Answers {
    pub foreground: Rgb,
    pub background: Rgb,
    pub cursor: Rgb,
    pub ansi: [Rgb; 16],
    pub size: WindowSize,
}

impl Default for Answers {
    fn default() -> Self {
        let grey = Rgb {
            r: 128,
            g: 128,
            b: 128,
        };
        Answers {
            foreground: Rgb {
                r: 236,
                g: 236,
                b: 238,
            },
            background: Rgb {
                r: 23,
                g: 23,
                b: 24,
            },
            cursor: grey,
            ansi: [grey; 16],
            size: WindowSize {
                num_lines: 24,
                num_cols: 80,
                cell_width: 8,
                cell_height: 16,
            },
        }
    }
}

impl Answers {
    /// The background is dark: the color scheme a program is told.
    pub fn dark(&self) -> bool {
        let Rgb { r, g, b } = self.background;
        // Rec. 709 luma.
        0.2126 * f32::from(r) + 0.7152 * f32::from(g) + 0.0722 * f32::from(b) < 128.
    }

    fn color(&self, index: usize) -> Rgb {
        match index {
            0..16 => self.ansi[index],
            16..232 => {
                let i = index - 16;
                let level = |v: usize| if v == 0 { 0 } else { (55 + v * 40) as u8 };
                Rgb {
                    r: level(i / 36),
                    g: level((i / 6) % 6),
                    b: level(i % 6),
                }
            }
            232..256 => {
                let v = (8 + (index - 232) * 10) as u8;
                Rgb { r: v, g: v, b: v }
            }
            256 => self.foreground,
            257 => self.background,
            _ => self.cursor,
        }
    }
}

/// Shared by the `Term`'s listener, the threads and the UI. Small: it outlives the `Term` only
/// until the threads see the pane is gone.
#[derive(Default)]
pub(crate) struct Outbox {
    io: Mutex<Option<Sender<Io>>>,
    notes: Mutex<Vec<Note>>,
    pub signal: Signal,
    answers: Mutex<Answers>,
    sync_deadline: Mutex<Option<Instant>>,
    closed: AtomicBool,
    /// Output parsed since the UI last looked.
    output: AtomicBool,
}

impl Outbox {
    fn write(&self, data: String, binary: bool) {
        if let Some(tx) = self.io.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            let _ = tx.send(Io::Write(data, binary));
        }
    }

    fn note(&self, note: Note) {
        self.notes
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(note);
        self.signal.notify();
    }

    pub fn take_notes(&self) -> Vec<Note> {
        std::mem::take(&mut *self.notes.lock().unwrap_or_else(|e| e.into_inner()))
    }

    pub fn take_sync_deadline(&self) -> Option<Instant> {
        self.sync_deadline
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
    }

    pub fn take_output(&self) -> bool {
        self.output.swap(false, Ordering::AcqRel)
    }

    fn closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }

    pub fn answers(&self) -> Answers {
        *self.answers.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn set_answers(&self, f: impl FnOnce(&mut Answers)) {
        f(&mut self.answers.lock().unwrap_or_else(|e| e.into_inner()));
    }
}

pub(crate) struct Listener(Arc<Outbox>);

impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        let out = &self.0;
        match event {
            Event::PtyWrite(text) => out.write(text, false),
            Event::Title(title) => out.note(Note::Title(Some(title))),
            Event::ResetTitle => out.note(Note::Title(None)),
            Event::Bell => out.note(Note::Bell),
            Event::ClipboardStore(_, text) if osc52_accepts(&text) => out.note(Note::Copy(text)),
            Event::ColorRequest(index, format) => {
                let color = out
                    .answers
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .color(index);
                out.write(format(color), false);
            }
            Event::TextAreaSizeRequest(format) => {
                let size = out.answers.lock().unwrap_or_else(|e| e.into_inner()).size;
                out.write(format(size), false);
            }
            _ => {}
        }
    }
}

/// The terminal and the parsers that feed it.
pub(crate) struct Emu {
    pub term: Term<Listener>,
    pub parser: crate::vt::Parser,
    scanner: Scanner,
    pub marks: Marks,
    found: Vec<(usize, Found)>,
    /// Inside an OSC that was cut: its bytes don't reach the parser.
    skipping: bool,
    counter: marks::ScrollCounter,
    kitty_notes: crate::agent::KittyNotes,
    /// Rows scrolled into the history since the start: what a point found earlier moved up by.
    pub scrolled: u64,
    /// A command started since the last prompt.
    command_ran: bool,
    outbox: Arc<Outbox>,
    /// The terminal's grid and scrollback, counted in the debug memory readout.
    _live: gitviber_ui::memory::Live,
}

impl Emu {
    fn new(columns: usize, lines: usize, config: Config, outbox: Arc<Outbox>) -> Self {
        let size = alacritty_terminal::term::test::TermSize::new(columns.max(2), lines.max(1));
        Emu {
            term: Term::new(config, &size, Listener(outbox.clone())),
            parser: crate::vt::Parser::new(),
            scanner: Scanner::default(),
            marks: Marks::default(),
            found: Vec::new(),
            skipping: false,
            counter: marks::ScrollCounter::default(),
            kitty_notes: Default::default(),
            scrolled: 0,
            command_ran: false,
            outbox,
            _live: gitviber_ui::memory::Live::new("Terminal"),
        }
    }

    /// Parses `bytes`, acting on the marks and queries between them where each one ends.
    pub fn feed(&mut self, bytes: &[u8]) {
        self.or_reset(|e| e.parse(bytes));
    }

    /// Shows a synchronized frame that never ended.
    pub fn stop_sync(&mut self) {
        self.or_reset(|e| e.parser.stop_sync(&mut e.term));
    }

    /// A panic in the emulator ended core's reader thread: the pane froze, its program blocked on
    /// a full PTY. The terminal starts over instead, as at a program's RIS.
    fn or_reset(&mut self, f: impl FnOnce(&mut Self)) {
        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(self))).is_err() {
            self.scanner = Scanner::default();
            self.skipping = false;
            self.parser = crate::vt::Parser::new();
            self.parser.advance(&mut self.term, b"\x1bc");
        }
    }

    fn parse(&mut self, bytes: &[u8]) {
        self.found.clear();
        self.scanner.feed(bytes, &mut self.found);
        if self.found.is_empty() {
            self.advance(bytes);
            return;
        }
        let found = std::mem::take(&mut self.found);
        let mut at = 0;
        for (end, f) in &found {
            self.advance(&bytes[at..*end]);
            at = *end;
            self.act(f);
        }
        self.advance(&bytes[at..]);
        self.found = found;
    }

    fn advance(&mut self, bytes: &[u8]) {
        if self.skipping || bytes.is_empty() {
            return;
        }
        let rows = self
            .counter
            .advance(&mut self.parser, &mut self.term, bytes);
        self.scrolled += rows as u64;
        if self.marks.waiting_for_output() {
            self.marks.scrolled(rows as i32);
        }
    }

    /// A fresh parser gives back the one that held a big OSC, unless a synchronized frame is in it.
    fn renew_parser(&mut self) {
        if self.parser.sync_timeout().sync_timeout().is_none() {
            self.parser.renew();
        }
    }

    fn act(&mut self, found: &Found) {
        match found {
            Found::CutOsc => {
                self.parser.advance(&mut self.term, b"\x18");
                self.skipping = true;
            }
            Found::OscSkipped { esc } => {
                self.skipping = false;
                self.renew_parser();
                if *esc {
                    self.parser.advance(&mut self.term, b"\x1b");
                }
            }
            Found::BigOscEnded => self.renew_parser(),
            Found::Notify(code, data) => {
                let note = match code {
                    9 => crate::agent::osc9_note(data),
                    777 => crate::agent::osc777_note(data),
                    _ => self.kitty_notes.read(data),
                };
                if let Some(note) = note {
                    self.outbox.note(Note::Attention(note));
                }
            }
            // XTVERSION names this app: answered as xterm.js, Claude Code took the pane for VS Code's
            // terminal and its wheel and glyph atlas workarounds.
            Found::XtVersion => self.outbox.write("\x1bP>|GitViber\x1b\\".into(), false),
            // Answered here, ahead of the DA1 that follows the probe: a later reply is taken as none.
            Found::ProgramStatus(data) if data.starts_with('?') => {
                self.outbox.write("\x1b]7501;?\x1b\\".into(), false)
            }
            Found::ProgramStatus(data) => {
                if let Some(state) = crate::agent::program_status(data) {
                    self.outbox.note(Note::Status(state));
                }
            }
            Found::ColorScheme => {
                let dark = self.outbox.answers().dark();
                self.outbox.write(scheme_report(dark), false);
            }
            Found::Cwd(url) => {
                if let Some(dir) = cwd_from_url(url) {
                    self.outbox.note(Note::Cwd(dir));
                }
            }
            Found::Mark(data) => self.mark(marks::parse_mark(data)),
            Found::CutMark(data) => self.mark(marks::parse_cut_mark(data)),
        }
    }

    fn mark(&mut self, mark: Option<marks::Mark>) {
        let Some(mark) = mark else {
            return;
        };
        // As if the program had turned them off itself.
        if marks::mouse_left_on(mark.kind, *self.term.mode()) {
            self.parser.advance(&mut self.term, marks::MODES_OFF);
        }
        match mark.kind {
            MarkKind::C => self.command_ran = true,
            MarkKind::A => {
                let ran = std::mem::take(&mut self.command_ran);
                if marks::keys_left_on(*self.term.mode(), ran) {
                    self.parser.advance(&mut self.term, marks::KEYS_OFF);
                }
            }
            _ => {}
        }
        match self.marks.on(&mark, &mut self.term, Instant::now()) {
            Some(Effect::Prompt) => self.outbox.note(Note::Prompt),
            Some(Effect::Ended(end)) => {
                self.outbox.note(Note::Ended(end));
                self.outbox.note(Note::Prompt);
            }
            None => {}
        }
    }

    pub fn mode(&self) -> TermMode {
        *self.term.mode()
    }
}

/// The color scheme report (DSR 997): sent to `CSI ? 996 n`, and unasked on a change under mode 2031.
pub(crate) fn scheme_report(dark: bool) -> String {
    format!("\x1b[?997;{}n", if dark { 1 } else { 2 })
}

/// How a pane's shell starts.
pub(crate) struct Spawn {
    pub cwd: String,
    /// Where the shell starts when it isn't `cwd`: a split from a shell that `cd`ed.
    pub folder: Option<String>,
    pub cols: u16,
    pub rows: u16,
    pub integration: bool,
}

/// alacritty's settings for a pane with `scrollback` lines of history. The kitty keyboard protocol
/// is for programs that turn it on; zsh and bash don't, so they get the keys as before.
pub(crate) fn term_config(
    scrollback: usize,
    cursor: crate::config::CursorStyle,
    blink: bool,
) -> Config {
    use alacritty_terminal::vte::ansi::{CursorShape, CursorStyle};
    let shape = match cursor {
        crate::config::CursorStyle::Block => CursorShape::Block,
        crate::config::CursorStyle::Bar => CursorShape::Beam,
        crate::config::CursorStyle::Underline => CursorShape::Underline,
    };
    Config {
        scrolling_history: scrollback,
        kitty_keyboard: true,
        default_cursor_style: CursorStyle {
            shape,
            blinking: blink,
        },
        ..Config::default()
    }
}

pub(crate) struct Session {
    pub emu: Arc<FairMutex<Emu>>,
    pub outbox: Arc<Outbox>,
    pty: Arc<OnceLock<u32>>,
    core: Arc<Core>,
    rx: Mutex<Option<Receiver<Io>>>,
}

impl Session {
    /// The terminal, at a placeholder size until the pane is laid out and `spawn` starts the shell.
    pub fn new(core: Arc<Core>, config: Config) -> Session {
        let outbox = Arc::new(Outbox::default());
        let (tx, rx) = channel();
        *outbox.io.lock().unwrap_or_else(|e| e.into_inner()) = Some(tx);
        let emu = Arc::new(FairMutex::new(Emu::new(80, 24, config, outbox.clone())));
        Session {
            emu,
            outbox,
            pty: Arc::new(OnceLock::new()),
            core,
            rx: Mutex::new(Some(rx)),
        }
    }

    /// Starts the shell; once only.
    pub fn spawn(&self, spawn: Spawn) {
        let Some(rx) = self.rx.lock().unwrap_or_else(|e| e.into_inner()).take() else {
            return;
        };
        let (core, weak, outbox, pty) = (
            self.core.clone(),
            Arc::downgrade(&self.emu),
            self.outbox.clone(),
            self.pty.clone(),
        );
        let started = std::thread::Builder::new()
            .name("terminal-io".into())
            .spawn(move || io_thread(core, spawn, weak, outbox, pty, rx));
        if let Err(e) = started {
            self.emu
                .lock()
                .feed(format!("\x1b[31m{e}\x1b[0m\r\n").as_bytes());
        }
    }

    pub fn pty(&self) -> Option<u32> {
        self.pty.get().copied()
    }

    pub fn write(&self, text: impl Into<String>) {
        self.outbox.write(text.into(), false);
    }

    /// Mouse reports in the default encoding: a char per byte, past 127 beyond column 95.
    pub fn write_binary(&self, data: String) {
        self.outbox.write(data, true);
    }

    pub fn resize_pty(&self, cols: u16, rows: u16) {
        if let Some(tx) = self
            .outbox
            .io
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
        {
            let _ = tx.send(Io::Resize(cols, rows));
        }
    }

    /// What was written for a shell that never started.
    #[cfg(test)]
    pub fn written(&self) -> Vec<String> {
        let rx = self.rx.lock().unwrap_or_else(|e| e.into_inner());
        rx.iter()
            .flat_map(|rx| rx.try_iter())
            .filter_map(|io| match io {
                Io::Write(text, _) => Some(text),
                Io::Resize(..) => None,
            })
            .collect()
    }

    /// The folder the shell is in now, asked of its process.
    pub fn cwd(&self) -> Option<String> {
        pty::pty_cwd(&self.core, self.pty()?)
    }

    /// Whether a command is running (one that killing the pane would stop).
    pub fn busy(&self) -> bool {
        self.pty()
            .is_some_and(|id| pty::pty_busy(&self.core, Some(vec![id])) > 0)
    }

    pub fn core(&self) -> &Arc<Core> {
        &self.core
    }

    /// Ends the shell and lets the threads go; the `Term` goes with the `Session`.
    pub fn close(&self) {
        self.outbox.closed.store(true, Ordering::Release);
        self.outbox
            .io
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        self.rx.lock().unwrap_or_else(|e| e.into_inner()).take();
        self.outbox.signal.close();
        if let Some(id) = self.pty() {
            pty::pty_kill(&self.core, id);
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.close();
    }
}

/// Hands the allocator's free pages back to the OS: a closed terminal's scrollback is thousands
/// of small rows, which malloc keeps for reuse, so the app's footprint stayed up after a close.
pub(crate) fn release_freed_memory() {
    #[cfg(target_os = "macos")]
    {
        unsafe extern "C" {
            fn malloc_zone_pressure_relief(zone: *mut std::ffi::c_void, goal: usize) -> usize;
        }
        // SAFETY: a null zone means every zone; it only returns free pages.
        unsafe { malloc_zone_pressure_relief(std::ptr::null_mut(), 0) };
    }
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    {
        unsafe extern "C" {
            fn malloc_trim(pad: usize) -> i32;
        }
        // SAFETY: glibc's own; it only returns free memory.
        unsafe { malloc_trim(0) };
    }
}

/// Starts the shell, then sends it what's typed and its sizes, in order. Writes block while the
/// program isn't reading, so they're kept off the UI thread and the reader's.
fn io_thread(
    core: Arc<Core>,
    spawn: Spawn,
    emu: Weak<FairMutex<Emu>>,
    outbox: Arc<Outbox>,
    pty_id: Arc<OnceLock<u32>>,
    rx: Receiver<Io>,
) {
    let began = Instant::now();
    let output = {
        let emu = emu.clone();
        let outbox = outbox.clone();
        let pty_id = pty_id.clone();
        let core = core.clone();
        // Bytes handed over that core hasn't heard were parsed: the previous reads', since core
        // counts a read only after this returns.
        let mut unacked = 0usize;
        move |bytes: &[u8]| -> bool {
            let Some(emu) = emu.upgrade() else {
                return false;
            };
            if outbox.closed() {
                return false;
            }
            if unacked > 0
                && let Some(&id) = pty_id.get()
            {
                pty::pty_ack(&core, id, unacked);
                unacked = 0;
            }
            let deadline = {
                let mut e = emu.lock_unfair();
                e.feed(bytes);
                e.parser.sync_timeout().sync_timeout()
            };
            drop(emu);
            unacked += bytes.len();
            if deadline.is_some() {
                *outbox
                    .sync_deadline
                    .lock()
                    .unwrap_or_else(|e| e.into_inner()) = deadline;
            }
            outbox.output.store(true, Ordering::Release);
            outbox.signal.notify();
            true
        }
    };
    let exit = {
        let outbox = outbox.clone();
        move |exit: Option<Exit>| {
            outbox.note(Note::Exited {
                exit,
                lived: began.elapsed(),
            })
        }
    };
    let spawned = pty::pty_spawn(
        &core,
        spawn.cwd,
        spawn.cols.max(2),
        spawn.rows.max(1),
        spawn.integration,
        spawn.folder.filter(|f| Path::new(f).is_dir()),
        output,
        exit,
    );
    let id = match spawned {
        Ok(s) => {
            let _ = pty_id.set(s.id);
            outbox.note(Note::Started {
                integrated: s.integrated,
            });
            s.id
        }
        Err(e) => {
            if let Some(emu) = emu.upgrade() {
                emu.lock()
                    .feed(format!("\x1b[31m{e}\x1b[0m\r\n").as_bytes());
            }
            outbox.note(Note::Failed);
            return;
        }
    };
    // Closed while it was starting.
    if outbox.closed() {
        pty::pty_kill(&core, id);
        return;
    }
    drop(emu);
    while let Ok(io) = rx.recv() {
        match io {
            Io::Write(mut data, binary) => {
                // What queued up behind a blocked write goes as one.
                while data.len() < MAX_OSC52_TEXT {
                    match rx.try_recv() {
                        Ok(Io::Write(more, b)) if b == binary => data.push_str(&more),
                        Ok(other) => {
                            let _ = pty::pty_write(&core, id, std::mem::take(&mut data), binary);
                            if let Io::Resize(c, r) = other {
                                let _ = pty::pty_resize(&core, id, c, r);
                            } else if let Io::Write(d, b) = other {
                                let _ = pty::pty_write(&core, id, d, b);
                            }
                            break;
                        }
                        Err(_) => break,
                    }
                }
                if !data.is_empty()
                    && pty::pty_write(&core, id, data, binary).is_err()
                    && outbox.closed()
                {
                    return;
                }
            }
            Io::Resize(cols, rows) => {
                let _ = pty::pty_resize(&core, id, cols.max(2), rows.max(1));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> Config {
        term_config(100, crate::config::CursorStyle::Block, false)
    }

    #[test]
    fn wakes_before_the_task_ran_fold_into_one_and_close_ends_the_wait() {
        let signal = Signal::default();
        signal.notify();
        signal.notify();
        let waker = Waker::noop();
        let mut cx = Context::from_waker(waker);
        let mut wait = signal.wait();
        assert_eq!(Pin::new(&mut wait).poll(&mut cx), Poll::Ready(true));
        let mut wait = signal.wait();
        assert_eq!(Pin::new(&mut wait).poll(&mut cx), Poll::Pending);
        signal.close();
        assert_eq!(Pin::new(&mut wait).poll(&mut cx), Poll::Ready(false));
    }

    #[test]
    fn xtversion_names_the_app_and_marks_land_between_the_bytes() {
        let outbox = Arc::new(Outbox::default());
        let (tx, rx) = channel();
        *outbox.io.lock().unwrap() = Some(tx);
        let mut emu = Emu::new(20, 5, test_config(), outbox.clone());
        emu.feed(b"\x1b[>q\x1b]133;A\x07$ ls\r\n\x1b]133;C;cmdline=ls\x07a\r\n\x1b]133;D;0\x07");
        let Ok(Io::Write(reply, false)) = rx.try_recv() else {
            panic!()
        };
        assert_eq!(reply, "\x1bP>|GitViber\x1b\\");
        let notes = outbox.take_notes();
        assert!(
            matches!(&notes[..], [Note::Prompt, Note::Ended(e), Note::Prompt] if e.exit == Some(0))
        );
        assert_eq!(emu.marks.last_output(&emu.term).as_deref(), Some("a"));
    }

    fn writes(rx: &Receiver<Io>) -> Vec<String> {
        rx.try_iter()
            .filter_map(|io| match io {
                Io::Write(text, _) => Some(text),
                Io::Resize(..) => None,
            })
            .collect()
    }

    /// The probe's answer goes out before alacritty's DA1 reply: one after it counts as none.
    #[test]
    fn the_status_probe_is_answered_before_the_da1_after_it_and_reports_are_noted() {
        let outbox = Arc::new(Outbox::default());
        let (tx, rx) = channel();
        *outbox.io.lock().unwrap() = Some(tx);
        let mut emu = Emu::new(20, 5, test_config(), outbox.clone());
        emu.feed(b"\x1b[>0q\x1b]7501;?\x1b\\\x1b[c");
        let w = writes(&rx);
        assert_eq!(w.len(), 3, "{w:?}");
        assert_eq!(w[1], "\x1b]7501;?\x1b\\");
        assert!(w[2].starts_with("\x1b[?"), "{w:?}");
        emu.feed(b"\x1b]7501;state=blocked:app=claude-code:kind=question\x07");
        emu.feed(b"\x1b]7501;state=working:app=claude-code:id=1\x07");
        let notes = outbox.take_notes();
        assert!(
            matches!(
                &notes[..],
                [Note::Status(crate::agent::AgentState::Waiting)]
            ),
            "{notes:?}"
        );
    }

    #[test]
    fn the_color_scheme_query_is_answered_from_the_background() {
        let outbox = Arc::new(Outbox::default());
        let (tx, rx) = channel();
        *outbox.io.lock().unwrap() = Some(tx);
        let mut emu = Emu::new(20, 5, test_config(), outbox.clone());
        emu.feed(b"\x1b[?996n");
        outbox.set_answers(|a| {
            a.background = Rgb {
                r: 250,
                g: 250,
                b: 248,
            }
        });
        emu.feed(b"\x1b[?996n");
        assert_eq!(writes(&rx), ["\x1b[?997;1n", "\x1b[?997;2n"]);
    }

    #[test]
    fn a_prompt_turns_off_the_mouse_a_dead_program_left_on() {
        let outbox = Arc::new(Outbox::default());
        let mut emu = Emu::new(20, 5, test_config(), outbox);
        emu.feed(b"\x1b[?1002h\x1b[?1006h");
        assert!(emu.mode().intersects(TermMode::MOUSE_MODE));
        emu.feed(b"\x1b]133;A\x07");
        assert!(
            !emu.mode()
                .intersects(TermMode::MOUSE_MODE | TermMode::SGR_MOUSE)
        );
    }

    /// A program that died with the kitty keys and focus reports on (kill -9, a crash) leaves a
    /// shell that can't read ⌃C and types [I and [O; its prompt turns them off. A shell's own,
    /// on while it draws its prompt again, stay.
    #[test]
    fn a_prompt_after_a_command_turns_off_the_keys_it_left_on() {
        let keys = TermMode::KITTY_KEYBOARD_PROTOCOL | TermMode::FOCUS_IN_OUT;
        let outbox = Arc::new(Outbox::default());
        let mut emu = Emu::new(20, 5, test_config(), outbox);
        emu.feed(b"\x1b]133;A\x07$ \x1b]133;C\x07");
        emu.feed(b"\x1b[>1u\x1b[>11u\x1b[?1004h");
        assert!(emu.mode().contains(TermMode::REPORT_EVENT_TYPES));
        emu.feed(b"\x1b]133;D;137\x07\x1b]133;A\x07$ ");
        assert!(!emu.mode().intersects(keys), "{:?}", emu.mode());
        // Nothing left pushed to come back.
        emu.feed(b"\x1b[<u");
        assert!(!emu.mode().intersects(keys));
        emu.feed(b"\x1b[=5u\x1b[?1004h\x1b]133;A\x07$ ");
        assert!(
            emu.mode()
                .contains(TermMode::DISAMBIGUATE_ESC_CODES | TermMode::FOCUS_IN_OUT)
        );
        // Inside a full-screen program, its own.
        emu.feed(b"\x1b]133;C\x07\x1b[?1049h\x1b[>1u\x1b]133;A\x07");
        assert!(emu.mode().contains(TermMode::DISAMBIGUATE_ESC_CODES));
    }

    #[test]
    fn a_synchronized_frame_shows_once_it_ends() {
        let outbox = Arc::new(Outbox::default());
        let mut emu = Emu::new(20, 5, test_config(), outbox);
        emu.feed(b"\x1b[?2026hframe");
        assert!(emu.parser.sync_timeout().sync_timeout().is_some());
        assert_eq!(
            emu.term.grid()[alacritty_terminal::index::Line(0)]
                [alacritty_terminal::index::Column(0)]
            .c,
            ' '
        );
        emu.feed(b" one\x1b[?2026l");
        assert!(emu.parser.sync_timeout().sync_timeout().is_none());
        assert_eq!(
            emu.term.grid()[alacritty_terminal::index::Line(0)]
                [alacritty_terminal::index::Column(0)]
            .c,
            'f'
        );
    }

    #[test]
    fn an_endless_osc_is_cut_and_the_screen_goes_on_after_it() {
        let outbox = Arc::new(Outbox::default());
        let mut emu = Emu::new(20, 5, test_config(), outbox);
        emu.feed(b"\x1b]52;c;");
        for _ in 0..3 {
            emu.feed(&vec![b'A'; 1 << 20]);
        }
        emu.feed(b"\x07ok");
        let row: String = (0..2)
            .map(|c| {
                emu.term.grid()[alacritty_terminal::index::Line(0)]
                    [alacritty_terminal::index::Column(c)]
                .c
            })
            .collect();
        assert_eq!(row, "ok");
    }

    /// A mark past what the scanner keeps is cut, not dropped: the command still ends, named, so
    /// its time and notification come.
    #[test]
    fn a_command_line_of_any_size_or_content_from_either_shell() {
        let outbox = Arc::new(Outbox::default());
        let mut emu = Emu::new(20, 5, test_config(), outbox.clone());
        let marks = [
            format!("C;cmdline_url={}", "%41".repeat(100_000)),
            format!("C;cmdline={}", "ş".repeat(5000)),
            "C;cmdline_url=%ZZ".into(),
            "C;cmdline=a;cmdline_url=b%20c".into(),
        ];
        for m in &marks {
            emu.feed(
                format!("\x1b]133;A\x07$ x\r\n\x1b]133;{m}\x07out\r\n\x1b]133;D;0\x07").as_bytes(),
            );
        }
        let ends: Vec<Option<String>> = outbox
            .take_notes()
            .into_iter()
            .filter_map(|n| match n {
                Note::Ended(e) if e.exit == Some(0) => Some(e.command),
                _ => None,
            })
            .collect();
        assert_eq!(
            ends,
            [
                Some(format!("{}…", "A".repeat(79))),
                Some(format!("{}…", "ş".repeat(79))),
                None,
                Some("a;cmdline_url=b%20c".into()),
            ]
        );
        assert_eq!(emu.marks.last_output(&emu.term).as_deref(), Some("out"));
    }

    /// OSC 52 copies text whichever clipboard it names; a read (`?`) is never answered: what the
    /// user copied elsewhere isn't the program's.
    #[test]
    fn osc_52_copies_text_and_never_answers_a_read() {
        let outbox = Arc::new(Outbox::default());
        let (tx, rx) = channel();
        *outbox.io.lock().unwrap() = Some(tx);
        let mut emu = Emu::new(20, 5, test_config(), outbox.clone());
        let copied = |emu: &mut Emu, osc: &str| {
            emu.feed(format!("\x1b]52;{osc}\x07").as_bytes());
            outbox
                .take_notes()
                .into_iter()
                .filter_map(|n| match n {
                    Note::Copy(text) => Some(text),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(copied(&mut emu, "c;aMOpbGxvIOKchQo="), ["héllo ✅\n"]);
        assert_eq!(copied(&mut emu, ";dG11eA=="), ["tmux"]);
        assert_eq!(copied(&mut emu, "s;c2Vs"), ["sel"]);
        for nothing in ["c;?", "c;", "c", "c;not base64!", "c;//4="] {
            assert!(copied(&mut emu, nothing).is_empty(), "{nothing}");
        }
        assert!(rx.try_recv().is_err());
    }

    fn screen(emu: &Emu) -> String {
        use alacritty_terminal::grid::Dimensions as _;
        use alacritty_terminal::index::{Column, Line};
        let grid = emu.term.grid();
        (0..grid.screen_lines() as i32)
            .map(|l| {
                let row: String = (0..grid.columns())
                    .map(|c| grid[Line(l)][Column(c)].c)
                    .collect();
                row.trim_end().to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// alacritty 0.26 panicked at the 4097th push, on core's reader thread: no more output was
    /// read, so the pane froze with its shell blocked on a full PTY.
    #[test]
    fn a_shell_pushing_the_kitty_keys_ten_thousand_times_keeps_its_pane() {
        // SAFETY: nextest gives each test its own process; nothing else reads it meanwhile.
        unsafe { std::env::set_var("SHELL", "/bin/sh") };
        let core = Arc::new(Core::new(Arc::new(|_| {})));
        let session = Session::new(core, test_config());
        session.spawn(Spawn {
            cwd: std::env::temp_dir().to_string_lossy().into_owned(),
            folder: None,
            cols: 60,
            rows: 8,
            integration: false,
        });
        session.write(
            "echo kept-$((2*3)); i=0; while [ $i -lt 10000 ]; do printf '\\033[>1u'; i=$((i+1)); done; echo done-$((6*7))\r",
        );
        let deadline = Instant::now() + Duration::from_secs(20);
        while !screen(&session.emu.lock()).contains("done-42") {
            assert!(Instant::now() < deadline, "{}", screen(&session.emu.lock()));
            std::thread::sleep(Duration::from_millis(20));
        }
        // Not by starting over (or_reset): what was on the screen before the pushes stays.
        assert!(screen(&session.emu.lock()).contains("kept-6"));
        assert!(
            session
                .emu
                .lock()
                .mode()
                .contains(TermMode::DISAMBIGUATE_ESC_CODES)
        );
        session.close();
    }

    #[test]
    fn a_panic_in_the_emulator_starts_the_terminal_over() {
        let outbox = Arc::new(Outbox::default());
        let mut emu = Emu::new(20, 5, test_config(), outbox);
        emu.feed(b"before\x1b[?1049h\x1b[?2026hframe");
        emu.or_reset(|_| panic!("an emulator bug"));
        assert!(emu.parser.sync_timeout().sync_timeout().is_none());
        assert!(!emu.mode().contains(TermMode::ALT_SCREEN));
        emu.feed(b"after");
        assert_eq!(screen(&emu), "after\n\n\n\n");
    }
}
