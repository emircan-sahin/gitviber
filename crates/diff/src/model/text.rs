//! One side's text, kept once: the rope the view slices lines from is the one the highlighter
//! parses (ropes share their chunks when cloned).

use gitviber_core::git::FileText;
use gpui_kit::base::input::{Rope, RopeExt as _};

#[derive(Clone, Default)]
pub struct SideText {
    pub rope: Rope,
    /// Lines as git counts them: a final newline ends the last line, it doesn't start one.
    pub lines: u32,
    /// False when the file isn't on this side (added, deleted).
    pub exists: bool,
    pub binary: bool,
    pub too_large: bool,
    /// Not the file's own UTF-8 (decoded lossily, UTF-16, a link's target): never written back.
    pub lossy: bool,
    pub lfs_missing: Option<String>,
    pub ends_with_newline: bool,
    pub crlf: bool,
}

impl SideText {
    pub fn new(f: FileText) -> Self {
        let FileText {
            text,
            binary,
            too_large,
            exists,
            lossy,
            lfs_missing,
        } = f;
        let mut side = SideText::from_text(&text);
        side.exists = exists;
        side.binary = binary;
        side.too_large = too_large;
        side.lossy = lossy;
        side.lfs_missing = lfs_missing;
        side
    }

    pub fn from_text(text: &str) -> Self {
        let newlines = text.bytes().filter(|&b| b == b'\n').count() as u32;
        let ends_with_newline = text.ends_with('\n');
        SideText {
            lines: newlines + u32::from(!text.is_empty() && !ends_with_newline),
            exists: true,
            ends_with_newline,
            crlf: text.contains("\r\n"),
            rope: Rope::from(text),
            ..Default::default()
        }
    }

    /// Shown as text: not binary, too large or an LFS pointer without its object.
    pub fn textual(&self) -> bool {
        !self.binary && !self.too_large && self.lfs_missing.is_none()
    }

    pub fn is_empty(&self) -> bool {
        self.rope.len() == 0
    }

    /// Line `line` (1-based) without its line ending; "" past the end.
    pub fn line(&self, line: u32) -> String {
        if line == 0 || line > self.lines {
            return String::new();
        }
        let mut s = self.rope.slice_line(line as usize - 1).to_string();
        if s.ends_with('\r') {
            s.pop();
        }
        s
    }

    /// Where line `line` (1-based) starts in the text.
    pub fn line_start(&self, line: u32) -> usize {
        self.rope.line_start_offset(line.saturating_sub(1) as usize)
    }

    /// Lines `start..=end` joined with newlines, as they'd be copied.
    pub fn lines_text(&self, start: u32, end: u32) -> String {
        let mut out = String::new();
        for l in start.max(1)..=end.min(self.lines) {
            out.push_str(&self.line(l));
            out.push('\n');
        }
        out
    }

    pub fn text(&self) -> String {
        self.rope.to_string()
    }

    /// Bytes held, for the memory budget.
    pub fn len(&self) -> usize {
        self.rope.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_as_git_counts_them() {
        let s = SideText::from_text("a\r\nb\nc");
        assert_eq!(
            (s.lines, s.line(1), s.line(3), s.line(4)),
            (3, "a".into(), "c".into(), String::new())
        );
        assert!(s.crlf && !s.ends_with_newline);
        assert_eq!(SideText::from_text("a\n").lines, 1);
        assert_eq!(SideText::from_text("").lines, 0);
        assert_eq!(SideText::from_text("x\ny\n").lines_text(1, 9), "x\ny\n");
        assert_eq!(SideText::from_text("x\ny\n").line_start(2), 2);
    }
}
