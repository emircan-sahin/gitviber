//! PR and issue text as GitHub shows it: @mentions and #123 link, and images load only from
//! GitHub's own hosts (anything else tells its host your IP and when you read the PR, so it
//! becomes a link). Done on the markdown before it's rendered; the markdown view loads no
//! other images in it anyway.

use std::ops::Range;

use gitviber_markdown::links::is_github_hosted;

fn word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The ref at `chars[i]` ('@' or '#'), as (end, href): @login (GitHub's username rules) or #123,
/// not inside a word (in any script), path or email address; #0 and #007 are no issue.
fn ref_at(chars: &[char], i: usize, repo: &str) -> Option<(usize, String)> {
    if i > 0 {
        let p = chars[i - 1];
        if word(p) || "@/.-".contains(p) {
            return None;
        }
    }
    let alnum = |c: Option<&char>| c.is_some_and(|c| c.is_ascii_alphanumeric());
    match chars[i] {
        '@' => {
            let mut end = i + 1;
            if !alnum(chars.get(end)) {
                return None;
            }
            end += 1;
            let mut len = 1;
            while len < 39 {
                match chars.get(end) {
                    Some(c) if c.is_ascii_alphanumeric() => end += 1,
                    Some('-') if alnum(chars.get(end + 1)) => end += 1,
                    _ => break,
                }
                len += 1;
            }
            if chars
                .get(end)
                .is_some_and(|c| word(*c) || *c == '/' || *c == '-')
            {
                return None;
            }
            let login: String = chars[i + 1..end].iter().collect();
            Some((end, format!("https://github.com/{login}")))
        }
        '#' => {
            let first = chars.get(i + 1)?;
            if !('1'..='9').contains(first) {
                return None;
            }
            let mut end = i + 2;
            while chars.get(end).is_some_and(char::is_ascii_digit) {
                end += 1;
            }
            if chars.get(end).is_some_and(|c| word(*c)) {
                return None;
            }
            let number: String = chars[i + 1..end].iter().collect();
            Some((end, format!("{repo}/issues/{number}")))
        }
        _ => None,
    }
}

/// Plain text as markdown, each @mention and #123 a link to the page GitHub gives it (it
/// redirects a PR's number); `repo` is https://github.com/owner/name.
fn link_refs(value: &str, repo: &str, out: &mut String) {
    let chars: Vec<char> = value.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if let Some((end, href)) = ref_at(&chars, i, repo) {
            out.push('[');
            out.extend(&chars[i..end]);
            out.push_str("](");
            out.push_str(&href);
            out.push(')');
            i = end;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
}

/// An image uploaded to GitHub: github.com/user-attachments/assets/<id>, or the older
/// github.com/<owner>/<repo>/assets/<n>/<id>. Its id, for a private repo's signed link.
pub fn attachment_id(src: &str) -> Option<&str> {
    let rest = src.strip_prefix("https://github.com/")?;
    let rest = rest.strip_prefix("user-attachments/assets/").or_else(|| {
        let mut parts = rest.splitn(4, '/');
        let (_, _, assets, tail) = (parts.next()?, parts.next()?, parts.next()?, parts.next()?);
        let (n, tail) = tail.split_once('/')?;
        (assets == "assets" && !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
            .then_some(tail)
    })?;
    let id = rest.split(['?', '#']).next()?;
    (id.len() == 36 && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-')).then_some(id)
}

fn host_of(src: &str) -> &str {
    src.split_once("://")
        .map_or(src, |(_, r)| r)
        .split(['/', '?', '#'])
        .next()
        .unwrap_or(src)
}

/// What an image becomes when it may not load: a link that says where it would come from.
fn image_link(alt: &str, src: &str) -> String {
    let alt = if alt.is_empty() {
        String::new()
    } else {
        format!("{alt} · ")
    };
    format!("[🖼 {alt}Load image from {}]({src})", host_of(src))
}

/// A link in PR or issue text as GitHub resolves it: relative to its page (`base`). None for
/// what isn't a page: an anchor, another scheme.
pub fn absolute(base: &str, href: &str) -> Option<String> {
    let lower = href.to_ascii_lowercase();
    if lower.starts_with("https://") || lower.starts_with("http://") || lower.starts_with("mailto:")
    {
        return Some(href.to_string());
    }
    if href.starts_with('#') || href.is_empty() || href.contains(':') {
        return None;
    }
    if let Some(path) = href.strip_prefix('/') {
        return Some(format!("https://github.com/{path}"));
    }
    let dir = base.rsplit_once('/').map_or(base, |(d, _)| d);
    Some(format!("{dir}/{href}"))
}

/// Markdown text the way GitHub shows it: refs linked outside code, links and HTML; relative
/// links made absolute against `base` (the PR's or issue's page); images from elsewhere turned
/// into links; and attachments swapped for `signed` links (a private repo's, which need a
/// github.com login the app doesn't have). A raw HTML block stays HTML, its images checked the
/// same way and its refs left as text: markdown written into it would show as written.
pub fn github_markdown(
    text: &str,
    repo: &str,
    base: &str,
    signed: &dyn Fn(&str) -> Option<String>,
) -> String {
    let cx = Rewrite { repo, base, signed };
    let mut out = String::with_capacity(text.len() + 64);
    let mut fence: Option<String> = None;
    let mut html: Option<(HtmlEnd, String)> = None;
    // The paragraph being gathered: a tag or a code span may run over its lines.
    let mut para = String::new();
    for line in text.split_inclusive('\n') {
        let blank = line.trim().is_empty();
        if let Some((end, mut block)) = html.take() {
            match end {
                HtmlEnd::Blank if blank => cx.html(&block, &mut out),
                HtmlEnd::Blank => {
                    block.push_str(line);
                    html = Some((end, block));
                    continue;
                }
                HtmlEnd::Marker(markers) => {
                    block.push_str(line);
                    if closes(line, markers) {
                        cx.html(&block, &mut out);
                    } else {
                        html = Some((end, block));
                    }
                    continue;
                }
            }
        }
        let trimmed = line.trim_start();
        let marker: String = trimmed
            .chars()
            .take_while(|c| *c == '`' || *c == '~')
            .collect();
        if let Some(open) = &fence {
            if trimmed.starts_with(open.as_str())
                && trimmed
                    .trim_end()
                    .chars()
                    .all(|c| c == open.chars().next().unwrap_or('`'))
            {
                fence = None;
            }
            out.push_str(line);
            continue;
        }
        if marker.len() >= 3
            && marker
                .chars()
                .all(|c| c == marker.chars().next().unwrap_or('`'))
        {
            cx.flush(&mut para, &mut out);
            fence = Some(marker);
            out.push_str(line);
            continue;
        }
        if blank {
            cx.flush(&mut para, &mut out);
            out.push_str(line);
            continue;
        }
        // Indented code (4 spaces) is code too, unless it carries on a paragraph.
        if para.is_empty() && (line.starts_with("    ") || line.starts_with('\t')) {
            out.push_str(line);
            continue;
        }
        if let Some(end) = html_start(line, !para.is_empty()) {
            cx.flush(&mut para, &mut out);
            match end {
                HtmlEnd::Marker(markers) if closes(line, markers) => cx.html(line, &mut out),
                end => html = Some((end, line.to_string())),
            }
            continue;
        }
        para.push_str(line);
        if is_heading(line) {
            cx.flush(&mut para, &mut out);
        }
    }
    if let Some((_, block)) = html {
        cx.html(&block, &mut out);
    }
    cx.flush(&mut para, &mut out);
    out
}

/// How a raw HTML block ends, by CommonMark's kinds: on the line holding one of these, or
/// before a blank line.
#[derive(Clone, Copy, PartialEq, Eq)]
enum HtmlEnd {
    Marker(&'static [&'static str]),
    Blank,
}

/// Tags that open an HTML block of their own (CommonMark's kind 6).
const BLOCK_TAGS: [&str; 62] = [
    "address",
    "article",
    "aside",
    "base",
    "basefont",
    "blockquote",
    "body",
    "caption",
    "center",
    "col",
    "colgroup",
    "dd",
    "details",
    "dialog",
    "dir",
    "div",
    "dl",
    "dt",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "frame",
    "frameset",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "head",
    "header",
    "hr",
    "html",
    "iframe",
    "legend",
    "li",
    "link",
    "main",
    "menu",
    "menuitem",
    "nav",
    "noframes",
    "ol",
    "optgroup",
    "option",
    "p",
    "param",
    "search",
    "section",
    "summary",
    "table",
    "tbody",
    "td",
    "tfoot",
    "th",
    "thead",
    "title",
    "tr",
    "track",
    "ul",
];

/// The raw HTML block `line` opens, if it opens one (CommonMark's start conditions). `in_para`:
/// it would carry on a paragraph, which a lone tag (kind 7) doesn't interrupt.
fn html_start(line: &str, in_para: bool) -> Option<HtmlEnd> {
    let indent = line.len() - line.trim_start_matches(' ').len();
    let lower = line[indent..].trim_end().to_ascii_lowercase();
    if indent > 3 || !lower.starts_with('<') {
        return None;
    }
    let ends_name = |rest: &str| rest.is_empty() || rest.starts_with([' ', '\t', '>']);
    for name in ["script", "pre", "style", "textarea"] {
        if lower[1..].strip_prefix(name).is_some_and(ends_name) {
            return Some(HtmlEnd::Marker(&[
                "</script>",
                "</pre>",
                "</style>",
                "</textarea>",
            ]));
        }
    }
    if lower.starts_with("<!--") {
        return Some(HtmlEnd::Marker(&["-->"]));
    }
    if lower.starts_with("<?") {
        return Some(HtmlEnd::Marker(&["?>"]));
    }
    if lower.starts_with("<![cdata[") {
        return Some(HtmlEnd::Marker(&["]]>"]));
    }
    if lower[1..].starts_with('!') && lower[2..].starts_with(|c: char| c.is_ascii_alphabetic()) {
        return Some(HtmlEnd::Marker(&[">"]));
    }
    let rest = lower[1..].strip_prefix('/').unwrap_or(&lower[1..]);
    let n = rest.bytes().take_while(u8::is_ascii_alphanumeric).count();
    let after = &rest[n..];
    if BLOCK_TAGS.contains(&&rest[..n]) && (ends_name(after) || after.starts_with("/>")) {
        return Some(HtmlEnd::Blank);
    }
    // A whole tag alone on its line.
    let lone = n > 0
        && rest.starts_with(|c: char| c.is_ascii_alphabetic())
        && lower.ends_with('>')
        && lower.matches('<').count() == 1;
    (lone && !in_para).then_some(HtmlEnd::Blank)
}

fn closes(line: &str, markers: &[&str]) -> bool {
    let lower = line.to_ascii_lowercase();
    markers.iter().any(|m| lower.contains(m))
}

fn is_heading(line: &str) -> bool {
    let t = line.trim_start_matches(' ');
    let hashes = t.bytes().take_while(|b| *b == b'#').count();
    line.len() - t.len() <= 3
        && (1..=6).contains(&hashes)
        && t[hashes..].starts_with([' ', '\t', '\n', '\r'])
}

/// An HTML attribute's value as written (quotes off) and where it is in the tag; one written
/// inside another's value counts too, which is fine for a tag rebuilt from what's read here.
fn attr<'a>(tag: &'a str, name: &str) -> Option<(Range<usize>, &'a str)> {
    let lower = tag.to_ascii_lowercase();
    let b = lower.as_bytes();
    let space = |j: usize| b.get(j).is_some_and(u8::is_ascii_whitespace);
    for (at, _) in lower.match_indices(name) {
        if at == 0 || !(b[at - 1].is_ascii_whitespace() || b[at - 1] == b'/') {
            continue;
        }
        let mut j = at + name.len();
        while space(j) {
            j += 1;
        }
        if b.get(j) != Some(&b'=') {
            continue;
        }
        j += 1;
        while space(j) {
            j += 1;
        }
        let (start, end) = match b.get(j)? {
            q @ (b'"' | b'\'') => {
                let s = j + 1;
                (s, lower[s..].find(*q as char).map_or(tag.len(), |e| s + e))
            }
            _ => (
                j,
                lower[j..]
                    .find(|c: char| c.is_ascii_whitespace() || c == '>')
                    .map_or(tag.len(), |e| j + e),
            ),
        };
        return Some((start..end, &tag[start..end]));
    }
    None
}

/// Text for an HTML attribute's double quotes. `amp`: an `&` is text too (a URL from outside
/// the HTML), not a reference already written as one.
fn html_escape(s: &str, amp: bool) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' if amp => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\n' | '\r' => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

/// The `>` ending the tag at `chars[i]`, past quoted values. `html`: as a browser reads it,
/// where a `<` is part of the tag; else as markdown does, where it means this was no tag. Looked
/// for no further than a long tag runs, so text full of `<` stays cheap.
fn tag_end(chars: &[char], i: usize, html: bool) -> Option<usize> {
    const LONGEST: usize = 8192;
    let end = chars.len().min(i + LONGEST);
    let mut after_eq = false;
    let mut j = i + 1;
    while j < end {
        match chars[j] {
            '>' => return Some(j),
            '<' if !html => return None,
            q @ ('"' | '\'') if after_eq => {
                j += 1 + chars[j + 1..end].iter().position(|c| *c == q)?;
                after_eq = false;
            }
            '=' => after_eq = true,
            c if c.is_whitespace() => {}
            _ => after_eq = false,
        }
        j += 1;
    }
    None
}

/// A tag's name, lowercase, and whether it closes.
fn tag_name(tag: &str) -> (String, bool) {
    let inner = tag.trim_start_matches('<');
    let (closing, inner) = match inner.strip_prefix('/') {
        Some(rest) => (true, rest),
        None => (false, inner),
    };
    let name = inner
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase();
    (name, closing)
}

/// What the rewriting needs: the repo, the page links resolve against, attachments' links.
struct Rewrite<'a> {
    repo: &'a str,
    base: &'a str,
    signed: &'a dyn Fn(&str) -> Option<String>,
}

impl Rewrite<'_> {
    fn flush(&self, para: &mut String, out: &mut String) {
        if !para.is_empty() {
            self.inline(para, false, out);
            para.clear();
        }
    }

    /// Markdown text: code spans, links and HTML tags kept as they are, but for what they hold
    /// that loads or links. `quiet`: inside a link, where refs stay text.
    fn inline(&self, s: &str, quiet: bool, out: &mut String) {
        let chars: Vec<char> = s.chars().collect();
        let marks = Marks::new(&chars);
        // Inside an <a>, a <code> or a <pre> refs stay text, as GitHub leaves them.
        let mut quiet_tags = 0usize;
        let mut text = String::new();
        let flush = |text: &mut String, out: &mut String, quiet: bool| {
            if quiet {
                out.push_str(text);
            } else {
                link_refs(text, self.repo, out);
            }
            text.clear();
        };
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            let hush = quiet || quiet_tags > 0;
            if c == '\\' && chars.get(i + 1).is_some_and(char::is_ascii_punctuation) {
                flush(&mut text, out, hush);
                out.extend(&chars[i..i + 2]);
                i += 2;
                continue;
            }
            // A code span: a run of backticks to the next run as long.
            if c == '`' {
                let n = marks.ticks(i);
                match marks.closing_ticks(i, n) {
                    Some(close) => {
                        flush(&mut text, out, hush);
                        out.extend(&chars[i..close + n]);
                        i = close + n;
                    }
                    None => {
                        text.extend(&chars[i..i + n]);
                        i += n;
                    }
                }
                continue;
            }
            // An image or a link: [text](url), ![alt](src).
            if c == '[' || (c == '!' && chars.get(i + 1) == Some(&'[')) {
                let open = if c == '!' { i + 1 } else { i };
                if let Some((label_end, url_end)) = marks.link(&chars, open) {
                    flush(&mut text, out, hush);
                    let label: String = chars[open + 1..label_end].iter().collect();
                    let target: String = chars[label_end + 2..url_end].iter().collect();
                    if c == '!' {
                        out.push_str(&self.md_image(&label, &target, quiet));
                    } else {
                        out.push('[');
                        // Links keep their text, but what loads in it is checked (one level
                        // down: a link can't hold another).
                        if quiet {
                            out.push_str(&label);
                        } else {
                            self.inline(&label, true, out);
                        }
                        let (href, title) = target
                            .split_once(' ')
                            .map_or((target.as_str(), ""), |(h, t)| (h, t));
                        let href = absolute(self.base, href).unwrap_or_else(|| href.to_string());
                        if title.is_empty() {
                            out.push_str(&format!("]({href})"));
                        } else {
                            out.push_str(&format!("]({href} {title})"));
                        }
                    }
                    i = url_end + 1;
                    continue;
                }
            }
            // An HTML tag or an autolink, to its `>`; a `<` before that is text.
            if c == '<'
                && chars
                    .get(i + 1)
                    .is_some_and(|c| c.is_ascii_alphabetic() || "/!?".contains(*c))
                && let Some(close) = tag_end(&chars, i, false)
            {
                flush(&mut text, out, hush);
                let tag: String = chars[i..=close].iter().collect();
                let (name, closing) = tag_name(&tag);
                if matches!(name.as_str(), "a" | "code" | "pre") && !tag.ends_with("/>") {
                    quiet_tags = if closing {
                        quiet_tags.saturating_sub(1)
                    } else {
                        quiet_tags + 1
                    };
                }
                out.push_str(&self.tag(&tag, false));
                i = close + 1;
                continue;
            }
            text.push(c);
            i += 1;
        }
        flush(&mut text, out, quiet || quiet_tags > 0);
    }

    /// A raw HTML block: its tags checked, its text and comments as written.
    fn html(&self, block: &str, out: &mut String) {
        let chars: Vec<char> = block.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] != '<' {
                out.push(chars[i]);
                i += 1;
                continue;
            }
            if chars[i..].starts_with(&['<', '!', '-', '-']) {
                let end = (i + 4..chars.len())
                    .find(|&j| chars[j..].starts_with(&['-', '-', '>']))
                    .map_or(chars.len(), |j| j + 3);
                out.extend(&chars[i..end]);
                i = end;
                continue;
            }
            let name = chars.get(i + 1 + usize::from(chars.get(i + 1) == Some(&'/')));
            let opens = name.is_some_and(char::is_ascii_alphabetic);
            match tag_end(&chars, i, true).filter(|_| opens) {
                Some(end) => {
                    let tag: String = chars[i..=end].iter().collect();
                    out.push_str(&self.tag(&tag, true));
                    i = end + 1;
                }
                // Not a tag, or one never closed: text, as the browser shows it.
                None => {
                    out.push_str(if opens { "&lt;" } else { "<" });
                    i += 1;
                }
            }
        }
    }

    /// One tag: an `<img>` checked, an `<a>`'s relative link made absolute, the rest as they
    /// are. `in_html`: in an HTML block, where what replaces an image has to be HTML too.
    fn tag(&self, tag: &str, in_html: bool) -> String {
        match tag_name(tag) {
            (name, false) if name == "img" => {
                let Some((_, src)) = attr(tag, "src") else {
                    return String::new();
                };
                let alt = attr(tag, "alt").map_or("", |(_, a)| a);
                self.html_image(tag, src, alt, in_html)
            }
            (name, false) if name == "a" => match attr(tag, "href") {
                Some((range, href)) => match absolute(self.base, href).filter(|a| a != href) {
                    Some(url) => format!(
                        "{}{}{}",
                        &tag[..range.start],
                        html_escape(&url, false),
                        &tag[range.end..]
                    ),
                    None => tag.to_string(),
                },
                None => tag.to_string(),
            },
            _ => tag.to_string(),
        }
    }

    /// A markdown image: as written when it may load, its signed link for an attachment, a
    /// link that says where it would come from otherwise; in a link (`in_link`, a badge), what
    /// it shows, as the link can't hold another.
    fn md_image(&self, alt: &str, target: &str, in_link: bool) -> String {
        let src = target.split_whitespace().next().unwrap_or("");
        if let Some(url) = attachment_id(src).and_then(self.signed) {
            return format!("![{alt}]({url})");
        }
        if is_github_hosted(src) {
            format!("![{alt}]({target})")
        } else if in_link {
            format!("🖼 {alt}").trim_end().to_string()
        } else {
            image_link(alt, src)
        }
    }

    /// An `<img>` rebuilt from its src, alt and size alone (nothing else it says is read), or
    /// the link that stands for it.
    fn html_image(&self, tag: &str, src: &str, alt: &str, in_html: bool) -> String {
        let (src, amp) = match attachment_id(src).and_then(self.signed) {
            Some(url) => (url, true),
            None => (src.to_string(), false),
        };
        if !is_github_hosted(&src) {
            if !in_html {
                return image_link(alt, &src);
            }
            let alt = if alt.is_empty() {
                String::new()
            } else {
                format!("{} · ", html_escape(alt, false))
            };
            return format!(
                "<a href=\"{}\">🖼 {alt}Load image from {}</a>",
                html_escape(&src, false),
                html_escape(host_of(&src), false)
            );
        }
        let mut out = format!(
            "<img src=\"{}\" alt=\"{}\"",
            html_escape(&src, amp),
            html_escape(alt, false)
        );
        for name in ["width", "height"] {
            if let Some((_, v)) = attr(tag, name)
                && !v.is_empty()
                && v.bytes()
                    .all(|b| b.is_ascii_digit() || b"%px.".contains(&b))
            {
                out.push_str(&format!(" {name}=\"{v}\""));
            }
        }
        out.push('>');
        out
    }
}

