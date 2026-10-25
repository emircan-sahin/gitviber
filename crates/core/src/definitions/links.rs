//! The file a path in code names, for Go to Definition: what an import, `@import`, `mod x;` or
//! Python import loads, a markdown link, a relative path in a string, a `path:line` in a
//! comment. Only the line asked about is read, around the place asked about; the file is looked
//! for in the worktree on disk or in a commit's tree.

use super::{Location, Request};
use crate::{fs, git};
use std::collections::HashMap;
use std::path::Path;

/// Characters either side of the place asked about that are read: a minified bundle is one
/// line of a megabyte.
const WINDOW: usize = 2000;

/// How a link's text becomes a file: each kind has its own lookup rules.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Module,
    Style,
    Doc,
    Path,
    Rust,
    Python,
    Url,
    File,
}

/// A link in a line: `[start, end)` in chars, `spec` the text between them.
#[derive(Debug, PartialEq)]
struct Link {
    start: usize,
    end: usize,
    spec: String,
    kind: Kind,
}

/// The kind of file a line is in, for the links it has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Family {
    Js,
    Css,
    Markdown,
    /// Markdown with imports.
    Mdx,
    Rust,
    Python,
    /// Plain text, where `name.ext` alone is a path too.
    Text,
    Other,
}

impl Family {
    fn of(path: &str) -> Family {
        let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
        let Some((_, ext)) = name.rsplit_once('.') else {
            return Family::Text;
        };
        match ext.to_ascii_lowercase().as_str() {
            "js" | "mjs" | "cjs" | "jsx" | "ts" | "mts" | "cts" | "tsx" | "vue" | "svelte"
            | "astro" => Family::Js,
            "mdx" => Family::Mdx,
            "css" | "scss" | "sass" | "less" | "pcss" | "postcss" => Family::Css,
            "md" | "markdown" => Family::Markdown,
            "rs" => Family::Rust,
            "py" | "pyi" | "pyw" => Family::Python,
            "txt" | "text" | "log" => Family::Text,
            _ => Family::Other,
        }
    }
}

/// Where a link goes: a repo file, at a line (1-based) and column (1-based) when it names one.
#[derive(Debug, PartialEq)]
struct Target {
    path: String,
    line: Option<u32>,
    column: Option<u32>,
}

/// The file the path or module string at the request's place names, and that string's UTF-16
/// columns on its line. None where there's none, or it names no file there is.
pub(super) fn follow(repo: &Path, req: &Request) -> Option<(Location, (u32, u32))> {
    let line = req
        .text
        .split('\n')
        .nth(req.line.checked_sub(1)? as usize)?;
    let line = line.strip_suffix('\r').unwrap_or(line);
    let at = byte_at_utf16(line, req.column);
    let (window, from_line_start, to_line_end, before) = window(line, at);
    let chars: Vec<char> = window.chars().collect();
    let col = window[..at - before].chars().count();
    let link = link_at(
        &chars,
        Family::of(&req.path),
        col,
        from_line_start,
        to_line_end,
    )?;
    let root = repo.to_string_lossy().replace('\\', "/");
    let target = match &req.rev {
        Some(rev) => resolve(&link, &req.path, &mut Commit::new(repo, rev), &root),
        None => resolve(&link, &req.path, &mut Disk::new(repo), &root),
    }?;
    let utf16 = |chars: &[char]| chars.iter().map(|c| c.len_utf16() as u32).sum::<u32>();
    let start = utf16_len(&line[..before]) + utf16(&chars[..link.start]);
    let end = start + utf16(&chars[link.start..link.end]);
    let column = target.column.map_or(0, |c| c.saturating_sub(1));
    let place = Location {
        path: target.path,
        line: target.line.unwrap_or(1).max(1),
        column,
        end_column: column,
    };
    Some((place, (start, end)))
}

fn utf16_len(s: &str) -> u32 {
    s.encode_utf16().count() as u32
}

/// The byte of `line` at UTF-16 column `column` (its end past it).
fn byte_at_utf16(line: &str, column: u32) -> usize {
    let mut units = 0;
    for (i, c) in line.char_indices() {
        if units >= column {
            return i;
        }
        units += c.len_utf16() as u32;
    }
    line.len()
}

/// The part of `line` within `WINDOW` chars of byte `at`: it, whether it starts and ends the
/// line, and the byte it starts at.
fn window(line: &str, at: usize) -> (&str, bool, bool, usize) {
    let start = line[..at]
        .char_indices()
        .rev()
        .nth(WINDOW - 1)
        .map_or(0, |(i, _)| i);
    let end = line[at..]
        .char_indices()
        .nth(WINDOW)
        .map_or(line.len(), |(i, _)| at + i);
    (&line[start..end], start == 0, end == line.len(), start)
}

/// The link at char `col` of `chars` (ends included: a click's column is the caret spot nearest
/// it); not a URL, which opens in the browser. A link the window cuts isn't one: its tail could
/// name another file.
fn link_at(chars: &[char], family: Family, col: usize, whole: bool, to_end: bool) -> Option<Link> {
    find_links(chars, family, whole)
        .into_iter()
        .filter(|l| (whole || l.start > 0) && (to_end || l.end < chars.len()))
        .find(|l| l.start <= col && col <= l.end)
        .filter(|l| l.kind != Kind::Url)
}

/// Keeps links in order; the first to claim some text keeps it.
#[derive(Default)]
struct Collector {
    links: Vec<Link>,
}

impl Collector {
    fn add(&mut self, s: &[char], start: usize, end: usize, kind: Kind) {
        if start >= end {
            return;
        }
        let at = self.links.partition_point(|l| l.start < start);
        if (at > 0 && self.links[at - 1].end > start)
            || (at < self.links.len() && self.links[at].start < end)
        {
            return;
        }
        let spec = s[start..end].iter().collect();
        self.links.insert(
            at,
            Link {
                start,
                end,
                spec,
                kind,
            },
        );
    }
}

/// The links in `s`, a line of a `family` file (or a part of one: `whole` when it starts the
/// line, for what's only found at a line's start), in order.
fn find_links(s: &[char], family: Family, whole: bool) -> Vec<Link> {
    let mut c = Collector::default();
    // First: a URL in an import or a markdown link is still a URL.
    urls(s, &mut c);
    match family {
        Family::Js | Family::Mdx => {
            each(s, &mut c, Kind::Module, |i| {
                keyword_string(s, i, &["from", "import"], false)
            });
            each(s, &mut c, Kind::Module, |i| {
                keyword_string(s, i, &["import", "require"], true)
            });
            each(s, &mut c, Kind::Module, |i| relative_string(s, i));
        }
        Family::Css => {
            each(s, &mut c, Kind::Style, |i| css_import(s, i));
            each(s, &mut c, Kind::Style, |i| css_url(s, i));
        }
        Family::Rust if whole => each(s, &mut c, Kind::Rust, |i| (i == 0).then(|| rust_mod(s))?),
        Family::Python if whole => each(s, &mut c, Kind::Python, |i| {
            (i == 0).then(|| python_from(s))?
        }),
        _ => {}
    }
    if matches!(family, Family::Markdown | Family::Mdx) {
        each(s, &mut c, Kind::Doc, |i| markdown_link(s, i));
        if whole {
            each(s, &mut c, Kind::Doc, |i| (i == 0).then(|| reference(s))?);
        }
        each(s, &mut c, Kind::Doc, |i| html_attribute(s, i));
    }
    each(s, &mut c, Kind::Path, |i| relative_string(s, i));
    if family == Family::Python && whole {
        python_imports(s, &mut c);
    }
    // src/lib/api.ts:42 in a comment or a doc. A bare name.ext only with a line, or `a.b`
    // would be one.
    file_paths(s, &mut c, matches!(family, Family::Markdown | Family::Text));
    c.links
}

/// Runs a pattern over `s` left to right: `at(i)` gives the link's `[start, end)` and where the
/// match ends, and the next try starts there.
fn each(
    s: &[char],
    c: &mut Collector,
    kind: Kind,
    at: impl Fn(usize) -> Option<(usize, usize, usize)>,
) {
    let mut i = 0;
    while i < s.len() {
        match at(i) {
            Some((start, end, after)) => {
                c.add(s, start, end, kind);
                i = after.max(i + 1);
            }
            None => i += 1,
        }
    }
}

fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

fn starts(s: &[char], i: usize, w: &str) -> bool {
    let mut rest = s.get(i..).unwrap_or_default().iter();
    w.chars().all(|ch| rest.next() == Some(&ch))
}

