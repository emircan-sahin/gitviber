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

/// Every match in the scrollback and the screen, top to bottom, up to MAX_MATCHES; true when
/// there were more. Empty matches are left out; whole word wants no word character either side.
pub fn find_all<T>(term: &Term<T>, regex: &mut RegexSearch, o: FindOptions) -> (Vec<Match>, bool) {
    let start = Point::new(term.topmost_line(), Column(0));
    let end = Point::new(term.bottommost_line(), term.last_column());
    let mut out = Vec::new();
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
        if out.len() == MAX_MATCHES {
            return (out, true);
        }
        out.push(m);
    }
    (out, false)
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

    fn count(t: &Term<VoidListener>, q: &str, o: FindOptions) -> usize {
        find_all(t, &mut compile(q, o).unwrap(), o).0.len()
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
        let (found, more) = find_all(
            &t,
            &mut compile("needle", FindOptions::default()).unwrap(),
            FindOptions::default(),
        );
        assert_eq!(found.len(), 10);
        assert!(!more);
        assert!(found[0].start().line < found[9].start().line);
    }
}
