//! What ⌘-click (Ctrl off macOS) opens in terminal output: URLs, paths with or without a line,
//! folders, paths shortened to their end, and what may be a commit or an issue. Found a line at a
//! time, on chars (a line's offsets are char indices), then resolved against the repo's file list.
//! Pure, so it's tested here.

use std::collections::HashSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkKind {
    Url,
    File,
    Commit,
    Issue,
}

/// A link in a line: `[start, end)` in chars, `spec` the text between them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    pub start: usize,
    pub end: usize,
    pub spec: String,
    pub kind: LinkKind,
    /// An absolute path with spaces in it may run on past this link: the longer readings,
    /// longest first.
    pub longer: Vec<Link>,
}

/// Where a link goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Url(String),
    /// A repo file at its line (1-based) and column, or a repo folder.
    Path {
        path: String,
        line: Option<u32>,
        column: Option<u32>,
        dir: bool,
    },
    /// A commit, by its full id.
    Commit(String),
    /// A pull request or an issue: `pull` when the URL said it's a pull request; `repo` its
    /// https://github.com/owner/name.
    Item {
        number: u64,
        pull: bool,
        repo: String,
    },
    /// A file or folder outside the repo, by its real path.
    External {
        path: String,
        line: Option<u32>,
        column: Option<u32>,
        dir: bool,
    },
}

/// Characters either side of the pointer that are read: terminal output wraps into one line of a
/// megabyte, and reading all of it on every pointer move would freeze the UI.
pub const LINK_WINDOW: usize = 2000;

fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Keeps links in order; the first to claim some text keeps it.
#[derive(Default)]
struct Collector {
    links: Vec<Link>,
}

impl Collector {
    fn add(&mut self, start: usize, spec: String, kind: LinkKind) {
        if spec.is_empty() {
            return;
        }
        let end = start + spec.chars().count();
        let at = self.links.partition_point(|l| l.start < start);
        if (at > 0 && self.links[at - 1].end > start)
            || (at < self.links.len() && self.links[at].start < end)
        {
            return;
        }
        self.links.insert(
            at,
            Link {
                start,
                end,
                spec,
                kind,
                longer: vec![],
            },
        );
    }
}

fn text(chars: &[char]) -> String {
    chars.iter().collect()
}

/// The links in a line of terminal output; with `near` (`[start, end)` chars), only those around
/// it. A link the window cuts isn't one: its tail could name another file.
pub fn find_terminal_links(line: &[char], near: Option<(usize, usize)>) -> Vec<Link> {
    let from = near.map_or(0, |(s, _)| s.saturating_sub(LINK_WINDOW));
    let to = near.map_or(line.len(), |(_, e)| (e + LINK_WINDOW).min(line.len()));
    let part = &line[from..to.max(from)];
    let mut c = Collector::default();
    urls(part, &mut c);
    file_paths(part, &mut c);
    git_refs(part, &mut c);
    let mut out: Vec<Link> = c
        .links
        .into_iter()
        .filter(|l| (from == 0 || l.start > 0) && (to == line.len() || l.end < part.len()))
        .map(|mut l| {
            l.start += from;
            l.end += from;
            l
        })
        .collect();
    for l in &mut out {
        if l.kind == LinkKind::File {
            l.longer = spaced_paths(line, l);
        }
    }
    out
}

/// Left off a URL's end: the sentence around it.
const TRAILING: &[char] = &['.', ',', ';', ':', '!', '?', '*', '\'', '"'];

fn urls(line: &[char], c: &mut Collector) {
    let lower = |i: usize| line.get(i).map(|c| c.to_ascii_lowercase());
    let mut i = 0;
    while i < line.len() {
        let starts = lower(i) == Some('h')
            && (i == 0 || !is_word(line[i - 1]))
            && lower(i + 1) == Some('t')
            && lower(i + 2) == Some('t')
            && lower(i + 3) == Some('p');
        if !starts {
            i += 1;
            continue;
        }
        let mut j = i + 4;
        if lower(j) == Some('s') {
            j += 1;
        }
        if line.get(j..j + 3) != Some(&[':', '/', '/'][..]) {
            i += 1;
            continue;
        }
        j += 3;
        let body = j;
        while j < line.len() && !line[j].is_whitespace() && !"\"'`<>{}|\\^".contains(line[j]) {
            j += 1;
        }
        if j == body {
            i = j;
            continue;
        }
        let url = &line[i..j];
        // A closing bracket belongs to the URL only if it opened one: (see https://x.com/a_(b)).
        let count = |ch: char| url.iter().filter(|&&x| x == ch).count() as i64;
        let (mut parens, mut brackets) = (count(')') - count('('), count(']') - count('['));
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
        c.add(i, text(&url[..end]), LinkKind::Url);
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
        (j > from).then(|| (j, text(&s[from..j]).parse::<u32>().unwrap_or(u32::MAX)))
    };
    match s.get(i)? {
        ':' => {
            let (j, line) = digits(i + 1)?;
            if s.get(j) == Some(&':')
                && let Some((k, col)) = digits(j + 1)
            {
                return Some((k, line, Some(col)));
            }
            Some((j, line, None))
        }
        '(' => {
            let (j, line) = digits(i + 1)?;
            if s.get(j) == Some(&')') {
                return Some((j + 1, line, None));
            }
            if s.get(j) != Some(&',') {
                return None;
            }
            let mut k = j + 1;
            while s.get(k).is_some_and(|c| c.is_whitespace()) {
                k += 1;
            }
            let (k, col) = digits(k)?;
            (s.get(k) == Some(&')')).then_some((k + 1, line, Some(col)))
        }
        '#' => {
            if s.get(i + 1) != Some(&'L') {
                return None;
            }
            let (mut j, line) = digits(i + 2)?;
            let mut col = None;
            if s.get(j) == Some(&'C')
                && let Some((k, c)) = digits(j + 1)
            {
                j = k;
                col = Some(c);
            }
            if s.get(j) == Some(&'-') {
                let mut k = j + 1;
                if s.get(k) == Some(&'L') {
                    k += 1;
                }
                if let Some((k, _)) = digits(k) {
                    j = k;
                    if s.get(j) == Some(&'C')
                        && let Some((k, _)) = digits(j + 1)
                    {
                        j = k;
                    }
                }
            }
            Some((j, line, col))
        }
        _ => None,
    }
}