fn skip_ws(s: &[char], mut i: usize) -> usize {
    while s.get(i).is_some_and(|c| c.is_whitespace()) {
        i += 1;
    }
    i
}

/// The first of `words` at `i`, as a word: where it ends.
fn word_at(s: &[char], i: usize, words: &[&str]) -> Option<usize> {
    if i > 0 && is_word(s[i - 1]) {
        return None;
    }
    let w = words.iter().find(|w| starts(s, i, w))?;
    Some(i + w.chars().count())
}

/// A string at `i` quoted with one of `quotes`: its text's `[start, end)` and the end past the
/// closing quote. The text is 1+ chars with none of `stop` in it.
fn quoted(
    s: &[char],
    i: usize,
    quotes: &[char],
    stop: impl Fn(char) -> bool,
) -> Option<(usize, usize, usize)> {
    let q = *s.get(i)?;
    if !quotes.contains(&q) {
        return None;
    }
    let mut k = i + 1;
    while k < s.len() && !stop(s[k]) {
        k += 1;
    }
    (k > i + 1 && s.get(k) == Some(&q)).then_some((i + 1, k, k + 1))
}

/// `from "x"`, `import "x"`, or with `call`, `import("x")` and `require('x')`.
fn keyword_string(
    s: &[char],
    i: usize,
    words: &[&str],
    call: bool,
) -> Option<(usize, usize, usize)> {
    let mut j = skip_ws(s, word_at(s, i, words)?);
    if call {
        if s.get(j) != Some(&'(') {
            return None;
        }
        j = skip_ws(s, j + 1);
    }
    quoted(s, j, &['"', '\''], |c| {
        c == '"' || c == '\'' || c.is_whitespace()
    })
}

/// A relative path in a string: `"./x"`, `'../x'`, `` `./x` ``.
fn relative_string(s: &[char], i: usize) -> Option<(usize, usize, usize)> {
    let head = match () {
        _ if starts(s, i + 1, "./") => 2,
        _ if starts(s, i + 1, "../") => 3,
        _ => return None,
    };
    let stop = |c: char| matches!(c, '"' | '\'' | '`') || c.is_whitespace();
    let q = *s.get(i)?;
    if !matches!(q, '"' | '\'' | '`') {
        return None;
    }
    let mut k = i + 1 + head;
    while k < s.len() && !stop(s[k]) {
        k += 1;
    }
    (s.get(k) == Some(&q)).then_some((i + 1, k, k + 1))
}

/// `@import "x"`, `@use 'x'`, `@forward "x"`.
fn css_import(s: &[char], i: usize) -> Option<(usize, usize, usize)> {
    if s.get(i) != Some(&'@') {
        return None;
    }
    let j = ["import", "use", "forward"]
        .iter()
        .find(|w| starts(s, i + 1, w))
        .map(|w| i + 1 + w.len())?;
    let k = skip_ws(s, j);
    if k == j {
        return None;
    }
    quoted(s, k, &['"', '\''], |c| c == '"' || c == '\'')
}

/// `url(x)`, `url("x")`.
fn css_url(s: &[char], i: usize) -> Option<(usize, usize, usize)> {
    let mut j = skip_ws(s, word_at(s, i, &["url("])?);
    let quote = s.get(j).copied().filter(|c| *c == '"' || *c == '\'');
    if quote.is_some() {
        j += 1;
    }
    let mut k = j;
    while k < s.len() && !(matches!(s[k], '"' | '\'' | ')') || s[k].is_whitespace()) {
        k += 1;
    }
    if k == j {
        return None;
    }
    let mut after = k;
    if let Some(q) = quote {
        if s.get(after) != Some(&q) {
            return None;
        }
        after += 1;
    }
    after = skip_ws(s, after);
    (s.get(after) == Some(&')')).then_some((j, k, after + 1))
}

/// A run of 1+ chars from `j` with none of `stop`, after an optional `<` (`<x>`): its bounds.
fn bracketed(s: &[char], j: usize, stop: impl Fn(char) -> bool) -> Option<(usize, usize)> {
    let run = |from: usize| {
        let mut k = from;
        while k < s.len() && !stop(s[k]) {
            k += 1;
        }
        (k > from).then_some((from, k))
    };
    let inner = if s.get(j) == Some(&'<') {
        run(j + 1)
    } else {
        None
    };
    inner.or_else(|| run(j))
}

/// `[text](x "title")`, `![alt](<x>)`.
fn markdown_link(s: &[char], i: usize) -> Option<(usize, usize, usize)> {
    if !starts(s, i, "](") {
        return None;
    }
    let j = skip_ws(s, i + 2);
    let (a, b) = bracketed(s, j, |c| c == ')' || c == '>' || c.is_whitespace())?;
    Some((a, b, b))
}

/// `[ref]: x`, at most 3 spaces in.
fn reference(s: &[char]) -> Option<(usize, usize, usize)> {
    let i = skip_ws(s, 0);
    if i > 3 || s.get(i) != Some(&'[') {
        return None;
    }
    let close = i + 1 + s[i + 1..].iter().position(|&c| c == ']')?;
    if close == i + 1 || s.get(close + 1) != Some(&':') {
        return None;
    }
    let j = skip_ws(s, close + 2);
    let (a, b) = bracketed(s, j, |c| c == '>' || c.is_whitespace())?;
    Some((a, b, b))
}

/// Inline HTML: `<img src="x">`, `<a href='x'>`.
fn html_attribute(s: &[char], i: usize) -> Option<(usize, usize, usize)> {
    let j = skip_ws(s, word_at(s, i, &["src", "href"])?);
    if s.get(j) != Some(&'=') {
        return None;
    }
    quoted(s, skip_ws(s, j + 1), &['"', '\''], |c| {
        c == '"' || c == '\''
    })
}

/// `mod x;`, `pub(crate) mod r#x;`, starting the line.
fn rust_mod(s: &[char]) -> Option<(usize, usize, usize)> {
    let mut i = skip_ws(s, 0);
    if starts(s, i, "pub") {
        let mut k = i + 3;
        if s.get(k) == Some(&'(') {
            k += 1 + s[k + 1..].iter().position(|&c| c == ')')? + 1;
        }
        let after = skip_ws(s, k);
        if after > k {
            i = after;
        }
    }
    if !starts(s, i, "mod") {
        return None;
    }
    let mut j = skip_ws(s, i + 3);
    if j == i + 3 {
        return None;
    }
    if starts(s, j, "r#") {
        j += 2;
    }
    let start = j;
    if !s
        .get(j)
        .is_some_and(|c| c.is_ascii_alphabetic() || *c == '_')
    {
        return None;
    }
    while s.get(j).is_some_and(|c| is_word(*c)) {
        j += 1;
    }
    let end = j;
    let semi = skip_ws(s, j);
    (s.get(semi) == Some(&';')).then_some((start, end, semi + 1))
}

/// A dotted Python name from `j`: `a.b.c` (a letter or `_` first).
fn dotted(s: &[char], j: usize) -> usize {
    if !s
        .get(j)
        .is_some_and(|c| c.is_ascii_alphabetic() || *c == '_')
    {
        return j;
    }
    let mut k = j + 1;
    while s.get(k).is_some_and(|c| is_word(*c) || *c == '.') {
        k += 1;
    }
    k
}

/// `from ..pkg.mod import x`, starting the line.
fn python_from(s: &[char]) -> Option<(usize, usize, usize)> {
    let i = skip_ws(s, 0);
    if !starts(s, i, "from") {
        return None;
    }
    let j = skip_ws(s, i + 4);
    if j == i + 4 {
        return None;
    }
    let mut k = j;
    while s.get(k) == Some(&'.') {
        k += 1;
    }
    let end = dotted(s, k);
    if end == j {
        return None;
    }
    let at = skip_ws(s, end);
    let after = (at > end).then(|| word_at(s, at, &["import"]))??;
    (!s.get(after).is_some_and(|c| is_word(*c))).then_some((j, end, after))
}

/// `import a.b, c as d`, starting the line: one link a module.
fn python_imports(s: &[char], c: &mut Collector) {
    let i = skip_ws(s, 0);
    if !starts(s, i, "import") {
        return;
    }
    let j = skip_ws(s, i + 6);
    if j == i + 6 {
        return;
    }
    let end = s.iter().position(|&ch| ch == '#').unwrap_or(s.len()).max(j);
    let mut at = j;
    for part in s[j..end].split(|&ch| ch == ',') {
        let k = skip_ws(part, 0);
        let e = dotted(part, k);
        c.add(s, at + k, at + e, Kind::Python);
        at += part.len() + 1;
    }
}

