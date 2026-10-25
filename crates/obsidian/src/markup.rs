//! Obsidian's syntax over markdown, rewritten into what gitviber-markdown renders: [[wikilinks]]
//! and markdown links to notes become links to the file they resolve to, ![[embeds]] of images
//! become images, ==highlights== and #tags `<mark>`s, %%comments%% and ^block-ids go. A note is
//! also cut into parts at its headings and at embeds alone on a line, so a long note is drawn a
//! part at a time and an embedded note can show in its own box. The rules follow Obsidian's
//! documented ones, as the old app's remark plugins did.

use std::cell::{Cell, OnceCell};
use std::collections::{HashMap, HashSet};
use std::ops::Range;

use gitviber_files::media::{MediaKind, media_kind};

use crate::links::{LinkIndex, note_name, split_target};

/// The highlight Obsidian's default theme gives ==text==, and a tag's tint: both read on either theme.
const HIGHLIGHT: &str = "#ffd00066";
const TAG: &str = "#a882ff33";

/// A link's href as the views write it: the vault-relative file, then (after a NUL, which no
/// file name holds) the heading or ^block. Percent-encoded, so markdown takes it as one path.
pub fn href(path: &str, anchor: &str) -> String {
    let mut out = String::from("/");
    encode(path, &mut out);
    if !anchor.is_empty() {
        out.push_str("%00");
        encode(anchor, &mut out);
    }
    out
}

/// The href of a link to a file the vault doesn't have: followed, it says so.
fn missing_href(name: &str) -> String {
    let mut out = String::from("/%01");
    encode(name, &mut out);
    out
}

fn encode(s: &str, out: &mut String) {
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~/".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
}

/// Where a followed link goes, from the path gitviber-markdown hands back (`href`, decoded).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// A file of the vault, and the part of it the link names ("" for none).
    File { path: String, anchor: String },
    /// A name the vault has no file for.
    Missing(String),
}

pub fn target(path: &str) -> Target {
    if let Some(name) = path.strip_prefix('\u{1}') {
        return Target::Missing(name.to_string());
    }
    match path.split_once('\0') {
        Some((p, a)) => Target::File {
            path: p.to_string(),
            anchor: a.to_string(),
        },
        None => Target::File {
            path: path.to_string(),
            anchor: String::new(),
        },
    }
}

/// A wikilink's parts: `[[path#anchor|alias]]`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Wikilink {
    pub path: String,
    /// A heading (subheadings joined by #), or ^block-id.
    pub anchor: String,
    pub alias: Option<String>,
}

/// `inner` of `[[inner]]`; a table cell's escaped `\|` is the alias bar.
pub fn parse_wikilink(inner: &str) -> Wikilink {
    let inner = inner.replace("\\|", "|");
    let (target, alias) = match inner.split_once('|') {
        Some((t, a)) => (t, Some(a.trim().to_string())),
        None => (inner.as_str(), None),
    };
    let (path, anchor) = split_target(target);
    Wikilink {
        path: path.trim().to_string(),
        anchor: anchor.trim().to_string(),
        alias,
    }
}

/// How Obsidian shows a wikilink without an alias: "note > Heading".
pub fn wikilink_label(w: &Wikilink) -> String {
    if let Some(alias) = w.alias.as_ref().filter(|a| !a.is_empty()) {
        return alias.clone();
    }
    let parts: Vec<&str> = std::iter::once(w.path.as_str())
        .filter(|p| !p.is_empty())
        .chain(w.anchor.split('#').filter(|s| !s.is_empty()))
        .collect();
    if parts.is_empty() {
        w.path.clone()
    } else {
        parts.join(" > ")
    }
}

/// A heading as a link names it. Obsidian leaves # | ^ : % [ ] out when it links a heading
/// ("Q&A: Part 1" is [[Note#Q&A Part 1]]), so neither those nor case or runs of spaces count.
pub fn heading_key(heading: &str) -> String {
    let spaced: String = heading
        .chars()
        .map(|c| if "#|^:%[]\\".contains(c) { ' ' } else { c })
        .collect();
    spaced
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// A heading's text as it reads: links reduced to their text, emphasis and code marks dropped.
fn heading_text(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(c) = rest.chars().next() {
        if let Some(after) = rest.strip_prefix("[[")
            && let Some(end) = after.find("]]")
        {
            out.push_str(&wikilink_label(&parse_wikilink(&after[..end])));
            rest = &after[end + 2..];
            continue;
        }
        if c == '['
            && let Some(link) = parse_link(&Line::new(rest), 0)
        {
            out.push_str(&rest[1..1 + link.text_len]);
            rest = &rest[link.end..];
            continue;
        }
        if !matches!(c, '*' | '_' | '`' | '=') {
            out.push(c);
        }
        rest = &rest[c.len_utf8()..];
    }
    out
}

/// A code fence line, also inside a callout or quote ("> ```") or a list item, at any indent
/// there: its marker character, its length, and what follows it.
fn fence_of(line: &str) -> Option<(char, usize, &str)> {
    let mut s = line;
    loop {
        let t = s.trim_start_matches([' ', '\t']);
        if let Some(r) = t.strip_prefix('>') {
            s = r;
            continue;
        }
        let mut chars = t.chars();
        if let (Some('-' | '*' | '+'), Some(' ' | '\t')) = (chars.next(), chars.next()) {
            s = &t[1..];
            continue;
        }
        let digits = t.bytes().take_while(u8::is_ascii_digit).count();
        if (1..=9).contains(&digits)
            && matches!(t.as_bytes().get(digits), Some(b'.' | b')'))
            && matches!(t.as_bytes().get(digits + 1), Some(b' ' | b'\t'))
        {
            s = &t[digits + 1..];
            continue;
        }
        s = t;
        break;
    }
    let c = s.chars().next().filter(|c| *c == '`' || *c == '~')?;
    let n = s.chars().take_while(|x| *x == c).count();
    (n >= 3).then(|| (c, n, &s[n..]))
}

/// Tracks code fences line by line: whether a line is code (the fence lines included).
#[derive(Default)]
struct Fences {
    open: Option<(char, usize)>,
}

impl Fences {
    fn code(&mut self, line: &str) -> bool {
        match (self.open, fence_of(line)) {
            (Some((c, n)), Some((c2, n2, rest)))
                if c == c2 && n2 >= n && rest.trim().is_empty() =>
            {
                self.open = None;
                true
            }
            (Some(_), _) => true,
            (None, Some((c, n, _))) => {
                self.open = Some((c, n));
                true
            }
            (None, None) => false,
        }
    }
}

/// Obsidian's %%comments%%, inline or over many lines, cut out; not inside code. One left open
/// hides the rest of the note, as in Obsidian.
pub fn strip_comments(src: &str) -> String {
    if !src.contains("%%") {
        return src.to_string();
    }
    let mut out = String::with_capacity(src.len());
    let mut fences = Fences::default();
    let mut comment = false;
    let lines: Vec<&str> = src.split('\n').collect();
    for (n, line) in lines.iter().enumerate() {
        let eol = if n + 1 < lines.len() { "\n" } else { "" };
        if !comment && fences.code(line) {
            out.push_str(line);
            out.push_str(eol);
            continue;
        }
        let mut i = 0;
        while i < line.len() {
            let rest = &line[i..];
            if comment {
                match rest.find("%%") {
                    Some(end) => {
                        comment = false;
                        i += end + 2;
                    }
                    None => i = line.len(),
                }
                continue;
            }
            if rest.starts_with('`') {
                let end = i + code_span(rest);
                out.push_str(&line[i..end]);
                i = end;
                continue;
            }
            if rest.starts_with("%%") {
                comment = true;
                i += 2;
                continue;
            }
            let c = rest.chars().next().unwrap_or(' ');
            out.push(c);
            i += c.len_utf8();
        }
        if !comment {
            out.push_str(eol);
        }
    }
    out
}

/// The length of the inline code span `s` starts with: to the next run of as many backticks,
/// or the run alone when none closes it.
fn code_span(s: &str) -> usize {
    let run = s.bytes().take_while(|b| *b == b'`').count();
    let ticks = &s[..run];
    match s[run..].find(ticks) {
        Some(close) => run + close + run,
        None => run,
    }
}

/// The note's properties block (`---` … `---`) and the rest.
pub fn split_frontmatter(src: &str) -> (&str, &str) {
    let first = src.split_inclusive('\n').next().unwrap_or("");
    if first.trim_end() != "---" {
        return ("", src);
    }
    let mut at = first.len();
    for line in src[at..].split_inclusive('\n') {
        at += line.len();
        if matches!(line.trim_end(), "---" | "...") {
            return (&src[..at], &src[at..]);
        }
    }
    ("", src)
}

/// An ATX heading's level and text, when `line` is one (indented at most three spaces).
fn heading(line: &str) -> Option<(usize, &str)> {
    let line = line.trim_end_matches(['\n', '\r']);
    let indent = line.len() - line.trim_start_matches(' ').len();
    if indent > 3 {
        return None;
    }
    let t = &line[indent..];
    let level = t.bytes().take_while(|b| *b == b'#').count();
    let rest = &t[level..];
    if !(1..=6).contains(&level) || !(rest.is_empty() || rest.starts_with([' ', '\t'])) {
        return None;
    }
    let text = rest.trim();
    // "## Title ##": a closing run of #'s needs a space before it.
    let closed = text.trim_end_matches('#');
    let text = if closed.is_empty() {
        ""
    } else if closed.ends_with([' ', '\t']) {
        closed.trim_end()
    } else {
        text
    };
    Some((level, text))
}

/// The ^block-id a line ends with, and where the line's text before it ends.
fn block_id(line: &str) -> Option<(usize, &str)> {
    let t = line.trim_end();
    let caret = t.rfind('^')?;
    let id = &t[caret + 1..];
    if id.is_empty() || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
        return None;
    }
    let before = &t[..caret];
    if !(before.is_empty() || before.ends_with([' ', '\t'])) {
        return None;
    }
    Some((before.trim_end().len(), id))
}

