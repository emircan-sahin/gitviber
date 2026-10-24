//! Find in a diff (⌘F): where the query is in a line, as the find box's toggles say.

use std::ops::Range;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FindOptions {
    pub match_case: bool,
    pub whole_word: bool,
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
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
            && (!o.whole_word
                || (!line[..i].chars().next_back().is_some_and(is_word)
                    && !line[i + n..].chars().next().is_some_and(is_word)));
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
    };
    const WORD: FindOptions = FindOptions {
        match_case: false,
        whole_word: true,
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
}
