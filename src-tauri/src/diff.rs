//! Line diff with word-level emphasis, computed here so the UI thread only renders.

use serde::Serialize;
use similar::{Algorithm, ChangeTag, DiffOp, InlineChangeOptions, TextDiff, WhitespaceMode};
use std::time::Duration;

#[derive(Serialize, Debug, PartialEq)]
pub struct Row {
    /// 0 = unchanged, 1 = added, 2 = removed
    pub k: u8,
    /// 1-based line number in the original, 0 when the line only exists in the new file.
    pub o: u32,
    /// 1-based line number in the new file, 0 when removed.
    pub n: u32,
    /// Changed ranges `[start, end)` in UTF-16 units (JS string offsets).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub e: Vec<[u32; 2]>,
}

fn kind(tag: ChangeTag) -> u8 {
    match tag {
        ChangeTag::Equal => 0,
        ChangeTag::Insert => 1,
        ChangeTag::Delete => 2,
    }
}

fn row(tag: ChangeTag, old: Option<usize>, new: Option<usize>, e: Vec<[u32; 2]>) -> Row {
    Row {
        k: kind(tag),
        o: old.map_or(0, |i| i as u32 + 1),
        n: new.map_or(0, |i| i as u32 + 1),
        e,
    }
}

// Word refinement is quadratic-ish; past this a replaced block is shown without emphasis.
const MAX_INLINE_CELLS: usize = 20_000;

/// Length in UTF-16 units, the offsets JS strings use.
fn utf16(s: &str) -> u32 {
    s.encode_utf16().count() as u32
}

/// What a diff may ignore, as git's `-w` (all whitespace) and `-b` (changes in its amount,
/// which covers re-indentation and trailing whitespace).
pub fn whitespace_mode(name: Option<&str>) -> WhitespaceMode {
    match name {
        Some("all") => WhitespaceMode::IgnoreAll,
        Some("amount") => WhitespaceMode::IgnoreChanges,
        _ => WhitespaceMode::Exact,
    }
}

/// The rows, and whether `ws` hid a change: lines paired as unchanged whose text differs.
pub fn rows(old: &str, new: &str, ws: WhitespaceMode) -> (Vec<Row>, bool) {
    let diff = TextDiff::configure()
        .algorithm(Algorithm::Patience)
        .timeout(Duration::from_secs(2))
        .whitespace_mode(ws)
        .diff_lines(old, new);
    let mut opts = InlineChangeOptions::new();
    opts.semantic_cleanup(true);
    let ignoring = ws != WhitespaceMode::Exact;
    let mut hidden = false;

    let mut out = Vec::new();
    for op in diff.ops() {
        if let DiffOp::Replace {
            old_len, new_len, ..
        } = *op
        {
            if old_len * new_len <= MAX_INLINE_CELLS {
                for change in diff.iter_inline_changes_with_options(op, opts) {
                    let mut ranges: Vec<[u32; 2]> = Vec::new();
                    let mut offset = 0u32;
                    for (emphasized, text) in change.iter_strings_lossy() {
                        let text = text.trim_end_matches(['\n', '\r']);
                        let len = utf16(text);
                        let (mut start, mut end) = (offset, offset + len);
                        // Whitespace at a span's edges is what the user asked not to see.
                        if ignoring {
                            start += len - utf16(text.trim_start());
                            end -= len - utf16(text.trim_end());
                        }
                        if emphasized && end > start {
                            match ranges.last_mut() {
                                Some(last) if last[1] == start => last[1] = end,
                                _ => ranges.push([start, end]),
                            }
                        }
                        offset += len;
                    }
                    // A fully emphasized line says nothing beyond the row color.
                    if ranges == [[0, offset]] {
                        ranges.clear();
                    }
                    out.push(row(
                        change.tag(),
                        change.old_index(),
                        change.new_index(),
                        ranges,
                    ));
                }
                continue;
            }
        }
        for change in diff.iter_changes(op) {
            if ignoring && !hidden && change.tag() == ChangeTag::Equal {
                let old = change.old_index().and_then(|i| diff.old_slice(i));
                hidden = old != change.new_index().and_then(|i| diff.new_slice(i));
            }
            out.push(row(
                change.tag(),
                change.old_index(),
                change.new_index(),
                vec![],
            ));
        }
    }
    (out, hidden)
}

/// The texts differ only in line endings (CRLF against LF), which marks every line changed.
pub fn eol_only(old: &str, new: &str) -> bool {
    fn lines(s: &str) -> impl Iterator<Item = &str> {
        s.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l))
    }
    old != new && lines(old).eq(lines(new))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_changed_words() {
        let (r, _) = rows(
            "let a = 1;\nkeep\n",
            "let a = 2;\nkeep\nnew\n",
            WhitespaceMode::Exact,
        );
        assert_eq!(r.len(), 4);
        assert_eq!((r[0].k, r[0].o, r[0].n), (2, 1, 0));
        assert_eq!((r[1].k, r[1].o, r[1].n), (1, 0, 1));
        assert_eq!(r[1].e, vec![[8, 10]]); // semantic cleanup keeps "2;" together
        assert_eq!((r[2].k, r[2].o, r[2].n), (0, 2, 2));
        assert_eq!((r[3].k, r[3].n, r[3].e.len()), (1, 3, 0));
    }

    #[test]
    fn utf16_offsets() {
        let (r, _) = rows("é 😀 x\n", "é 😀 y\n", WhitespaceMode::Exact);
        assert_eq!(r[1].e, vec![[5, 6]]);
    }

    #[test]
    fn ignores_whitespace_but_keeps_the_real_change() {
        let old = "if (a) {\n  let b = 1;\n}\n";
        let new = "if (a) {\n    let b = 2;  \n}\n";
        let (r, _) = rows(old, new, WhitespaceMode::Exact);
        assert_eq!(r[2].e, vec![[0, 4], [12, 16]]);
        // Not the new indentation or the trailing spaces.
        let (r, hidden) = rows(old, new, whitespace_mode(Some("amount")));
        assert_eq!((r[2].e.as_slice(), hidden), (&[[12, 14]][..], false));
        let (r, hidden) = rows(
            old,
            "if (a) {\n    let b = 1;  \n}\n",
            whitespace_mode(Some("amount")),
        );
        assert!(hidden && r.iter().all(|r| r.k == 0));
        // -b still sees whitespace added where there was none; -w doesn't.
        let spaced = "if (a) {\n  let b=1;\n}\n";
        assert!(rows(old, spaced, whitespace_mode(Some("amount")))
            .0
            .iter()
            .any(|r| r.k != 0));
        assert!(rows(old, spaced, whitespace_mode(Some("all")))
            .0
            .iter()
            .all(|r| r.k == 0));
        assert!(!rows(old, old, whitespace_mode(Some("all"))).1);
    }

    #[test]
    fn line_endings_only() {
        assert!(eol_only("a\nb\n", "a\r\nb\r\n"));
        assert!(!eol_only("a\nb\n", "a\nb\n"));
        assert!(!eol_only("a\nb\n", "a\r\nc\r\n"));
    }
}