/// Left off a URL's end: the sentence around it.
const TRAILING: &[char] = &['.', ',', ';', ':', '!', '?', '*', '\'', '"'];

fn urls(s: &[char], c: &mut Collector) {
    let lower = |i: usize| s.get(i).map(|c| c.to_ascii_lowercase());
    let mut i = 0;
    while i < s.len() {
        let scheme = (i == 0 || !is_word(s[i - 1]))
            && lower(i) == Some('h')
            && lower(i + 1) == Some('t')
            && lower(i + 2) == Some('t')
            && lower(i + 3) == Some('p');
        let mut j = i + 4;
        if scheme && lower(j) == Some('s') {
            j += 1;
        }
        if !scheme || !starts(s, j, "://") {
            i += 1;
            continue;
        }
        j += 3;
        let body = j;
        while j < s.len() && !s[j].is_whitespace() && !"\"'`<>{}|\\^".contains(s[j]) {
            j += 1;
        }
        if j == body {
            i = j;
            continue;
        }
        let url = &s[i..j];
        // A closing bracket belongs to the URL only if it opened one: (see https://x.com/a_(b)).
        let count = |ch: char| url.iter().filter(|&&x| x == ch).count();
        let (mut parens, mut brackets) = (
            count(')').saturating_sub(count('(')),
            count(']').saturating_sub(count('[')),
        );
        let mut end = url.len();
        while end > 0 {
            let ch = url[end - 1];
            if ch == ')' && parens > 0 {
                parens -= 1;
            } else if ch == ']' && brackets > 0 {
                brackets -= 1;
            } else if !TRAILING.contains(&ch) {
                break;
            }
            end -= 1;
        }
        c.add(s, i, i + end, Kind::Url);
        i = j;
    }
}

fn path_char(c: char) -> bool {
    is_word(c) || "@.+-".contains(c)
}

fn sep(c: char) -> bool {
    c == '/' || c == '\\'
}

/// `:12`, `:12:5`, `(12,5)` (tsc), `#L12`, `#L12C5`, `#L12-L20` (GitHub) at `i`: its end, and the
/// line and column it names.
fn position_at(s: &[char], i: usize) -> Option<(usize, u32, Option<u32>)> {
    let digits = |from: usize| {
        let mut j = from;
        while j < s.len() && s[j].is_ascii_digit() {
            j += 1;
        }
        (j > from).then(|| {
            let n: String = s[from..j].iter().collect();
            (j, n.parse::<u32>().unwrap_or(u32::MAX))
        })
    };
    match s.get(i)? {
        ':' => {
            let (j, line) = digits(i + 1)?;
            match digits(j + 1).filter(|_| s.get(j) == Some(&':')) {
                Some((k, col)) => Some((k, line, Some(col))),
                None => Some((j, line, None)),
            }
        }
        '(' => {
            let (j, line) = digits(i + 1)?;
            if s.get(j) == Some(&')') {
                return Some((j + 1, line, None));
            }
            if s.get(j) != Some(&',') {
                return None;
            }
            let (k, col) = digits(skip_ws(s, j + 1))?;
            (s.get(k) == Some(&')')).then_some((k + 1, line, Some(col)))
        }
        '#' => {
            if s.get(i + 1) != Some(&'L') {
                return None;
            }
            let (mut j, line) = digits(i + 2)?;
            let column = |j: usize| digits(j + 1).filter(|_| s.get(j) == Some(&'C'));
            let mut col = None;
            if let Some((k, c)) = column(j) {
                j = k;
                col = Some(c);
            }
            if s.get(j) == Some(&'-') {
                let k = j + 1 + usize::from(s.get(j + 1) == Some(&'L'));
                if let Some((k, _)) = digits(k) {
                    j = column(k).map_or(k, |(k, _)| k);
                }
            }
            Some((j, line, col))
        }
        _ => None,
    }
}

/// A path at `i`: an optional `…/`, `X:\`, `./`, `../` or `/` head, a name and more names after
/// separators, maybe a trailing separator and a position. Its end.
fn file_token(s: &[char], i: usize) -> Option<usize> {
    if i > 0 && (path_char(s[i - 1]) || sep(s[i - 1]) || s[i - 1] == '~') {
        return None;
    }
    let body = |from: usize| -> Option<usize> {
        let first = *s.get(from)?;
        if !(is_word(first) || "@+-".contains(first)) {
            return None;
        }
        let mut j = from + 1;
        while j < s.len() && path_char(s[j]) {
            j += 1;
        }
        while j + 1 < s.len() && sep(s[j]) && path_char(s[j + 1]) {
            j += 1;
            while j < s.len() && path_char(s[j]) {
                j += 1;
            }
        }
        if j < s.len() && sep(s[j]) {
            j += 1;
        }
        Some(position_at(s, j).map_or(j, |(k, ..)| k))
    };
    let at = |k: usize| s.get(i + k).copied();
    let heads = [
        match at(0) {
            Some('…') if at(1).is_some_and(sep) => 2,
            Some('.') if starts(s, i, "...") && at(3).is_some_and(sep) => 4,
            _ => 0,
        },
        match (at(0), at(1), at(2)) {
            (Some(c), Some(':'), Some(d)) if c.is_ascii_alphabetic() && sep(d) => 3,
            _ => 0,
        },
        match (at(0), at(1), at(2)) {
            (Some('.'), Some(a), _) if sep(a) => 2,
            (Some('.'), Some('.'), Some(b)) if sep(b) => 3,
            _ => 0,
        },
        match at(0) {
            Some(a) if sep(a) => 1,
            _ => 0,
        },
    ];
    for h in heads.into_iter().filter(|&h| h > 0) {
        if let Some(end) = body(i + h) {
            return Some(end);
        }
    }
    body(i)
}

/// `.ext` at the end of a name: a dot, a letter, then word characters and dashes.
fn has_extension(name: &[char]) -> bool {
    let Some(dot) = name.iter().rposition(|&c| c == '.') else {
        return false;
    };
    let ext = &name[dot + 1..];
    ext.first().is_some_and(|c| c.is_ascii_alphabetic())
        && ext.iter().all(|&c| is_word(c) || c == '-')
}

/// Where a position ends `s`: the earliest start that reads through to the end.
fn position_start(s: &[char]) -> Option<usize> {
    (0..s.len()).find(|&i| {
        matches!(s[i], ':' | '(' | '#') && position_at(s, i).is_some_and(|(end, ..)| end == s.len())
    })
}

/// Paths with an extension or a position (`bare`: a lone `name.ext` too); not folders.
fn file_paths(s: &[char], c: &mut Collector, bare: bool) {
    let mut i = 0;
    while i < s.len() {
        let Some(end) = file_token(s, i) else {
            i += 1;
            continue;
        };
        let m = &s[i..end];
        let at = position_start(m);
        let mut path = &m[..at.unwrap_or(m.len())];
        // A sentence's full stop isn't the extension's.
        while path.last() == Some(&'.') {
            path = &path[..path.len() - 1];
        }
        let slash = path.iter().rposition(|&c| sep(c));
        let name = &path[slash.map_or(0, |s| s + 1)..];
        let folder = path.last().is_some_and(|&c| sep(c));
        if !folder
            && (has_extension(name) || at.is_some())
            && (slash.is_some() || at.is_some() || bare)
        {
            let len = if at == Some(path.len()) {
                m.len()
            } else {
                path.len()
            };
            c.add(s, i, i + len, Kind::File);
        }
        i = end.max(i + 1);
    }
}

/// A path and the line / column its link names (`a.ts:12:5`, `a.md#L12`).
fn split_position(spec: &str) -> (String, Option<u32>, Option<u32>) {
    let s: Vec<char> = spec.chars().collect();
    match position_start(&s).and_then(|at| Some((at, position_at(&s, at)?))) {
        Some((at, (_, line, col))) => (s[..at].iter().collect(), Some(line), col),
        None => (spec.to_string(), None, None),
    }
}

// ---------------------------------------------------------------- resolving

/// The files a link may name: the worktree's or a commit's.
trait Files {
    /// The first of `paths` that's a file.
    fn first(&mut self, paths: &[String]) -> Option<String>;
    /// The tsconfig and jsconfig files in folder `dir` ("" the root).
    fn configs(&mut self, dir: &str) -> Vec<String>;
    fn read(&mut self, path: &str) -> Option<String>;
}

fn dirname(p: &str) -> &str {
    p.rsplit_once('/').map_or("", |(d, _)| d)
}

fn basename(p: &str) -> &str {
    p.rsplit_once('/').map_or(p, |(_, b)| b)
}