/// `![[target|alias]]` alone on its line: an embed shown as a block.
fn block_embed(line: &str) -> Option<(&str, String)> {
    let t = line.trim();
    if line.len() - line.trim_start().len() > 3 {
        return None;
    }
    let inner = t.strip_prefix("![[")?.strip_suffix("]]")?;
    if inner.contains(['[', ']']) {
        return None;
    }
    let w = parse_wikilink(inner);
    let target = &inner[..inner.find('|').unwrap_or(inner.len())];
    Some((target.trim(), w.alias.unwrap_or_default()))
}

/// What a note shows one after another.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Part {
    Text {
        /// The note's properties, as written, in front of its first part.
        front: String,
        /// Obsidian markdown, rewritten when it's drawn (`to_markdown`).
        source: String,
        /// The key (`heading_key`) of the heading the part starts with.
        heading: Option<String>,
        /// The ^block-ids in it.
        ids: Vec<String>,
    },
    /// `![[target|alias]]` alone on a line.
    Embed { target: String, alias: String },
}

/// `src` cut into parts: at each embed alone on a line, and when `by_heading`, before each
/// heading too (unless reference definitions or footnotes, which work only within one part,
/// are in it). Comments are already gone.
pub fn parts(src: &str, by_heading: bool) -> Vec<Part> {
    let (front, body) = split_frontmatter(src);
    let by_heading = by_heading && !has_definitions(body);
    let mut out = Vec::new();
    let mut text = String::new();
    let mut ids = Vec::new();
    let mut head: Option<String> = None;
    let mut front = front.to_string();
    let mut fences = Fences::default();
    let flush = |out: &mut Vec<Part>,
                 text: &mut String,
                 ids: &mut Vec<String>,
                 head: &mut Option<String>,
                 front: &mut String| {
        if !text.trim().is_empty() || !front.is_empty() {
            out.push(Part::Text {
                front: std::mem::take(front),
                source: std::mem::take(text),
                heading: head.take(),
                ids: std::mem::take(ids),
            });
        }
        text.clear();
        ids.clear();
        *head = None;
    };
    for line in body.split_inclusive('\n') {
        if fences.code(line) {
            text.push_str(line);
            continue;
        }
        if let Some((target, alias)) = block_embed(line) {
            flush(&mut out, &mut text, &mut ids, &mut head, &mut front);
            out.push(Part::Embed {
                target: target.to_string(),
                alias,
            });
            continue;
        }
        if by_heading && let Some((_, h)) = heading(line) {
            flush(&mut out, &mut text, &mut ids, &mut head, &mut front);
            head = Some(heading_key(&heading_text(h)));
        }
        if let Some((_, id)) = block_id(line) {
            ids.push(format!("^{id}"));
        }
        text.push_str(line);
    }
    flush(&mut out, &mut text, &mut ids, &mut head, &mut front);
    out
}

/// Reference link definitions or footnotes (`[x]: …`, `[^1]: …`): they name text elsewhere.
/// Inline footnotes (`^[…]`) too: their notes, numbered through the note, go after all of it.
fn has_definitions(body: &str) -> bool {
    body.contains("^[")
        || body.lines().any(|l| {
            let t = l.trim_start();
            l.len() - t.len() <= 3
                && t.starts_with('[')
                && t.find("]:")
                    .is_some_and(|i| i > 1 && !t[1..i].contains(']'))
        })
}

