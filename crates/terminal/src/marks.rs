//! OSC 133 command marks (shell integration): A a prompt starts, B the typed command does, C its
//! output does, D it ended. Each command that ran gets a mark on its prompt's row, and the last
//! one keeps where its output is.
//!
//! alacritty's grid has no markers that follow lines as they scroll, so a prompt's row carries a
//! private-use zero-width character in its first cell: it moves with the row through scrolling and
//! reflow, and goes when the row is trimmed off the history or cleared, as the commands it marks
//! do. The renderer and every copy leave it out.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::{Term, TermMode};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkKind {
    A,
    B,
    C,
    D,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mark {
    pub kind: MarkKind,
    pub exit: Option<i32>,
    pub command: Option<String>,
}

/// A mark from what follows `133;`. C may say what was typed: fish's `cmdline_url=`
/// (percent-encoded), or `cmdline=`, the rest of the mark as typed.
pub fn parse_mark(data: &str) -> Option<Mark> {
    parse(data, false)
}

/// A mark the scanner kept only the start of: a command line it cuts is longer, and an escape it
/// cuts in half is left out.
pub fn parse_cut_mark(data: &str) -> Option<Mark> {
    parse(data, true)
}

fn parse(data: &str, cut: bool) -> Option<Mark> {
    let mut fields = data.split(';');
    let kind = match fields.next()? {
        "A" => MarkKind::A,
        "B" => MarkKind::B,
        "C" => MarkKind::C,
        "D" => MarkKind::D,
        _ => return None,
    };
    let arg = fields.next();
    let exit = match kind {
        MarkKind::D => arg
            .filter(|a| !a.is_empty() && a.bytes().all(|b| b.is_ascii_digit()))
            .and_then(|a| a.parse().ok()),
        _ => None,
    };
    let command = (kind == MarkKind::C)
        .then(|| command_line(data, cut))
        .flatten();
    Some(Mark {
        kind,
        exit,
        command,
    })
}

/// A notification's or a tooltip's worth of it: a long one ends in "…".
const MAX_COMMAND: usize = 80;

fn command_line(data: &str, cut: bool) -> Option<String> {
    let (text, cut) = if let Some(at) = data.find(";cmdline=") {
        (data[at + ";cmdline=".len()..].to_string(), cut)
    } else {
        let at = data.find(";cmdline_url=")?;
        let rest = &data[at + ";cmdline_url=".len()..];
        let cut = cut && !rest.contains(';');
        // Malformed escapes: no name, rather than a wrong one.
        (
            percent_decode(rest.split(';').next().unwrap_or(""), cut)?,
            cut,
        )
    };
    let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.is_empty() {
        return None;
    }
    if cut || line.chars().count() > MAX_COMMAND {
        let mut cut: String = line.chars().take(MAX_COMMAND - 1).collect();
        cut.push('…');
        return Some(cut);
    }
    Some(line)
}

/// As decodeURIComponent: `%XX` bytes that must make UTF-8, any other escape an error. `cut`: `s`
/// was cut short, so an escape or a character it ends inside of is left out.
fn percent_decode(s: &str, cut: bool) -> Option<String> {
    let mut out = Vec::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if cut && i + 3 > bytes.len() {
                break;
            }
            let hex = s.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    match String::from_utf8(out) {
        Ok(text) => Some(text),
        Err(e) if cut && e.utf8_error().error_len().is_none() => {
            let whole = e.utf8_error().valid_up_to();
            let mut bytes = e.into_bytes();
            bytes.truncate(whole);
            String::from_utf8(bytes).ok()
        }
        Err(_) => None,
    }
}

/// How a command ended, from its output's mark (C) to its end (D) or the next prompt.
#[derive(Clone, Debug, PartialEq)]
pub struct CommandEnd {
    /// As typed, when the shell said (zsh and fish do; bash doesn't).
    pub command: Option<String>,
    pub duration: Duration,
    /// None when the shell was interrupted, or doesn't say.
    pub exit: Option<i32>,
}

/// "350ms", "4.2s", "45s", "2m 3s", "5m", "1h 5m": how long a command took.
pub fn format_duration(ms: f64) -> String {
    if ms.round() < 1000. {
        return format!("{}ms", ms.round());
    }
    let tenths = (ms / 100.).round() as u64;
    if tenths < 100 {
        return if tenths.is_multiple_of(10) {
            format!("{}s", tenths / 10)
        } else {
            format!("{}.{}s", tenths / 10, tenths % 10)
        };
    }
    let whole = (ms / 1000.).round() as u64;
    if whole < 60 {
        return format!("{whole}s");
    }
    let (h, m, s) = (whole / 3600, (whole % 3600) / 60, whole % 60);
    let pair = |a: u64, x: &str, b: u64, y: &str| {
        if b > 0 {
            format!("{a}{x} {b}{y}")
        } else {
            format!("{a}{x}")
        }
    };
    if h > 0 {
        pair(h, "h", m, "m")
    } else {
        pair(m, "m", s, "s")
    }
}

/// The mark's hover.
pub fn end_title(end: &CommandEnd) -> String {
    let took = format_duration(end.duration.as_secs_f64() * 1000.);
    match end.exit {
        None => format!("Ended after {took}"),
        Some(0) => format!("Took {took}"),
        Some(code) => format!("Failed after {took} (exit code {code})"),
    }
}