/// Where a paragraph's code spans and links end, found once for all of it, so a text full of
/// unclosed ones costs no more than a plain one.
struct Marks {
    /// The length of the backtick run starting at each index (0 inside or outside one).
    runs: Vec<usize>,
    /// Backtick runs by length: their starts, in order.
    by_len: std::collections::HashMap<usize, Vec<usize>>,
    /// The `]` closing the `[` at each index.
    bracket: std::collections::HashMap<usize, usize>,
    /// The first `)` at or after each index.
    paren: Vec<Option<usize>>,
}

impl Marks {
    fn new(chars: &[char]) -> Marks {
        let n = chars.len();
        let mut runs = vec![0; n];
        let mut by_len: std::collections::HashMap<usize, Vec<usize>> = Default::default();
        let mut i = 0;
        while i < n {
            if chars[i] == '`' {
                let len = chars[i..].iter().take_while(|c| **c == '`').count();
                runs[i] = len;
                by_len.entry(len).or_default().push(i);
                i += len;
            } else {
                i += 1;
            }
        }
        let mut bracket = std::collections::HashMap::new();
        let mut open = Vec::new();
        let mut escaped = false;
        for (i, c) in chars.iter().enumerate() {
            match c {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '[' => open.push(i),
                ']' => {
                    if let Some(o) = open.pop() {
                        bracket.insert(o, i);
                    }
                }
                _ => {}
            }
        }
        let mut paren = vec![None; n + 1];
        for i in (0..n).rev() {
            paren[i] = if chars[i] == ')' {
                Some(i)
            } else {
                paren[i + 1]
            };
        }
        Marks {
            runs,
            by_len,
            bracket,
            paren,
        }
    }