/// What an embed of `src` shows: for `Heading` the heading and what's under it, to the next
/// heading as high; for `^id` the block with that id; for the whole note (""), all but its
/// properties. Err: Obsidian's words for a part the note doesn't have.
pub fn section(src: &str, anchor: &str) -> Result<String, String> {
    let (_, body) = split_frontmatter(src);
    if anchor.is_empty() {
        return Ok(body.to_string());
    }
    let lines: Vec<&str> = body.split_inclusive('\n').collect();
    let mut fences = Fences::default();
    let code: Vec<bool> = lines.iter().map(|l| fences.code(l)).collect();
    if let Some(id) = anchor.strip_prefix('^') {
        let block = block_lines(&lines, &code, id).ok_or(format!("Unable to find block ^{id}"))?;
        return Ok(lines[block].concat());
    }
    let want = heading_key(anchor.split('#').rfind(|s| !s.is_empty()).unwrap_or(""));
    let found = (0..lines.len()).find_map(|i| {
        let (level, text) = heading(lines[i]).filter(|_| !code[i])?;
        (heading_key(&heading_text(text)) == want).then_some((i, level))
    });
    let (start, level) = found.ok_or_else(|| format!("Unable to find section #{anchor}"))?;
    let end = (start + 1..lines.len())
        .find(|&i| !code[i] && heading(lines[i]).is_some_and(|(l, _)| l <= level))
        .unwrap_or(lines.len());
    Ok(lines[start..end].concat())
}

/// The lines of the block `^id` names: the one it ends, a list item's own line, or the block
/// before an id alone on its line (a table, a list). `code`: which lines are in code fences.
fn block_lines(lines: &[&str], code: &[bool], id: &str) -> Option<Range<usize>> {
    let at =
        (0..lines.len()).find(|&i| !code[i] && block_id(lines[i]).is_some_and(|(_, b)| b == id))?;
    let blank = |i: usize| lines[i].trim().is_empty();
    let end = if block_id(lines[at]).is_some_and(|(len, _)| len == 0) {
        let mut e = at;
        while e > 0 && blank(e - 1) {
            e -= 1;
        }
        e
    } else {
        at + 1
    };
    let is_item = |l: &str| {
        let t = l.trim_start();
        t.starts_with(['-', '*', '+']) && t[1..].starts_with([' ', '\t'])
            || t.split_once(['.', ')']).is_some_and(|(n, r)| {
                !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) && r.starts_with(' ')
            })
    };
    let start = if end == at + 1 && is_item(lines[at]) {
        at
    } else {
        let mut s = end.saturating_sub(1);
        while s > 0 && !blank(s - 1) {
            s -= 1;
        }
        s
    };
    (start < end).then_some(start..end)
}

/// Where the block `^id` is in a part's markdown (`to_markdown` keeps its lines), as a byte
/// range of its `source`.
pub fn block_range(source: &str, markdown: &str, id: &str) -> Option<Range<usize>> {
    let lines: Vec<&str> = source.split_inclusive('\n').collect();
    let mut fences = Fences::default();
    let code: Vec<bool> = lines.iter().map(|l| fences.code(l)).collect();
    let want = block_lines(&lines, &code, id)?;
    let mut at = 0;
    let mut range = None::<Range<usize>>;
    for (n, line) in markdown.split_inclusive('\n').enumerate().take(want.end) {
        let end = at + line.trim_end_matches(['\n', '\r']).len();
        if n >= want.start {
            range = Some(range.map_or(at..end, |r| r.start..end));
        }
        at += line.len();
    }
    range.filter(|r| !r.is_empty())
}

/// What the rewriting needs to know: the vault's files and the note being drawn.
pub struct Context<'a> {
    pub index: &'a LinkIndex,
    /// The note's vault-relative path; links resolve from its folder.
    pub note: &'a str,
}

impl Context<'_> {
    fn link_href(&self, path: &str, anchor: &str) -> String {
        if path.is_empty() {
            return href(self.note, anchor);
        }
        match self.index.resolve(path, self.note) {
            Some(file) => href(&file, anchor),
            None => missing_href(path),
        }
    }
}

/// Backslashes in front of what markdown or a table would read as syntax.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if "\\[]`*_<|#!=~$".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn is_external(href: &str) -> bool {
    let Some((scheme, _)) = href.split_once(':') else {
        return href.starts_with("//");
    };
    let mut chars = scheme.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-'))
}

/// "300", "300x200", "alt|300" or "alt|300x200" (an image's alt text, an embed's alias) sets its
/// size, as Obsidian reads it; "Photo 2023" is just alt text.
fn sized(alt: &str) -> (&str, Option<(u32, Option<u32>)>) {
    let (text, size) = match alt.rsplit_once('|') {
        Some((t, s)) => (t, s),
        None => ("", alt),
    };
    let size = size.trim();
    let (w, h) = match size.split_once('x') {
        Some((w, h)) => (w, Some(h)),
        None => (size, None),
    };
    let num = |s: &str| {
        (!s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
            .then(|| s.parse().ok())
            .flatten()
    };
    match (num(w), h.map(num)) {
        (Some(w), None) => (text, Some((w, None))),
        (Some(w), Some(Some(h))) => (text, Some((w, Some(h)))),
        _ => (alt, None),
    }
}

fn image(src: &str, alt: &str) -> String {
    match sized(alt) {
        (alt, Some((w, h))) => {
            let alt: String = alt.chars().filter(|c| !"\"<>&".contains(*c)).collect();
            let h = h.map(|h| format!(" height=\"{h}\"")).unwrap_or_default();
            format!("<img src=\"{src}\" alt=\"{alt}\" width=\"{w}\"{h}>")
        }
        (alt, None) => format!("![{}]({src})", escape(alt)),
    }
}

fn not_here(name: &str) -> String {
    format!("*“{}” isn't in this vault*", escape(name))
}

/// A part's Obsidian markdown as gitviber-markdown renders it, its properties in front as they
/// are. Line for line, but for its inline footnotes, which go after it as markdown's.
pub fn to_markdown(front: &str, source: &str, cx: &Context) -> String {
    let mut out = String::with_capacity(front.len() + source.len() + source.len() / 8);
    out.push_str(front);
    let mut notes = Notes::new(source);
    let mut fences = Fences::default();
    let mut math = false;
    let mut prev_blank = true;
    let mut in_list = false;
    let mut indented = false;
    for line in source.split_inclusive('\n') {
        let (body, eol) = match line.strip_suffix('\n') {
            Some(b) => (b, "\n"),
            None => (line, ""),
        };
        let blank = body.trim().is_empty();
        if fences.code(body) {
            out.push_str(line);
            prev_blank = false;
            continue;
        }
        let t = body.trim();
        let opens = t.starts_with("$$") && !(t.len() >= 4 && t.ends_with("$$"));
        if opens || math && t.ends_with("$$") {
            math = !math;
            out.push_str(line);
            continue;
        }
        let indent = body.len() - body.trim_start_matches([' ', '\t']).len();
        let deep = indent >= 4 || body.starts_with('\t');
        if indent < 4 && is_list_item(body.trim_start()) {
            in_list = true;
        } else if !blank && indent == 0 && prev_blank {
            in_list = false;
        }
        // Indented code: after a blank line and outside a list, on while lines stay indented.
        indented = if indented {
            blank || deep
        } else {
            deep && !in_list && prev_blank
        };
        if math || indented {
            out.push_str(line);
            prev_blank = blank;
            continue;
        }
        prev_blank = blank;
        let body = match block_id(body) {
            Some((len, _)) => &body[..len],
            None => body,
        };
        match task_status(body) {
            // Its status in a box (Obsidian's themes draw it as an icon); `-` is dropped.
            Some((lead, status, text)) => {
                let status = match status {
                    '<' => "&lt;".to_string(),
                    '&' => "&amp;".to_string(),
                    c => c.to_string(),
                };
                out.push_str(&format!("{lead}<code>{status}</code> "));
                let dropped = status == "-";
                if dropped {
                    out.push_str("<s>");
                }
                inline(text, cx, &mut notes, &mut out);
                if dropped {
                    out.push_str("</s>");
                }
            }
            None => inline(body, cx, &mut notes, &mut out),
        }
        out.push_str(eol);
    }
    if !notes.list.is_empty() {
        out.push_str(if out.ends_with('\n') { "\n" } else { "\n\n" });
        for (id, note) in notes.list {
            out.push_str(&format!("[^{id}]: {note}\n"));
        }
    }
    out
}

fn is_list_item(t: &str) -> bool {
    let mut chars = t.chars();
    if let (Some('-' | '*' | '+'), Some(' ' | '\t') | None) = (chars.next(), chars.next()) {
        return true;
    }
    let digits = t.bytes().take_while(u8::is_ascii_digit).count();
    (1..=9).contains(&digits)
        && matches!(t.as_bytes().get(digits), Some(b'.' | b')'))
        && matches!(t.as_bytes().get(digits + 1), Some(b' ' | b'\t') | None)
}

/// A line read for links, its brackets matched up once and the next `)` or `>` remembered, so
/// a line full of unclosed ones costs no more to read than a plain one.
struct Line<'a> {
    s: &'a str,
    /// The `]` closing each `[`, by their byte offsets.
    close: OnceCell<HashMap<usize, usize>>,
    /// The last `)` and `>` looked for: from where, and where the first one is (the line's
    /// length: none), which answers any later look from between the two.
    paren: Cell<(usize, usize)>,
    gt: Cell<(usize, usize)>,
}