/// The program and one subcommand word, for a notification: "pnpm test", "cargo build".
/// Arguments can hold tokens and passwords, so the second word is kept only if it's plain: not a
/// flag, an assignment, a path, a URL, an address, long, or anything quoted or escaped.
/// `NAME=value` words before the program are dropped, a quoted one names nothing, and a path is
/// its file name.
pub fn short_command(line: Option<&str>) -> Option<String> {
    let quoted = |w: &str| w.contains(['\'', '"', '\\']);
    let mut words = line.unwrap_or("").split_whitespace().peekable();
    while let Some(w) = words.peek() {
        if !w.contains('=') {
            break;
        }
        if quoted(w) {
            return None;
        }
        words.next();
    }
    let program = words.next()?.rsplit('/').next()?;
    if program.is_empty()
        || program.chars().count() > 24
        || program.starts_with('-')
        || quoted(program)
    {
        return None;
    }
    let plain = |w: &str| {
        w.chars().count() <= 24
            && !w.starts_with('-')
            && !w.contains(['=', '/', ':', '@'])
            && !quoted(w)
    };
    match words.next() {
        Some(next) if plain(next) => Some(format!("{program} {next}")),
        _ => Some(program.to_string()),
    }
}

/// What a notification says of a long command: "pnpm test failed after 2m 3s (exit code 1)".
pub fn end_text(end: &CommandEnd) -> String {
    let took = format_duration(end.duration.as_secs_f64() * 1000.);
    let what = short_command(end.command.as_deref()).unwrap_or_else(|| "A command".into());
    match end.exit {
        None => format!("{what} ended after {took}"),
        Some(0) => format!("{what} finished after {took}"),
        Some(code) => format!("{what} failed after {took} (exit code {code})"),
    }
}

/// What a full-screen program turns off on its way out: mouse tracking in each of its modes, SGR
/// and UTF-8 reports, focus reports (which the shell would read as typed ^[[I and ^[[O).
pub const MODES_OFF: &[u8] = b"\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1005l\x1b[?1006l\x1b[?1004l";

/// Back to the history's buffer, the cursor shown: what a full-screen program sends on its way out.
pub const ALT_SCREEN_OFF: &[u8] = b"\x1b[?1049l";
pub const CURSOR_ON: &[u8] = b"\x1b[?25h";

/// A prompt in the history's buffer while the mouse is still reported: the program that asked
/// for it died without turning it off (a crash, `kill -9`), and a drag at the prompt would select
/// nothing. A shell under a full-screen program (tmux) prompts in the other buffer.
pub fn mouse_left_on(kind: MarkKind, mode: TermMode) -> bool {
    kind == MarkKind::A
        && !mode.contains(TermMode::ALT_SCREEN)
        && mode.intersects(TermMode::MOUSE_MODE)
}

/// What a program that reads keys its own way turns off on its way out: the kitty keyboard
/// protocol, every level of it pushed (zsh and bash take ⌃C as CSI 99;5u, and it does nothing),
/// and focus reports.
pub const KEYS_OFF: &[u8] = b"\x1b[<65535u\x1b[?1004l";

/// At a prompt in the history's buffer after a command, they're still on: it died without turning
/// them off. Only after a command: a shell that turns them on itself (fish) draws its prompt
/// again with them on, at every resize.
pub fn keys_left_on(mode: TermMode, command_ran: bool) -> bool {
    command_ran
        && !mode.contains(TermMode::ALT_SCREEN)
        && mode.intersects(TermMode::KITTY_KEYBOARD_PROTOCOL | TermMode::FOCUS_IN_OUT)
}

/// Commands kept marked.
const MAX_COMMANDS: usize = 300;

/// The private-use plane the row marks are drawn from: one character per command id.
const MARK_BASE: u32 = 0x10_0000;
const MARK_IDS: u32 = 0xFFFE;

pub fn is_mark_char(c: char) -> bool {
    (MARK_BASE..MARK_BASE + MARK_IDS).contains(&(c as u32))
}

fn mark_char(id: u32) -> char {
    char::from_u32(MARK_BASE + id % MARK_IDS).unwrap_or('\u{10_0000}')
}

/// `text` without the row marks, for anything copied out of the grid.
pub fn strip_marks(text: String) -> String {
    if text.chars().any(is_mark_char) {
        text.chars().filter(|&c| !is_mark_char(c)).collect()
    } else {
        text
    }
}

/// The command id marking `line`'s row, if any.
pub fn mark_on<T>(term: &Term<T>, line: Line) -> Option<u32> {
    let cell = &term.grid()[line][Column(0)];
    let c = cell
        .zerowidth()?
        .iter()
        .copied()
        .find(|&c| is_mark_char(c))?;
    Some((c as u32 - MARK_BASE) % MARK_IDS)
}

#[derive(Clone, Debug)]
pub struct Command {
    pub id: u32,
    pub command: Option<String>,
    started: Instant,
    /// Where the output starts from the prompt's row, in logical lines and cells: rows would be
    /// wrong once a column change reflows them.
    out: (u32, usize),
    pub end: Option<Ended>,
}

#[derive(Clone, Debug)]
pub struct Ended {
    pub end: CommandEnd,
    /// Where the cursor was at the end, as `out`.
    at: (u32, usize),
}

/// The prompt shown now, or the command running from it.
#[derive(Debug)]
struct Current {
    /// Where A was, and the rows scrolled since: the prompt's row once C comes.
    line: i32,
    scrolled: i32,
    id: Option<u32>,
}

/// What a mark changed, for the pane to act on.
#[derive(Debug, PartialEq)]
pub enum Effect {
    /// A prompt is up: where a `cd` has settled, and the shell reads what's typed now.
    Prompt,
    Ended(CommandEnd),
}