/// `rel` from folder `dir`, normalized; None when it climbs out of the repo.
fn join(dir: &str, rel: &str) -> Option<String> {
    let mut parts: Vec<&str> = dir.split('/').filter(|p| !p.is_empty()).collect();
    for seg in rel.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            seg => parts.push(seg),
        }
    }
    Some(parts.join("/"))
}

const MODULE_EXTS: &[&str] = &[
    ".ts", ".tsx", ".d.ts", ".js", ".jsx", ".mjs", ".cjs", ".mts", ".cts", ".json", ".vue",
    ".svelte", ".css",
];
const STYLE_EXTS: &[&str] = &[".css", ".scss", ".sass", ".less"];

/// A query (vite's `?worker`, `?raw`) or a fragment isn't part of the path.
fn bare(spec: &str) -> &str {
    spec.find(['?', '#']).map_or(spec, |i| &spec[..i])
}

fn is_relative(spec: &str) -> bool {
    matches!(spec, "." | "..") || spec.starts_with("./") || spec.starts_with("../")
}

/// A URL (`https:`, `data:`, `mailto:`), protocol-relative, or a fragment of this page.
fn is_external(spec: &str) -> bool {
    if spec.starts_with("//") || spec.starts_with('#') {
        return true;
    }
    let scheme = spec.find(':').map_or("", |i| &spec[..i]);
    scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c))
}

fn module_candidates(base: &str) -> Vec<String> {
    let dir = if base.is_empty() {
        String::new()
    } else {
        format!("{base}/")
    };
    let mut out = vec![];
    if !base.is_empty() {
        out.push(base.to_string());
        out.extend(MODULE_EXTS.iter().map(|e| format!("{base}{e}")));
    }
    // TypeScript's ESM imports name the output: "./a.js" is a.ts.
    let emitted: &[(&str, &[&str])] = &[
        (".js", &[".ts", ".tsx"]),
        (".jsx", &[".tsx"]),
        (".mjs", &[".mts"]),
        (".cjs", &[".cts"]),
    ];
    let name = basename(base);
    let ext = name.rfind('.').map_or("", |i| &name[i..]);
    if let Some((_, to)) = emitted.iter().find(|(e, _)| *e == ext) {
        let stem = &base[..base.len() - ext.len()];
        out.extend(to.iter().map(|e| format!("{stem}{e}")));
    }
    out.extend(MODULE_EXTS.iter().map(|e| format!("{dir}index{e}")));
    out
}

fn style_candidates(base: &str) -> Vec<String> {
    let dir = dirname(base);
    let partial = if dir.is_empty() {
        format!("_{}", basename(base))
    } else {
        format!("{dir}/_{}", basename(base))
    };
    let mut out = vec![base.to_string()];
    out.extend(STYLE_EXTS.iter().map(|e| format!("{base}{e}")));
    out.extend(STYLE_EXTS.iter().map(|e| format!("{partial}{e}")));
    for e in STYLE_EXTS {
        out.push(format!("{base}/index{e}"));
        out.push(format!("{base}/_index{e}"));
    }
    out
}

/// A tsconfig `paths` entry, its targets from the repo's root.
#[derive(Clone, Debug, PartialEq)]
struct Alias {
    pattern: String,
    targets: Vec<String>,
}

/// Where the aliases send `spec`, the most specific pattern first (TypeScript's order).
fn alias_targets(spec: &str, aliases: &[Alias]) -> Vec<String> {
    let mut hits: Vec<(usize, Vec<String>)> = vec![];
    for a in aliases {
        let Some((pre, post)) = a.pattern.split_once('*') else {
            if a.pattern == spec {
                hits.push((usize::MAX, a.targets.clone()));
            }
            continue;
        };
        if spec.len() < pre.len() + post.len() || !spec.starts_with(pre) || !spec.ends_with(post) {
            continue;
        }
        let middle = &spec[pre.len()..spec.len() - post.len()];
        hits.push((
            pre.len(),
            a.targets
                .iter()
                .map(|t| t.replacen('*', middle, 1))
                .collect(),
        ));
    }
    hits.sort_by_key(|(n, _)| std::cmp::Reverse(*n));
    hits.into_iter().flat_map(|(_, t)| t).collect()
}

/// Percent-escapes decoded, as a browser's `decodeURI`: not those of a URL's separators.
fn decode_uri(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = |k: usize| b.get(k).and_then(|&c| (c as char).to_digit(16));
        let byte = match (b[i], hex(i + 1), hex(i + 2)) {
            (b'%', Some(h), Some(l)) => Some((h * 16 + l) as u8),
            _ => None,
        };
        if let Some(byte) = byte.filter(|c| !b";/?:@&=+$,#".contains(c)) {
            out.push(byte);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_string())
}

/// Where `link` in file `from` goes; None when it names no file there is. `root` is the repo's
/// absolute path, for absolute paths in the text.
fn resolve(link: &Link, from: &str, files: &mut dyn Files, root: &str) -> Option<Target> {
    let dir = dirname(from);
    let spec = link.spec.as_str();
    let found = |path: Option<String>| {
        path.map(|path| Target {
            path,
            line: None,
            column: None,
        })
    };
    match link.kind {
        Kind::Url => None,
        Kind::File => resolve_file(spec, dir, files, root),
        Kind::Module => {
            let path = bare(spec);
            // A package's name stays unlinked: node_modules isn't the repo's.
            let bases = if is_relative(path) {
                join(dir, path).into_iter().collect()
            } else {
                alias_targets(path, &load_aliases(from, files))
            };
            found(
                files.first(
                    &bases
                        .iter()
                        .flat_map(|b| module_candidates(b))
                        .collect::<Vec<_>>(),
                ),
            )
        }
        Kind::Style => {
            let path = bare(spec);
            // "~pkg" is a package; a leading "/" is the site's root, not the repo's.
            if is_external(path) || path.starts_with('~') || path.starts_with('/') {
                return None;
            }
            // @import "a.css" means "./a.css"; Vite also takes the tsconfig aliases.
            let near = join(dir, path)
                .map(|b| style_candidates(&b))
                .unwrap_or_default();
            if let Some(hit) = files.first(&near) {
                return found(Some(hit));
            }
            let aliased: Vec<String> = alias_targets(path, &load_aliases(from, files))
                .iter()
                .flat_map(|b| style_candidates(b))
                .collect();
            found(files.first(&aliased))
        }
        Kind::Doc => {
            let (spec, line, column) = split_position(spec);
            if is_external(&spec) {
                return None;
            }
            let path = decode_uri(bare(&spec));
            // On GitHub a leading "/" is the repo's root.
            let target = join(if path.starts_with('/') { "" } else { dir }, &path)?;
            let path = files.first(&[target])?;
            Some(Target { path, line, column })
        }
        Kind::Path => found(files.first(&[join(dir, spec)?])),
        Kind::Rust => {
            // mod.rs, lib.rs and main.rs own their folder; a.rs owns a/.
            let name = basename(from);
            let owner = if matches!(name, "mod.rs" | "lib.rs" | "main.rs") {
                dir.to_string()
            } else {
                join(dir, name.strip_suffix(".rs").unwrap_or(name))?
            };
            let base = join(&owner, spec)?;
            found(files.first(&[format!("{base}.rs"), format!("{base}/mod.rs")]))
        }
        Kind::Python => {
            let dots = spec.len() - spec.trim_start_matches('.').len();
            let rest: Vec<&str> = spec[dots..].split('.').filter(|p| !p.is_empty()).collect();
            let rest = rest.join("/");
            let py = |base: Option<String>| -> Vec<String> {
                let Some(base) = base else { return vec![] };
                if rest.is_empty() {
                    vec![join(&base, "__init__.py").unwrap_or_default()]
                } else {
                    vec![
                        format!("{base}.py"),
                        format!("{base}.pyi"),
                        format!("{base}/__init__.py"),
                    ]
                }
            };
            let candidates = if dots > 0 {
                py(join(dir, &format!("{}{rest}", "../".repeat(dots - 1))))
            } else {
                // Absolute: from the file's folder up to the root, as its package root is unknown.
                let mut out = vec![];
                let mut d = dir;
                loop {
                    out.extend(py(join(d, &rest)));
                    if d.is_empty() {
                        break;
                    }
                    d = dirname(d);
                }
                out
            };
            found(files.first(&candidates))
        }
    }
}