impl<'a> Line<'a> {
    fn new(s: &'a str) -> Line<'a> {
        Line {
            s,
            close: OnceCell::new(),
            paren: Cell::new((usize::MAX, 0)),
            gt: Cell::new((usize::MAX, 0)),
        }
    }

    /// The `]` closing the `[` at `at`, past escapes, code spans and nested pairs.
    fn close(&self, at: usize) -> Option<usize> {
        let close = self.close.get_or_init(|| {
            let (s, b) = (self.s, self.s.as_bytes());
            let mut close = HashMap::new();
            let mut open = Vec::new();
            let mut i = 0;
            while let Some(c) = b.get(i) {
                match c {
                    b'\\' => i += 2,
                    b'`' => i += code_span(&s[i..]),
                    b'[' => {
                        open.push(i);
                        i += 1;
                    }
                    b']' => {
                        if let Some(o) = open.pop() {
                            close.insert(o, i);
                        }
                        i += 1;
                    }
                    _ => i += 1,
                }
            }
            close
        });
        close.get(&at).copied()
    }

    fn next(&self, memo: &Cell<(usize, usize)>, c: u8, from: usize) -> Option<usize> {
        let len = self.s.len();
        let (was, found) = memo.get();
        let at = if was <= from && from <= found {
            found
        } else {
            let at = self.s.as_bytes()[from.min(len)..]
                .iter()
                .position(|b| *b == c)
                .map_or(len, |p| from + p);
            memo.set((from, at));
            at
        };
        (at < len).then_some(at)
    }

    /// The first `)` at or after `from`.
    fn paren(&self, from: usize) -> Option<usize> {
        self.next(&self.paren, b')', from)
    }

    /// The first `>` at or after `from`.
    fn gt(&self, from: usize) -> Option<usize> {
        self.next(&self.gt, b'>', from)
    }
}

/// A markdown link or image in a line, at its `[`: where its text ends, its destination and
/// title as written, and where it ends (byte offsets in the line).
struct MdLink<'a> {
    text_len: usize,
    dest: &'a str,
    title: &'a str,
    end: usize,
}

fn parse_link<'a>(line: &Line<'a>, at: usize) -> Option<MdLink<'a>> {
    let (s, b) = (line.s, line.s.as_bytes());
    if b.get(at) != Some(&b'[') {
        return None;
    }
    let close = line.close(at)?;
    if b.get(close + 1) != Some(&b'(') {
        return None;
    }
    let mut j = close + 2;
    while b.get(j) == Some(&b' ') {
        j += 1;
    }
    let (dest, k) = if b.get(j) == Some(&b'<') {
        let end = line.gt(j + 1)?;
        (&s[j + 1..end], end + 1)
    } else {
        let mut parens = 0usize;
        let mut k = j;
        while let Some(&c) = b.get(k) {
            match c {
                b'\\' => k += 1,
                b'(' => parens += 1,
                b')' if parens == 0 => break,
                b')' => parens -= 1,
                b' ' | b'\t' => break,
                _ => {}
            }
            k += 1;
        }
        (&s[j..k.min(s.len())], k)
    };
    let title_end = line.paren(k)?;
    Some(MdLink {
        text_len: close - at - 1,
        dest,
        title: s[k..title_end].trim(),
        end: title_end + 1,
    })
}

/// Inline footnotes (`^[…]`) met in a part, as markdown's, under ids no written one has.
struct Notes {
    taken: HashSet<String>,
    next: usize,
    list: Vec<(String, String)>,
}

impl Notes {
    fn new(source: &str) -> Notes {
        let taken = source
            .match_indices("[^")
            .filter_map(|(at, _)| {
                let rest = &source[at + 2..];
                rest.find(']').map(|end| rest[..end].to_lowercase())
            })
            .collect();
        Notes {
            taken,
            next: 0,
            list: Vec::new(),
        }
    }

    /// A note's id, its markdown kept for the end of the part.
    fn add(&mut self, markdown: String) -> String {
        loop {
            self.next += 1;
            let id = format!("inline-{}", self.next);
            if !self.taken.contains(&id) {
                self.list.push((id.clone(), markdown));
                return id;
            }
        }
    }
}

/// `- [/] item`: a task with a status past done and to do, which GFM leaves as text: the list
/// marker, the status, the text after it.
fn task_status(body: &str) -> Option<(&str, char, &str)> {
    let t = body.trim_start_matches([' ', '\t']);
    if !is_list_item(t) {
        return None;
    }
    let after = t[t.find([' ', '\t'])?..].trim_start_matches([' ', '\t']);
    let mut chars = after.chars();
    let (Some('['), Some(status), Some(']')) = (chars.next(), chars.next(), chars.next()) else {
        return None;
    };
    let rest = chars.as_str();
    if matches!(status, ' ' | 'x' | 'X') || !(rest.is_empty() || rest.starts_with([' ', '\t'])) {
        return None;
    }
    Some((&body[..body.len() - after.len()], status, rest.trim_start()))
}