#[derive(Default)]
pub struct Marks {
    /// Commands that ran, oldest first.
    commands: VecDeque<Command>,
    current: Option<Current>,
    /// The last one that ended: the only one whose output is kept.
    last: Option<u32>,
    next_id: u32,
}

impl Marks {
    /// `now`: when the mark came, which times its command.
    pub fn on<T>(&mut self, mark: &Mark, term: &mut Term<T>, now: Instant) -> Option<Effect> {
        // A shell under a full-screen program (tmux) marks lines that aren't the history's.
        if term.mode().contains(TermMode::ALT_SCREEN) {
            return None;
        }
        let cursor = term.grid().cursor.point;
        match mark.kind {
            MarkKind::A => {
                let mut effect = None;
                // No D (the shell was interrupted, or doesn't send one): it ended here.
                if let Some(id) = self.current.take().and_then(|c| c.id) {
                    effect = self.ended(term, id, None, now).map(Effect::Ended);
                }
                self.current = Some(Current {
                    line: cursor.line.0,
                    scrolled: 0,
                    id: None,
                });
                // An unmarked prompt matters too: it's what a waiting command is typed at.
                effect.or(Some(Effect::Prompt))
            }
            MarkKind::C => {
                let current = self.current.as_mut().filter(|c| c.id.is_none())?;
                // Where the prompt went since A: up by what scrolled, inside the screen (a ⌃L
                // cleared it), above the output.
                let row = (current.line - current.scrolled).clamp(0, (cursor.line.0 - 1).max(0));
                let id = self.next_id;
                // Ids are what a row's mark can tell apart.
                self.next_id = (self.next_id + 1) % MARK_IDS;
                term.grid_mut()[Line(row)][Column(0)].push_zerowidth(mark_char(id));
                current.id = Some(id);
                self.commands.push_back(Command {
                    id,
                    command: mark.command.clone(),
                    started: now,
                    out: logical_offset(term, Line(row), cursor),
                    end: None,
                });
                if self.commands.len() > MAX_COMMANDS {
                    self.commands.pop_front();
                }
                None
            }
            MarkKind::D => {
                let id = self.current.as_ref().and_then(|c| c.id)?;
                self.current = None;
                self.ended(term, id, mark.exit, now).map(Effect::Ended)
            }
            MarkKind::B => None,
        }
    }

    fn ended<T>(
        &mut self,
        term: &Term<T>,
        id: u32,
        exit: Option<i32>,
        now: Instant,
    ) -> Option<CommandEnd> {
        let cursor = term.grid().cursor.point;
        let row = find_mark(term, id, cursor.line);
        let c = self
            .commands
            .iter_mut()
            .find(|c| c.id == id && c.end.is_none())?;
        let end = CommandEnd {
            command: c.command.clone(),
            duration: now.saturating_duration_since(c.started),
            exit,
        };
        c.end = Some(Ended {
            end: end.clone(),
            at: row.map_or((0, 0), |r| logical_offset(term, r, cursor)),
        });
        self.last = Some(id);
        Some(end)
    }

    /// Between a prompt and its command's output: the rows scrolled are counted (`scrolled`).
    pub fn waiting_for_output(&self) -> bool {
        self.current.as_ref().is_some_and(|c| c.id.is_none())
    }

    pub fn scrolled(&mut self, rows: i32) {
        if let Some(c) = self.current.as_mut() {
            c.scrolled += rows;
        }
    }

    /// Whether any command is marked, for the jump keys.
    pub fn has_commands(&self) -> bool {
        !self.commands.is_empty()
    }

    pub fn command(&self, id: u32) -> Option<&Command> {
        self.commands.iter().find(|c| c.id == id)
    }

    /// The top of the view on the prompt of the command above (-1) or below (1) the one at the top
    /// now; below the last one, the bottom.
    pub fn jump<T: alacritty_terminal::event::EventListener>(&self, term: &mut Term<T>, dir: i32) {
        use alacritty_terminal::grid::Scroll;
        let offset = term.grid().display_offset() as i32;
        let top = -offset;
        let marked = (term.topmost_line().0..=term.bottommost_line().0)
            .filter(|&l| mark_on(term, Line(l)).is_some_and(|id| self.command(id).is_some()));
        let to = if dir < 0 {
            marked.filter(|&l| l < top).last()
        } else {
            marked.into_iter().find(|&l| l > top)
        };
        let want = match to {
            Some(line) => (-line).clamp(0, term.history_size() as i32),
            None if dir > 0 => 0,
            None => return,
        };
        term.scroll_display(Scroll::Delta(want - offset));
    }

    /// The last ended command's output rows (start, end, end column); None without one.
    fn last_rows<T>(&self, term: &Term<T>) -> Option<(Line, Line, usize)> {
        let c = self.command(self.last?)?;
        let ended = c.end.as_ref()?;
        let row = find_mark(term, c.id, term.bottommost_line())?;
        let start = logical_point(term, row, c.out).line;
        let end = logical_point(term, row, ended.at);
        Some((start, end.line, end.column.0))
    }

    pub fn has_output<T>(&self, term: &Term<T>) -> bool {
        self.last_rows(term).is_some()
    }

    /// The last ended command's output; None without one, "" when it printed nothing.
    pub fn last_output<T>(&self, term: &Term<T>) -> Option<String> {
        let (start, end, end_x) = self.last_rows(term)?;
        let (end, end_col) = match end_x {
            0 if end > start => (end - 1, term.last_column()),
            0 => return Some(String::new()),
            x => (end, Column(x - 1)),
        };
        if end < start {
            return Some(String::new());
        }
        let text = term.bounds_to_string(Point::new(start, Column(0)), Point::new(end, end_col));
        Some(strip_marks(text).trim_end_matches('\n').to_string())
    }

