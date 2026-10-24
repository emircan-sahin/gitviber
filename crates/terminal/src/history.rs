//! A terminal's history as the escape sequences that draw it again: the session save keeps it, a
//! restored pane is fed it. Text and its colors and styles only: no modes, no alt screen, no
//! links, so nothing a program left on leaks into the new shell.

use std::fmt::Write as _;

use alacritty_terminal::Term;
use alacritty_terminal::grid::Dimensions as _;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::vte::ansi::{Color, NamedColor};

use crate::marks::is_mark_char;

/// Lines of scrollback kept, as the old app's save did: ~100 KB a pane with colors.
pub const HISTORY_LINES: usize = 1000;

const STYLES: Flags = Flags::BOLD
    .union(Flags::DIM)
    .union(Flags::ITALIC)
    .union(Flags::ALL_UNDERLINES)
    .union(Flags::INVERSE)
    .union(Flags::HIDDEN)
    .union(Flags::STRIKEOUT);

fn color(out: &mut String, c: Color, fg: bool) {
    let (base, bright, ext) = if fg { (30, 90, 38) } else { (40, 100, 48) };
    match c {
        Color::Named(n) => {
            let i = n as usize;
            let dim = |base_color: NamedColor| base_color as usize;
            let i = match n {
                NamedColor::Foreground
                | NamedColor::Background
                | NamedColor::BrightForeground
                | NamedColor::DimForeground
                | NamedColor::Cursor => {
                    return;
                }
                NamedColor::DimBlack => dim(NamedColor::Black),
                NamedColor::DimRed => dim(NamedColor::Red),
                NamedColor::DimGreen => dim(NamedColor::Green),
                NamedColor::DimYellow => dim(NamedColor::Yellow),
                NamedColor::DimBlue => dim(NamedColor::Blue),
                NamedColor::DimMagenta => dim(NamedColor::Magenta),
                NamedColor::DimCyan => dim(NamedColor::Cyan),
                NamedColor::DimWhite => dim(NamedColor::White),
                _ => i,
            };
            let _ = if i < 8 {
                write!(out, ";{}", base + i)
            } else {
                write!(out, ";{}", bright + i - 8)
            };
        }
        Color::Indexed(i) => {
            let _ = write!(out, ";{ext};5;{i}");
        }
        Color::Spec(rgb) => {
            let _ = write!(out, ";{ext};2;{};{};{}", rgb.r, rgb.g, rgb.b);
        }
    }
}

/// The SGR that sets a cell's look from scratch.
fn sgr(cell: &Cell) -> String {
    let mut out = String::from("\x1b[0");
    let f = cell.flags;
    for (flag, code) in [
        (Flags::BOLD, "1"),
        (Flags::DIM, "2"),
        (Flags::ITALIC, "3"),
        (Flags::INVERSE, "7"),
        (Flags::HIDDEN, "8"),
        (Flags::STRIKEOUT, "9"),
    ] {
        if f.contains(flag) {
            out.push(';');
            out.push_str(code);
        }
    }
    if f.contains(Flags::DOUBLE_UNDERLINE) {
        out.push_str(";21");
    } else if f.contains(Flags::UNDERCURL) {
        out.push_str(";4:3");
    } else if f.contains(Flags::DOTTED_UNDERLINE) {
        out.push_str(";4:4");
    } else if f.contains(Flags::DASHED_UNDERLINE) {
        out.push_str(";4:5");
    } else if f.contains(Flags::UNDERLINE) {
        out.push_str(";4");
    }
    color(&mut out, cell.fg, true);
    color(&mut out, cell.bg, false);
    out.push('m');
    out
}

fn plain(cell: &Cell) -> bool {
    cell.flags.intersection(STYLES).is_empty()
        && cell.fg == Color::Named(NamedColor::Foreground)
        && cell.bg == Color::Named(NamedColor::Background)
}