/// Rewrites one line's inline syntax into `out`; inline footnotes go to `notes`.
fn inline(text: &str, cx: &Context, notes: &mut Notes, out: &mut String) {
    let line = Line::new(text);
    let b = text.as_bytes();
    let mut i = 0;
    // No ==highlight== closes after here: one that opened here found none, and a later one
    // would look over the same text.
    let mut no_highlight = usize::MAX;
    while i < text.len() {
        let rest = &text[i..];
        let c = rest.chars().next().unwrap_or(' ');
        let prev_space = i == 0 || text[..i].ends_with(char::is_whitespace);
        match c {
            '\\' => {
                let n = 1 + rest[1..].chars().next().map_or(0, char::len_utf8);
                out.push_str(&rest[..n]);
                i += n;
                continue;
            }
            '`' => {
                let n = code_span(rest);
                out.push_str(&rest[..n]);
                i += n;
                continue;
            }
            '<' if b
                .get(i + 1)
                .is_some_and(|c| c.is_ascii_alphabetic() || *c == b'/' || *c == b'!') =>
            {
                // Raw HTML or an autolink: as written.
                let end = line.gt(i).map_or(i + 1, |e| e + 1);
                out.push_str(&text[i..end]);
                i = end;
                continue;
            }
            '!' if rest.starts_with("![[") => {
                if let Some((inner, n)) = wiki_inner(&rest[1..]) {
                    out.push_str(&inline_embed(inner, cx));
                    i += 1 + n;
                    continue;
                }
            }
            '[' if rest.starts_with("[[") => {
                if let Some((inner, n)) = wiki_inner(rest) {
                    let w = parse_wikilink(inner);
                    let label = escape(&wikilink_label(&w));
                    out.push_str(&format!("[{label}]({})", cx.link_href(&w.path, &w.anchor)));
                    i += n;
                    continue;
                }
            }
            '^' if rest.starts_with("^[") => {
                if let Some(close) = line.close(i + 1) {
                    let mut note = String::new();
                    inline(&text[i + 2..close], cx, notes, &mut note);
                    out.push_str(&format!("[^{}]", notes.add(note)));
                    i = close + 1;
                    continue;
                }
            }
            '!' | '[' => {
                let at = i + usize::from(c == '!');
                if let Some(link) = parse_link(&line, at) {
                    let label = &text[at + 1..at + 1 + link.text_len];
                    out.push_str(&md_link(c == '!', label, &link, cx));
                    i = link.end;
                    continue;
                }
            }
            '=' if rest.starts_with("==") => {
                match (i < no_highlight).then(|| highlight_end(rest)).flatten() {
                    Some(n) => {
                        out.push_str(&format!("<mark color=\"{HIGHLIGHT}\">"));
                        inline(&rest[2..n - 2], cx, notes, out);
                        out.push_str("</mark>");
                        i += n;
                        continue;
                    }
                    None if opens_highlight(rest) => no_highlight = no_highlight.min(i),
                    None => {}
                }
                // A run of ='s is text: the next one mustn't open a highlight.
                let n = rest.bytes().take_while(|b| *b == b'=').count();
                out.push_str(&rest[..n]);
                i += n;
                continue;
            }
            '#' if prev_space => {
                if let Some(n) = tag_len(&rest[1..]) {
                    out.push_str(&format!(
                        "<mark color=\"{TAG}\">#{}</mark>",
                        &rest[1..1 + n]
                    ));
                    i += 1 + n;
                    continue;
                }
            }
            _ => {}
        }
        out.push(c);
        i += c.len_utf8();
    }
}

/// `[[inner]]` at the start of `s`: inner, and the length of it all. Inner holds no bracket,
/// so the first one after `[[` ends it.
fn wiki_inner(s: &str) -> Option<(&str, usize)> {
    let after = s.strip_prefix("[[")?;
    let end = after.find(['[', ']'])?;
    (end > 0 && after[end..].starts_with("]]")).then(|| (&after[..end], end + 4))
}

/// Whether the `==` at the start of `s` can open a highlight: text right after it.
fn opens_highlight(s: &str) -> bool {
    s[2..]
        .chars()
        .next()
        .is_some_and(|c| !c.is_whitespace() && c != '=')
}

/// `==text==` at the start of `s`: its length. Neither end may touch a space.
fn highlight_end(s: &str) -> Option<usize> {
    if !opens_highlight(s) {
        return None;
    }
    let inner = &s[2..];
    let mut from = 0;
    while let Some(at) = inner[from..].find("==") {
        let end = from + at;
        if end > 0 && !inner[..end].ends_with(char::is_whitespace) {
            return Some(2 + end + 2);
        }
        from = end + 2;
    }
    None
}

/// A #tag's length after the '#': letters, digits, _ - / and emoji, with at least one that
/// isn't a digit (#123 is a number); a trailing / isn't part of it.
fn tag_len(s: &str) -> Option<usize> {
    let n: usize = s
        .chars()
        .take_while(|c| {
            c.is_alphanumeric() || matches!(c, '_' | '-' | '/') || is_mark(*c) || is_emoji(*c)
        })
        .map(char::len_utf8)
        .sum();
    let tag = s[..n].trim_end_matches('/');
    tag.chars()
        .any(|c| !c.is_ascii_digit() && c != '/')
        .then_some(tag.len())
}

fn is_mark(c: char) -> bool {
    matches!(c as u32, 0x0300..=0x036F | 0x1AB0..=0x1AFF | 0x20D0..=0x20FF | 0xFE20..=0xFE2F)
}

fn is_emoji(c: char) -> bool {
    matches!(c as u32, 0x1F000..=0x1FAFF | 0x2600..=0x27BF | 0x2B00..=0x2BFF | 0xFE0F | 0x200D)
}

/// `![[inner]]` inside text: an image in place, anything else a link to it.
fn inline_embed(inner: &str, cx: &Context) -> String {
    let w = parse_wikilink(inner);
    let alias = w.alias.clone().unwrap_or_default();
    let file = if w.path.is_empty() {
        Some(cx.note.to_string())
    } else {
        cx.index.resolve(&w.path, cx.note)
    };
    let Some(file) = file else {
        return not_here(&w.path);
    };
    if media_kind(&file) == Some(MediaKind::Image) {
        return image(&href(&file, ""), &alias);
    }
    let label = if alias.is_empty() {
        std::iter::once(note_name(&file))
            .chain(w.anchor.split('#').filter(|s| !s.is_empty()))
            .collect::<Vec<_>>()
            .join(" > ")
    } else {
        alias
    };
    format!("[{}]({})", escape(&label), href(&file, &w.anchor))
}