    pub fn select_last_output<T: alacritty_terminal::event::EventListener>(
        &self,
        term: &mut Term<T>,
    ) {
        let Some((start, end, end_x)) = self.last_rows(term) else {
            return;
        };
        let end = if end_x > 0 { end } else { end - 1 };
        if end < start {
            return;
        }
        let mut selection = Selection::new(
            SelectionType::Lines,
            Point::new(start, Column(0)),
            Side::Left,
        );
        selection.update(Point::new(end, term.last_column()), Side::Right);
        term.selection = Some(selection);
        let top = -(term.grid().display_offset() as i32);
        if start.0 < top {
            let delta = (-start.0).clamp(0, term.history_size() as i32) + top;
            term.scroll_display(alacritty_terminal::grid::Scroll::Delta(delta));
        }
    }
}

/// Counts the rows scrolled into the history as bytes are parsed: held off the bottom, alacritty's
/// view moves along with each row going in, full or not, where the history's length stops growing
/// once it's full. Marks and find's matches move up by it.
#[derive(Default)]
pub struct ScrollCounter {
    /// A switch to the other screen came with the view held up: the history's view goes back down
    /// once that screen is left.
    lower_primary: bool,
}

impl ScrollCounter {
    pub fn advance<T: alacritty_terminal::event::EventListener>(
        &mut self,
        parser: &mut crate::vt::Parser,
        term: &mut Term<T>,
        bytes: &[u8],
    ) -> usize {
        use alacritty_terminal::grid::Scroll;
        if term.mode().contains(TermMode::ALT_SCREEN) {
            parser.advance(term, bytes);
            if self.lower_primary && !term.mode().contains(TermMode::ALT_SCREEN) {
                term.grid_mut().scroll_display(Scroll::Bottom);
                self.lower_primary = false;
            }
            return 0;
        }
        let user = term.grid().display_offset();
        let history = term.history_size();
        let base = user.max(1).min(history);
        if base == 0 {
            // No history yet, so none is full: its growth is the rows scrolled.
            parser.advance(term, bytes);
            return term.history_size().saturating_sub(history);
        }
        term.grid_mut()
            .scroll_display(Scroll::Delta(base as i32 - user as i32));
        parser.advance(term, bytes);
        if term.mode().contains(TermMode::ALT_SCREEN) {
            self.lower_primary = user == 0;
            return 0;
        }
        let after = term.grid().display_offset();
        // Back at the bottom, or where the rows the user was reading went.
        if user == 0 {
            term.grid_mut().scroll_display(Scroll::Bottom);
        }
        after.saturating_sub(base)
    }
}

fn wraps<T>(term: &Term<T>, row: Line) -> bool {
    term.grid()[row][term.last_column()]
        .flags
        .contains(alacritty_terminal::term::cell::Flags::WRAPLINE)
}

/// `to` from `from`'s row: the logical lines (rows not wrapping on) started since, and the cells
/// into the last one.
fn logical_offset<T>(term: &Term<T>, from: Line, to: Point) -> (u32, usize) {
    let mut lines = 0;
    let mut start = from;
    let mut row = from;
    while row < to.line {
        if !wraps(term, row) {
            lines += 1;
            start = row + 1;
        }
        row += 1;
    }
    let cells = (to.line.0 - start.0).max(0) as usize * term.columns() + to.column.0;
    (lines, cells)
}

/// `logical_offset` back to a point in the grid as it is now, inside it.
fn logical_point<T>(term: &Term<T>, from: Line, (lines, cells): (u32, usize)) -> Point {
    let bottom = term.bottommost_line();
    let cols = term.columns();
    let mut row = from.max(term.topmost_line());
    let mut n = 0;
    while n < lines && row < bottom {
        if !wraps(term, row) {
            n += 1;
        }
        row += 1;
    }
    let row = Line((row.0 + (cells / cols) as i32).min(bottom.0));
    Point::new(row, Column((cells % cols).min(cols - 1)))
}

