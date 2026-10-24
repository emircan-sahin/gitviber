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
