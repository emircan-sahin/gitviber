//! Find in a pane's text, its scrollback included, with the find box's three toggles.

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Boundary, Column, Direction, Point};
use alacritty_terminal::term::Term;
use alacritty_terminal::term::search::{Match, RegexIter, RegexSearch};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FindOptions {
    pub match_case: bool,
    pub whole_word: bool,
    pub regex: bool,
}

/// Matches marked at most, as the old find box's: past it the count says so.
pub const MAX_MATCHES: usize = 1000;

/// The query as the pattern searched, or why it isn't one (a regex that doesn't parse).
pub fn pattern(query: &str, o: FindOptions) -> String {
    let source = if o.regex {
        query.to_string()
    } else {
        escape(query)
    };
    // alacritty's search is smart-case on its own; the toggle decides here.
    let flags = if o.match_case { "(?-i)" } else { "(?i)" };
    format!("{flags}(?:{source})")
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if "\\.+*?()|[]{}^$#&-~".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

pub fn compile(query: &str, o: FindOptions) -> Result<RegexSearch, String> {
    RegexSearch::new(&pattern(query, o)).map_err(|e| {
        // The DFA builder's message names the pattern we built; the first line says what's wrong.
        e.to_string()
            .lines()
            .next()
            .unwrap_or("Invalid regex")
            .to_string()
    })
}

/// git grep -w's word characters.
fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn char_at<T>(term: &Term<T>, p: Point) -> Option<char> {
    let line = p.line;
    if line < term.topmost_line() || line > term.bottommost_line() || p.column > term.last_column()
    {
        return None;
    }
    Some(term.grid()[line][p.column].c)
}

/// Matches marked at once, around the current one.
#[derive(Clone, Debug, Default)]
pub struct Window {
    pub matches: Vec<Match>,
    /// The first one's number among all.
    pub first: usize,
    pub total: usize,
    /// The first at or past the center, in `matches`.
    pub at: Option<usize>,
}

/// Every match in the scrollback and the screen, top to bottom, counted, with up to MAX_MATCHES
/// kept around `center` (half before it at most): the newest output is as reachable as the
/// oldest. Empty matches are left out; whole word wants no word character either side.
pub fn find_around<T>(
    term: &Term<T>,
    regex: &mut RegexSearch,
    o: FindOptions,
    center: Point,
) -> Window {
    let start = Point::new(term.topmost_line(), Column(0));
    let end = Point::new(term.bottommost_line(), term.last_column());
    let half = MAX_MATCHES / 2;
    let mut kept = std::collections::VecDeque::new();
    let mut w = Window::default();
    for m in RegexIter::new(start, end, Direction::Right, term, regex) {
        if o.whole_word {
            let before = char_at(term, m.start().sub(term, Boundary::None, 1))
                .filter(|_| *m.start() != start);
            let after =
                char_at(term, m.end().add(term, Boundary::None, 1)).filter(|_| *m.end() != end);
            if before.is_some_and(is_word) || after.is_some_and(is_word) {
                continue;
            }
        }
        w.total += 1;
        if w.at.is_none() && *m.start() >= center {
            w.at = Some(kept.len());
        }
        match w.at {
            None => {
                kept.push_back(m);
                if kept.len() > half {
                    kept.pop_front();
                    w.first += 1;
                }
            }
            Some(_) if kept.len() < MAX_MATCHES => kept.push_back(m),
            Some(_) => {}
        }
    }
    w.matches = kept.into();
    w
}

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::event::VoidListener;
    use alacritty_terminal::term::{Config, test::TermSize};
    use alacritty_terminal::vte::ansi::Processor;

    fn term(text: &str) -> Term<VoidListener> {
        let mut t = Term::new(Config::default(), &TermSize::new(30, 4), VoidListener);
        Processor::<alacritty_terminal::vte::ansi::StdSyncHandler>::new()
            .advance(&mut t, text.as_bytes());
        t
    }

    fn top(t: &Term<VoidListener>) -> Point {
        Point::new(t.topmost_line(), Column(0))
    }

    fn count(t: &Term<VoidListener>, q: &str, o: FindOptions) -> usize {
        find_around(t, &mut compile(q, o).unwrap(), o, top(t)).total
    }

    #[test]
    fn the_toggles_decide_case_words_and_regex() {
        let t = term("Error error ERROR\r\nterror a.b axb\r\n");
        let none = FindOptions::default();
        assert_eq!(count(&t, "error", none), 4);
        assert_eq!(
            count(&t, "Error", none),
            4,
            "not smart-case: the toggle decides"
        );
        assert_eq!(
            count(
                &t,
                "Error",
                FindOptions {
                    match_case: true,
                    ..none
                }
            ),
            1
        );
        assert_eq!(
            count(
                &t,
                "error",
                FindOptions {
                    whole_word: true,
                    ..none
                }
            ),
            3
        );
        assert_eq!(count(&t, "a.b", none), 1, "a literal dot");
        assert_eq!(
            count(
                &t,
                "a.b",
                FindOptions {
                    regex: true,
                    ..none
                }
            ),
            2
        );
        assert!(
            compile(
                "(",
                FindOptions {
                    regex: true,
                    ..none
                }
            )
            .is_err()
        );
        assert!(compile("(", none).is_ok());
    }

    #[test]
    fn matches_scrolled_into_the_history_are_found_too() {
        let t = term(&"needle\r\nhay\r\n".repeat(10));
        let o = FindOptions::default();
        let w = find_around(&t, &mut compile("needle", o).unwrap(), o, top(&t));
        assert_eq!(
            (w.matches.len(), w.total, w.first, w.at),
            (10, 10, 0, Some(0))
        );
        assert!(w.matches[0].start().line < w.matches[9].start().line);
    }

    #[test]
    fn past_the_marked_ones_the_newest_are_kept_around_the_center() {
        let lines = MAX_MATCHES * 3;
        let t = Term::new(
            Config {
                scrolling_history: lines + 10,
                ..Config::default()
            },
            &TermSize::new(30, 4),
            VoidListener,
        );
        let mut t = t;
        let mut p = Processor::<alacritty_terminal::vte::ansi::StdSyncHandler>::new();
        for i in 0..lines {
            p.advance(&mut t, format!("hit {i}\r\n").as_bytes());
        }
        let o = FindOptions::default();
        let mut regex = compile("hit", o).unwrap();
        // From the bottom: the last ones, the newest output.
        let bottom = Point::new(t.bottommost_line(), Column(0));
        let w = find_around(&t, &mut regex, o, bottom);
        assert_eq!(w.total, lines);
        assert_eq!(w.at, None);
        assert_eq!(w.first + w.matches.len(), lines);
        // From the middle: half before it, the rest after.
        let middle = *find_around(&t, &mut regex, o, top(&t)).matches[MAX_MATCHES - 1].start();
        let w = find_around(&t, &mut regex, o, middle);
        assert_eq!(w.matches.len(), MAX_MATCHES);
        assert_eq!(w.at, Some(MAX_MATCHES / 2));
        assert_eq!(w.first + MAX_MATCHES / 2, MAX_MATCHES - 1);
    }
}
