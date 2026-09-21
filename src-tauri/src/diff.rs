//! Line diff with word-level emphasis, computed here so the UI thread only renders.

use serde::Serialize;
use similar::{Algorithm, ChangeTag, DiffOp, InlineChangeOptions, TextDiff};
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

pub fn rows(old: &str, new: &str) -> Vec<Row> {
    let diff = TextDiff::configure()
        .algorithm(Algorithm::Patience)
        .timeout(Duration::from_secs(2))
        .diff_lines(old, new);
    let mut opts = InlineChangeOptions::new();
    opts.semantic_cleanup(true);

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
                    let mut all = true;
                    for (emphasized, text) in change.iter_strings_lossy() {
                        let len = text.trim_end_matches(['\n', '\r']).encode_utf16().count() as u32;
                        if emphasized && len > 0 {
                            match ranges.last_mut() {
                                Some(last) if last[1] == offset => last[1] += len,
                                _ => ranges.push([offset, offset + len]),
                            }
                        } else if len > 0 {
                            all = false;
                        }
                        offset += len;
                    }
                    // A fully emphasized line says nothing beyond the row color.
                    if all {
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
            out.push(row(
                change.tag(),
                change.old_index(),
                change.new_index(),
                vec![],
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_changed_words() {
        let r = rows("let a = 1;\nkeep\n", "let a = 2;\nkeep\nnew\n");
        assert_eq!(r.len(), 4);
        assert_eq!((r[0].k, r[0].o, r[0].n), (2, 1, 0));
        assert_eq!((r[1].k, r[1].o, r[1].n), (1, 0, 1));
        assert_eq!(r[1].e, vec![[8, 10]]); // semantic cleanup keeps "2;" together
        assert_eq!((r[2].k, r[2].o, r[2].n), (0, 2, 2));
        assert_eq!((r[3].k, r[3].n, r[3].e.len()), (1, 3, 0));
    }

    #[test]
    fn utf16_offsets() {
        let r = rows("é 😀 x\n", "é 😀 y\n");
        assert_eq!(r[1].e, vec![[5, 6]]);
    }
}
