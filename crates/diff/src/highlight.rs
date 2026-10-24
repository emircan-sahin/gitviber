//! Syntax colors: each side parsed once off the main thread, then styles read for the lines on
//! screen only, a chunk of lines at a time, and kept for a while.

use gpui_kit::HighlightStyle;
use gpui_kit::base::input::{Rope, RopeExt as _};
use gpui_kit::component::highlighter::{HighlightTheme, SyntaxHighlighter};
use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;

/// Past this a side isn't parsed: a minified bundle or a lockfile costs more than colors help.
pub const MAX_BYTES: usize = 1_500_000;
const CHUNK: usize = 32;
const KEEP_CHUNKS: usize = 256;

/// A line's styles: byte ranges into the line, as the file has it.
pub type LineStyles = Rc<[(Range<usize>, HighlightStyle)]>;

/// The highlighter language for a path ("text" when there's none).
pub fn language_for(path: &str) -> &'static str {
    let name = path
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(path)
        .to_ascii_lowercase();
    match name.as_str() {
        "makefile" | "gnumakefile" => return "make",
        "cmakelists.txt" => return "cmake",
        "cargo.lock" | "pipfile" => return "toml",
        ".bashrc" | ".zshrc" | ".profile" | ".bash_profile" | ".zprofile" | ".envrc" => {
            return "bash";
        }
        _ => {}
    }
    match name.rsplit_once('.').map(|(_, ext)| ext).unwrap_or("") {
        "rs" => "rust",
        "ts" | "mts" | "cts" => "typescript",
        "tsx" => "tsx",
        "js" | "mjs" | "cjs" | "jsx" => "javascript",
        "json" | "jsonc" | "json5" => "json",
        "py" | "pyi" => "python",
        "go" => "go",
        "rb" | "rake" | "gemspec" => "ruby",
        "java" => "java",
        "kt" | "kts" => "kotlin",
        "swift" => "swift",
        "c" | "h" => "c",
        "cc" | "cpp" | "cxx" | "hpp" | "hxx" | "hh" => "cpp",
        "cs" => "csharp",
        "css" | "scss" => "css",
        "html" | "htm" => "html",
        "md" | "mdx" | "markdown" => "markdown",
        "toml" => "toml",
        "yaml" | "yml" => "yaml",
        "sh" | "bash" | "zsh" => "bash",
        "sql" => "sql",
        "lua" => "lua",
        "zig" => "zig",
        "scala" | "sc" => "scala",
        "ex" | "exs" => "elixir",
        "erb" => "erb",
        "ejs" => "ejs",
        "graphql" | "gql" => "graphql",
        "proto" => "proto",
        "svelte" => "svelte",
        "astro" => "astro",
        "php" | "phtml" => "php",
        "cmake" => "cmake",
        "diff" | "patch" => "diff",
        _ => "text",
    }
}

/// Parses `rope` as `language`; blocking, so run it in the background.
pub fn parse(language: &str, rope: &Rope) -> Option<SyntaxHighlighter> {
    if language == "text" || rope.len() > MAX_BYTES {
        return None;
    }
    let mut h = SyntaxHighlighter::new(language);
    h.update(None, rope, None);
    Some(h)
}

/// A parsed side and the styles read from it so far.
pub struct Highlighted {
    parsed: SyntaxHighlighter,
    rope: Rope,
    theme: Arc<HighlightTheme>,
    chunks: RefCell<HashMap<usize, Rc<Vec<LineStyles>>>>,
}

impl Highlighted {
    pub fn new(parsed: SyntaxHighlighter, rope: Rope, theme: Arc<HighlightTheme>) -> Self {
        Highlighted {
            parsed,
            rope,
            theme,
            chunks: RefCell::default(),
        }
    }

    /// Styles of line `line` (1-based) under `theme`.
    pub fn line(&self, line: u32, theme: &Arc<HighlightTheme>) -> Option<LineStyles> {
        if line == 0 {
            return None;
        }
        if !Arc::ptr_eq(theme, &self.theme) {
            return None;
        }
        let ix = line as usize - 1;
        let chunk = ix / CHUNK;
        let mut chunks = self.chunks.borrow_mut();
        if !chunks.contains_key(&chunk) {
            if chunks.len() >= KEEP_CHUNKS {
                chunks.clear();
            }
            chunks.insert(chunk, Rc::new(self.read(chunk)));
        }
        chunks.get(&chunk)?.get(ix % CHUNK).cloned()
    }

    fn read(&self, chunk: usize) -> Vec<LineStyles> {
        let first = chunk * CHUNK;
        let last = (first + CHUNK).min(self.rope.lines_len());
        if first >= last {
            return vec![];
        }
        let starts: Vec<usize> = (first..=last)
            .map(|l| self.rope.line_start_offset(l))
            .collect();
        let range = starts[0]..starts[last - first].min(self.rope.len());
        let styles = self.parsed.styles(&range, &*self.theme);
        let mut out: Vec<Vec<(Range<usize>, HighlightStyle)>> = vec![vec![]; last - first];
        for (r, style) in styles {
            if style == HighlightStyle::default() {
                continue;
            }
            // A style may run over several lines (a block comment): each gets its part.
            let mut l = starts.partition_point(|s| *s <= r.start).saturating_sub(1);
            while l < out.len() && starts[l] < r.end {
                let (ls, le) = (starts[l], starts[l + 1]);
                let (a, b) = (r.start.max(ls) - ls, r.end.min(le) - ls);
                if b > a {
                    out[l].push((a..b, style));
                }
                l += 1;
            }
        }
        out.into_iter().map(Rc::from).collect()
    }

    /// The theme changed: styles are read again.
    pub fn set_theme(&mut self, theme: Arc<HighlightTheme>) {
        self.theme = theme;
        self.chunks.borrow_mut().clear();
    }

    pub fn theme(&self) -> &Arc<HighlightTheme> {
        &self.theme
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn languages_by_name_and_extension() {
        assert_eq!(language_for("src/a/B.TSX"), "tsx");
        assert_eq!(language_for("Makefile"), "make");
        assert_eq!(language_for("x/Cargo.lock"), "toml");
        assert_eq!(language_for("README"), "text");
    }
}
