//! PR and issue text as GitHub shows it: @mentions and #123 link, and images load only from
//! GitHub's own hosts (anything else tells its host your IP and when you read the PR, so it
//! becomes a link). Done on the markdown before it's rendered.

/// A piece of plain text, with the page it links to for a ref.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Piece {
    pub text: String,
    pub href: Option<String>,
}

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

/// Plain text in pieces, @mentions and #123 with the page GitHub gives them (it redirects a
/// PR's number); `repo` is https://github.com/owner/name. For a commit message's subject.
pub fn github_refs(value: &str, repo: &str) -> Vec<Piece> {
    let chars: Vec<char> = value.chars().collect();
    let mut out = Vec::new();
    let mut last = 0;
    let mut i = 0;
    while i < chars.len() {
        if let Some((end, href)) = ref_at(&chars, i, repo) {
            if i > last {
                out.push(Piece {
                    text: chars[last..i].iter().collect(),
                    href: None,
                });
            }
            out.push(Piece {
                text: chars[i..end].iter().collect(),
                href: Some(href),
            });
            last = end;
            i = end;
        } else {
            i += 1;
        }
    }
    if last < chars.len() || out.is_empty() {
        out.push(Piece {
            text: chars[last..].iter().collect(),
            href: None,
        });
    }
    out
}

/// Hosts GitHub serves images from; anything else loads only on request.
pub fn is_github_hosted(src: &str) -> bool {
    let Some(rest) = src.strip_prefix("https://") else {
        return false;
    };
    let host = rest
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    host == "github.com" || host.ends_with(".githubusercontent.com")
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

/// Markdown text the way GitHub shows it: refs linked outside code and links, relative links
/// made absolute against `base` (the PR's or issue's page), images from elsewhere turned into
/// links, and attachments swapped for `signed` links (a private repo's, which need a
/// github.com login the app doesn't have).
pub fn github_markdown(
    text: &str,
    repo: &str,
    base: &str,
    signed: &dyn Fn(&str) -> Option<String>,
) -> String {
    let mut out = String::with_capacity(text.len() + 64);
    let mut fence: Option<String> = None;
    for line in text.split_inclusive('\n') {
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
            fence = Some(marker);
            out.push_str(line);
            continue;
        }
        // Indented code (4 spaces) is code too.
        if line.starts_with("    ") || line.starts_with('\t') {
            out.push_str(line);
            continue;
        }
        inline(line, repo, base, signed, &mut out);
    }
    out
}

/// One line outside code blocks: code spans, links and HTML tags kept as they are.
fn inline(
    line: &str,
    repo: &str,
    base: &str,
    signed: &dyn Fn(&str) -> Option<String>,
    out: &mut String,
) {
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    let mut text = String::new();
    let flush = |text: &mut String, out: &mut String| {
        for p in github_refs(text, repo) {
            match p.href {
                Some(href) => out.push_str(&format!("[{}]({href})", p.text)),
                None => out.push_str(&p.text),
            }
        }
        text.clear();
    };
    while i < chars.len() {
        let c = chars[i];
        // A code span: as many backticks close it as opened it.
        if c == '`' {
            let n = chars[i..].iter().take_while(|x| **x == '`').count();
            let ticks: String = "`".repeat(n);
            let rest: String = chars[i + n..].iter().collect();
            if let Some(close) = rest.find(&ticks) {
                flush(&mut text, out);
                let span_len = n + rest[..close].chars().count() + n;
                out.extend(&chars[i..i + span_len]);
                i += span_len;
                continue;
            }
        }
        // An image or a link: [text](url), ![alt](src).
        if c == '[' || (c == '!' && chars.get(i + 1) == Some(&'[')) {
            let open = if c == '!' { i + 1 } else { i };
            if let Some((label_end, url_end)) = link_at(&chars, open) {
                flush(&mut text, out);
                let label: String = chars[open + 1..label_end].iter().collect();
                let target: String = chars[label_end + 2..url_end].iter().collect();
                if c == '!' {
                    out.push_str(&image(&label, &target, signed));
                } else {
                    // Links keep their text: a ref inside one is already a link.
                    let (href, title) = target
                        .split_once(' ')
                        .map_or((target.as_str(), ""), |(h, t)| (h, t));
                    match absolute(base, href).filter(|a| a != href) {
                        Some(url) if title.is_empty() => out.push_str(&format!("[{label}]({url})")),
                        Some(url) => out.push_str(&format!("[{label}]({url} {title})")),
                        None => out.extend(&chars[i..=url_end]),
                    }
                }
                i = url_end + 1;
                continue;
            }
        }
        // An HTML tag or an autolink: an <img> is checked, the rest kept whole.
        if c == '<'
            && let Some(close) = chars[i..].iter().position(|x| *x == '>')
        {
            flush(&mut text, out);
            let tag: String = chars[i..=i + close].iter().collect();
            out.push_str(&html_tag(&tag, signed));
            i += close + 1;
            continue;
        }
        text.push(c);
        i += 1;
    }
    flush(&mut text, out);
}