/// A markdown link or image written in the note: one to a vault file goes to the file Obsidian
/// would open, web ones stay as written.
fn md_link(image_link: bool, text: &str, link: &MdLink, cx: &Context) -> String {
    let bang = if image_link { "!" } else { "" };
    let title = if link.title.is_empty() {
        String::new()
    } else {
        format!(" {}", link.title)
    };
    let verbatim = || format!("{bang}[{text}]({}{title})", link.dest);
    if link.dest.is_empty() || is_external(link.dest) {
        return verbatim();
    }
    // Encoded as a URL; split before decoding: "C%23%20basics.md" is the note "C# basics.md".
    let (path, anchor) = split_target(link.dest);
    let (path, anchor) = (
        gitviber_markdown::links::decode(path),
        gitviber_markdown::links::decode(anchor),
    );
    if image_link {
        return match cx
            .index
            .resolve(&path, cx.note)
            .filter(|_| !path.is_empty())
        {
            Some(file) => image(&href(&file, ""), text),
            None => not_here(&path),
        };
    }
    format!("[{text}]({}{title})", cx.link_href(&path, &anchor))
}

/// A part's text as it reads, roughly as it renders: what Find counts in, since a part not built
/// has no rendered text. Links read as their text, embeds and markup characters go, code stays.
pub fn plain(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut fences = Fences::default();
    for line in source.lines() {
        if fences.code(line) {
            if fence_of(line).is_none() {
                out.push_str(line);
                out.push('\n');
            }
            continue;
        }
        let mut t = line.trim_start();
        // Block markers: quotes, headings, list items and their checkboxes.
        loop {
            let before = t;
            t = t.trim_start_matches('>').trim_start();
            if let Some(rest) = t.strip_prefix(|c: char| c == '#') {
                t = rest.trim_start_matches('#').trim_start();
            }
            if is_list_item(t) {
                t = t
                    .split_once([' ', '\t'])
                    .map_or("", |(_, r)| r)
                    .trim_start();
            }
            // A task's box, whatever its status.
            let mut chars = t.chars();
            if let (Some('['), Some(_), Some(']'), Some(' ')) =
                (chars.next(), chars.next(), chars.next(), chars.next())
            {
                t = chars.as_str();
            }
            if t == before {
                break;
            }
        }
        let t = match block_id(t) {
            Some((len, _)) => &t[..len],
            None => t,
        };
        plain_inline(t, &mut out);
        out.push('\n');
    }
    out
}

