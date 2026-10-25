//! Code shows 4 columns an indentation level everywhere: a file indented with fewer spaces a
//! level is drawn wider, tabs go to the next 4-column stop. Only the drawing changes; copies
//! and staging read the file's own text.

pub const TAB: usize = 4;

/// Spaces per indentation level to widen, from the most common step between indented lines;
/// 0 when it's already a tab stop or more, or when a tab starts a line's indentation.
pub fn indent_unit<'a>(texts: impl IntoIterator<Item = &'a str>) -> usize {
    let mut steps = [0usize; 9];
    for text in texts {
        let mut prev = 0;
        for line in text.split('\n') {
            let rest = line.trim_start_matches(' ');
            if rest.starts_with('\t') {
                return 0;
            }
            if rest.trim().is_empty() {
                continue;
            }
            let spaces = line.len() - rest.len();
            if spaces > prev && spaces - prev <= 8 {
                steps[spaces - prev] += 1;
            }
            prev = spaces;
        }
    }
    let mut unit = 0;
    for step in 1..steps.len() {
        if steps[step] > steps[unit] {
            unit = step;
        }
    }
    // One-space steps are alignment (a JSDoc's " * "), not indentation.
    if (2..TAB).contains(&unit) { unit } else { 0 }
}

/// A line as drawn, and where its bytes land in it.
#[derive(Debug, PartialEq)]
pub struct Shown {
    pub text: String,
    map: Option<Map>,
}

#[derive(Debug, PartialEq)]
struct Map {
    unit: usize,
    /// The leading spaces, and what they became.
    indent: (usize, usize),
    /// After each tab past the indentation: (source offset, shown offset).
    tabs: Vec<(usize, usize)>,
}

impl Shown {
    /// `line` widened by `unit` (0: as it is) with its tabs expanded.
    pub fn new(line: &str, unit: usize) -> Self {
        let spaces = if unit > 0 {
            line.len() - line.trim_start_matches(' ').len()
        } else {
            0
        };
        if spaces == 0 && !line.contains('\t') {
            return Shown {
                text: line.to_string(),
                map: None,
            };
        }
        let wide = spaces / unit.max(1) * TAB + spaces % unit.max(1);
        let mut text = String::with_capacity(line.len() + wide);
        text.extend(std::iter::repeat_n(' ', wide));
        let mut tabs = Vec::new();
        let mut col = wide;
        for (i, c) in line[spaces..].char_indices() {
            if c == '\t' {
                let n = TAB - col % TAB;
                text.extend(std::iter::repeat_n(' ', n));
                col += n;
                tabs.push((spaces + i + 1, text.len()));
            } else {
                text.push(c);
                col += 1;
            }
        }
        Shown {
            text,
            map: Some(Map {
                unit: unit.max(1),
                indent: (spaces, wide),
                tabs,
            }),
        }
    }

    /// Where byte `x` of the source line is in `text`.
    pub fn offset(&self, x: usize) -> usize {
        let Some(m) = &self.map else {
            return x.min(self.text.len());
        };
        let (src, wide) = m.indent;
        if x < src {
            return x / m.unit * TAB + x % m.unit;
        }
        let (s, d) = m
            .tabs
            .iter()
            .rev()
            .find(|(s, _)| *s <= x)
            .copied()
            .unwrap_or((src, wide));
        (d + (x - s)).min(self.text.len())
    }
}

/// Where `n` leading spaces land once widened by `unit`.
fn widened(n: usize, unit: usize) -> usize {
    n / unit * TAB + n % unit
}

/// `text` for an editor that shows 4 columns a level: each line's leading spaces widened by
/// `unit` (0: as it is). Its own lines come back exactly through `narrow`, as the remainder of
/// a level is under `unit`, which is under a tab stop.
pub fn widen(text: &str, unit: usize) -> String {
    reindent(text, |n| if unit == 0 { n } else { widened(n, unit) })
}

/// Text from a widened editor as the file holds it: each level of 4 back to `unit` spaces.
pub fn narrow(text: &str, unit: usize) -> String {
    reindent(text, |n| {
        if unit == 0 {
            n
        } else {
            n / TAB * unit + n % TAB
        }
    })
}

fn reindent(text: &str, to: impl Fn(usize) -> usize) -> String {
    let mut out = String::with_capacity(text.len() + text.len() / 4);
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let rest = line.trim_start_matches(' ');
        out.extend(std::iter::repeat_n(' ', to(line.len() - rest.len())));
        out.push_str(rest);
    }
    out
}

/// Text pasted into a widened editor where `before` is its line up to the caret: its lines'
/// indentation widened by `unit`, the first line's only where the caret is in the line's.
pub fn widen_paste(text: &str, before: &str, unit: usize) -> String {
    if unit == 0 {
        return text.to_string();
    }
    let (first, rest) = text
        .split_once('\n')
        .map_or((text, None), |(a, b)| (a, Some(b)));
    let mut out = if before.bytes().all(|b| b == b' ') {
        widen(first, unit)
    } else {
        first.to_string()
    };
    if let Some(rest) = rest {
        out.push('\n');
        out.push_str(&widen(rest, unit));
    }
    out
}

