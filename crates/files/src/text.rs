//! Pure text rules of the file view: line endings it can keep, the indent step, and the change
//! bars in its gutter.

use gitviber_core::diff::Row;

/// Whether the file view can save `text` as it found it: the editor joins lines with one kind
/// of ending, so a file mixing CRLF and LF (or with a lone CR) would change on lines no one
/// touched.
pub fn keeps_line_endings(text: &str) -> bool {
    let bytes = text.as_bytes();
    let crlf = text.contains("\r\n");
    let lone_cr = bytes
        .iter()
        .enumerate()
        .any(|(i, b)| *b == b'\r' && bytes.get(i + 1) != Some(&b'\n'));
    let lone_lf = bytes
        .iter()
        .enumerate()
        .any(|(i, b)| *b == b'\n' && (i == 0 || bytes[i - 1] != b'\r'));
    !lone_cr && !(crlf && lone_lf)
}

/// The file's own line ending, for writing back what the editor holds with `\n`.
pub fn uses_crlf(text: &str) -> bool {
    text.contains("\r\n")
}

/// Spaces per indentation level, from the most common step between indented lines; 0 when a
/// tab starts a line or ends its leading spaces, or when nothing is indented. One-space steps are
/// alignment (a JSDoc's " * "), not indentation.
pub fn indent_unit(text: &str) -> usize {
    let mut steps = [0usize; 9];
    let mut prev = 0;
    for line in text.lines() {
        let spaces = line.len() - line.trim_start_matches(' ').len();
        if line[spaces..].starts_with('\t') {
            return 0;
        }
        if line.trim().is_empty() {
            continue;
        }
        if spaces > prev && spaces - prev <= 8 {
            steps[spaces - prev] += 1;
        }
        prev = spaces;
    }
    let mut unit = 0;
    for step in 2..=8 {
        if steps[step] > steps[unit] {
            unit = step;
        }
    }
    unit
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BarKind {
    Added,
    Modified,
    /// Lines were deleted before this one.
    Deleted,
}

/// A line to mark in the gutter (1-based, as the file now has it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bar {
    pub line: u32,
    pub kind: BarKind,
}

/// The gutter's marks from a diff's rows (k: 0 same, 1 added, 2 deleted; n: the new line).
pub fn change_bars(rows: &[Row]) -> Vec<Bar> {
    let mut out = Vec::new();
    let mut last = 0u32;
    let mut i = 0;
    while i < rows.len() {
        if rows[i].k == 0 {
            last = rows[i].n;
            i += 1;
            continue;
        }
        let mut j = i;
        let mut dels = 0;
        while j < rows.len() && rows[j].k != 0 {
            if rows[j].k == 2 {
                dels += 1;
            }
            j += 1;
        }
        let adds: Vec<u32> = rows[i..j]
            .iter()
            .filter(|r| r.k == 1)
            .map(|r| r.n)
            .collect();
        let kind = if dels > 0 {
            BarKind::Modified
        } else {
            BarKind::Added
        };
        out.extend(adds.iter().map(|&line| Bar { line, kind }));
        // Deleted lines: marked on the line after them (the last line at the end of the file).
        if dels > 0 && adds.is_empty() {
            let line = if j < rows.len() {
                rows[j].n
            } else {
                last.max(1)
            };
            out.push(Bar {
                line,
                kind: BarKind::Deleted,
            });
        }
        if let Some(&l) = adds.last() {
            last = l;
        }
        i = j;
    }
    out
}

/// The first line of each change, for next/previous change.
pub fn change_starts(bars: &[Bar]) -> Vec<u32> {
    let mut starts: Vec<u32> = bars
        .iter()
        .enumerate()
        .filter(|(i, b)| *i == 0 || bars[i - 1].line + 1 != b.line)
        .map(|(_, b)| b.line)
        .collect();
    starts.dedup();
    starts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(k: u8, o: u32, n: u32) -> Row {
        Row { k, o, n, e: vec![] }
    }

    #[test]
    fn one_kind_of_line_ending_survives() {
        assert!(keeps_line_endings("a\nb\n"));
        assert!(keeps_line_endings("a\r\nb\r\n"));
        assert!(keeps_line_endings(""));
        assert!(keeps_line_endings("no newline"));
        assert!(!keeps_line_endings("a\r\nb\n"));
        assert!(!keeps_line_endings("\na\r\n"));
        assert!(!keeps_line_endings("a\rb\r"));
        assert!(!keeps_line_endings("a\r\nb\rc\r\n"));
    }

    #[test]
    fn indent_steps() {
        let ts = "function a() {\n  /**\n   * Doc.\n   */\n  if (x) {\n    y();\n  }\n}\n";
        assert_eq!(indent_unit(ts), 2);
        assert_eq!(indent_unit("fn a() {\n    b();\n        c();\n}\n"), 4);
        assert_eq!(indent_unit("a\n  b\n\tc\n"), 0);
        assert_eq!(indent_unit("a\n  b\n  \tc\n"), 0);
        assert_eq!(indent_unit("a\nb\n"), 0);
    }

    #[test]
    fn bars_mark_adds_changes_and_deletions() {
        // 1 same, 2 changed, 3 added, then a deletion before 4, and one at the end.
        let rows = [
            row(0, 1, 1),
            row(2, 2, 0),
            row(1, 0, 2),
            row(0, 3, 3),
            row(1, 0, 4),
            row(0, 4, 5),
            row(2, 5, 0),
            row(0, 6, 6),
            row(2, 7, 0),
        ];
        let bars = change_bars(&rows);
        assert_eq!(
            bars,
            vec![
                Bar {
                    line: 2,
                    kind: BarKind::Modified
                },
                Bar {
                    line: 4,
                    kind: BarKind::Added
                },
                Bar {
                    line: 6,
                    kind: BarKind::Deleted
                },
                Bar {
                    line: 6,
                    kind: BarKind::Deleted
                },
            ]
        );
        assert_eq!(change_starts(&bars), vec![2, 4, 6]);
    }
}