/// The row marked for `id`, looking up from `from`.
fn find_mark<T>(term: &Term<T>, id: u32, from: Line) -> Option<Line> {
    let top = term.topmost_line().0;
    (top..=from.0)
        .rev()
        .map(Line)
        .find(|&l| mark_on(term, l) == Some(id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vt::Parser;
    use alacritty_terminal::event::VoidListener;
    use alacritty_terminal::term::{Config, test::TermSize};

    fn mark(kind: MarkKind) -> Mark {
        Mark {
            kind,
            exit: None,
            command: None,
        }
    }

    #[test]
    fn osc_133_marks_with_their_exit_code_and_the_extensions_other_shells_add() {
        assert_eq!(parse_mark("A"), Some(mark(MarkKind::A)));
        assert_eq!(parse_mark("A;click_events=1"), Some(mark(MarkKind::A)));
        assert_eq!(
            parse_mark("C;cmdline_url=ls%20-l")
                .unwrap()
                .command
                .as_deref(),
            Some("ls -l")
        );
        assert_eq!(parse_mark("D;0").unwrap().exit, Some(0));
        assert_eq!(parse_mark("D;130").unwrap().exit, Some(130));
        assert_eq!(parse_mark("D;1;aid=12").unwrap().exit, Some(1));
        assert_eq!(parse_mark("D"), Some(mark(MarkKind::D)));
        assert_eq!(parse_mark("D;"), Some(mark(MarkKind::D)));
        assert_eq!(parse_mark("P;k=s"), None);
        assert_eq!(parse_mark(""), None);
    }

    #[test]
    fn a_prompt_turns_off_mouse_reporting_a_dead_program_left_on_but_not_under_tmux() {
        let any = TermMode::MOUSE_MOTION;
        assert!(mouse_left_on(MarkKind::A, any));
        assert!(mouse_left_on(MarkKind::A, TermMode::MOUSE_REPORT_CLICK));
        assert!(mouse_left_on(MarkKind::A, TermMode::MOUSE_DRAG));
        assert!(!mouse_left_on(MarkKind::A, TermMode::empty()));
        assert!(!mouse_left_on(MarkKind::A, any | TermMode::ALT_SCREEN));
        assert!(!mouse_left_on(MarkKind::B, any));
        assert!(!mouse_left_on(MarkKind::C, any));
        assert!(!mouse_left_on(MarkKind::D, TermMode::MOUSE_DRAG));
    }

    #[test]
    fn the_command_line_a_c_mark_carries() {
        let cmd = |s: &str| parse_mark(s).unwrap().command;
        assert_eq!(cmd("C"), None);
        assert_eq!(cmd("C;cmdline=echo a;b=c").as_deref(), Some("echo a;b=c"));
        assert_eq!(
            cmd("C;cmdline=for x in 1 2;   do echo $x; done ").as_deref(),
            Some("for x in 1 2; do echo $x; done")
        );
        assert_eq!(
            cmd("C;cmdline_url=git%20log%20%3B;aid=3").as_deref(),
            Some("git log ;")
        );
        assert_eq!(cmd("C;cmdline_url=%E0%A4%A"), None);
        assert_eq!(cmd("C;cmdline=   "), None);
        let long = cmd(&format!("C;cmdline={}", "x".repeat(200))).unwrap();
        assert_eq!(long.chars().count(), 80);
        assert!(long.ends_with('…'));
    }

    /// The scanner keeps a long mark's start: the command line it cuts goes on, and the escapes it
    /// cuts in half are left out.
    #[test]
    fn a_cut_marks_command_line_is_its_start() {
        let cut = |s: &str| parse_cut_mark(s).unwrap().command;
        assert_eq!(cut("C;cmdline=echo ab").as_deref(), Some("echo ab…"));
        assert_eq!(cut("C;cmdline_url=make%20a%2").as_deref(), Some("make a…"));
        assert_eq!(cut("C;cmdline_url=make%20a%").as_deref(), Some("make a…"));
        assert_eq!(
            cut("C;cmdline_url=echo%20%C5%9F%C5").as_deref(),
            Some("echo ş…")
        );
        // A field that ended before the cut is whole; a bad escape before it is still no name.
        assert_eq!(cut("C;cmdline_url=ls;aid=12").as_deref(), Some("ls"));
        assert_eq!(cut("C;cmdline_url=%ZZls%2"), None);
        assert_eq!(parse_cut_mark("D;0").unwrap().exit, Some(0));
        // Whole, a half escape at the end is no name.
        assert_eq!(
            parse_mark("C;cmdline_url=make%20a%2").unwrap().command,
            None
        );
    }

    fn end(ms: u64, exit: Option<i32>, command: Option<&str>) -> CommandEnd {
        CommandEnd {
            command: command.map(Into::into),
            duration: Duration::from_millis(ms),
            exit,
        }
    }

    #[test]
    fn how_long_a_command_took_as_its_mark_and_a_notification_say_it() {
        assert_eq!(format_duration(0.), "0ms");
        assert_eq!(format_duration(349.6), "350ms");
        assert_eq!(format_duration(999.6), "1s");
        assert_eq!(format_duration(4230.), "4.2s");
        assert_eq!(format_duration(5000.), "5s");
        assert_eq!(format_duration(9960.), "10s");
        assert_eq!(format_duration(45_400.), "45s");
        assert_eq!(format_duration(60_000.), "1m");
        assert_eq!(format_duration(123_000.), "2m 3s");
        assert_eq!(format_duration(300_000.), "5m");
        assert_eq!(format_duration(3_900_000.), "1h 5m");
        assert_eq!(format_duration(7_200_000.), "2h");

        assert_eq!(end_title(&end(2100, Some(0), None)), "Took 2.1s");
        assert_eq!(
            end_title(&end(123_000, Some(1), None)),
            "Failed after 2m 3s (exit code 1)"
        );
        assert_eq!(end_title(&end(61_000, None, None)), "Ended after 1m 1s");
        assert_eq!(
            end_text(&end(123_000, Some(1), Some("pnpm test --watch"))),
            "pnpm test failed after 2m 3s (exit code 1)"
        );
        assert_eq!(
            end_text(&end(45_000, Some(0), Some("cargo build"))),
            "cargo build finished after 45s"
        );
        assert_eq!(
            end_text(&end(45_000, Some(0), None)),
            "A command finished after 45s"
        );
        assert_eq!(
            end_text(&end(45_000, None, None)),
            "A command ended after 45s"
        );
    }

    #[test]
    fn a_notification_names_the_program_and_subcommand_never_the_arguments() {
        let s = |l: &str| short_command(Some(l));
        assert_eq!(s("pnpm test").as_deref(), Some("pnpm test"));
        assert_eq!(s("cargo build --release").as_deref(), Some("cargo build"));
        assert_eq!(s("git push origin main").as_deref(), Some("git push"));
        assert_eq!(s("vault login s.abc").as_deref(), Some("vault login"));
        assert_eq!(
            s("curl -H \"Authorization: Bearer abc\" https://x.test").as_deref(),
            Some("curl")
        );
        assert_eq!(s("curl https://x.test/a?token=1").as_deref(), Some("curl"));
        assert_eq!(s("deploy prod user@host").as_deref(), Some("deploy prod"));
        assert_eq!(s("mysql --password=hunter2").as_deref(), Some("mysql"));
        assert_eq!(s("TOKEN=abc123 pnpm test").as_deref(), Some("pnpm test"));
        assert_eq!(
            s("./scripts/build.sh fast").as_deref(),
            Some("build.sh fast")
        );
        assert_eq!(
            s("make a_very_long_target_name_over_the_limit").as_deref(),
            Some("make")
        );
        assert_eq!(s("TOKEN=abc"), None);
        assert_eq!(s("TOKEN='tok en' deploy"), None);
        assert_eq!(s("FOO=\"a b\" cmd"), None);
        assert_eq!(s("echo \"sekret words\"").as_deref(), Some("echo"));
        assert_eq!(s("echo 'sekret words'").as_deref(), Some("echo"));
        assert_eq!(s("\"curl\" -x"), None);
        assert_eq!(s("echo a\\ b").as_deref(), Some("echo"));
        assert_eq!(s(""), None);
        assert_eq!(short_command(None), None);
        assert_eq!(
            end_text(&end(45_000, Some(0), Some("curl -H 'X-Key: s3cret' x"))),
            "curl finished after 45s"
        );
    }

    /// A shell's session as the integration marks it, fed to a terminal 20 columns by 5 rows.
    struct Shell {
        term: Term<VoidListener>,
        parser: Parser,
        marks: Marks,
        counter: ScrollCounter,
        effects: Vec<Effect>,
        /// The clock the marks come at, set by hand.
        start: Instant,
        now: Instant,
    }

    impl Shell {
        fn new(scrollback: usize) -> Self {
            let config = Config {
                scrolling_history: scrollback,
                ..Config::default()
            };
            Shell {
                term: Term::new(config, &TermSize::new(20, 5), VoidListener),
                parser: Parser::new(),
                marks: Marks::default(),
                counter: ScrollCounter::default(),
                effects: vec![],
                start: Instant::now(),
                now: Instant::now(),
            }
        }
        /// The marks that follow come `ms` into the session.
        fn at(&mut self, ms: u64) -> &mut Self {
            self.now = self.start + Duration::from_millis(ms);
            self
        }
        /// What ended, in order: the command, how long it took in ms, its exit code.
        fn ends(&self) -> Vec<(Option<&str>, u128, Option<i32>)> {
            self.effects
                .iter()
                .filter_map(|e| match e {
                    Effect::Ended(e) => {
                        Some((e.command.as_deref(), e.duration.as_millis(), e.exit))
                    }
                    Effect::Prompt => None,
                })
                .collect()
        }
        fn print(&mut self, text: &str) {
            let rows = self
                .counter
                .advance(&mut self.parser, &mut self.term, text.as_bytes());
            if self.marks.waiting_for_output() {
                self.marks.scrolled(rows as i32);
            }
        }
        fn mark(&mut self, data: &str) {
            let mark = parse_mark(data).unwrap();
            if let Some(e) = self.marks.on(&mark, &mut self.term, self.now) {
                self.effects.push(e);
            }
        }
        fn marks(&mut self, data: &[&str]) {
            for d in data {
                self.mark(d);
            }
        }
        /// A prompt, a command typed at it, its output and its end.
        fn run(&mut self, command: &str, output: &str, exit: i32) {
            self.mark("A");
            self.print("$ ");
            self.print(command);
            self.print("\r\n");
            self.mark(&format!("C;cmdline={command}"));
            self.print(output);
            self.mark(&format!("D;{exit}"));
        }
        fn marked_lines(&self) -> Vec<i32> {
            (self.term.topmost_line().0..=self.term.bottommost_line().0)
                .filter(|&l| mark_on(&self.term, Line(l)).is_some())
                .collect()
        }
    }

    #[test]
    fn a_command_marks_its_prompt_row_and_keeps_its_output() {
        let mut sh = Shell::new(100);
        sh.run("ls", "a\r\nb\r\n", 0);
        assert_eq!(sh.marked_lines(), vec![0]);
        assert_eq!(sh.marks.last_output(&sh.term).as_deref(), Some("a\nb"));
        assert!(
            matches!(&sh.effects[..], [Effect::Prompt, Effect::Ended(e)] if e.exit == Some(0) && e.command.as_deref() == Some("ls"))
        );
        // The prompt after it, and a failing one: its mark scrolls with the rows.
        sh.run("false", "", 1);
        sh.run("seq", "1\r\n2\r\n3\r\n4\r\n5\r\n6\r\n", 0);
        assert_eq!(
            sh.marks.last_output(&sh.term).as_deref(),
            Some("1\n2\n3\n4\n5\n6")
        );
        let lines = sh.marked_lines();
        assert_eq!(lines.len(), 3);
        let first = mark_on(&sh.term, Line(lines[0])).unwrap();
        let failed = mark_on(&sh.term, Line(lines[1])).unwrap();
        assert_eq!(
            sh.marks
                .command(first)
                .unwrap()
                .end
                .as_ref()
                .unwrap()
                .end
                .exit,
            Some(0)
        );
        assert_eq!(
            sh.marks
                .command(failed)
                .unwrap()
                .end
                .as_ref()
                .unwrap()
                .end
                .exit,
            Some(1)
        );
        // Copying a marked row leaves the mark out.
        let text = sh.term.bounds_to_string(
            Point::new(Line(lines[0]), Column(0)),
            Point::new(Line(lines[0]), Column(5)),
        );
        assert_eq!(strip_marks(text), "$ ls");
    }

    #[test]
    fn a_prompt_nothing_ran_from_is_not_marked_and_one_without_a_d_ends_at_the_next_prompt() {
        let mut sh = Shell::new(100);
        sh.mark("A");
        sh.print("$ \r\n");
        sh.mark("A");
        sh.print("$ sleep 9\r\n");
        sh.mark("C");
        sh.print("^C\r\n");
        sh.mark("A");
        assert_eq!(sh.marked_lines().len(), 1);
        assert!(matches!(sh.effects.last(), Some(Effect::Ended(e)) if e.exit.is_none()));
        // A D with no command before it (bash's empty line) ends nothing.
        let before = sh.effects.len();
        sh.mark("D;0");
        assert_eq!(sh.effects.len(), before);
    }

    #[test]
    fn marks_in_a_full_screen_program_are_not_the_historys() {
        let mut sh = Shell::new(100);
        sh.print("\x1b[?1049h");
        sh.run("ls", "a\r\n", 0);
        assert!(sh.marked_lines().is_empty());
        assert!(!sh.marks.has_commands());
    }

    #[test]
    fn the_jump_keys_put_the_prompt_above_or_below_at_the_top() {
        let mut sh = Shell::new(100);
        for i in 0..4 {
            sh.run("seq", &"x\r\n".repeat(4 + i), 0);
        }
        sh.mark("A");
        sh.print("$ ");
        let lines = sh.marked_lines();
        assert_eq!(lines.len(), 4);
        sh.marks.jump(&mut sh.term, -1);
        let top = -(sh.term.grid().display_offset() as i32);
        assert_eq!(top, *lines.iter().rfind(|&&l| l < 0).unwrap());
        sh.marks.jump(&mut sh.term, -1);
        sh.marks.jump(&mut sh.term, 1);
        assert_eq!(-(sh.term.grid().display_offset() as i32), top);
        // Below the last one: the bottom.
        for _ in 0..5 {
            sh.marks.jump(&mut sh.term, 1);
        }
        assert_eq!(sh.term.grid().display_offset(), 0);
    }

    #[test]
    fn a_mark_trimmed_off_the_history_takes_its_command_with_it() {
        let mut sh = Shell::new(3);
        sh.run("ls", "a\r\n", 0);
        sh.print(&"y\r\n".repeat(20));
        assert!(sh.marked_lines().is_empty());
        assert_eq!(sh.marks.last_output(&sh.term), None);
        assert!(!sh.marks.has_output(&sh.term));
    }

    #[test]
    fn selecting_the_last_output_selects_its_lines() {
        let mut sh = Shell::new(100);
        sh.run("ls", "one\r\ntwo\r\n", 0);
        sh.marks.select_last_output(&mut sh.term);
        assert_eq!(
            sh.term.selection_to_string().map(strip_marks).as_deref(),
            Some("one\ntwo\n")
        );
    }

    #[test]
    fn a_column_change_that_reflows_the_output_keeps_it_whole() {
        let mut sh = Shell::new(100);
        sh.run("cat", &format!("{}\r\nshort\r\n", "x".repeat(18)), 0);
        assert_eq!(
            sh.marks.last_output(&sh.term).as_deref(),
            Some(&*format!("{}\nshort", "x".repeat(18)))
        );
        // Narrower: the long line wraps onto two rows; rows counted from the prompt would run
        // past the grid's end.
        sh.term.resize(TermSize::new(8, 5));
        let out = sh.marks.last_output(&sh.term).unwrap();
        assert_eq!(out.replace('\n', ""), format!("{}short", "x".repeat(18)));
        sh.marks.select_last_output(&mut sh.term);
        assert!(sh.term.selection_to_string().is_some());
        sh.term.resize(TermSize::new(30, 5));
        assert!(sh.marks.last_output(&sh.term).is_some());
    }

    #[test]
    fn with_the_history_full_a_two_line_prompt_is_marked_on_its_first_line() {
        let mut sh = Shell::new(3);
        sh.print(&"y\r\n".repeat(20));
        assert_eq!(sh.term.history_size(), 3);
        // A at the bottom row, then the prompt's two lines scroll the screen.
        sh.print("\x1b[5;1H");
        sh.mark("A");
        sh.print("~/repo main\r\n$ ls\r\n");
        sh.mark("C;cmdline=ls");
        sh.print("a\r\n");
        sh.mark("D;0");
        let lines = sh.marked_lines();
        assert_eq!(lines.len(), 1);
        let row = sh.term.bounds_to_string(
            Point::new(Line(lines[0]), Column(0)),
            Point::new(Line(lines[0]), Column(10)),
        );
        assert_eq!(strip_marks(row), "~/repo main");
        assert_eq!(sh.marks.last_output(&sh.term).as_deref(), Some("a"));
    }

    #[test]
    fn counting_rows_leaves_the_view_where_it_was() {
        let mut sh = Shell::new(5);
        sh.print(&"y\r\n".repeat(8));
        assert_eq!(sh.term.grid().display_offset(), 0);
        // A full-screen program comes and goes within reads: the history's view stays down.
        sh.print("a\r\n\x1b[?1049h");
        sh.print("x\x1b[?1049l");
        assert_eq!(sh.term.grid().display_offset(), 0);
        // Scrolled up, the view stays on the rows it showed as more come.
        sh.term
            .grid_mut()
            .scroll_display(alacritty_terminal::grid::Scroll::Delta(2));
        let rows = sh.counter.advance(&mut sh.parser, &mut sh.term, b"z\r\n");
        assert_eq!(rows, 1);
        assert_eq!(sh.term.grid().display_offset(), 3);
    }

    #[test]
    fn a_commands_time_runs_from_its_c_to_its_d() {
        let mut sh = Shell::new(100);
        sh.at(0).mark("A");
        sh.at(1000).mark("C;cmdline=pnpm test");
        sh.at(13_500).mark("D;1");
        assert_eq!(sh.ends(), [(Some("pnpm test"), 12_500, Some(1))]);
        // No D (a killed shell's command): it ends at the next prompt, how unknown; a dead one never.
        let mut sh = Shell::new(100);
        sh.at(0).marks(&["A", "C;cmdline=sleep 100"]);
        sh.at(20_000).mark("A");
        sh.at(21_000).mark("C;cmdline=tail -f log");
        assert_eq!(sh.ends(), [(Some("sleep 100"), 20_000, None)]);
    }

    #[test]
    fn an_empty_line_a_ctrl_c_at_the_prompt_and_a_d_with_no_c_end_nothing() {
        let mut sh = Shell::new(100);
        sh.marks(&["D;0", "A", "D;0", "A", "D;130", "A", "B", "A"]);
        assert_eq!(
            sh.effects,
            [
                Effect::Prompt,
                Effect::Prompt,
                Effect::Prompt,
                Effect::Prompt
            ]
        );
    }

    #[test]
    fn ssh_to_a_host_that_marks_its_prompts_too_then_exit() {
        let mut sh = Shell::new(100);
        sh.at(0).marks(&["A", "C;cmdline=ssh box"]);
        // The remote's first prompt ends the local command as far as the marks can tell.
        sh.at(4000).mark("A");
        sh.at(5000).mark("C;cmdline=make");
        sh.at(65_000).marks(&["D;0", "A"]);
        sh.at(66_000).mark("C;cmdline=exit");
        // The local shell's D for ssh lands on the remote's `exit`.
        sh.at(66_200).marks(&["D;255", "A"]);
        assert_eq!(
            sh.ends(),
            [
                (Some("ssh box"), 4000, None),
                (Some("make"), 60_000, Some(0)),
                (Some("exit"), 200, Some(255)),
            ]
        );
        // A sub-shell without marks: the command that started it runs until it exits.
        let mut sh = Shell::new(100);
        sh.at(0).marks(&["A", "C;cmdline=zsh"]);
        sh.at(600_000).marks(&["D;0", "A"]);
        assert_eq!(sh.ends(), [(Some("zsh"), 600_000, Some(0))]);
    }

    #[test]
    fn marks_under_a_full_screen_program_leave_the_command_that_started_it_running() {
        let mut sh = Shell::new(100);
        sh.at(0).marks(&["A", "C;cmdline=tmux"]);
        sh.print("\x1b[?1049h");
        sh.at(1000).marks(&["A", "C;cmdline=make", "D;2", "A"]);
        sh.print("\x1b[?1049l");
        sh.at(90_000).marks(&["D;0", "A"]);
        assert_eq!(sh.ends(), [(Some("tmux"), 90_000, Some(0))]);
    }

    #[test]
    fn a_second_c_or_a_d_twice_changes_nothing() {
        let mut sh = Shell::new(100);
        sh.at(0).marks(&["A", "C;cmdline=first"]);
        sh.at(5000).mark("C;cmdline=second");
        sh.at(9000).marks(&["D;0", "D;1"]);
        assert_eq!(sh.ends(), [(Some("first"), 9000, Some(0))]);
        assert_eq!(sh.marked_lines().len(), 1);
    }

    #[test]
    fn a_thousand_commands_each_end_once_and_the_oldest_are_let_go() {
        let mut sh = Shell::new(100);
        for i in 0..1000u64 {
            sh.at(i * 10).marks(&["A", &format!("C;cmdline=echo {i}")]);
            sh.at(i * 10 + 5).mark(&format!("D;{}", i % 3));
        }
        let ends = sh.ends();
        assert_eq!(ends.len(), 1000);
        assert_eq!(ends[999], (Some("echo 999"), 5, Some(0)));
        assert_eq!(sh.marks.commands.len(), MAX_COMMANDS);
        assert_eq!(
            sh.marks.commands.front().and_then(|c| c.command.as_deref()),
            Some("echo 700")
        );
    }

    /// A row's mark tells MARK_IDS ids apart: past that many commands the ids start over, and the
    /// newest still find their rows.
    #[test]
    fn command_ids_start_over_where_the_row_marks_do() {
        let mut sh = Shell::new(100);
        sh.marks.next_id = MARK_IDS - 1;
        sh.run("ls", "a\r\n", 0);
        sh.run("seq", "1\r\n2\r\n3\r\n4\r\n5\r\n", 0);
        assert_eq!(
            sh.marks.last_output(&sh.term).as_deref(),
            Some("1\n2\n3\n4\n5")
        );
        assert_eq!(sh.marks.next_id, 1);
        let lines = sh.marked_lines();
        let ids: Vec<u32> = lines
            .iter()
            .filter_map(|&l| mark_on(&sh.term, Line(l)))
            .collect();
        assert_eq!(ids, [MARK_IDS - 1, 0]);
        assert!(ids.iter().all(|&id| sh.marks.command(id).is_some()));
        sh.mark("A");
        sh.print("$ ");
        sh.marks.jump(&mut sh.term, -1);
        assert_eq!(-(sh.term.grid().display_offset() as i32), lines[1]);
    }
}