/// The last `max` lines of history and the screen, down to the last one with text in it. None
/// while the alt screen shows: the history under it can't be read, and the last save stands.
pub fn serialize<T>(term: &Term<T>, max: usize) -> Option<String> {
    if term
        .mode()
        .contains(alacritty_terminal::term::TermMode::ALT_SCREEN)
    {
        return None;
    }
    let grid = term.grid();
    let cols = term.columns();
    let blank = |c: &Cell| {
        (c.c == ' ' || c.c == '\t')
            && c.bg == Color::Named(NamedColor::Background)
            && !c
                .flags
                .intersects(Flags::INVERSE | Flags::ALL_UNDERLINES | Flags::STRIKEOUT)
    };
    let bottom = term.bottommost_line().0;
    let mut last = bottom;
    while last > term.topmost_line().0 && (0..cols).all(|x| blank(&grid[Line(last)][Column(x)])) {
        last -= 1;
    }
    let first = (last - (max + term.screen_lines()) as i32 + 1).max(term.topmost_line().0);
    let mut out = String::new();
    let mut look: Option<String> = None;
    for l in first..=last {
        let row = &grid[Line(l)];
        let wraps = cols > 0 && row[Column(cols - 1)].flags.contains(Flags::WRAPLINE);
        let end = if wraps {
            cols
        } else {
            (0..cols)
                .rposition(|x| !blank(&row[Column(x)]))
                .map_or(0, |x| x + 1)
        };
        for x in 0..end {
            let cell = &row[Column(x)];
            if cell
                .flags
                .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
            {
                continue;
            }
            let wanted = (!plain(cell)).then(|| sgr(cell));
            if wanted != look {
                out.push_str(wanted.as_deref().unwrap_or("\x1b[0m"));
                look = wanted;
            }
            out.push(if cell.c == '\t' { ' ' } else { cell.c });
            out.extend(
                cell.zerowidth()
                    .unwrap_or_default()
                    .iter()
                    .filter(|c| !is_mark_char(**c)),
            );
        }
        if !wraps && l < last {
            if look.take().is_some() {
                out.push_str("\x1b[0m");
            }
            out.push_str("\r\n");
        }
    }
    if look.is_some() {
        out.push_str("\x1b[0m");
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::event::VoidListener;
    use alacritty_terminal::term::{Config, test::TermSize};
    use alacritty_terminal::vte::ansi::{Processor, StdSyncHandler};

    fn term(cols: usize, text: &str) -> Term<VoidListener> {
        let mut t = Term::new(
            Config {
                scrolling_history: 5000,
                ..Config::default()
            },
            &TermSize::new(cols, 5),
            VoidListener,
        );
        let mut p: Processor<StdSyncHandler> = Processor::new();
        p.advance(&mut t, text.as_bytes());
        t
    }

    fn rows<T>(t: &Term<T>) -> Vec<String> {
        (t.topmost_line().0..=t.bottommost_line().0)
            .map(|l| {
                (0..t.columns())
                    .map(|x| t.grid()[Line(l)][Column(x)].c)
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect::<Vec<_>>()
            .into_iter()
            .skip_while(String::is_empty)
            .collect()
    }

    #[test]
    fn what_it_writes_draws_the_same_text_and_colors_again() {
        let text = "\x1b[1;31mred\x1b[0m plain \x1b[38;2;1;2;3mrgb\x1b[0m\r\nnext 😀 wide\r\n\x1b[44m  \x1b[0m bg\r\nwraps-over-the-edge";
        let a = term(12, text);
        let saved = serialize(&a, HISTORY_LINES).unwrap();
        let b = term(12, &saved);
        assert_eq!(rows(&a), rows(&b));
        let cell = |t: &Term<VoidListener>, l: i32, x: usize| t.grid()[Line(l)][Column(x)].clone();
        let find = |t: &Term<VoidListener>, c: char| {
            (t.topmost_line().0..=t.bottommost_line().0)
                .flat_map(|l| (0..t.columns()).map(move |x| (l, x)))
                .find(|&(l, x)| t.grid()[Line(l)][Column(x)].c == c)
                .unwrap()
        };
        for ch in ['r', 'g', 'p'] {
            let ((la, xa), (lb, xb)) = (find(&a, ch), find(&b, ch));
            let (ca, cb) = (cell(&a, la, xa), cell(&b, lb, xb));
            assert_eq!(
                (ca.fg, ca.bg, ca.flags & STYLES),
                (cb.fg, cb.bg, cb.flags & STYLES),
                "{ch}"
            );
        }
        // The colored blank cells are kept, the trailing plain ones aren't.
        assert!(saved.contains("\x1b[0;44m  "));
        assert!(!saved.contains("   \r\n"));
    }

    #[test]
    fn only_the_last_lines_and_never_the_alt_screen() {
        let lines: String = (0..3000).map(|i| format!("line {i}\r\n")).collect();
        let t = term(20, &lines);
        let saved = serialize(&t, 1000).unwrap();
        let n = saved.matches("\r\n").count();
        assert!((1000..1010).contains(&n), "{n}");
        assert!(saved.ends_with("line 2999"));
        let alt = term(20, "kept\r\n\x1b[?1049hfull screen");
        assert_eq!(serialize(&alt, 1000), None);
    }
}