    fn ticks(&self, i: usize) -> usize {
        self.runs[i].max(1)
    }

    /// The run of exactly `n` backticks that closes the one at `i`.
    fn closing_ticks(&self, i: usize, n: usize) -> Option<usize> {
        let starts = self.by_len.get(&n)?;
        starts.get(starts.partition_point(|s| *s <= i)).copied()
    }

    /// `[label](url)` from the `[` at `open`: where the label's `]` and the url's `)` are.
    fn link(&self, chars: &[char], open: usize) -> Option<(usize, usize)> {
        let label_end = *self.bracket.get(&open)?;
        if chars.get(label_end + 1) != Some(&'(') {
            return None;
        }
        Some((label_end, self.paren[label_end + 2]?))
    }
}

/// An error as markdown that reads exactly as written: its punctuation escaped, each line its
/// own, and its web links (a SAML sign-on's, a docs page) live; a sentence's last dot or a
/// closing bracket stays out of the link.
pub fn error_markdown(error: &str) -> String {
    let mut out = String::with_capacity(error.len() + 16);
    for (n, line) in error.lines().enumerate() {
        if n > 0 {
            out.push_str("  \n");
        }
        let mut rest = line;
        while !rest.is_empty() {
            let at = ["https://", "http://"]
                .iter()
                .filter_map(|s| rest.find(s))
                .min()
                .unwrap_or(rest.len());
            for c in rest[..at].chars() {
                if c.is_ascii_punctuation() {
                    out.push('\\');
                }
                out.push(c);
            }
            rest = &rest[at..];
            let end = rest
                .find(|c: char| c.is_whitespace() || c == '<' || c == '>' || c == '"')
                .unwrap_or(rest.len());
            let url = rest[..end].trim_end_matches(['.', ',', ';', ':', ')', ']', '\'', '!', '?']);
            if !url.is_empty() {
                out.push('<');
                out.push_str(url);
                out.push('>');
            }
            rest = &rest[url.len()..];
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_error_reads_as_written_and_its_links_work() {
        let saml = "GitHub 403: Resource protected by organization SAML enforcement. Authorize \
                    this token for the organization's single sign-on at \
                    https://github.com/orgs/acme/sso?authorization_request=A_b-1.";
        assert_eq!(
            error_markdown(saml),
            "GitHub 403\\: Resource protected by organization SAML enforcement\\. Authorize this \
             token for the organization\\'s single sign\\-on at \
             <https://github.com/orgs/acme/sso?authorization_request=A_b-1>\\."
        );
        // Markdown's own marks are only text; lines stay apart; a bracketed link stays inside.
        assert_eq!(
            error_markdown("*not* `code`\n# no heading (see http://x.io/a_b)"),
            "\\*not\\* \\`code\\`  \n\\# no heading \\(see <http://x.io/a_b>\\)"
        );
        assert_eq!(error_markdown("plain"), "plain");
    }

    const REPO: &str = "https://github.com/o/r";
    const PAGE: &str = "https://github.com/o/r/pull/7";

    #[test]
    fn links_in_pr_text_resolve_against_its_page() {
        assert_eq!(
            absolute(PAGE, "https://x.y/z").as_deref(),
            Some("https://x.y/z")
        );
        assert_eq!(
            absolute(PAGE, "/o/r/issues/3").as_deref(),
            Some("https://github.com/o/r/issues/3")
        );
        assert_eq!(
            absolute(PAGE, "files").as_deref(),
            Some("https://github.com/o/r/pull/files")
        );
        assert_eq!(absolute(PAGE, "#fn-1"), None);
        assert_eq!(absolute(PAGE, "javascript:alert(1)"), None);
        let none = |_: &str| None;
        assert_eq!(
            github_markdown("[the files](files) and [top](#top)", REPO, PAGE, &none),
            "[the files](https://github.com/o/r/pull/files) and [top](#top)"
        );
    }

    #[test]
    fn plain_text_links_its_refs() {
        let none = |_: &str| None;
        assert_eq!(
            github_markdown("Fix login (#12), thanks @octo; not x#3", REPO, PAGE, &none),
            "Fix login ([#12](https://github.com/o/r/issues/12)), thanks \
             [@octo](https://github.com/octo); not x#3"
        );
        for text in ["no refs", "#0", "#007", "é#8", "#8é", "ü@octo", "a@b.com"] {
            assert_eq!(github_markdown(text, REPO, PAGE, &none), text, "{text}");
        }
    }

    #[test]
    fn mentions_and_refs_link_but_not_inside_links_or_code() {
        let none = |_: &str| None;
        let md = github_markdown(
            "Thanks @octo-cat, fixes #12; not `@a #4` or [see @c #6](https://example.com).\n```\n@b #5\n```\n",
            REPO,
            PAGE,
            &none,
        );
        assert!(md.contains("[@octo-cat](https://github.com/octo-cat)"));
        assert!(md.contains("[#12](https://github.com/o/r/issues/12)"));
        assert!(md.contains("`@a #4`"));
        assert!(md.contains("[see @c #6](https://example.com)"));
        assert!(md.contains("\n@b #5\n"));
    }

    #[test]
    fn only_githubs_own_image_hosts_load_without_asking() {
        let none = |_: &str| None;
        let md = github_markdown(
            "![badge](https://img.shields.io/b.svg) <img src=\"https://t.example/p.png\" alt=\"x\">",
            REPO,
            PAGE,
            &none,
        );
        assert_eq!(
            md,
            "[🖼 badge · Load image from img.shields.io](https://img.shields.io/b.svg) [🖼 x · Load image from t.example](https://t.example/p.png)"
        );
    }

    #[test]
    fn refs_stay_text_inside_raw_html() {
        let none = |_: &str| None;
        let md = github_markdown(
            "Thanks @octo, <code>@b #5</code> or <a href=\"https://example.com\">see @c #6</a>, <b>#7</b>.",
            REPO,
            PAGE,
            &none,
        );
        assert!(md.contains("[@octo](https://github.com/octo)"), "{md}");
        assert!(md.contains("<code>@b #5</code>"), "{md}");
        assert!(
            md.contains("<a href=\"https://example.com\">see @c #6</a>"),
            "{md}"
        );
        assert!(
            md.contains("<b>[#7](https://github.com/o/r/issues/7)</b>"),
            "{md}"
        );
        // Dependabot's list of commits: an HTML block, where markdown would show as written.
        let block = "<details>\n<summary>Commits</summary>\n<ul>\n<li><a href=\"https://github.com/x/y/commit/abc\"><code>abc</code></a> Fix #1234 by @bot</li>\n</ul>\n</details>\n";
        let md = github_markdown(
            &format!("Bumps x from 1 to 2.\n{block}\nThanks #12\n"),
            REPO,
            PAGE,
            &none,
        );
        assert_eq!(
            md,
            format!(
                "Bumps x from 1 to 2.\n{block}\nThanks [#12](https://github.com/o/r/issues/12)\n"
            )
        );
        // A lone tag doesn't break into a paragraph; after a blank line it opens a block.
        let md = github_markdown("text\n<span>#3</span>\n\n<span>\n#4\n", REPO, PAGE, &none);
        assert!(md.contains("<span>[#3]("), "{md}");
        assert!(md.ends_with("<span>\n#4\n"), "{md}");
    }

    #[test]
    fn html_blocks_keep_their_images_as_html() {
        let id = "012a2451-fa01-4fe5-8736-33f4a4d179f1";
        let shot = format!("https://github.com/user-attachments/assets/{id}");
        let none = |_: &str| None;
        let md = github_markdown(
            &format!(
                "<details><summary>Shot</summary><img src=\"{shot}\" width=\"300\" onerror=\"x()\"></details>"
            ),
            REPO,
            PAGE,
            &none,
        );
        assert_eq!(
            md,
            format!(
                "<details><summary>Shot</summary><img src=\"{shot}\" alt=\"\" width=\"300\"></details>"
            )
        );
        let signed = |i: &str| {
            (i == id).then(|| {
                "https://private-user-images.githubusercontent.com/1/a.png?jwt=x&y=1".to_string()
            })
        };
        let md = github_markdown(
            &format!("<p>\n<img alt=\"a\" src='{shot}'>\n</p>"),
            REPO,
            PAGE,
            &signed,
        );
        assert!(
            md.contains("<img src=\"https://private-user-images.githubusercontent.com/1/a.png?jwt=x&amp;y=1\" alt=\"a\">"),
            "{md}"
        );
        // From elsewhere, an HTML link to it; a tag over several lines is read whole.
        let md = github_markdown(
            "<div>\n<img\n  src=\"https://t.example/p.png\" alt=\"<x>\">\n</div>",
            REPO,
            PAGE,
            &none,
        );
        assert_eq!(
            md,
            "<div>\n<a href=\"https://t.example/p.png\">🖼 &lt;x&gt; · Load image from t.example</a>\n</div>"
        );
        // Relative links in HTML resolve against the page too.
        let md = github_markdown("<p><a href=\"files\">f</a></p>", REPO, PAGE, &none);
        assert_eq!(
            md,
            "<p><a href=\"https://github.com/o/r/pull/files\">f</a></p>"
        );
    }

    #[test]
    fn images_markdown_would_find_are_found_first() {
        let none = |_: &str| None;
        let loads = |md: &str| md.contains("![") || md.contains("<img");
        for text in [
            // A run of two backticks with no closing pair isn't a code span.
            "``![x](https://e.example/a.png)`",
            // An escaped backtick opens nothing.
            "\\`![x](https://e.example/a.png)`",
            // A `<` that isn't a tag doesn't hide the one after it.
            "a < b <img src=\"https://e.example/a.png\">",
            "<img\nsrc=\"https://e.example/a.png\"> over two lines",
        ] {
            let md = github_markdown(text, REPO, PAGE, &none);
            assert!(!loads(&md) && md.contains("e.example"), "{text}: {md}");
        }
        // A badge in a link keeps the link and shows what it is.
        assert_eq!(
            github_markdown(
                "[![build](https://img.shields.io/b.svg)](https://x.dev)",
                REPO,
                PAGE,
                &none
            ),
            "[🖼 build](https://x.dev)"
        );
    }

    #[test]
    fn unclosed_code_spans_and_links_dont_stall_a_body() {
        let none = |_: &str| None;
        let start = std::time::Instant::now();
        for unit in ["`` [x](", "a < b ", "[[x] ", "![y]( "] {
            github_markdown(&unit.repeat(5000), REPO, PAGE, &none);
        }
        assert!(start.elapsed().as_millis() < 2000, "{:?}", start.elapsed());
    }

    #[test]
    fn attachments_take_their_signed_link() {
        let id = "012a2451-fa01-4fe5-8736-33f4a4d179f1";
        assert_eq!(
            attachment_id(&format!("https://github.com/user-attachments/assets/{id}")),
            Some(id)
        );
        assert_eq!(
            attachment_id(&format!("https://github.com/o/r/assets/12/{id}?x")),
            Some(id)
        );
        assert_eq!(
            attachment_id("https://github.com/o/r/blob/main/a.png"),
            None
        );
        let signed = |i: &str| (i == id).then(|| "https://signed.example/a".to_string());
        let md = github_markdown(
            &format!("![shot](https://github.com/user-attachments/assets/{id})"),
            REPO,
            PAGE,
            &signed,
        );
        assert_eq!(md, "![shot](https://signed.example/a)");
    }
}