/// A path's token at `i` (FILE in the old app): an optional `…/`, `X:\`, `./`, `../` or `/` head,
/// then a name and more names after separators, maybe a trailing separator and a position.
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
    let heads: [usize; 4] = [
        // …/ or .../
        match s.get(i) {
            Some('…') if s.get(i + 1).is_some_and(|&c| sep(c)) => 2,
            Some('.')
                if s.get(i..i + 3) == Some(&['.', '.', '.'][..])
                    && s.get(i + 3).is_some_and(|&c| sep(c)) =>
            {
                4
            }
            _ => 0,
        },
        // A drive letter.
        match (s.get(i), s.get(i + 1), s.get(i + 2)) {
            (Some(c), Some(':'), Some(&d)) if c.is_ascii_alphabetic() && sep(d) => 3,
            _ => 0,
        },
        // ./ or ../
        match (s.get(i), s.get(i + 1), s.get(i + 2)) {
            (Some('.'), Some(&a), _) if sep(a) => 2,
            (Some('.'), Some('.'), Some(&b)) if sep(b) => 3,
            _ => 0,
        },
        // / or ~/ (a home path).
        match (s.get(i), s.get(i + 1)) {
            (Some(&a), _) if sep(a) => 1,
            (Some('~'), Some(&b)) if sep(b) => 2,
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

fn file_paths(line: &[char], c: &mut Collector) {
    let mut i = 0;
    while i < line.len() {
        let Some(end) = file_token(line, i) else {
            i += 1;
            continue;
        };
        let m = &line[i..end];
        let at = position_start(m);
        let mut path = &m[..at.unwrap_or(m.len())];
        // A sentence's full stop isn't the extension's.
        while path.last() == Some(&'.') {
            path = &path[..path.len() - 1];
        }
        let slash = path.iter().rposition(|&c| sep(c));
        let name = &path[slash.map_or(0, |s| s + 1)..];
        let ext = has_extension_tail(name);
        let ok = ext || at.is_some() || slash.is_some();
        if ok {
            let spec = if at.is_some_and(|a| a == path.len()) {
                m
            } else {
                path
            };
            c.add(i, text(spec), LinkKind::File);
        }
        i = end.max(i + 1);
    }
}

/// `.ext` at the end of a name: a dot, a letter, then word characters and dashes.
fn has_extension_tail(name: &[char]) -> bool {
    let Some(dot) = name.iter().rposition(|&c| c == '.') else {
        return false;
    };
    let ext = &name[dot + 1..];
    ext.first().is_some_and(|c| c.is_ascii_alphabetic())
        && ext.iter().all(|&c| is_word(c) || c == '-')
}

/// Where a position ends `spec` (POSITION$): the earliest start that reads through to the end.
fn position_start(s: &[char]) -> Option<usize> {
    (0..s.len()).find(|&i| {
        matches!(s[i], ':' | '(' | '#') && position_at(s, i).is_some_and(|(end, ..)| end == s.len())
    })
}

/// A path and the line / column its link names (`a.ts:12:5`, `a.md#L12`).
pub fn split_position(spec: &str) -> (String, Option<u32>, Option<u32>) {
    let s: Vec<char> = spec.chars().collect();
    match position_start(&s) {
        Some(at) => {
            let (_, line, col) = position_at(&s, at).unwrap_or((0, 0, None));
            (text(&s[..at]), Some(line), col)
        }
        None => (spec.to_string(), None, None),
    }
}

/// An extension at the end, maybe with a position after it.
fn has_extension(spec: &[char]) -> bool {
    let end = position_start(spec)
        .filter(|&a| spec[a] == ':')
        .unwrap_or(spec.len());
    let name_start = spec[..end]
        .iter()
        .rposition(|&c| sep(c))
        .map_or(0, |s| s + 1);
    has_extension_tail(&spec[name_start..end])
}

const MAX_SPACED_WORDS: usize = 6;

/// An absolute path that stops where its folder or file has a space (`~/Desktop/Ekran Resmi
/// 2026.png`, a macOS screenshot): the readings that carry on over the next words, up to one ending
/// in an extension, longest first. Only the disk says whether one is a file, so all are offered.
fn spaced_paths(line: &[char], link: &Link) -> Vec<Link> {
    let spec: Vec<char> = link.spec.chars().collect();
    let absolute =
        spec.first() == Some(&'/') || (spec.first() == Some(&'~') && spec.get(1) == Some(&'/'));
    if !absolute || has_extension(&spec) || spec.last() == Some(&'/') {
        return vec![];
    }
    let mut out = vec![];
    let mut end = link.end;
    for _ in 0..MAX_SPACED_WORDS {
        if line.get(end) != Some(&' ') {
            break;
        }
        let word_end = (end + 1..line.len())
            .find(|&j| line[j].is_whitespace())
            .unwrap_or(line.len());
        let word = &line[end + 1..word_end];
        // A word's path part; what follows it (a `]`, a comma) ends the path.
        let mut p = 0;
        while p < word.len() && (is_word(word[p]) || "@.+()\\/-".contains(word[p])) {
            p += 1;
        }
        if p == 0 {
            break;
        }
        if let Some((k, ..)) = position_at(word, p).filter(|_| word[p] == ':') {
            p = k;
        }
        end += 1 + p;
        if has_extension(&word[..p]) {
            out.push(Link {
                start: link.start,
                end,
                spec: text(&line[link.start..end]),
                kind: LinkKind::File,
                longer: vec![],
            });
        }
        if p < word.len() {
            break;
        }
    }
    out.reverse();
    out
}

/// Commits and issues a line may name, for the repo to confirm: a run of digits alone is a number
/// far more often than a short SHA, so a SHA needs a letter.
fn git_refs(line: &[char], c: &mut Collector) {
    let hex = |ch: char| ch.is_ascii_digit() || ('a'..='f').contains(&ch);
    let mut i = 0;
    while i < line.len() {
        // 7 to 40 hex digits on their own: not in a word, a path, a version, a hex number or a
        // color; either side of `..` or `...` they are.
        let after_dots = i >= 2 && line[i - 1] == '.' && line[i - 2] == '.';
        let free =
            i == 0 || !(is_word(line[i - 1]) || "./:#@-".contains(line[i - 1])) || after_dots;
        if free && hex(line[i]) {
            let mut j = i;
            while j < line.len() && hex(line[j]) && j - i <= 40 {
                j += 1;
            }
            let run = j - i;
            let clear = line
                .get(j)
                .is_none_or(|&ch| !(is_word(ch) || "/@-".contains(ch)));
            if (7..=40).contains(&run)
                && clear
                && line[i..j].iter().any(|ch| ch.is_ascii_alphabetic())
            {
                c.add(i, text(&line[i..j]), LinkKind::Commit);
            }
            i = j;
            continue;
        }
        i += 1;
    }
    for i in 0..line.len() {
        if line[i] != '#' {
            continue;
        }
        let before = i.checked_sub(1).map(|p| line[p]);
        if before.is_some_and(|b| b.is_alphanumeric() || "_@/.&#-".contains(b)) {
            continue;
        }
        let Some(&first) = line.get(i + 1) else {
            continue;
        };
        if !('1'..='9').contains(&first) {
            continue;
        }
        let mut j = i + 2;
        while j < line.len() && line[j].is_ascii_digit() {
            j += 1;
        }
        if j - i - 1 > 7
            || line
                .get(j)
                .is_some_and(|&ch| ch.is_alphanumeric() || ch == '_')
        {
            continue;
        }
        if !css_color(line, i, j - i) {
            c.add(i, text(&line[i..j]), LinkKind::Issue);
        }
    }
}

/// `#333` after a color property, as many digits as a hex color has: a stylesheet's diff.
fn css_color(line: &[char], at: usize, len: usize) -> bool {
    if ![4, 5, 7, 9].contains(&len) {
        return false;
    }
    let before = text(&line[at.saturating_sub(120)..at]).to_ascii_lowercase();
    let Some(colon) = before.rfind(':') else {
        return false;
    };
    if before[colon..].contains([';', '{', '}', '#']) {
        return false;
    }
    let head = before[..colon].trim_end();
    let word_start = head
        .rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_'))
        .map_or(0, |p| p + 1);
    let prop = &head[word_start..];
    [
        "color",
        "background",
        "border",
        "outline",
        "shadow",
        "fill",
        "stroke",
    ]
    .iter()
    .any(|p| prop.contains(p))
}

/// The pull request or issue a link opens in the app: #123, or a URL of one in `repos` (each
/// https://github.com/owner/name, in any case), a tab of it or a comment in it included.
pub fn github_item(link: &Link, repos: &[String]) -> Option<Target> {
    match link.kind {
        LinkKind::Issue => {
            let repo = repos.first()?;
            Some(Target::Item {
                number: link.spec[1..].parse().ok()?,
                pull: false,
                repo: repo.clone(),
            })
        }
        LinkKind::Url => {
            let rest = link.spec.strip_prefix("https://github.com/")?;
            let mut parts = rest.splitn(4, '/');
            let (owner, name, kind, tail) =
                (parts.next()?, parts.next()?, parts.next()?, parts.next()?);
            let pull = match kind.to_ascii_lowercase().as_str() {
                "pull" => true,
                "issues" => false,
                _ => return None,
            };
            let digits: String = tail.chars().take_while(|c| c.is_ascii_digit()).collect();
            let after = tail[digits.len()..].chars().next();
            if digits.is_empty()
                || digits.starts_with('0')
                || digits.len() > 9
                || after.is_some_and(|c| !"/?#".contains(c))
            {
                return None;
            }
            let base = format!("https://github.com/{owner}/{name}");
            let repo = repos.iter().find(|r| r.eq_ignore_ascii_case(&base))?;
            Some(Target::Item {
                number: digits.parse().ok()?,
                pull,
                repo: repo.clone(),
            })
        }
        _ => None,
    }
}

/// A row of terminal cells: each cell's text (a char and its combining marks; "" for the second
/// half of a wide character, whose width is 0), and whether the terminal wrapped it onto the next.
#[derive(Clone, Debug, Default)]
pub struct CellRow {
    pub cells: Vec<(String, u8)>,
    pub wraps: bool,
}

impl CellRow {
    #[cfg(test)]
    pub fn of(s: &str, cols: usize) -> Self {
        let mut cells: Vec<(String, u8)> = s.chars().map(|c| (c.to_string(), 1)).collect();
        cells.resize(cols, (" ".into(), 1));
        CellRow {
            cells,
            wraps: false,
        }
    }
}

fn is_path_tail_char(c: char) -> bool {
    path_char(c) || ":#~".contains(c)
}

const INDENT_MAX: usize = 8;

/// Whether `next` carries `prev`'s path on: a program that wraps its own output (Claude Code) cuts
/// a long path at the last column and goes on a row down, indented under its text, with no soft
/// wrap for the terminal to know. The indent to leave out, or None. Only a row filled to the edge
/// and ending in part of a path counts; whether the joined text is a file is the resolver's to say.
pub fn hard_wrap_indent(prev: &CellRow, next: &CellRow) -> Option<usize> {
    let last = prev.cells.last()?;
    if last.1 > 0 && last.0.trim().is_empty() {
        return None;
    }
    let mut tail: Vec<char> = vec![];
    for (text, width) in prev.cells.iter().rev() {
        if tail.len() >= 512 {
            break;
        }
        if *width == 0 {
            continue;
        }
        if text.trim().is_empty() {
            break;
        }
        let mut cs: Vec<char> = text.chars().collect();
        cs.extend(tail);
        tail = cs;
    }
    // A separator, then path characters to the end.
    let s = tail.iter().rposition(|&c| sep(c))?;
    if !tail[s + 1..].iter().all(|&c| is_path_tail_char(c)) {
        return None;
    }
    let mut indent = 0;
    let mut head = String::new();
    for (text, width) in &next.cells {
        if head.len() >= 512 {
            break;
        }
        if *width == 0 {
            continue;
        }
        if text.trim().is_empty() {
            if !head.is_empty() {
                break;
            }
            indent += 1;
            continue;
        }
        head.push_str(text);
    }
    let first = head.chars().next()?;
    let starts = is_word(first) || "@.+~/\\".contains(first);
    (indent <= INDENT_MAX && starts).then_some(indent)
}

/// Rows of cells as one line, and each character's cell (along the rows, `cols` to a row) then the
/// one past the last; `starts[r]` is where row `r` starts in the text. `skip[r]`: a continuation's
/// indent, which isn't part of the text it carries on.
pub fn cell_text(
    rows: &[CellRow],
    cols: usize,
    skip: &[usize],
) -> (Vec<char>, Vec<usize>, Vec<usize>) {
    let mut text = vec![];
    let mut cells = vec![];
    let mut starts = vec![];
    let mut end = 0;
    for (r, row) in rows.iter().enumerate() {
        starts.push(text.len());
        for x in skip.get(r).copied().unwrap_or(0)..cols.min(row.cells.len()) {
            let (chars, width) = &row.cells[x];
            if *width == 0 {
                continue;
            }
            let chars = if chars.is_empty() {
                " "
            } else {
                chars.as_str()
            };
            for ch in chars.chars() {
                text.push(ch);
                cells.push(r * cols + x);
            }
            end = r * cols + x + *width as usize;
        }
    }
    starts.push(text.len());
    cells.push(end);
    (text, cells, starts)
}

/// The repo's files, and their folders once a terminal link asks for one.
#[derive(Default)]
pub struct FileIndex {
    pub files: HashSet<String>,
    folders: std::sync::OnceLock<HashSet<String>>,
}

impl FileIndex {
    pub fn new(files: impl IntoIterator<Item = String>) -> Self {
        FileIndex {
            files: files.into_iter().collect(),
            folders: Default::default(),
        }
    }

    pub fn folders(&self) -> &HashSet<String> {
        self.folders.get_or_init(|| {
            let mut dirs = HashSet::new();
            for f in &self.files {
                let mut d = dirname(f);
                while !d.is_empty() && dirs.insert(d.to_string()) {
                    d = dirname(d);
                }
            }
            dirs
        })
    }

    /// The one file whose path ends in `tail`; None for none, or more than one.
    fn ending_with(&self, tail: &str) -> Option<String> {
        let mut hit = None;
        for f in &self.files {
            if f == tail || f.ends_with(&format!("/{tail}")) {
                if hit.is_some() {
                    return None;
                }
                hit = Some(f.clone());
            }
        }
        hit
    }
}

fn dirname(p: &str) -> &str {
    p.rfind('/').map_or("", |i| &p[..i])
}

pub fn slashes(p: &str) -> String {
    p.replace('\\', "/")
}

/// `rel` from folder `dir`, normalized; None when it climbs out of the repo.
pub fn join(dir: &str, rel: &str) -> Option<String> {
    let mut parts: Vec<&str> = if dir.is_empty() {
        vec![]
    } else {
        dir.split('/').collect()
    };
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

fn shortened(path: &str) -> Option<&str> {
    path.strip_prefix("…/")
        .or_else(|| path.strip_prefix(".../"))
}

/// The repo paths a path may be from folder `dir` (None: outside the repo), then from the root
/// unless `root_too` is false (a shell in a worktree inside this one: the root is another checkout).
fn candidates(path: &str, dir: Option<&str>, root: &str, root_too: bool) -> Vec<String> {
    let base = slashes(root);
    let drive = path.len() > 2
        && path.as_bytes()[1] == b':'
        && path.as_bytes()[0].is_ascii_alphabetic()
        && path.as_bytes()[2] == b'/';
    if path.starts_with('/') || drive {
        return path
            .strip_prefix(&format!("{base}/"))
            .filter(|_| !base.is_empty())
            .map(|rel| rel.trim_end_matches('/').to_string())
            .filter(|r| !r.is_empty())
            .into_iter()
            .collect();
    }
    let Some(dir) = dir else { return vec![] };
    let mut out: Vec<String> = join(&slashes(dir), path).into_iter().collect();
    if root_too && !dir.is_empty() {
        out.extend(join("", path));
    }
    out.retain(|c| !c.is_empty());
    out
}

/// A terminal link's target in the repo: paths from the shell's folder `dir` (repo-relative; None
/// when it's outside the repo, where only absolute paths into it resolve) or the root; a folder
/// when no line is named, and a shortened path when one file ends that way.
pub fn resolve_terminal_link(
    link: &Link,
    dir: Option<&str>,
    index: &FileIndex,
    root: &str,
    root_too: bool,
) -> Option<Target> {
    match link.kind {
        LinkKind::Url => return Some(Target::Url(link.spec.clone())),
        LinkKind::Commit | LinkKind::Issue => return None,
        LinkKind::File => {}
    }
    let (raw, line, column) = split_position(&link.spec);
    let path = slashes(&raw);
    if let Some(tail) = shortened(&path) {
        let found = if root_too {
            index.ending_with(tail)
        } else {
            None
        }?;
        return Some(Target::Path {
            path: found,
            line,
            column,
            dir: false,
        });
    }
    let paths = candidates(&path, dir, root, root_too);
    if let Some(found) = paths.iter().find(|p| index.files.contains(*p)) {
        return Some(Target::Path {
            path: found.clone(),
            line,
            column,
            dir: false,
        });
    }
    if line.is_some() {
        return None;
    }
    let folders = index.folders();
    let folder = paths.iter().find(|p| folders.contains(*p))?;
    Some(Target::Path {
        path: folder.clone(),
        line: None,
        column: None,
        dir: true,
    })
}

/// For a terminal link the index doesn't have (an ignored file or folder): the repo paths it may
/// be, to ask the disk about, nearest first. None for a URL or a shortened path.
pub fn disk_candidates(link: &Link, dir: Option<&str>, root: &str, root_too: bool) -> Vec<String> {
    let path = slashes(&split_position(&link.spec).0);
    if link.kind != LinkKind::File || shortened(&path).is_some() {
        return vec![];
    }
    // Digits and slashes are a date or a fraction (2026/09/27, 3/4).
    candidates(&path, dir, root, root_too)
        .into_iter()
        .filter(|c| !c.chars().all(|ch| ch.is_ascii_digit() || ch == '/'))
        .collect()
}

/// A terminal link's target at `path`, found on disk as a file or a folder; a folder with a line
/// isn't one.
pub fn disk_target(spec: &str, path: &str, dir: bool) -> Option<Target> {
    let (_, line, column) = split_position(spec);
    if dir && line.is_some() {
        return None;
    }
    Some(Target::Path {
        path: path.to_string(),
        line: if dir { None } else { line },
        column: if dir { None } else { column },
        dir,
    })
}

/// Whether `spec` is an absolute or home path elsewhere on disk worth asking about.
pub fn outside_candidate(link: &Link) -> Option<String> {
    if link.kind != LinkKind::File {
        return None;
    }
    let (path, ..) = split_position(&link.spec);
    let absolute = path.starts_with('/')
        || path.starts_with("~/")
        || path == "~"
        || (path.len() > 2 && path.as_bytes()[1] == b':');
    (absolute && !path.chars().all(|c| c.is_ascii_digit() || c == '/')).then_some(path)
}

/// Where an OSC 8 hyperlink goes: an http(s) page, or a file:// path in `root` by its repo-relative
/// path ("" for `root` itself), at `:12` or `#L12` when it names a line. Other schemes, other
/// machines' files, and files outside the repo or in .git go nowhere.
pub fn hyperlink_target(uri: &str, root: &str) -> Option<Target> {
    let lower = uri.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return Some(Target::Url(uri.to_string()));
    }
    let rest = uri.strip_prefix("file://")?;
    if root.is_empty() {
        return None;
    }
    let slash = rest.find('/')?;
    let host = &rest[..slash];
    // ls --hyperlink, eza, fd and rg name this machine.
    if !host.is_empty() && host != "localhost" && !crate::scan::host_is_this_machine(host) {
        return None;
    }
    let (path_part, hash) = match rest[slash..].find('#') {
        Some(h) => (&rest[slash..slash + h], &rest[slash + h..]),
        None => (&rest[slash..], ""),
    };
    let mut parts = vec![];
    for p in path_part.split('/') {
        let decoded = percent_decode(p)?;
        if decoded == "." || decoded == ".." || decoded.contains(['/', '\\']) {
            return None;
        }
        parts.push(decoded);
    }
    let hash_chars: Vec<char> = hash.chars().collect();
    let fragment = if !hash_chars.is_empty()
        && position_at(&hash_chars, 0).is_some_and(|(e, ..)| e == hash_chars.len())
    {
        hash
    } else {
        ""
    };
    let mut joined = parts.join("/");
    // Windows' /C:/repo is C:/repo.
    if joined.len() > 3 && joined.as_bytes()[0] == b'/' && joined.as_bytes()[2] == b':' {
        joined.remove(0);
    }
    let (path, line, column) = split_position(&format!("{joined}{fragment}"));
    let file = if path.len() > 1 {
        path.trim_end_matches('/').to_string()
    } else {
        path
    };
    let base = slashes(root);
    if file == base {
        return Some(Target::Path {
            path: String::new(),
            line: None,
            column: None,
            dir: true,
        });
    }
    let rel = file.strip_prefix(&format!("{base}/"))?;
    if rel.split('/').any(|p| p.eq_ignore_ascii_case(".git")) {
        return None;
    }
    Some(Target::Path {
        path: rel.to_string(),
        line,
        column,
        dir: false,
    })
}

fn percent_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            out.push(u8::from_str_radix(s.get(i + 1..i + 3)?, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// A page on this machine (a dev server), which opens in a browser tab. The host as a browser
/// reads it: `http://evil.example\@localhost/` is evil.example's.
pub fn is_local_url(url: &str) -> bool {
    let Some(host) = url::Url::parse(url)
        .ok()
        .filter(|u| matches!(u.scheme(), "http" | "https"))
        .and_then(|u| u.host_str().map(str::to_ascii_lowercase))
    else {
        return false;
    };
    matches!(
        host.as_str(),
        "localhost" | "127.0.0.1" | "0.0.0.0" | "[::1]"
    ) || host.ends_with(".localhost")
}

/// The folders a line names (a path ending in `/`, or a file path's folder), nearest first: a
/// program lists files by bare name under a line that says where they are.
pub fn named_dirs(links: &[Link], out: &mut Vec<String>) {
    for l in links.iter().rev() {
        if l.kind != LinkKind::File {
            continue;
        }
        let (path, ..) = split_position(&l.spec);
        let dir = if let Some(d) = path.strip_suffix('/') {
            Some(d.to_string())
        } else if path.contains('/') {
            Some(dirname(&path).to_string())
        } else {
            None
        };
        if let Some(d) = dir.filter(|d| !d.is_empty() && !out.contains(d))
            && out.len() < 4
        {
            out.push(d);
        }
    }
}

/// A terminal's rows, by grid line (0 the screen's top, negative into the history).
pub trait RowSource {
    fn cols(&self) -> usize;
    /// The history's first line.
    fn top(&self) -> i32;
    fn row(&mut self, line: i32) -> Option<std::rc::Rc<CellRow>>;
}

/// The line of output a row is in: a long one wraps over several rows, read around it as far as
/// links are looked for.
#[derive(Debug, Default)]
pub struct LogicalLine {
    pub first: i32,
    pub last: i32,
    pub text: Vec<char>,
    /// Each char's cell, along the rows (`cols` to a row), then the one past the last.
    pub cells: Vec<usize>,
    /// Where each row starts in `text`, then its end.
    pub starts: Vec<usize>,
    cols: usize,
    cut_before: bool,
    cut_after: bool,
}

impl LogicalLine {
    /// A link's first and last cell, as (line, column).
    pub fn range(&self, l: &Link) -> ((i32, usize), (i32, usize)) {
        let cols = self.cols.max(1);
        let at = |c: usize| (self.first + (c / cols) as i32, c % cols);
        let end = self.cells.get(l.end).copied().unwrap_or(0).max(1) - 1;
        (at(self.cells.get(l.start).copied().unwrap_or(0)), at(end))
    }

    /// No row cut off either side may carry the link on past what was read.
    pub fn whole(&self, l: &Link) -> bool {
        (!self.cut_before || l.start > 0) && (!self.cut_after || l.end < self.text.len())
    }
}

/// How `line` goes on from the one above: 0 when the terminal wrapped it, the indent to leave out
/// when a program cut a path at the edge (`hard_wrap_indent`), None when it doesn't.
fn joined(rows: &mut dyn RowSource, line: i32) -> Option<usize> {
    if line <= rows.top() {
        return None;
    }
    let (prev, row) = (rows.row(line - 1)?, rows.row(line)?);
    if prev.wraps {
        Some(0)
    } else {
        hard_wrap_indent(&prev, &row)
    }
}

pub fn logical_line(rows: &mut dyn RowSource, y: i32) -> LogicalLine {
    let cols = rows.cols();
    let reach = (LINK_WINDOW / cols.max(1)) as i32 + 2;
    let mut first = y;
    while first > y - reach && joined(rows, first).is_some() {
        first -= 1;
    }
    let mut last = y;
    while last < y + reach && joined(rows, last + 1).is_some() {
        last += 1;
    }
    let (cut_before, cut_after) = (
        joined(rows, first).is_some(),
        joined(rows, last + 1).is_some(),
    );
    let mut read = vec![];
    let mut skip = vec![];
    for line in first..=last {
        read.push(rows.row(line).map(|r| (*r).clone()).unwrap_or_default());
        skip.push(if line > first {
            joined(rows, line).unwrap_or(0)
        } else {
            0
        });
    }
    let (text, cells, starts) = cell_text(&read, cols, &skip);
    LogicalLine {
        first,
        last,
        text,
        cells,
        starts,
        cols,
        cut_before,
        cut_after,
    }
}

/// How far up the output a bare name looks for its folder.
pub const CONTEXT_LINES: usize = 40;

/// The folders a line names, as `named_dirs` puts them.
pub fn line_dirs(text: &[char]) -> Vec<String> {
    let mut out = vec![];
    named_dirs(&find_terminal_links(text, None), &mut out);
    out
}

/// What `context_dirs` says from the lines above's own folders (`line_dirs`), oldest first:
/// one pass down a screen keeps the last CONTEXT_LINES instead of reading them again per line.
pub fn dirs_above(lines: &std::collections::VecDeque<Vec<String>>) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for d in lines.iter().rev().take(CONTEXT_LINES).flatten() {
        if out.len() >= 4 {
            break;
        }
        if !out.contains(d) {
            out.push(d.clone());
        }
    }
    out
}

/// The folders the output above line `y`'s names, nearest first (`named_dirs`).
pub fn context_dirs(rows: &mut dyn RowSource, y: i32) -> Vec<String> {
    let mut dirs = vec![];
    let mut r = logical_line(rows, y).first - 1;
    let mut seen = 0;
    while r >= rows.top() && seen < CONTEXT_LINES && dirs.len() < 4 {
        let line = logical_line(rows, r);
        named_dirs(&find_terminal_links(&line.text, None), &mut dirs);
        r = line.first - 1;
        seen += 1;
    }
    dirs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chars(s: &str) -> Vec<char> {
        s.chars().collect()
    }
    fn term(line: &str) -> Vec<(String, LinkKind)> {
        let c = chars(line);
        find_terminal_links(&c, None)
            .into_iter()
            .map(|l| (text(&c[l.start..l.end]), l.kind))
            .collect()
    }
    fn specs(line: &str) -> Vec<String> {
        find_terminal_links(&chars(line), None)
            .into_iter()
            .map(|l| l.spec)
            .collect()
    }
    fn refs(line: &str) -> Vec<String> {
        find_terminal_links(&chars(line), None)
            .into_iter()
            .filter(|l| matches!(l.kind, LinkKind::Commit | LinkKind::Issue))
            .map(|l| l.spec)
            .collect()
    }
    fn index() -> FileIndex {
        FileIndex::new(
            [
                "src/lib/api.ts",
                "src/lib/utils.ts",
                "src/lib/editor/index.tsx",
                "README.md",
                "docs/guide.md",
                "src-tauri/src/git.rs",
                "package.json",
            ]
            .map(String::from),
        )
    }
    fn link(spec: &str) -> Link {
        let kind = if spec.starts_with("http") {
            LinkKind::Url
        } else {
            LinkKind::File
        };
        Link {
            start: 0,
            end: spec.chars().count(),
            spec: spec.into(),
            kind,
            longer: vec![],
        }
    }
    fn path(p: &str, line: Option<u32>, column: Option<u32>) -> Option<Target> {
        Some(Target::Path {
            path: p.into(),
            line,
            column,
            dir: false,
        })
    }
    fn dir(p: &str) -> Option<Target> {
        Some(Target::Path {
            path: p.into(),
            line: None,
            column: None,
            dir: true,
        })
    }
    use LinkKind::*;

    #[test]
    fn urls_without_the_punctuation_around_them() {
        assert_eq!(
            specs("see https://github.com/a/b/issues/40."),
            ["https://github.com/a/b/issues/40"]
        );
        assert_eq!(
            specs("(docs: https://en.wikipedia.org/wiki/Tree_(graph_theory))"),
            ["https://en.wikipedia.org/wiki/Tree_(graph_theory)"]
        );
        assert_eq!(
            specs("[GitHub CLI](https://cli.github.com), ok"),
            ["https://cli.github.com"]
        );
        assert_eq!(specs("(https://x.com/a_(b))."), ["https://x.com/a_(b)"]);
    }

    #[test]
    fn positions() {
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
        assert_eq!(specs("src/a.ts(12,5): error TS2304"), ["src/a.ts(12,5)"]);
    }

    #[test]
    fn terminal_output_urls_paths_bare_names() {
        assert_eq!(
            term("  ➜  Local:   http://localhost:1420/"),
            [("http://localhost:1420/".into(), Url)]
        );
        assert_eq!(
            term("\tmodified:   src/lib/api.ts"),
            [("src/lib/api.ts".into(), File)]
        );
        assert_eq!(
            term("src/lib/api.ts:42:7: error: nope"),
            [("src/lib/api.ts:42:7".into(), File)]
        );
        assert_eq!(
            term("README.md  package.json  src"),
            [("README.md".into(), File), ("package.json".into(), File)]
        );
        assert_eq!(
            term("  --> src-tauri/src/git.rs:1465:5"),
            [("src-tauri/src/git.rs:1465:5".into(), File)]
        );
        assert_eq!(term("v1.2.3 released, 3.5s"), []);
        let idx = index();
        let open = |spec: &str, cwd: Option<&str>| {
            resolve_terminal_link(&link(spec), cwd, &idx, "/repo", true)
        };
        assert_eq!(
            open("api.ts:4", Some("src/lib")),
            path("src/lib/api.ts", Some(4), None)
        );
        assert_eq!(
            open("src/lib/api.ts", Some("src-tauri")),
            path("src/lib/api.ts", None, None)
        );
        assert_eq!(
            open("/repo/docs/guide.md", None),
            path("docs/guide.md", None, None)
        );
        assert_eq!(
            open("src/lib/api.ts", None),
            None,
            "a shell outside the repo"
        );
        assert_eq!(
            open("package.json", Some("")),
            path("package.json", None, None)
        );
        assert_eq!(
            open("https://x.com/a", None),
            Some(Target::Url("https://x.com/a".into()))
        );
    }

    #[test]
    fn windows_paths() {
        assert_eq!(
            specs("C:\\repo\\src\\lib\\api.ts:4:2 failed"),
            ["C:\\repo\\src\\lib\\api.ts:4:2"]
        );
        let idx = index();
        let open = |spec: &str, cwd: Option<&str>| {
            resolve_terminal_link(&link(spec), cwd, &idx, "C:/repo", true)
        };
        assert_eq!(
            open("C:\\repo\\src\\lib\\api.ts:4:2", None),
            path("src/lib/api.ts", Some(4), Some(2))
        );
        assert_eq!(
            open("lib\\api.ts", Some("src")),
            path("src/lib/api.ts", None, None)
        );
        assert_eq!(open("D:\\other\\api.ts", Some("src")), None);
    }

    #[test]
    fn what_may_be_a_commit_or_an_issue() {
        assert_eq!(
            term("[main 9a1c2e4] fix the thing"),
            [("9a1c2e4".into(), Commit)]
        );
        assert_eq!(
            term("   1f3a9c0..7bd4e21  main -> main"),
            [("1f3a9c0".into(), Commit), ("7bd4e21".into(), Commit)]
        );
        assert_eq!(
            term("git diff 1f3a9c0...7bd4e21"),
            [("1f3a9c0".into(), Commit), ("7bd4e21".into(), Commit)]
        );
        let full = "0123456789abcdef0123456789abcdef01234567";
        assert_eq!(
            term(&format!("commit {full} (HEAD)")),
            [(full.into(), Commit)]
        );
        assert_eq!(
            term("^9a1c2e4 (Ada 2026-01-01) x"),
            [("9a1c2e4".into(), Commit)]
        );
        assert_eq!(
            term("see 9a1c2e4^ and 9a1c2e4~2."),
            [("9a1c2e4".into(), Commit), ("9a1c2e4".into(), Commit)]
        );
        assert_eq!(term("pid 1234567 took 20260927 ms"), []);
        assert_eq!(
            refs(&format!(
                "0x1a2b3c4d #ff00aa v1.2.3abcdef1 img-9a1c2e4 sha256:9a1c2e4f a/9a1c2e4f abc123 {}",
                "a".repeat(41)
            )),
            Vec::<String>::new()
        );
        assert_eq!(term("9A1C2E4"), []);
        assert_eq!(
            refs("Merge pull request #42 from ada/fix (#7)"),
            ["#42", "#7"]
        );
        assert_eq!(refs("&#123; a#1 #0 #007 ##5 #12abc"), Vec::<String>::new());
        assert_eq!(
            term("https://github.com/o/r/commit/9a1c2e4f#3"),
            [("https://github.com/o/r/commit/9a1c2e4f#3".into(), Url)]
        );
    }

    #[test]
    fn hex_that_isnt_a_commit() {
        for line in [
            "550e8400-e29b-41d4-a716-446655440000",
            "{550e8400-e29b-41d4-a716-446655440000}",
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  file.txt",
            "inet6 fe80::1c2a:3bff:fe4d:5e6f%en0 prefixlen 64",
            "2001:0db8:85a3:0000:0000:8a2e:0370:7334",
            "ether a4:83:e7:12:34:56",
            "0xdeadbeef 0Xcafebabe 0x9a1c2e4f",
            "PID 48213 started 20261002 took 1234567ms",
            "Version 1.2.3-beta.abcdef12",
            "#a1b2c3d4 #ffffff",
            "9A1C2E4F DEADBEEF",
        ] {
            assert_eq!(refs(line), Vec::<String>::new(), "{line}");
        }
        assert_eq!(
            refs("d41d8cd98f00b204e9800998ecf8427e  empty"),
            ["d41d8cd98f00b204e9800998ecf8427e"]
        );
        assert_eq!(refs("pick 9a1c2e4 fix the thing"), ["9a1c2e4"]);
        assert_eq!(refs("Updating 1f3a9c0..7bd4e21"), ["1f3a9c0", "7bd4e21"]);
        assert_eq!(
            refs("+ 1f3a9c0...7bd4e21 main -> main (forced update)"),
            ["1f3a9c0", "7bd4e21"]
        );
        assert_eq!(refs("* 9a1c2e4 - fix (2 days ago)"), ["9a1c2e4"]);
    }

    #[test]
    fn issues_next_to_markdown_and_code() {
        assert_eq!(
            refs("See #12, (#13), [#14](url), `#15`, issue#16, #17."),
            ["#12", "#13", "#14", "#15", "#17"]
        );
        assert_eq!(
            refs("**#18** ~#20~ \"#21\" '#22' <#23>"),
            ["#18", "#20", "#21", "#22", "#23"]
        );
        assert_eq!(
            refs("https://github.com/o/r/pull/7#issuecomment-123 and #8"),
            ["#8"]
        );
        assert_eq!(
            refs("http://localhost:3000/#12 file.html#3 a.md#L12"),
            Vec::<String>::new()
        );
        assert_eq!(
            refs("fixes o/r#99, gh-12, &#123; &#x7b; C#7 F#2"),
            Vec::<String>::new()
        );
        assert_eq!(refs("#!/bin/sh # 1 ## 2 #0 #01"), Vec::<String>::new());
        assert_eq!(refs("+  color: #333;"), Vec::<String>::new());
        assert_eq!(refs("   background: #666666;"), Vec::<String>::new());
        assert_eq!(refs("border: 1px solid #111"), Vec::<String>::new());
    }

    #[test]
    fn a_repos_pull_requests_and_issues_open_in_the_app() {
        let repos = vec![
            "https://github.com/Ada/Repo".to_string(),
            "https://github.com/up/repo".to_string(),
        ];
        let item = |spec: &str| {
            let kind = if spec.starts_with('#') { Issue } else { Url };
            github_item(
                &Link {
                    start: 0,
                    end: 0,
                    spec: spec.into(),
                    kind,
                    longer: vec![],
                },
                &repos,
            )
        };
        let it = |n, pull, r: usize| {
            Some(Target::Item {
                number: n,
                pull,
                repo: repos[r].clone(),
            })
        };
        assert_eq!(item("#42"), it(42, false, 0));
        assert_eq!(item("https://github.com/ada/repo/pull/7"), it(7, true, 0));
        assert_eq!(
            item("https://github.com/ada/repo/pull/7/files#diff-1"),
            it(7, true, 0)
        );
        assert_eq!(
            item("https://github.com/ada/repo/issues/9#issuecomment-1"),
            it(9, false, 0)
        );
        assert_eq!(
            item("https://github.com/up/repo/pull/3?w=1"),
            it(3, true, 1)
        );
        assert_eq!(
            item("https://github.com/ADA/Repo/issues/3"),
            it(3, false, 0)
        );
        for bad in [
            "https://github.com/other/repo/pull/7",
            "https://github.com/ada/repo/pulls",
            "https://github.com/ada/repo/pull/7x",
            "https://github.com/ada/repo-two/pull/7",
            "https://github.com/ada/repo/pull/0",
            "https://github.com/ada/repo/pull/99999999999",
            "https://github.com.evil.example/ada/repo/pull/3",
            "http://github.com/ada/repo/pull/3",
            "https://github.com/ada/repo/discussions/3",
        ] {
            assert_eq!(item(bad), None, "{bad}");
        }
        assert_eq!(
            github_item(
                &Link {
                    start: 0,
                    end: 0,
                    spec: "#3".into(),
                    kind: Issue,
                    longer: vec![]
                },
                &[]
            ),
            None
        );
    }

    #[test]
    fn folders_shortened_paths_and_what_the_disk_is_asked_about() {
        assert_eq!(
            specs("see src/features/ and …/lib/api.ts:4, .../editor/index.tsx"),
            ["src/features/", "…/lib/api.ts:4", ".../editor/index.tsx"]
        );
        let idx = index();
        let open = |spec: &str, cwd: Option<&str>| {
            resolve_terminal_link(&link(spec), cwd, &idx, "/repo", true)
        };
        assert_eq!(open("src/lib", Some("")), dir("src/lib"));
        assert_eq!(open("lib/editor", Some("src")), dir("src/lib/editor"));
        assert_eq!(open("/repo/src-tauri/src", Some("")), dir("src-tauri/src"));
        assert_eq!(open("src/lib:12", Some("")), None, "a folder has no lines");
        assert_eq!(
            open("…/lib/api.ts:4", Some("")),
            path("src/lib/api.ts", Some(4), None)
        );
        assert_eq!(
            open(".../editor/index.tsx", None),
            path("src/lib/editor/index.tsx", None, None)
        );
        assert_eq!(open("/repo/src/lib/", Some("")), dir("src/lib"));
        assert_eq!(open("…/utils", Some("")), None);
        let twice = FileIndex::new(["a/x/b.ts", "c/x/b.ts"].map(String::from));
        assert_eq!(
            resolve_terminal_link(&link("…/x/b.ts"), Some(""), &twice, "/repo", true),
            None
        );
        assert_eq!(
            resolve_terminal_link(&link("…/a/x/b.ts"), Some(""), &twice, "/repo", true),
            path("a/x/b.ts", None, None)
        );
        assert_eq!(
            disk_candidates(&link("dist/index.js:3"), Some("src"), "/repo", true),
            ["src/dist/index.js", "dist/index.js"]
        );
        assert_eq!(
            disk_candidates(&link("/repo/target/debug"), None, "/repo", true),
            ["target/debug"]
        );
        assert!(disk_candidates(&link("/tmp/x.png"), Some(""), "/repo", true).is_empty());
        assert!(disk_candidates(&link("…/a.ts"), Some(""), "/repo", true).is_empty());
        assert_eq!(
            disk_candidates(&link("dist/index.js"), Some(""), "/repo", true),
            ["dist/index.js"]
        );
        assert!(disk_candidates(&link("2026/09/27"), Some(""), "/repo", true).is_empty());
        let wt = ".claude/worktrees/x";
        assert_eq!(
            resolve_terminal_link(&link("src/lib/api.ts"), Some(wt), &idx, "/repo", false),
            None
        );
        assert_eq!(
            resolve_terminal_link(&link("…/lib/api.ts"), Some(wt), &idx, "/repo", false),
            None
        );
        assert_eq!(
            disk_candidates(&link("src/lib/api.ts"), Some(wt), "/repo", false),
            [format!("{wt}/src/lib/api.ts")]
        );
        assert_eq!(
            resolve_terminal_link(
                &link("/repo/src/lib/api.ts"),
                Some(wt),
                &idx,
                "/repo",
                false
            ),
            path("src/lib/api.ts", None, None)
        );
        assert_eq!(
            disk_target("dist/index.js:3", "dist/index.js", false),
            path("dist/index.js", Some(3), None)
        );
        assert_eq!(
            disk_target("target/debug", "target/debug", true),
            dir("target/debug")
        );
        assert_eq!(disk_target("target:3", "target", true), None);
    }

    #[test]
    fn maps_terminal_text_back_to_cells_past_wide_characters() {
        let row = |cells: Vec<(&str, u8)>, cols: usize| {
            let mut cells: Vec<(String, u8)> =
                cells.into_iter().map(|(s, w)| (s.to_string(), w)).collect();
            cells.resize(cols, (" ".into(), 1));
            CellRow {
                cells,
                wraps: false,
            }
        };
        let rows = [
            row(
                vec![
                    ("✅", 2),
                    ("", 0),
                    (" ", 1),
                    ("a", 1),
                    (".", 1),
                    ("t", 1),
                    ("s", 1),
                ],
                8,
            ),
            row(vec![("🔥", 2), ("", 0), ("b", 1)], 8),
        ];
        let (text_, cells, starts) = cell_text(&rows, 8, &[]);
        assert_eq!(text(&text_), "✅ a.ts 🔥b     ");
        assert_eq!(starts[1], 7);
        let l = find_terminal_links(&text_[..starts[1]], None).remove(0);
        assert_eq!(l.spec, "a.ts");
        assert_eq!((cells[l.start], cells[l.end] - 1), (3, 6));
        assert_eq!(cells[7..9], [8, 10]);
        assert_eq!(*cells.last().unwrap(), 16);
    }

    #[test]
    fn an_agents_long_scratch_path_is_one_link_outside_the_repo() {
        let p = "/private/tmp/claude-501/-Users-me-Documents-GitHub-online-pvp-game/a4a26d65-9872-4ab4/scratchpad/onb-c/s2.png";
        let l = find_terminal_links(&chars(&format!("- 852 genişlik: {p}")), None).remove(0);
        assert_eq!(l.spec, p);
        let idx = FileIndex::new(["src/a.ts".to_string()]);
        assert_eq!(
            resolve_terminal_link(&l, Some(""), &idx, "/Users/me/repo", true),
            None
        );
        assert!(disk_candidates(&l, Some(""), "/Users/me/repo", true).is_empty());
        assert_eq!(outside_candidate(&l).as_deref(), Some(p));
    }

    #[test]
    fn an_absolute_path_with_spaces_is_offered_in_its_longer_readings() {
        let line =
            "[Image: source: /Users/me/Desktop/Ekran Resmi 2026-10-15 18.34.46.png] and more.txt";
        let found = find_terminal_links(&chars(line), None);
        let mut files = found.iter().filter(|l| l.start >= 16 && l.kind == File);
        let first = files.next().unwrap();
        assert_eq!(first.spec, "/Users/me/Desktop/Ekran");
        assert_eq!(
            first
                .longer
                .iter()
                .map(|l| l.spec.as_str())
                .collect::<Vec<_>>(),
            ["/Users/me/Desktop/Ekran Resmi 2026-10-15 18.34.46.png"]
        );
        assert!(files.any(|l| l.spec == "18.34.46.png"));
        let folder = find_terminal_links(&chars("/a/My Folder/b.md:12 next words.txt"), None)
            .into_iter()
            .find(|l| l.start == 0)
            .unwrap();
        assert_eq!(
            folder
                .longer
                .iter()
                .map(|l| l.spec.as_str())
                .collect::<Vec<_>>(),
            [
                "/a/My Folder/b.md:12 next words.txt",
                "/a/My Folder/b.md:12"
            ]
        );
        for plain in [
            "/a/b.png and c.txt",
            "/a/b/ and c.txt",
            "src/a and c.txt",
            "/a/b, c.txt",
        ] {
            assert!(
                find_terminal_links(&chars(plain), None)[0]
                    .longer
                    .is_empty(),
                "{plain}"
            );
        }
        assert_eq!(specs("~/notes.md"), ["~/notes.md"]);
    }

    #[test]
    fn a_path_a_program_cut_at_the_edge_and_went_on_indented_is_one_link() {
        let cols = 20;
        let (a, b) = (
            CellRow::of("see /tmp/aaaa/bbbb/c", cols),
            CellRow::of("  dd/e.png and more", cols),
        );
        assert_eq!(hard_wrap_indent(&a, &b), Some(2));
        assert_eq!(
            hard_wrap_indent(&a, &CellRow::of("dd/e.png", cols)),
            Some(0)
        );
        let (t, cells, starts) = cell_text(&[a, b], cols, &[0, 2]);
        assert_eq!(text(&t[..starts[1] + 8]), "see /tmp/aaaa/bbbb/cdd/e.png");
        let l = find_terminal_links(&t, None).remove(0);
        assert_eq!(l.spec, "/tmp/aaaa/bbbb/cdd/e.png");
        assert_eq!((cells[l.start], cells[l.end] - 1), (4, 29));
        let full = CellRow::of("see /tmp/aaaa/bbbb/c", cols);
        assert_eq!(
            hard_wrap_indent(
                &CellRow::of("see /tmp/aaaa/bbbb/", cols),
                &CellRow::of("dd/e.png", cols)
            ),
            None,
            "not at the edge"
        );
        assert_eq!(
            hard_wrap_indent(
                &CellRow::of("hello world and more", cols),
                &CellRow::of("dd/e.png", cols)
            ),
            None,
            "no path"
        );
        assert_eq!(
            hard_wrap_indent(&full, &CellRow::of("- next item", cols)),
            None,
            "a bullet"
        );
        assert_eq!(
            hard_wrap_indent(&full, &CellRow::of("      ", cols)),
            None,
            "blank"
        );
        assert_eq!(
            hard_wrap_indent(&full, &CellRow::of("          dd/e.png", cols)),
            None,
            "too far in"
        );
    }

    #[test]
    fn osc_8_hyperlinks_pages_and_files_in_the_repo() {
        let root = "/Users/me/repo";
        let t = |uri: &str| hyperlink_target(uri, root);
        assert_eq!(
            t("https://github.com/a/b"),
            Some(Target::Url("https://github.com/a/b".into()))
        );
        assert_eq!(
            t("file:///Users/me/repo/src/a%20b.ts"),
            path("src/a b.ts", None, None)
        );
        assert_eq!(
            t("file:///Users/me/repo/src/a.ts#L12"),
            path("src/a.ts", Some(12), None)
        );
        assert_eq!(
            t("file:///Users/me/repo/src/a.ts:3:4"),
            path("src/a.ts", Some(3), Some(4))
        );
        assert_eq!(t("file:///Users/me/repo/src/"), path("src", None, None));
        assert_eq!(t("file:///Users/me/repo"), dir(""));
        assert_eq!(t("file:///Users/me/repo-other/a.ts"), None);
        assert_eq!(t("file:///Users/me/repo/../secret"), None);
        assert_eq!(t("file:///Users/me/repo/a%2F..%2F..%2Fsecret"), None);
        assert_eq!(t("file:///Users/me/repo/a%E0.ts"), None);
        assert_eq!(t("file://otherhost/Users/me/repo/a.ts"), None);
        // This machine by name, as ls --hyperlink and eza write it.
        let host = std::process::Command::new("hostname").output().unwrap();
        let host = String::from_utf8_lossy(&host.stdout).trim().to_string();
        assert_eq!(
            t(&format!("file://{host}/Users/me/repo/a.ts")),
            path("a.ts", None, None)
        );
        assert_eq!(
            t("file://localhost/Users/me/repo/a.ts"),
            path("a.ts", None, None)
        );
        assert_eq!(
            t("file:///Users/me/repo/a.ts#top"),
            path("a.ts", None, None)
        );
        assert_eq!(
            t("file:///Users/me/repo/a.ts#L3-L9"),
            path("a.ts", Some(3), None)
        );
        assert_eq!(t("file:///Users/me/repo/.git/config"), None);
        assert_eq!(t("file:///Users/me/repo/sub/.GIT/HEAD"), None);
        assert_eq!(
            hyperlink_target("file:///C:/repo/src/a.ts:4", "C:\\repo"),
            path("src/a.ts", Some(4), None)
        );
        assert_eq!(t("file:///etc/passwd"), None);
        assert_eq!(t("javascript:alert(1)"), None);
        assert_eq!(t("vscode://file/Users/me/repo/a.ts"), None);
        assert_eq!(t("not a url"), None);
    }

    #[test]
    fn long_and_pathological_lines_stay_quick() {
        use std::time::{Duration, Instant};
        let quick = |f: &dyn Fn()| {
            let t = Instant::now();
            f();
            assert!(t.elapsed() < Duration::from_millis(500));
        };
        let out = chars(&"src/lib/api.ts:12 ".repeat(28_000));
        quick(&|| {
            let found = find_terminal_links(&out, Some((250_000, 250_080)));
            assert!(found.len() > 10 && found.iter().all(|l| l.spec == "src/lib/api.ts:12"));
        });
        let long = format!("{}c.ts", "a/".repeat(3000));
        assert_eq!(specs(&long), std::slice::from_ref(&long));
        let lc = chars(&long);
        assert!(find_terminal_links(&lc, Some((lc.len() - 1, lc.len()))).is_empty());
        quick(&|| {
            drop(find_terminal_links(
                &chars(&format!("https://x.com/{}a", ".".repeat(100_000))),
                None,
            ))
        });
        quick(&|| {
            drop(find_terminal_links(
                &chars(&format!("https://x.com/a{}", ")".repeat(20_000))),
                None,
            ))
        });
        quick(&|| {
            drop(find_terminal_links(
                &chars(&format!("a{}b", ".".repeat(20_000))),
                None,
            ))
        });
        for huge in [
            "9".repeat(300_000),
            "a".repeat(300_000),
            "ab12 ".repeat(60_000),
            "#1 ".repeat(100_000),
        ] {
            quick(&|| drop(find_terminal_links(&chars(&huge), Some((1000, 1001)))));
        }
    }

    #[test]
    fn a_dev_servers_address_is_this_machines_another_is_not() {
        for url in [
            "http://localhost:3000/",
            "https://localhost/",
            "http://127.0.0.1:5173/a?b#c",
            "http://app.localhost:3000/",
            "http://0.0.0.0:8080/",
            "http://[::1]:3000/",
            "HTTP://LOCALHOST:3000/",
        ] {
            assert!(is_local_url(url), "{url}");
        }
        for url in [
            "https://example.dev/",
            "http://192.168.1.5:3000/",
            "http://localhost.evil.example/",
            "http://127.0.0.1.evil.example/",
            "http://localhost:3000@evil.example/",
            "ws://localhost:3000/",
            "file:///tmp/x",
            "javascript:alert(1)",
            "nope",
            "",
        ] {
            assert!(!is_local_url(url), "{url}");
        }
    }

    #[test]
    fn local_pages_and_the_folders_a_line_names() {
        assert!(is_local_url("http://localhost:1420/"));
        assert!(is_local_url("http://127.0.0.1:3000/x"));
        assert!(is_local_url("http://[::1]:8080"));
        assert!(is_local_url("http://app.localhost:5173"));
        assert!(!is_local_url("https://example.com"));
        // A browser reads the backslash as a slash: the host is the one before it.
        assert!(!is_local_url("http://evil.example\\@localhost/"));
        assert!(!is_local_url("http://localhost.evil.example/"));
        assert!(!is_local_url("localhost:3000"));
        let mut dirs = vec![];
        named_dirs(
            &find_terminal_links(&chars("Files in /tmp/onb-c/ and src/lib/api.ts"), None),
            &mut dirs,
        );
        assert_eq!(dirs, ["src/lib", "/tmp/onb-c"]);
    }

    struct Rows(Vec<CellRow>, usize);

    impl RowSource for Rows {
        fn cols(&self) -> usize {
            self.1
        }
        fn top(&self) -> i32 {
            0
        }
        fn row(&mut self, line: i32) -> Option<std::rc::Rc<CellRow>> {
            self.0
                .get(usize::try_from(line).ok()?)
                .cloned()
                .map(std::rc::Rc::new)
        }
    }

    #[test]
    fn a_line_wrapped_by_the_terminal_or_cut_at_the_edge_is_read_whole() {
        let cols = 10;
        let mut a = CellRow::of("see src/aa", cols);
        a.wraps = true;
        let rows = vec![
            CellRow::of("in crates/", cols),
            CellRow::of("x", cols),
            a,
            CellRow::of("/b.rs:3 ok", cols),
            CellRow::of("/repo/abcd", cols),
            CellRow::of("  ef/g.png", cols),
            CellRow::of("- p4.png", cols),
        ];
        let mut src = Rows(rows, cols);
        let line = logical_line(&mut src, 3);
        assert_eq!((line.first, line.last), (2, 3));
        let links = find_terminal_links(&line.text, None);
        let l = links.iter().find(|l| l.spec == "src/aa/b.rs:3").unwrap();
        assert_eq!(line.range(l), ((2, 4), (3, 6)));
        assert!(line.whole(l));
        let cut = logical_line(&mut src, 5);
        assert_eq!((cut.first, cut.last), (4, 5));
        assert!(
            find_terminal_links(&cut.text, None)
                .iter()
                .any(|l| l.spec == "/repo/abcdef/g.png")
        );
        // A bare name under a line naming a folder; the nearest first.
        assert_eq!(
            context_dirs(&mut src, 6),
            vec!["/repo/abcdef".to_string(), "src/aa".into(), "crates".into()]
        );
        // One pass down, each line's own folders kept: the same answer.
        let mut seen = std::collections::VecDeque::new();
        let mut y = 0;
        while y < 6 {
            let line = logical_line(&mut src, y);
            seen.push_back(line_dirs(&line.text));
            y = line.last + 1;
        }
        assert_eq!(dirs_above(&seen), context_dirs(&mut src, 6));
    }
}
