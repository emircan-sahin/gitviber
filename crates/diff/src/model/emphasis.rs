//! Word-level changes on a line: core's UTF-16 columns as byte ranges, kept only where they
//! help, and the line's syntax tokens cut where they start and end.

use std::ops::Range;

/// core's UTF-16 `[start, end)` columns of `line` as byte ranges.
pub fn utf16_to_bytes(line: &str, ranges: &[[u32; 2]]) -> Vec<Range<usize>> {
    let at = |unit: u32| {
        let mut u = 0u32;
        for (i, c) in line.char_indices() {
            if u >= unit {
                return i;
            }
            u += c.len_utf16() as u32;
        }
        line.len()
    };
    ranges
        .iter()
        .map(|&[a, b]| at(a)..at(b))
        .filter(|r| r.end > r.start)
        .collect()
}

const MOSTLY: f32 = 0.6;

/// Word-level emphasis only where it helps: never on indentation, and not at all when most of
/// the line changed (the row color already says it, and fragments are just noise).
pub fn useful(text: &str, ranges: Vec<Range<usize>>) -> Vec<Range<usize>> {
    if ranges.is_empty() {
        return ranges;
    }
    let indent = text.len() - text.trim_start().len();
    let ranges: Vec<_> = ranges
        .into_iter()
        .map(|r| r.start.max(indent)..r.end.min(text.len()))
        .filter(|r| {
            r.end > r.start && text.is_char_boundary(r.start) && text.is_char_boundary(r.end)
        })
        .collect();
    let solid = |s: &str| s.chars().filter(|c| !c.is_whitespace()).count();
    let changed: usize = ranges.iter().map(|r| solid(&text[r.clone()])).sum();
    let total = solid(text);
    if total > 0 && changed as f32 / total as f32 <= MOSTLY {
        ranges
    } else {
        vec![]
    }
}

/// A line's tokens cut where the word-level `ranges` start and end: each piece with its
/// token (None between tokens) and whether it's a word change.
pub fn cut<'a, T>(
    len: usize,
    tokens: &'a [(Range<usize>, T)],
    ranges: &[Range<usize>],
) -> Vec<(Range<usize>, Option<&'a T>, bool)> {
    // Tokens with the gaps between them, so the pieces cover the line.
    let mut runs: Vec<(Range<usize>, Option<&T>)> = Vec::with_capacity(tokens.len() * 2 + 1);
    let mut at = 0;
    for (r, t) in tokens {
        let (start, end) = (r.start.max(at).min(len), r.end.min(len));
        if start > at {
            runs.push((at..start, None));
        }
        if end > start {
            runs.push((start..end, Some(t)));
            at = end;
        }
    }
    if at < len || runs.is_empty() {
        runs.push((at..len, None));
    }
    let mut out = Vec::with_capacity(runs.len() + ranges.len() * 2);
    for (run, t) in runs {
        // Every range edge inside this run cuts it.
        let mut cuts: Vec<usize> = ranges
            .iter()
            .flat_map(|r| [r.start, r.end])
            .filter(|&c| c > run.start && c < run.end)
            .collect();
        cuts.push(run.start);
        cuts.push(run.end);
        cuts.sort_unstable();
        cuts.dedup();
        for w in cuts.windows(2) {
            let (a, b) = (w[0], w[1]);
            let emph = ranges.iter().any(|r| a >= r.start && b <= r.end);
            out.push((a..b, t, emph));
        }
        if run.is_empty() && len == 0 {
            out.push((run, t, false));
        }
    }
    out
}

#[cfg(test)]
#[allow(clippy::single_range_in_vec_init)]
mod tests {
    use super::*;

    #[test]
    fn utf16_columns_to_bytes() {
        // As core's test: "é 😀 y" changes at [5, 6).
        assert_eq!(utf16_to_bytes("é 😀 y", &[[5, 6]]), vec![8..9]);
        assert_eq!(utf16_to_bytes("abc", &[[1, 9], [2, 2]]), vec![1..3]);
    }

    #[test]
    fn emphasis_stays_off_indentation_and_mostly_rewritten_lines() {
        assert_eq!(useful("    foo(bar);", vec![0..7]), vec![4..7]);
        assert_eq!(useful("foo(bar);", vec![0..8]), Vec::<Range<usize>>::new());
        assert_eq!(useful("foo(bar);", vec![]), Vec::<Range<usize>>::new());
    }

    fn texts<'a>(
        line: &'a str,
        pieces: &[(Range<usize>, Option<&&str>, bool)],
    ) -> Vec<(&'a str, String, bool)> {
        pieces
            .iter()
            .map(|(r, t, e)| {
                (
                    &line[r.clone()],
                    t.map(|t| t.to_string()).unwrap_or_default(),
                    *e,
                )
            })
            .collect()
    }

    fn want<'a>(v: &[(&'a str, &str, bool)]) -> Vec<(&'a str, String, bool)> {
        v.iter().map(|(a, b, c)| (*a, b.to_string(), *c)).collect()
    }

    #[test]
    fn word_changes_cut_the_tokens_they_cross() {
        let line = "const foo = 1;";
        let tokens = [(0..6, "#c"), (6..9, "#v"), (9..14, "#p")];
        assert_eq!(
            texts(line, &cut(line.len(), &tokens, &[4..9])),
            want(&[
                ("cons", "#c", false),
                ("t ", "#c", true),
                ("foo", "#v", true),
                (" = 1;", "#p", false)
            ])
        );
        assert_eq!(
            texts("ab", &cut(2, &[] as &[(Range<usize>, &str)], &[])),
            want(&[("ab", "", false)])
        );
        // Ranges on a token's edges, empty ones, and past the end of the line.
        let tokens = [(0..2, "#1"), (2..4, "#2")];
        assert_eq!(
            texts("abcd", &cut(4, &tokens, &[2..4])),
            want(&[("ab", "#1", false), ("cd", "#2", true)])
        );
        assert_eq!(
            texts(
                "abcd",
                &cut(4, &[] as &[(Range<usize>, &str)], &[1..1, 3..9])
            ),
            want(&[("a", "", false), ("bc", "", false), ("d", "", true)])
        );
        // Tokens that leave gaps: the gaps are plain.
        assert_eq!(
            texts("a b", &cut(3, &[(2..3, "#v")], &[])),
            want(&[("a ", "", false), ("b", "#v", false)])
        );
    }
}
