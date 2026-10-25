//! Find in a diff (⌘F): where the query is in a line, as the find box's toggles say.

use std::ops::Range;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FindOptions {
    pub match_case: bool,
    pub whole_word: bool,
    pub regex: bool,
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// A query ready to run over many lines: a regex is compiled once.
pub enum Finder {
    Text(String, FindOptions),
    Regex(regex::Regex, bool),
}

impl Finder {
    /// Err: why the regex doesn't parse, its parser's last line.
    pub fn new(query: &str, o: FindOptions) -> Result<Finder, String> {
        if !o.regex {
            return Ok(Finder::Text(query.to_string(), o));
        }
        regex::RegexBuilder::new(query)
            .case_insensitive(!o.match_case)
            .build()
            .map(|re| Finder::Regex(re, o.whole_word))
            .map_err(|e| {
                let msg = e.to_string();
                let last = msg.lines().rev().find(|l| !l.trim().is_empty());
                last.unwrap_or("Invalid regex")
                    .trim()
                    .trim_start_matches("error: ")
                    .to_string()
            })
    }

    /// Byte ranges of the query in `line`, left to right, none overlapping or empty.
    pub fn matches(&self, line: &str) -> Vec<Range<usize>> {
        match self {
            Finder::Text(query, o) => matches(line, query, *o),
            Finder::Regex(re, whole_word) => {
                let mut out = vec![];
                let mut at = 0;
                while let Some(m) = re.find_at(line, at) {
                    let (start, end) = (m.start(), m.end());
                    if start < end && (!whole_word || is_whole(line, start, end)) {
                        out.push(start..end);
                        at = end;
                        continue;
                    }
                    // An empty match, or one inside a word: try again a character on.
                    match line[start..].chars().next() {
                        Some(c) => at = start + c.len_utf8(),
                        None => break,
                    }
                }
                out
            }
        }
    }
}

fn is_whole(line: &str, start: usize, end: usize) -> bool {
    !line[..start].chars().next_back().is_some_and(is_word)
        && !line[end..].chars().next().is_some_and(is_word)
}

/// Byte ranges of `query` in `line`, left to right, none overlapping. Without Match Case only
/// ASCII letters fold, so a range is always the line's own bytes.
pub fn matches(line: &str, query: &str, o: FindOptions) -> Vec<Range<usize>> {
    let (hay, needle) = (line.as_bytes(), query.as_bytes());
    let n = needle.len();
    let mut out = vec![];
    if n == 0 || n > hay.len() {
        return out;
    }
    let mut i = 0;
    while i + n <= hay.len() {
        let window = &hay[i..i + n];
        let same = if o.match_case {
            window == needle
        } else {
            window.eq_ignore_ascii_case(needle)
        };
        let found = same
            && line.is_char_boundary(i)
            && line.is_char_boundary(i + n)
            && (!o.whole_word || is_whole(line, i, i + n));
        if found {
            out.push(i..i + n);
            i += n;
        } else {
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const CASE: FindOptions = FindOptions {
        match_case: true,
        whole_word: false,
        regex: false,
    };
    const WORD: FindOptions = FindOptions {
        match_case: false,
        whole_word: true,
        regex: false,
    };
    const REGEX: FindOptions = FindOptions {
        match_case: false,
        whole_word: false,
        regex: true,
    };

    #[test]
    fn ascii_folds_unless_match_case() {
        let any = FindOptions::default();
        assert_eq!(matches("Foo foo FOO", "foo", any), vec![0..3, 4..7, 8..11]);
        assert_eq!(matches("Foo foo FOO", "foo", CASE), vec![4..7]);
        assert_eq!(matches("aaaa", "aa", any), vec![0..2, 2..4]);
        assert!(matches("abc", "", any).is_empty());
    }

    #[test]
    fn whole_words_and_wide_characters() {
        assert_eq!(matches("let x_1 = x + xx;", "x", WORD), vec![10..11]);
        assert_eq!(matches("café cafe", "cafe", WORD), vec![6..10]);
        assert_eq!(matches("ünï ünï", "ünï", CASE), vec![0..5, 6..11]);
    }

    #[test]
    fn a_regex_runs_with_the_other_toggles_and_says_why_it_cannot() {
        let find = |q: &str, o: FindOptions, line: &str| Finder::new(q, o).unwrap().matches(line);
        assert_eq!(
            find(r"f\w+", REGEX, "fn Foo(x: f32)"),
            vec![0..2, 3..6, 10..13]
        );
        let case = FindOptions {
            match_case: true,
            ..REGEX
        };
        assert_eq!(find(r"f\w+", case, "fn Foo(x: f32)"), vec![0..2, 10..13]);
        // Whole Word skips a match inside a word and tries the next start.
        let word = FindOptions {
            whole_word: true,
            ..REGEX
        };
        assert_eq!(find("x+", word, "xx axx x"), vec![0..2, 7..8]);
        // Empty matches find nothing.
        assert!(find("a*", REGEX, "bbb").is_empty());
        // Plain text stays plain with the toggle off.
        assert_eq!(find("a.c", FindOptions::default(), "abc a.c"), vec![4..7]);
        assert!(Finder::new("(", REGEX).is_err_and(|e| !e.is_empty()));
    }
}