fn plain_inline(line: &str, out: &mut String) {
    let scan = Line::new(line);
    let mut i = 0;
    while i < line.len() {
        let rest = &line[i..];
        let c = rest.chars().next().unwrap_or(' ');
        if rest.starts_with("![[") {
            if let Some((_, n)) = wiki_inner(&rest[1..]) {
                i += 1 + n;
                continue;
            }
        } else if rest.starts_with("[[") {
            if let Some((inner, n)) = wiki_inner(rest) {
                out.push_str(&wikilink_label(&parse_wikilink(inner)));
                i += n;
                continue;
            }
        } else if rest.starts_with("^[") {
            if let Some(close) = scan.close(i + 1) {
                plain_inline(&line[i + 2..close], out);
                i = close + 1;
                continue;
            }
        } else if c == '!' || c == '[' {
            let at = i + usize::from(c == '!');
            if let Some(link) = parse_link(&scan, at) {
                if c == '[' {
                    plain_inline(&line[at + 1..at + 1 + link.text_len], out);
                }
                i = link.end;
                continue;
            }
        } else if c == '<' && rest[1..].starts_with(|c: char| c.is_ascii_alphabetic() || c == '/') {
            i = scan.gt(i).map_or(i + 1, |e| e + 1);
            continue;
        } else if c == '\\' {
            if let Some(next) = rest[1..].chars().next() {
                out.push(next);
                i += 1 + next.len_utf8();
                continue;
            }
        } else if matches!(c, '*' | '`' | '~' | '=') {
            i += 1;
            continue;
        } else if c == '_' {
            // Emphasis, not the underscore inside a word.
            let prev = line[..i]
                .chars()
                .next_back()
                .is_some_and(char::is_alphanumeric);
            let next = rest[1..].chars().next().is_some_and(char::is_alphanumeric);
            if !(prev && next) {
                i += 1;
                continue;
            }
        }
        out.push(c);
        i += c.len_utf8();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index() -> LinkIndex {
        LinkIndex::new(
            [
                "Home.md",
                "Note.md",
                "Other.md",
                "Projects/Alpha.md",
                "assets/pic.png",
                "C# basics.md",
            ]
            .map(String::from)
            .to_vec(),
        )
    }

    fn md(src: &str) -> String {
        let index = index();
        let cx = Context {
            index: &index,
            note: "Home.md",
        };
        to_markdown("", src, &cx)
    }

    #[test]
    fn comments_are_cut_except_in_code() {
        assert_eq!(strip_comments("a %%hidden%% b"), "a  b");
        assert_eq!(strip_comments("a\n%%\nhidden\n%%\nb"), "a\n\nb");
        assert_eq!(strip_comments("`%%kept%%` and %%gone%%"), "`%%kept%%` and ");
        assert_eq!(
            strip_comments("```\n%%kept%%\n```\n%%gone"),
            "```\n%%kept%%\n```\n"
        );
        // printf formats, SQL LIKE patterns and Mermaid's own %% comments in fenced code, in a
        // callout, a list or a quote.
        let callout = strip_comments(
            "> [!example] Formatting\n> ```py\n> print(\"100%%\")\n> ```\n\nAfter the callout",
        );
        assert!(callout.contains("100%%") && callout.contains("After the callout"));
        let list = strip_comments(
            "- Architecture\n\n    ```mermaid\n    %% services\n    graph TD\n    ```\n\nAfter the list",
        );
        assert!(list.contains("%% services") && list.contains("After the list"));
        let tilde = strip_comments("~~~~\n%% kept too\n~~~~\nend %%x%%");
        assert_eq!(tilde, "~~~~\n%% kept too\n~~~~\nend ");
    }

    #[test]
    fn wikilinks_split_into_path_heading_or_block_and_alias() {
        assert_eq!(
            parse_wikilink("Folder/Note#Part#Sub|Shown"),
            Wikilink {
                path: "Folder/Note".into(),
                anchor: "Part#Sub".into(),
                alias: Some("Shown".into())
            }
        );
        assert_eq!(
            wikilink_label(&parse_wikilink("Note#Part#Sub")),
            "Note > Part > Sub"
        );
        assert_eq!(wikilink_label(&parse_wikilink("#Part")), "Part");
        assert_eq!(
            parse_wikilink("Note\\|Alias").alias.as_deref(),
            Some("Alias")
        );
    }

    #[test]
    fn links_go_to_the_file_they_resolve_to() {
        let out = md("See [[Note|this]], [[Other#Part]], [[alpha]] and `[[code]]`.");
        assert!(
            out.contains(&format!("[this]({})", href("Note.md", ""))),
            "{out}"
        );
        assert!(
            out.contains(&format!("[Other > Part]({})", href("Other.md", "Part"))),
            "{out}"
        );
        assert!(
            out.contains(&format!("[alpha]({})", href("Projects/Alpha.md", ""))),
            "{out}"
        );
        assert!(out.contains("`[[code]]`"), "{out}");
        let gone = md("[[Nope]]");
        assert_eq!(
            target(&gitviber_markdown::links::decode(
                &gone[gone.find("](").unwrap() + 3..gone.len() - 1]
            )),
            Target::Missing("Nope".into())
        );
        // A markdown link to a note, URL-encoded, and one to the web as written.
        let out = md("[a](C%23%20basics.md#Intro) [w](https://x.dev/#a) [s](#Top)");
        assert!(
            out.contains(&format!("[a]({})", href("C# basics.md", "Intro"))),
            "{out}"
        );
        assert!(out.contains("[w](https://x.dev/#a)"), "{out}");
        assert!(
            out.contains(&format!("[s]({})", href("Home.md", "Top"))),
            "{out}"
        );
        // The href comes back through gitviber-markdown's decoding as the file and its part.
        let back = gitviber_markdown::links::decode(&href("C# basics.md", "Intro")[1..]);
        assert_eq!(
            target(&back),
            Target::File {
                path: "C# basics.md".into(),
                anchor: "Intro".into()
            }
        );
    }

    #[test]
    fn a_wikilink_with_an_escaped_alias_bar_in_a_table_keeps_its_alias() {
        let out = md("| a | b |\n| - | - |\n| [[Note\\|Alias]] | x |");
        assert!(
            out.contains(&format!("| [Alias]({}) | x |", href("Note.md", ""))),
            "{out}"
        );
    }

    #[test]
    fn embeds_inline_images_in_place_notes_as_links() {
        let out =
            md("an ![[pic.png|300]] inline ![[pic.png]] and ![[Note#Part]] and ![[gone.png]]");
        assert!(
            out.contains(&format!(
                "<img src=\"{}\" alt=\"\" width=\"300\">",
                href("assets/pic.png", "")
            )),
            "{out}"
        );
        assert!(
            out.contains(&format!("![]({})", href("assets/pic.png", ""))),
            "{out}"
        );
        assert!(
            out.contains(&format!("[Note > Part]({})", href("Note.md", "Part"))),
            "{out}"
        );
        assert!(out.contains("*“gone.png” isn't in this vault*"), "{out}");
        let out = md("![shot|200x100](assets/pic.png) ![x](https://e.com/a.png)");
        assert!(out.contains("width=\"200\" height=\"100\""), "{out}");
        assert!(out.contains("![x](https://e.com/a.png)"), "{out}");
    }

    #[test]
    fn highlights_and_tags() {
        let out = md("==a **b**== and == no == #tag/sub #123 x#not and ==`c`==");
        assert!(
            out.contains(&format!("<mark color=\"{HIGHLIGHT}\">a **b**</mark>")),
            "{out}"
        );
        assert!(out.contains("== no =="), "{out}");
        assert!(
            out.contains(&format!("<mark color=\"{TAG}\">#tag/sub</mark>")),
            "{out}"
        );
        assert!(out.contains(" #123 ") && out.contains("x#not"), "{out}");
        assert!(
            out.contains(&format!("<mark color=\"{HIGHLIGHT}\">`c`</mark>")),
            "{out}"
        );
        // Headings stay headings; a tag in one is still a tag.
        let out = md("# Title\n## Tagged #todo");
        assert!(out.starts_with("# Title\n## Tagged <mark"), "{out}");
        // Prices stay text, and Obsidian's syntax after them still works.
        let out = md("Paid $5 and $10 for [[Note]] #food");
        assert!(out.starts_with("Paid $5 and $10 for [Note]("), "{out}");
        assert!(out.contains("#food</mark>"), "{out}");
    }

    #[test]
    fn code_math_and_html_are_left_as_written() {
        let src = "```\n[[Note]] #tag ==x==\n```\n\n    #include <x>\n\n$$\na == b #c\n$$\n<span style=\"color: #fff\">#x</span>";
        let out = md(src);
        assert!(out.contains("```\n[[Note]] #tag ==x==\n```"), "{out}");
        assert!(out.contains("    #include <x>"), "{out}");
        assert!(out.contains("$$\na == b #c\n$$"), "{out}");
        assert!(out.contains("<span style=\"color: #fff\">"), "{out}");
        // A list's indented continuation isn't code.
        let out = md("- item\n\n    more #tag");
        assert!(out.contains("more <mark"), "{out}");
    }

    #[test]
    fn block_ids_go_from_the_text() {
        assert_eq!(
            md("A line ^para1\n- item ^item1\n\n^table1"),
            "A line\n- item\n\n"
        );
        assert_eq!(md("x^y and 2^10"), "x^y and 2^10");
    }

    #[test]
    fn notes_split_at_headings_and_embeds_alone_on_a_line() {
        let src = "---\ntags: [a]\n---\nintro\n# One\ntext ^b1\n![[Note]]\n```\n# not a heading\n![[nor this]]\n```\n## Two\nend";
        let p = parts(src, true);
        let heads: Vec<Option<&str>> = p
            .iter()
            .map(|p| match p {
                Part::Text { heading, .. } => heading.as_deref(),
                Part::Embed { .. } => Some("<embed>"),
            })
            .collect();
        assert_eq!(
            heads,
            [None, Some("one"), Some("<embed>"), None, Some("two")]
        );
        let Part::Text { front, ids, .. } = &p[0] else {
            panic!()
        };
        assert_eq!(front, "---\ntags: [a]\n---\n");
        assert!(ids.is_empty());
        let Part::Text { ids, .. } = &p[1] else {
            panic!()
        };
        assert_eq!(ids, &["^b1"]);
        assert_eq!(
            p[2],
            Part::Embed {
                target: "Note".into(),
                alias: String::new()
            }
        );
        // Footnotes need their definitions in the same part: no split at headings then.
        assert_eq!(parts("# A\nx[^1]\n# B\n[^1]: note", true).len(), 1);
        assert_eq!(parts("# A\nx^[note]\n# B\ny^[two]", true).len(), 1);
        assert_eq!(parts("# A\nx\n# B\ny", false).len(), 1);
    }

    #[test]
    fn an_embed_of_a_heading_or_block_keeps_just_that_part() {
        let note = "# Top\nintro\n## Part\nin part\n### Deeper\nstill\n## Next\nout\n\nA block ^b1";
        let part = section(note, "Part").unwrap();
        assert_eq!(part, "## Part\nin part\n### Deeper\nstill\n");
        assert_eq!(section(note, "^b1").unwrap(), "A block ^b1");
        assert_eq!(
            section(note, "Goals").unwrap_err(),
            "Unable to find section #Goals"
        );
        assert_eq!(
            section(note, "^gone").unwrap_err(),
            "Unable to find block ^gone"
        );
        assert_eq!(section("---\ntags: [a]\n---\nBody", "").unwrap(), "Body");
        let list = "- one\n- two ^i2\n- three";
        assert_eq!(section(list, "^i2").unwrap(), "- two ^i2\n");
        let table = "| a |\n| - |\n| 1 |\n\n^t1\n";
        assert_eq!(section(table, "^t1").unwrap(), "| a |\n| - |\n| 1 |\n");
        // Obsidian leaves out what a link can't hold (":", "#") when it links a heading.
        let note = "# Top\n## Q&A: Part 1\nanswer\n## Tagged #todo\ntagged\n## See [the docs](x.md)\ndocs\n## After\nout";
        assert!(section(note, "Q&A: Part 1").unwrap().contains("answer"));
        assert!(section(note, "Q&A Part 1").unwrap().contains("answer"));
        assert!(section(note, "Tagged todo").unwrap().contains("tagged"));
        assert!(section(note, "See the docs").unwrap().contains("docs"));
        assert!(section(note, "Top#Q&A Part 1").unwrap().contains("answer"));
    }

    #[test]
    fn hostile_names_render_inert() {
        let out = md("[[<script>alert(1)</script>|<b>x</b>]] [[javascript:alert(1)]]");
        // Every < is escaped: markdown shows it as text, never as a tag.
        assert!(
            out.match_indices('<')
                .all(|(i, _)| out[..i].ends_with('\\')),
            "{out}"
        );
        assert!(!out.contains("](javascript:"), "{out}");
    }

    #[test]
    fn find_reads_a_part_as_it_renders() {
        let src = "## Goals ^g\n> [!tip] See [[Plan#Goals|the plan]] and [docs](x.md)\n- [x] **bold** ==mark== `code` snake_case\n![[pic.png]] ![alt](a.png) <b>x</b>\n```\n# kept *as is*\n```\n";
        assert_eq!(
            plain(src),
            "Goals\n[!tip] See the plan and docs\nbold mark code snake_case\n  x\n# kept *as is*\n"
        );
    }

    #[test]
    fn inline_footnotes_become_markdowns_under_ids_of_their_own() {
        let out = md("x ^[a [b] note with [[Note]]] and y^[two]\n- item ^[three]");
        assert_eq!(
            out,
            format!(
                "x [^inline-1] and y[^inline-2]\n- item [^inline-3]\n\n[^inline-1]: a [b] note with [Note]({})\n[^inline-2]: two\n[^inline-3]: three\n",
                href("Note.md", "")
            )
        );
        // A written id is never taken.
        let out = md("Written[^inline-1] and inline^[mine].\n\n[^inline-1]: The user's.");
        assert!(out.contains("inline[^inline-2]."), "{out}");
        assert!(out.ends_with("[^inline-2]: mine\n"), "{out}");
        // Unclosed, or in code, it's text.
        assert_eq!(md("x ^[open and `^[code]`"), "x ^[open and `^[code]`");
        assert_eq!(plain("a ^[b] c"), "a b c\n");
    }

    #[test]
    fn task_statuses_past_done_show_their_mark() {
        let out = md(
            "- [/] half\n- [-] dropped [[Note]]\n- [x] done\n- [ ] open\n  1. [<] later\n- [/]x",
        );
        assert_eq!(
            out,
            format!(
                "- <code>/</code> half\n- <code>-</code> <s>dropped [Note]({})</s>\n- [x] done\n- [ ] open\n  1. <code>&lt;</code> later\n- [/]x",
                href("Note.md", "")
            )
        );
        assert_eq!(plain("- [/] half\n- [x] done"), "half\ndone\n");
    }

    #[test]
    fn a_wikilink_whose_name_has_two_prices_is_one_link() {
        // Its dollars are escaped, so math can't start in one price and end in the other.
        let out = md("See [[Budget $5k vs $10k]] and [[Note|$5 or $6]].");
        assert_eq!(
            out,
            format!(
                "See [Budget \\$5k vs \\$10k]({}) and [\\$5 or \\$6]({}).",
                missing_href("Budget $5k vs $10k"),
                href("Note.md", "")
            )
        );
    }

    #[test]
    fn a_block_is_found_in_its_parts_markdown() {
        let src = "intro\n\nfirst line\nsecond ^p1\n\n- one\n- two ^i2\n\n| a |\n| - |\n\n^t1\n";
        let index = index();
        let cx = Context {
            index: &index,
            note: "Home.md",
        };
        let markdown = to_markdown("", src, &cx);
        let at = |id: &str| block_range(src, &markdown, id).map(|r| &markdown[r]);
        assert_eq!(at("p1"), Some("first line\nsecond"));
        assert_eq!(at("i2"), Some("- two"));
        assert_eq!(at("t1"), Some("| a |\n| - |"));
        assert_eq!(at("gone"), None);
    }

    /// The best of three runs of `f`, in seconds.
    fn best(f: impl Fn()) -> f64 {
        (0..3)
            .map(|_| {
                let start = std::time::Instant::now();
                f();
                start.elapsed().as_secs_f64()
            })
            .fold(f64::MAX, f64::min)
    }

    #[test]
    fn a_long_line_costs_in_proportion_to_its_length() {
        // Each unit opens something that never closes. Four times the text may take about
        // four times as long; quadratic work would take sixteen.
        for unit in [
            "a ==b ", "#t [x ", "x ^[a ", "`` y ", "<a b ", "[[x ", "$5 [[a ", "[x]( ", "[x](<y ",
        ] {
            let time = |n: usize| {
                let line = unit.repeat(n);
                best(|| {
                    md(&line);
                })
            };
            let (small, big) = (time(2000), time(8000));
            assert!(
                big < small * 9. + 0.005,
                "{unit:?}: {small:.4}s -> {big:.4}s"
            );
        }
    }

    #[test]
    fn a_long_note_written_line_by_line_costs_little() {
        let lines: String = (0..2000)
            .map(|i| {
                format!(
                    "{i} met [[Person {}]] about ==this== #project/{} ^[n{i}]\n",
                    i % 50,
                    i % 7
                )
            })
            .collect();
        let out = md(&lines);
        assert!(out.contains("Person 49"), "{}", &out[..200]);
        let took = best(|| {
            md(&lines);
        });
        assert!(took < 0.5, "{took:.3}s");
    }

    #[test]
    fn many_unclosed_highlights_dont_stall_a_note() {
        let start = std::time::Instant::now();
        md(&"a ==b ".repeat(3000));
        md(&"#t [x ".repeat(3000));
        assert!(start.elapsed().as_millis() < 2000, "{:?}", start.elapsed());
    }
}