/// Where column `col` of a file's `line` shows once the line is widened by `unit`. Leading
/// spaces are one unit each in bytes and UTF-16 alike, so either counts.
pub fn widen_column(line: &str, col: usize, unit: usize) -> usize {
    let spaces = line.len() - line.trim_start_matches(' ').len();
    if unit == 0 || spaces == 0 {
        col
    } else if col <= spaces {
        widened(col, unit)
    } else {
        col - spaces + widened(spaces, unit)
    }
}

/// Where column `col` of a widened `shown` line is in the file's line.
pub fn narrow_column(shown: &str, col: usize, unit: usize) -> usize {
    let spaces = shown.len() - shown.trim_start_matches(' ').len();
    let back = |n: usize| n / TAB * unit + n % TAB;
    if unit == 0 || spaces == 0 {
        col
    } else if col <= spaces {
        back(col)
    } else {
        col - spaces + back(spaces)
    }
}

/// Columns the widest line takes once drawn: the view's scroll width.
pub fn widest(text: &str, unit: usize) -> usize {
    text.split('\n')
        .map(|line| {
            if line.len() < 40 && !line.contains('\t') {
                return line.chars().count();
            }
            let spaces = if unit > 0 {
                line.len() - line.trim_start_matches(' ').len()
            } else {
                0
            };
            let mut col = spaces / unit.max(1) * TAB + spaces % unit.max(1);
            for c in line[spaces..].chars() {
                col += if c == '\t' { TAB - col % TAB } else { 1 };
            }
            col
        })
        .max()
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_from_the_common_step() {
        assert_eq!(indent_unit(["a\n  b\n    c\n  d\n"]), 2);
        assert_eq!(indent_unit(["a\n    b\n"]), 0);
        assert_eq!(indent_unit(["a\n\tb\n  c\n    d\n"]), 0);
        // JSDoc alignment.
        assert_eq!(indent_unit(["/**\n * a\n */\n"]), 0);
        assert_eq!(indent_unit(["", "x\n   y\n"]), 3);
    }

    #[test]
    fn nothing_indented_nothing_to_widen() {
        assert_eq!(indent_unit(["a\nb\n"]), 0);
        assert_eq!(indent_unit(["", "\n\n"]), 0);
        assert_eq!(indent_unit(std::iter::empty::<&str>()), 0);
        // A tab after the leading spaces couldn't be widened exactly.
        assert_eq!(indent_unit(["a\n  b\n  \tc\n"]), 0);
    }

    #[test]
    fn widening_turns_back_exactly() {
        let ts = "function a() {\n  /**\n   * Doc.\n   */\n  if (x) {\n    y();\n  }\n}\n";
        let wide = widen(ts, 2);
        assert_eq!(
            wide,
            "function a() {\n    /**\n     * Doc.\n     */\n    if (x) {\n        y();\n    }\n}\n"
        );
        assert_eq!(narrow(&wide, 2), ts);
        let three = "a\n   b\n      c\n    d\n";
        assert_eq!(narrow(&widen(three, 3), 3), three);
        assert_eq!(widen(ts, 0), ts);
        assert_eq!(narrow("\r\n  x", 0), "\r\n  x");
        // Every count of leading spaces, whatever follows them.
        for n in 0..40 {
            let line = format!("{}\tx  y", " ".repeat(n));
            assert_eq!(narrow(&widen(&line, 2), 2), line);
            assert_eq!(narrow(&widen(&line, 3), 3), line);
        }
    }

    #[test]
    fn a_paste_is_widened_where_it_lands_in_indentation() {
        let block = "  a();\n    b();\n  c();";
        assert_eq!(
            widen_paste(block, "    ", 2),
            "    a();\n        b();\n    c();"
        );
        assert_eq!(
            widen_paste(block, "", 2),
            "    a();\n        b();\n    c();"
        );
        // After code on its line, the first line's spaces are the text's own.
        assert_eq!(
            widen_paste(block, "x =", 2),
            "  a();\n        b();\n    c();"
        );
        assert_eq!(widen_paste(block, "    ", 0), block);
        // Saved, what was pasted under a level is the file's level and the paste's own.
        let shown = format!("    {}", widen_paste(block, "    ", 2));
        assert_eq!(narrow(&shown, 2), format!("  {block}"));
    }

    #[test]
    fn columns_follow_the_widened_indentation() {
        assert_eq!(widen_column("    y();", 4, 2), 8);
        assert_eq!(widen_column("    y();", 6, 2), 10);
        assert_eq!(widen_column("    y();", 3, 2), 5);
        assert_eq!(widen_column("   * Doc", 3, 2), 5);
        assert_eq!(widen_column("y();", 2, 2), 2);
        assert_eq!(widen_column("    y();", 6, 0), 6);
        for col in 0..12 {
            let line = "      x = 1;";
            let shown = widen(line, 2);
            assert_eq!(narrow_column(&shown, widen_column(line, col, 2), 2), col);
        }
    }

    #[test]
    fn widened_lines_and_their_offsets() {
        let s = Shown::new("    let x = 1;", 2);
        assert_eq!(s.text, "        let x = 1;");
        assert_eq!((s.offset(4), s.offset(8), s.offset(2)), (8, 12, 4));
        let t = Shown::new("a\tb", 0);
        assert_eq!(t.text, "a   b");
        assert_eq!((t.offset(0), t.offset(2), t.offset(3)), (0, 4, 5));
        assert_eq!(Shown::new("plain", 2).offset(3), 3);
        assert_eq!(widest("ab\n\tx\n", 0), 5);
    }
}