/// A path with or without a position, from folder `dir` or the root; an absolute one when it's
/// in the repo at `root`.
fn resolve_file(spec: &str, dir: &str, files: &mut dyn Files, root: &str) -> Option<Target> {
    let (raw, line, column) = split_position(spec);
    let path = raw.replace('\\', "/");
    let absolute = path.starts_with('/')
        || (path.as_bytes().get(1) == Some(&b':') && path.as_bytes().get(2) == Some(&b'/'));
    let candidates: Vec<String> = if absolute {
        let base = root.trim_end_matches('/');
        let rel = path.strip_prefix(base).and_then(|r| r.strip_prefix('/'));
        rel.filter(|_| !base.is_empty())
            .map(|r| r.trim_end_matches('/').to_string())
            .into_iter()
            .collect()
    } else {
        let here = join(dir, &path);
        let top = (!dir.is_empty()).then(|| join("", &path)).flatten();
        here.into_iter().chain(top).collect()
    };
    let path = files.first(&candidates)?;
    Some(Target { path, line, column })
}

fn is_config(name: &str) -> bool {
    matches!(name, "tsconfig.json" | "jsconfig.json")
        || name
            .strip_prefix("tsconfig.")
            .and_then(|r| r.strip_suffix(".json"))
            .is_some_and(|m| !m.is_empty())
}

/// The `compilerOptions.paths` aliases for file `from`: those of the nearest folder whose
/// configs have some (tsconfig.json often only references tsconfig.app.json, which has them).
fn load_aliases(from: &str, files: &mut dyn Files) -> Vec<Alias> {
    let mut dir = dirname(from);
    loop {
        let mut found: Vec<Alias> = vec![];
        for config in files.configs(dir) {
            for a in config_aliases(&config, files, 0) {
                // tsconfig.json extending tsconfig.base.json next to it brings the same ones twice.
                if !found.iter().any(|f| f.pattern == a.pattern) {
                    found.push(a);
                }
            }
        }
        if !found.is_empty() || dir.is_empty() {
            return found;
        }
        dir = dirname(dir);
    }
}

fn config_aliases(path: &str, files: &mut dyn Files, depth: usize) -> Vec<Alias> {
    let Some(json) = files.read(path).and_then(|t| parse_jsonc(&t)) else {
        return vec![];
    };
    let dir = dirname(path);
    let options = json.get("compilerOptions");
    if let Some(paths) = options
        .and_then(|o| o.get("paths"))
        .and_then(|p| p.as_object())
    {
        // Targets are from baseUrl, or from the config that sets `paths`.
        let base = match options
            .and_then(|o| o.get("baseUrl"))
            .and_then(|b| b.as_str())
        {
            Some(b) => join(dir, b),
            None => Some(dir.to_string()),
        };
        let Some(base) = base else { return vec![] };
        return paths
            .iter()
            .map(|(pattern, targets)| Alias {
                pattern: pattern.clone(),
                targets: targets
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|t| join(&base, t.as_str()?))
                    .collect(),
            })
            .collect();
    }
    if depth > 4 {
        return vec![];
    }
    // Inherited from a config in the repo; a package's ("@tsconfig/node20") isn't the repo's.
    let parents: Vec<&str> = match json.get("extends") {
        Some(serde_json::Value::String(s)) => vec![s.as_str()],
        Some(serde_json::Value::Array(a)) => a.iter().filter_map(|v| v.as_str()).collect(),
        _ => vec![],
    };
    // The last one wins in TypeScript.
    for e in parents.into_iter().rev().filter(|e| is_relative(e)) {
        let Some(p) = join(dir, e) else { continue };
        let parent = if p.ends_with(".json") {
            p
        } else {
            format!("{p}.json")
        };
        if files.first(std::slice::from_ref(&parent)).is_none() {
            continue;
        }
        let found = config_aliases(&parent, files, depth + 1);
        if !found.is_empty() {
            return found;
        }
    }
    vec![]
}

/// JSON with comments and trailing commas, as tsconfig files are written; None if it isn't.
fn parse_jsonc(text: &str) -> Option<serde_json::Value> {
    // Comments out, then commas before a closing bracket; never inside a string.
    let strip = |text: &str, comments: bool| {
        let b = text.as_bytes();
        let mut out = Vec::with_capacity(b.len());
        let mut i = 0;
        while i < b.len() {
            match b[i] {
                b'"' => {
                    let start = i;
                    i += 1;
                    while i < b.len() && b[i] != b'"' {
                        i += if b[i] == b'\\' { 2 } else { 1 };
                    }
                    i = (i + 1).min(b.len());
                    out.extend_from_slice(&b[start..i]);
                    continue;
                }
                b'/' if comments && b.get(i + 1) == Some(&b'/') => {
                    while i < b.len() && b[i] != b'\n' {
                        i += 1;
                    }
                    continue;
                }
                b'/' if comments && b.get(i + 1) == Some(&b'*') => {
                    i = text[i + 2..].find("*/").map_or(b.len(), |e| i + 2 + e + 2);
                    out.push(b' ');
                    continue;
                }
                b',' if !comments => {
                    let next = b[i + 1..].iter().find(|c| !c.is_ascii_whitespace());
                    if matches!(next, Some(b'}' | b']')) {
                        i += 1;
                        continue;
                    }
                }
                _ => {}
            }
            out.push(b[i]);
            i += 1;
        }
        String::from_utf8_lossy(&out).into_owned()
    };
    serde_json::from_str(&strip(&strip(text, true), false)).ok()
}

/// Whether a repo path is git's own (`.git/…`), which no link opens.
fn in_git_dir(path: &str) -> bool {
    path.split('/').any(|p| p.eq_ignore_ascii_case(".git"))
}

/// The worktree's files, as they are on disk: their names compared in their own case, as a
/// case-insensitive disk would open `Utils.ts` for `utils.ts` under another tab's name.
struct Disk<'a> {
    repo: &'a Path,
    /// Each folder read: its entries, and whether each is a file.
    dirs: HashMap<String, Option<HashMap<String, bool>>>,
}

impl<'a> Disk<'a> {
    fn new(repo: &'a Path) -> Self {
        Disk {
            repo,
            dirs: HashMap::new(),
        }
    }

    fn entries(&mut self, dir: &str) -> Option<&HashMap<String, bool>> {
        let repo = self.repo;
        self.dirs
            .entry(dir.to_string())
            .or_insert_with(|| {
                let read = std::fs::read_dir(repo.join(dir)).ok()?;
                Some(
                    read.flatten()
                        .map(|e| {
                            let file = match e.file_type() {
                                Ok(t) if t.is_symlink() => e.path().is_file(),
                                Ok(t) => t.is_file(),
                                Err(_) => false,
                            };
                            (e.file_name().to_string_lossy().into_owned(), file)
                        })
                        .collect(),
                )
            })
            .as_ref()
    }

    fn is_file(&mut self, path: &str) -> bool {
        if path.is_empty() || in_git_dir(path) {
            return false;
        }
        let mut dir = String::new();
        let parts: Vec<&str> = path.split('/').collect();
        for (i, part) in parts.iter().enumerate() {
            let last = i == parts.len() - 1;
            match self.entries(&dir).and_then(|e| e.get(*part)) {
                Some(&file) if file == last => {}
                _ => return false,
            }
            if !dir.is_empty() {
                dir.push('/');
            }
            dir.push_str(part);
        }
        true
    }
}

impl Files for Disk<'_> {
    fn first(&mut self, paths: &[String]) -> Option<String> {
        paths.iter().find(|p| self.is_file(p)).cloned()
    }

    fn configs(&mut self, dir: &str) -> Vec<String> {
        let mut names: Vec<String> = self
            .entries(dir)
            .into_iter()
            .flatten()
            .filter(|(name, file)| **file && is_config(name))
            .map(|(name, _)| join(dir, name).unwrap_or_default())
            .collect();
        names.sort();
        names
    }

    fn read(&mut self, path: &str) -> Option<String> {
        let f = fs::read_file(self.repo, path);
        (f.exists && !f.binary && !f.too_large).then_some(f.text)
    }
}

/// A commit's files, asked of git a few at a time.
struct Commit<'a> {
    repo: &'a Path,
    rev: &'a str,
    /// The configs of the folders asked about and those above them.
    configs: Option<HashMap<String, Vec<String>>>,
}

impl<'a> Commit<'a> {
    fn new(repo: &'a Path, rev: &'a str) -> Self {
        Commit {
            repo,
            rev,
            configs: None,
        }
    }