/// `[label](url)` starting at `open` ('['): where the label's ']' and the url's ')' are.
fn link_at(chars: &[char], open: usize) -> Option<(usize, usize)> {
    let mut depth = 0;
    let mut label_end = None;
    for (j, c) in chars.iter().enumerate().skip(open) {
        match c {
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    label_end = Some(j);
                    break;
                }
            }
            _ => {}
        }
    }
    let label_end = label_end?;
    if chars.get(label_end + 1) != Some(&'(') {
        return None;
    }
    let url_end = chars[label_end + 2..].iter().position(|c| *c == ')')? + label_end + 2;
    Some((label_end, url_end))
}

fn image(alt: &str, target: &str, signed: &dyn Fn(&str) -> Option<String>) -> String {
    let src = target.split_whitespace().next().unwrap_or("");
    if let Some(url) = attachment_id(src).and_then(signed) {
        return format!("![{alt}]({url})");
    }
    if is_github_hosted(src) {
        format!("![{alt}]({target})")
    } else {
        image_link(alt, src)
    }
}

/// An `<img>` tag's src; other tags pass through.
fn html_tag(tag: &str, signed: &dyn Fn(&str) -> Option<String>) -> String {
    let lower = tag.to_ascii_lowercase();
    if !lower.starts_with("<img") {
        return tag.to_string();
    }
    let attr = |name: &str| -> Option<String> {
        let at = lower.find(&format!("{name}="))? + name.len() + 1;
        let rest = &tag[at..];
        let quote = rest.chars().next()?;
        if quote == '"' || quote == '\'' {
            rest[1..].split(quote).next().map(str::to_string)
        } else {
            rest.split([' ', '>', '/']).next().map(str::to_string)
        }
    };
    let Some(src) = attr("src") else {
        return String::new();
    };
    let alt = attr("alt").unwrap_or_default();
    image(&alt, &src, signed)
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn piece(text: &str, href: Option<&str>) -> Piece {
        Piece {
            text: text.into(),
            href: href.map(str::to_string),
        }
    }

    #[test]
    fn plain_text_links_its_refs() {
        assert_eq!(
            github_refs("Fix login (#12), thanks @octo; not x#3", REPO),
            vec![
                piece("Fix login (", None),
                piece("#12", Some("https://github.com/o/r/issues/12")),
                piece("), thanks ", None),
                piece("@octo", Some("https://github.com/octo")),
                piece("; not x#3", None),
            ]
        );
        assert_eq!(github_refs("no refs", REPO), vec![piece("no refs", None)]);
        for text in ["#0", "#007", "é#8", "#8é", "ü@octo", "a@b.com"] {
            assert_eq!(github_refs(text, REPO), vec![piece(text, None)], "{text}");
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
        assert!(is_github_hosted(
            "https://github.com/user-attachments/assets/012a2451-fa01-4fe5-8736-33f4a4d179f1"
        ));
        assert!(is_github_hosted("https://camo.githubusercontent.com/abc"));
        assert!(is_github_hosted(
            "https://private-user-images.githubusercontent.com/1/2-x.png?jwt=y"
        ));
        assert!(!is_github_hosted("https://img.shields.io/badge/x"));
        assert!(!is_github_hosted("https://github.com.evil.example/a.png"));
        assert!(!is_github_hosted("http://github.com/a.png"));
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