    /// `git ls-tree` of `paths` (a folder's with a trailing `/` lists what's in it): each
    /// entry's path, and whether it's a file.
    fn list(&self, paths: &[&str]) -> Vec<(String, bool)> {
        let mut args = vec!["ls-tree", "-z", "--full-tree", self.rev, "--"];
        args.extend(paths);
        let Ok(out) = git::run(self.repo, &args) else {
            return vec![];
        };
        out.split(|&b| b == 0)
            .filter_map(|entry| {
                let entry = String::from_utf8_lossy(entry);
                let (meta, path) = entry.split_once('\t')?;
                Some((path.to_string(), meta.split(' ').nth(1) == Some("blob")))
            })
            .collect()
    }
}

impl Files for Commit<'_> {
    fn first(&mut self, paths: &[String]) -> Option<String> {
        let asked: Vec<&str> = paths
            .iter()
            .map(String::as_str)
            .filter(|p| !p.is_empty() && !in_git_dir(p))
            .collect();
        if asked.is_empty() {
            return None;
        }
        let blobs: Vec<String> = self
            .list(&asked)
            .into_iter()
            .filter_map(|(p, blob)| blob.then_some(p))
            .collect();
        asked
            .into_iter()
            .find(|p| blobs.iter().any(|b| b == p))
            .map(String::from)
    }

    fn configs(&mut self, dir: &str) -> Vec<String> {
        if self.configs.is_none() {
            // The folder and all above it, in two calls: the root lists without a path.
            let mut above = vec![];
            let mut d = dir;
            while !d.is_empty() {
                above.push(format!("{d}/"));
                d = dirname(d);
            }
            let above: Vec<&str> = above.iter().map(String::as_str).collect();
            let mut listed = if above.is_empty() {
                vec![]
            } else {
                self.list(&above)
            };
            listed.extend(self.list(&[]));
            let mut configs: HashMap<String, Vec<String>> = HashMap::new();
            for (path, blob) in listed {
                if blob && is_config(basename(&path)) {
                    configs
                        .entry(dirname(&path).to_string())
                        .or_default()
                        .push(path);
                }
            }
            for list in configs.values_mut() {
                list.sort();
                list.dedup();
            }
            self.configs = Some(configs);
        }
        self.configs
            .as_ref()
            .and_then(|c| c.get(dir))
            .cloned()
            .unwrap_or_default()
    }

    fn read(&mut self, path: &str) -> Option<String> {
        let f = git::text_at(self.repo, self.rev, path).ok()?;
        (f.exists && !f.binary && !f.too_large).then_some(f.text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// Files in memory, with the texts of some.
    struct Index {
        files: HashSet<String>,
        texts: HashMap<String, String>,
    }

    impl Index {
        fn new(files: &[&str]) -> Self {
            Index {
                files: files.iter().map(|f| f.to_string()).collect(),
                texts: HashMap::new(),
            }
        }
    }

    impl Files for Index {
        fn first(&mut self, paths: &[String]) -> Option<String> {
            paths.iter().find(|p| self.files.contains(*p)).cloned()
        }
        fn configs(&mut self, dir: &str) -> Vec<String> {
            let mut out: Vec<String> = self
                .files
                .iter()
                .filter(|f| dirname(f) == dir && is_config(basename(f)))
                .cloned()
                .collect();
            out.sort();
            out
        }
        fn read(&mut self, path: &str) -> Option<String> {
            self.texts.get(path).cloned()
        }
    }

    fn family(lang: &str) -> Family {
        match lang {
            "typescript" | "javascript" | "tsx" => Family::Js,
            "css" | "scss" => Family::Css,
            "markdown" => Family::Markdown,
            "rust" => Family::Rust,
            "python" => Family::Python,
            "text" => Family::Text,
            _ => Family::Other,
        }
    }

    /// Each link of a whole line: its text and kind.
    fn specs(line: &str, lang: &str) -> Vec<(String, Kind)> {
        let chars: Vec<char> = line.chars().collect();
        find_links(&chars, family(lang), true)
            .into_iter()
            .map(|l| (chars[l.start..l.end].iter().collect(), l.kind))
            .collect()
    }

    fn of(pairs: &[(&str, Kind)]) -> Vec<(String, Kind)> {
        pairs.iter().map(|(s, k)| (s.to_string(), *k)).collect()
    }

    use Kind::*;

    /// Lines, their language, and the links found in each.
    type Cases<'a> = &'a [(&'a str, &'a str, &'a [(&'a str, Kind)])];

    #[test]
    fn finds_js_and_ts_module_specifiers() {
        let cases: Cases = &[
            (
                r#"import { a } from "./a";"#,
                "typescript",
                &[("./a", Module)],
            ),
            (
                "export * from '../b.js'",
                "javascript",
                &[("../b.js", Module)],
            ),
            (
                r#"} from "@/lib/utils";"#,
                "tsx",
                &[("@/lib/utils", Module)],
            ),
            (
                r#"import "./side-effect.css";"#,
                "typescript",
                &[("./side-effect.css", Module)],
            ),
            (
                r#"const m = await import("./lazy"), r = require('fs');"#,
                "javascript",
                &[("./lazy", Module), ("fs", Module)],
            ),
            (
                r#"vi.mock("./api", () => ({}))"#,
                "typescript",
                &[("./api", Module)],
            ),
            (r#"const x = "not a path";"#, "typescript", &[]),
            (r#"const important = "x";"#, "typescript", &[]),
        ];
        for (line, lang, want) in cases {
            assert_eq!(specs(line, lang), of(want), "{line}");
        }
    }

    #[test]
    fn finds_css_rust_python_and_markdown_links() {
        let cases: Cases = &[
            (
                r#"@import "tailwindcss";"#,
                "css",
                &[("tailwindcss", Style)],
            ),
            (
                r#"  background: url("../img/bg.png") no-repeat;"#,
                "css",
                &[("../img/bg.png", Style)],
            ),
            (
                "  background: url(img/bg.png);",
                "css",
                &[("img/bg.png", Style)],
            ),
            ("@use 'sass:math';", "scss", &[("sass:math", Style)]),
            (
                "pub(crate) mod scenario_tests;",
                "rust",
                &[("scenario_tests", Rust)],
            ),
            ("mod git;", "rust", &[("git", Rust)]),
            ("    pub mod r#async ;", "rust", &[("async", Rust)]),
            ("mod tests {", "rust", &[]),
            (
                "from ..core.models import User",
                "python",
                &[("..core.models", Python)],
            ),
            (
                "import os.path, pkg.util as u  # comment, x",
                "python",
                &[("os.path", Python), ("pkg.util", Python)],
            ),
            (
                r#"See [the guide](docs/guide.md#setup "Guide") and ![logo](<assets/logo.png>)."#,
                "markdown",
                &[("docs/guide.md#setup", Doc), ("assets/logo.png", Doc)],
            ),
            (
                "[ref]: ./CONTRIBUTING.md",
                "markdown",
                &[("./CONTRIBUTING.md", Doc)],
            ),
            (
                r#"<img src="assets/icon.png" width="64">"#,
                "markdown",
                &[("assets/icon.png", Doc)],
            ),
            (
                r#"path = "./fixtures/a.json""#,
                "toml",
                &[("./fixtures/a.json", Path)],
            ),
        ];
        for (line, lang, want) in cases {
            assert_eq!(specs(line, lang), of(want), "{line}");
        }
    }

    #[test]
    fn finds_urls_without_the_punctuation_around_them() {
        let cases: &[(&str, &str, &str)] = &[
            (
                "// see https://github.com/a/b/issues/40.",
                "typescript",
                "https://github.com/a/b/issues/40",
            ),
            (
                "(docs: https://en.wikipedia.org/wiki/Tree_(graph_theory))",
                "text",
                "https://en.wikipedia.org/wiki/Tree_(graph_theory)",
            ),
            (
                "[GitHub CLI](https://cli.github.com), ok",
                "markdown",
                "https://cli.github.com",
            ),
            (
                r#"import x from "https://esm.sh/react";"#,
                "typescript",
                "https://esm.sh/react",
            ),
            (
                r#"const u = "http://localhost:1420/?fixture";"#,
                "typescript",
                "http://localhost:1420/?fixture",
            ),
        ];
        for (line, lang, url) in cases {
            assert_eq!(specs(line, lang), of(&[(url, Url)]), "{line}");
        }
    }

    #[test]
    fn finds_paths_with_a_line_and_column() {
        let cases: Cases = &[
            (
                "// moved to src/lib/api.ts:42, see there.",
                "typescript",
                &[("src/lib/api.ts:42", File)],
            ),
            (
                "Fixed in src/lib/api.ts:42:7 and README.md:3.",
                "markdown",
                &[("src/lib/api.ts:42:7", File), ("README.md:3", File)],
            ),
            (
                "src/a.ts(12,5): error TS2304",
                "text",
                &[("src/a.ts(12,5)", File)],
            ),
            (
                "[the loop](docs/guide.md#L10-L20)",
                "markdown",
                &[("docs/guide.md#L10-L20", Doc)],
            ),
            // Member access isn't a file; a bare name.ext only counts with a line.
            ("const p = e.target.position; api.ts", "typescript", &[]),
            ("see api.ts:12", "typescript", &[("api.ts:12", File)]),
            ("see api.ts in the notes", "text", &[("api.ts", File)]),
            ("// ~/notes.md is mine", "typescript", &[]),
        ];
        for (line, lang, want) in cases {
            assert_eq!(specs(line, lang), of(want), "{line}");
        }
        assert_eq!(
            split_position("src/a.ts:12:5"),
            ("src/a.ts".into(), Some(12), Some(5))
        );
        assert_eq!(
            split_position("src/a.ts(12,5)"),
            ("src/a.ts".into(), Some(12), Some(5))
        );
        assert_eq!(
            split_position("a.md#L10C3-L20"),
            ("a.md".into(), Some(10), Some(3))
        );
        assert_eq!(split_position("src/a.ts"), ("src/a.ts".into(), None, None));
    }

    #[test]
    fn join_stays_inside_the_repo() {
        assert_eq!(
            join("src/lib", "../features/./A.tsx").as_deref(),
            Some("src/features/A.tsx")
        );
        assert_eq!(join("src", "../.."), None);
        assert_eq!(join("", "a//b/").as_deref(), Some("a/b"));
    }

    fn index() -> Index {
        Index::new(&[
            "src/lib/utils.ts",
            "src/lib/api.ts",
            "src/lib/monaco.ts",
            "src/lib/editor/index.tsx",
            "src/lib/types.d.ts",
            "src/lib/data.json",
            "src/features/A.tsx",
            "src/styles/_mixins.scss",
            "src/styles/base.css",
            "src/img/bg.png",
            "src-tauri/src/lib.rs",
            "src-tauri/src/git.rs",
            "src-tauri/src/git/blame.rs",
            "src-tauri/src/net/mod.rs",
            "app/core/__init__.py",
            "app/core/models.py",
            "app/api/views.py",
            "docs/guide.md",
            "docs/my file.md",
            "README.md",
            "tsconfig.json",
        ])
    }

    /// Where `spec` of `kind` in `from` goes, with `@/*` → `src/*` as the repo's alias.
    fn resolved(spec: &str, kind: Kind, from: &str) -> Option<String> {
        let mut files = index();
        files.texts.insert(
            "tsconfig.json".into(),
            r#"{ "compilerOptions": { "paths": { "@/*": ["./src/*"] } } }"#.into(),
        );
        let link = Link {
            start: 0,
            end: spec.chars().count(),
            spec: spec.into(),
            kind,
        };
        resolve(&link, from, &mut files, "").map(|t| t.path)
    }

    #[test]
    fn resolves_modules_extensions_index_files_emitted_names_aliases() {
        let cases = [
            ("./utils", "src/lib/api.ts", Some("src/lib/utils.ts")),
            (
                "./editor",
                "src/lib/api.ts",
                Some("src/lib/editor/index.tsx"),
            ),
            ("./types", "src/lib/api.ts", Some("src/lib/types.d.ts")),
            ("./data.json", "src/lib/api.ts", Some("src/lib/data.json")),
            ("./utils.js", "src/lib/api.ts", Some("src/lib/utils.ts")),
            (
                "../lib/monaco?worker",
                "src/features/A.tsx",
                Some("src/lib/monaco.ts"),
            ),
            (
                "@/lib/utils",
                "src/features/A.tsx",
                Some("src/lib/utils.ts"),
            ),
            ("@/features/A", "src/lib/api.ts", Some("src/features/A.tsx")),
            ("react", "src/lib/api.ts", None),
            ("./missing", "src/lib/api.ts", None),
            ("../../../../x", "src/lib/api.ts", None),
            // .cjsx looks emitted but has no source extension.
            ("./x.cjsx", "src/lib/api.ts", None),
        ];
        for (spec, from, want) in cases {
            assert_eq!(resolved(spec, Module, from).as_deref(), want, "{spec}");
        }
    }

    #[test]
    fn resolves_styles_docs_and_plain_paths() {
        let cases = [
            (
                "base.css",
                Style,
                "src/styles/app.css",
                Some("src/styles/base.css"),
            ),
            (
                "mixins",
                Style,
                "src/styles/app.scss",
                Some("src/styles/_mixins.scss"),
            ),
            (
                "../img/bg.png",
                Style,
                "src/styles/app.css",
                Some("src/img/bg.png"),
            ),
            (
                "@/styles/base",
                Style,
                "src/features/a.css",
                Some("src/styles/base.css"),
            ),
            ("tailwindcss", Style, "src/index.css", None),
            ("data:image/png;base64,x", Style, "src/index.css", None),
            (
                "docs/guide.md#setup",
                Doc,
                "README.md",
                Some("docs/guide.md"),
            ),
            ("../README.md", Doc, "docs/guide.md", Some("README.md")),
            ("/README.md", Doc, "docs/guide.md", Some("README.md")),
            (
                "my%20file.md",
                Doc,
                "docs/guide.md",
                Some("docs/my file.md"),
            ),
            ("https://example.com/README.md", Doc, "README.md", None),
            ("./guide.md", Path, "docs/x.toml", Some("docs/guide.md")),
            ("./guide", Path, "docs/x.toml", None),
        ];
        for (spec, kind, from, want) in cases {
            assert_eq!(resolved(spec, kind, from).as_deref(), want, "{spec}");
        }
    }

    #[test]
    fn resolves_rust_modules_and_python_imports() {
        let cases = [
            (
                "git",
                Rust,
                "src-tauri/src/lib.rs",
                Some("src-tauri/src/git.rs"),
            ),
            (
                "net",
                Rust,
                "src-tauri/src/lib.rs",
                Some("src-tauri/src/net/mod.rs"),
            ),
            (
                "blame",
                Rust,
                "src-tauri/src/git.rs",
                Some("src-tauri/src/git/blame.rs"),
            ),
            ("missing", Rust, "src-tauri/src/lib.rs", None),
            (
                ".models",
                Python,
                "app/core/__init__.py",
                Some("app/core/models.py"),
            ),
            (
                "..core.models",
                Python,
                "app/api/views.py",
                Some("app/core/models.py"),
            ),
            (
                "..core",
                Python,
                "app/api/views.py",
                Some("app/core/__init__.py"),
            ),
            (
                "app.core.models",
                Python,
                "app/api/views.py",
                Some("app/core/models.py"),
            ),
            ("os.path", Python, "app/api/views.py", None),
        ];
        for (spec, kind, from, want) in cases {
            assert_eq!(resolved(spec, kind, from).as_deref(), want, "{spec}");
        }
    }

    #[test]
    fn resolves_paths_with_positions_from_the_files_folder_or_the_root() {
        let at = |spec: &str, kind: Kind, from: &str, root: &str| {
            let link = Link {
                start: 0,
                end: spec.len(),
                spec: spec.into(),
                kind,
            };
            resolve(&link, from, &mut index(), root)
        };
        let t = |path: &str, line: Option<u32>, column: Option<u32>| {
            Some(Target {
                path: path.into(),
                line,
                column,
            })
        };
        assert_eq!(
            at("src/lib/api.ts:42:7", File, "docs/guide.md", ""),
            t("src/lib/api.ts", Some(42), Some(7))
        );
        assert_eq!(
            at("utils.ts:3", File, "src/lib/api.ts", ""),
            t("src/lib/utils.ts", Some(3), None)
        );
        assert_eq!(
            at("/repo/README.md:1", File, "src/lib/api.ts", "/repo"),
            t("README.md", Some(1), None)
        );
        assert_eq!(
            at("/elsewhere/README.md", File, "src/lib/api.ts", "/repo"),
            None
        );
        assert_eq!(at("/README.md", File, "src/lib/api.ts", ""), None);
        assert_eq!(at("src/lib/nope.ts:1", File, "README.md", ""), None);
        assert_eq!(
            at("guide.md#L10", Doc, "docs/x.md", ""),
            t("docs/guide.md", Some(10), None)
        );
        assert_eq!(
            at(
                "C:\\repo\\src\\lib\\api.ts:4:2",
                File,
                "README.md",
                "C:/repo"
            ),
            t("src/lib/api.ts", Some(4), Some(2))
        );
        assert_eq!(at("https://x.com/a", Url, "README.md", ""), None);
    }

    #[test]
    fn tsconfig_aliases_come_from_the_nearest_folders_configs_through_extends() {
        let mut files = Index::new(&[
            "tsconfig.json",
            "tsconfig.app.json",
            "web/tsconfig.json",
            "web/tsconfig.base.json",
            "src/a.ts",
            "web/app/b.ts",
        ]);
        for (path, text) in [
            ("tsconfig.json", r#"{ "files": [], "references": [{ "path": "./tsconfig.app.json" }] }"#),
            (
                "tsconfig.app.json",
                "{\n  // Vite's layout\n  \"compilerOptions\": { \"paths\": { \"@/*\": [\"./src/*\"], }, /* trailing comma */ },\n}",
            ),
            ("web/tsconfig.json", r#"{ "extends": "./tsconfig.base", "compilerOptions": { "strict": true } }"#),
            (
                "web/tsconfig.base.json",
                r#"{ "compilerOptions": { "baseUrl": "app", "paths": { "~/*": ["*"], "cfg": ["config/index.ts"] } } }"#,
            ),
        ] {
            files.texts.insert(path.into(), text.into());
        }
        let alias = |pattern: &str, targets: &[&str]| Alias {
            pattern: pattern.into(),
            targets: targets.iter().map(|t| t.to_string()).collect(),
        };
        assert_eq!(
            load_aliases("src/a.ts", &mut files),
            [alias("@/*", &["src/*"])]
        );
        let mut web = load_aliases("web/app/b.ts", &mut files);
        web.sort_by(|a, b| a.pattern.cmp(&b.pattern));
        assert_eq!(
            web,
            [
                alias("cfg", &["web/app/config/index.ts"]),
                alias("~/*", &["web/app/*"])
            ]
        );
        assert_eq!(load_aliases("a.ts", &mut Index::new(&["a.ts"])), []);
    }

    #[test]
    fn jsonc_keeps_comment_like_text_inside_strings() {
        assert_eq!(
            parse_jsonc("{ \"a\": \"http://x/*y*/, }\", // c\n \"b\": [1,], }"),
            Some(serde_json::json!({ "a": "http://x/*y*/, }", "b": [1] }))
        );
        assert_eq!(
            parse_jsonc("{ \"a\\\"\": 1 }"),
            Some(serde_json::json!({ "a\"": 1 }))
        );
        assert_eq!(parse_jsonc("{ nope"), None);
    }

    #[test]
    fn a_long_line_costs_only_the_window_around_the_place() {
        // A minified bundle: one 1 MB line of require()s.
        let bundle = r#"var a=require("./a.js");"#.repeat(45_000);
        let req = |column: u32| Request {
            path: "dist/bundle.js".into(),
            text: bundle.clone(),
            line: 1,
            column,
            rev: None,
        };
        let mid = (bundle.len() / 2) as u32;
        let t = std::time::Instant::now();
        let at = byte_at_utf16(&bundle, mid);
        let (w, whole, to_end, before) = window(&bundle, at);
        assert!(!whole && !to_end && w.chars().count() <= 2 * WINDOW);
        let chars: Vec<char> = w.chars().collect();
        let spot = at - before + w[at - before..].find("./a.js").unwrap() + 1;
        let link = link_at(&chars, Family::Js, spot, whole, to_end).unwrap();
        assert_eq!(link.spec, "./a.js");
        assert!(t.elapsed() < std::time::Duration::from_millis(150));
        assert!(follow(std::path::Path::new("/nonexistent"), &req(mid)).is_none());
        // A link the window cuts isn't one.
        let long = format!("\"./{}c.ts\"", "a/".repeat(3000));
        let chars: Vec<char> = long.chars().collect();
        assert!(link_at(&chars, Family::Js, 1, true, true).is_some());
        let (w, whole, to_end, _) = window(&long, long.len() - 2);
        let part: Vec<char> = w.chars().collect();
        assert!(link_at(&part, Family::Js, part.len() - 2, whole, to_end).is_none());
    }

    #[test]
    fn pathological_input_stays_linear() {
        let time = |line: String, family: Family| {
            let chars: Vec<char> = line.chars().collect();
            let t = std::time::Instant::now();
            find_links(&chars, family, true);
            t.elapsed()
        };
        let bound = std::time::Duration::from_millis(150);
        assert!(time(format!("from {}", ".".repeat(20_000)), Family::Python) < bound);
        assert!(time(format!("see {}", "a.".repeat(20_000)), Family::Markdown) < bound);
        assert!(time(format!("https://x.com/{}a", ".".repeat(4000)), Family::Text) < bound);
        assert!(time(format!("import \"{}", "a".repeat(4000)), Family::Js) < bound);
        assert!(time("\"./a ".repeat(1000), Family::Js) < bound);
        assert!(time("](".repeat(2000), Family::Markdown) < bound);
    }

    /// A repo with `files` committed, then `after` written but not committed.
    fn repo(
        name: &str,
        files: &[(&str, &str)],
        after: &[(&str, &str)],
    ) -> (std::path::PathBuf, String) {
        let dir =
            std::env::temp_dir().join(format!("gitviber-links-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let write = |files: &[(&str, &str)]| {
            for (path, text) in files {
                let p = dir.join(path);
                std::fs::create_dir_all(p.parent().unwrap()).unwrap();
                std::fs::write(p, text).unwrap();
            }
        };
        write(files);
        for args in [
            &["init", "-q", "-b", "main"][..],
            &["add", "-A"],
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "commit",
                "-q",
                "-m",
                "c",
            ],
        ] {
            git::run(&dir, args).unwrap();
        }
        let head = String::from_utf8(git::run(&dir, &["rev-parse", "HEAD"]).unwrap()).unwrap();
        write(after);
        (dir, head.trim().to_string())
    }

    #[test]
    fn the_worktree_and_a_commit_each_have_their_own_files() {
        let (dir, head) = repo(
            "trees",
            &[
                ("src/lib.rs", "mod git;\nmod net;\n"),
                ("src/git.rs", ""),
                ("src/net/mod.rs", ""),
                (
                    "web/tsconfig.json",
                    "{ \"extends\": \"../tsconfig.base.json\" }",
                ),
                (
                    "tsconfig.base.json",
                    "{ \"compilerOptions\": { \"paths\": { \"@/*\": [\"./web/src/*\"] } } }",
                ),
                ("web/src/util/Index.ts", ""),
                (
                    "web/src/a.ts",
                    "import { u } from \"@/util/Index\";\nimport x from './util/index';\n",
                ),
            ],
            &[("src/fresh.rs", "")],
        );
        let ask = |path: &str, text: &str, line: u32, column: u32, rev: Option<&str>| {
            let req = Request {
                path: path.into(),
                text: text.into(),
                line,
                column,
                rev: rev.map(String::from),
            };
            follow(&dir, &req).map(|(l, span)| (l.path, span))
        };
        let lib = "mod git;\nmod net;\nmod fresh;\n";
        for rev in [None, Some(head.as_str())] {
            assert_eq!(
                ask("src/lib.rs", lib, 1, 5, rev),
                Some(("src/git.rs".into(), (4, 7)))
            );
            assert_eq!(
                ask("src/lib.rs", lib, 2, 4, rev),
                Some(("src/net/mod.rs".into(), (4, 7)))
            );
            // Off the name, nothing.
            assert_eq!(ask("src/lib.rs", lib, 1, 0, rev), None);
            let a = "import { u } from \"@/util/Index\";\nimport x from './util/index';\n";
            assert_eq!(
                ask("web/src/a.ts", a, 1, 22, rev),
                Some(("web/src/util/Index.ts".into(), (19, 31)))
            );
            // Its own case: a case-insensitive disk would open it under another name.
            assert_eq!(ask("web/src/a.ts", a, 2, 18, rev), None);
        }
        // Not committed: on disk only.
        assert_eq!(
            ask("src/lib.rs", lib, 3, 5, None),
            Some(("src/fresh.rs".into(), (4, 9)))
        );
        assert_eq!(ask("src/lib.rs", lib, 3, 5, Some(&head)), None);
        // Go to Definition on `mod git;` opens git.rs, not its own line.
        let req = Request {
            path: "src/lib.rs".into(),
            text: lib.into(),
            line: 1,
            column: 5,
            rev: None,
        };
        let opened = super::super::find(&dir, &req).unwrap();
        assert_eq!(
            opened,
            [Location {
                path: "src/git.rs".into(),
                line: 1,
                column: 0,
                end_column: 0
            }]
        );
        // git's own files aren't opened.
        assert_eq!(ask("notes.md", "[c](.git/config)", 1, 6, None), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
