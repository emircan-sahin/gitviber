//! Where a link in rendered markdown goes, and GitHub's heading anchors.

use std::ops::Range;

/// What following a link does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Link {
    /// `#section`: a heading on this page, by its slug.
    Anchor(String),
    /// http(s): the browser.
    Web(String),
    /// Another scheme (mailto:, vscode:): copied, never opened from here.
    Other(String),
    /// A file in the repo, repo-relative, with the line a `#L12` names.
    File { path: String, line: Option<u32> },
    /// A relative link that climbs out of the repo.
    Outside(String),
}

/// `%XX` decoded where it makes valid UTF-8; the text as written otherwise.
pub fn decode(text: &str) -> String {
    if !text.contains('%') {
        return text.to_string();
    }
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(b) = text
                .get(i + 1..i + 3)
                .and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(b);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| text.to_string())
}

fn has_scheme(href: &str) -> bool {
    let Some((scheme, _)) = href.split_once(':') else {
        return false;
    };
    let mut chars = scheme.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-'))
}

/// `href` written in `from` (a repo-relative file) as a repo-relative path; None when it climbs
/// out of the repo. A leading `/` is the repo's root, as on GitHub.
pub fn resolve(from: &str, href: &str) -> Option<String> {
    let mut parts: Vec<&str> = if href.starts_with('/') {
        vec![]
    } else {
        from.split('/').collect()
    };
    if !href.starts_with('/') {
        parts.pop();
    }
    let path = href.split(['?', '#']).next().unwrap_or("");
    let decoded = decode(path);
    let mut out: Vec<String> = parts.into_iter().map(str::to_string).collect();
    for seg in decoded.split('/') {
        match seg {
            ".." => {
                out.pop()?;
            }
            "" | "." => {}
            seg => out.push(seg.to_string()),
        }
    }
    Some(out.join("/"))
}

/// The line a GitHub-style fragment names: `#L12`, `#L12-L20`, `#L12C3`.
pub fn fragment_line(href: &str) -> Option<u32> {
    let frag = href.split_once('#')?.1;
    let digits: String = frag
        .strip_prefix('L')?
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().ok().filter(|n| *n > 0)
}

/// What `href`, written in the file `from` (repo-relative; None when the text isn't a file),
/// points at.
pub fn classify(from: Option<&str>, href: &str) -> Link {
    if let Some(anchor) = href.strip_prefix('#') {
        return Link::Anchor(decode(anchor));
    }
    let lower = href.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return Link::Web(href.to_string());
    }
    if has_scheme(href) {
        return Link::Other(href.to_string());
    }
    match resolve(from.unwrap_or(""), href) {
        Some(path) if !path.is_empty() => Link::File {
            path,
            line: fragment_line(href),
        },
        Some(_) => Link::Anchor(String::new()),
        None => Link::Outside(href.to_string()),
    }
}

/// GitHub's anchor for a heading: lowercase, punctuation dropped, spaces as dashes.
pub fn slug(heading: &str) -> String {
    heading
        .trim()
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace() || *c == '_' || *c == '-')
        .map(|c| if c.is_whitespace() { '-' } else { c })
        .collect()
}

/// A heading's plain text from its source line: markup characters GitHub drops from the slug
/// anyway, and links reduced to their text.
fn heading_text(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            // [text](url): keep the text, skip the url.
            ']' if chars.peek() == Some(&'(') => {
                for c in chars.by_ref() {
                    if c == ')' {
                        break;
                    }
                }
            }
            '[' | ']' | '`' | '*' => {}
            c => out.push(c),
        }
    }
    out
}

/// The source byte range of the ATX heading (`## Title`) whose slug is `anchor`, outside code
/// fences; repeated headings get `-1`, `-2`… as on GitHub.
pub fn heading_range(source: &str, anchor: &str) -> Option<Range<usize>> {
    let want = slug(anchor);
    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut fence: Option<&str> = None;
    let mut at = 0;
    for line in source.split_inclusive('\n') {
        let start = at;
        at += line.len();
        let trimmed = line.trim_start();
        let marker = if trimmed.starts_with("```") {
            Some("```")
        } else if trimmed.starts_with("~~~") {
            Some("~~~")
        } else {
            None
        };
        if let Some(m) = marker {
            match fence {
                Some(open) if open == m => fence = None,
                None => fence = Some(m),
                _ => {}
            }
            continue;
        }
        if fence.is_some() || line.len() - trimmed.len() > 3 {
            continue;
        }
        let hashes = trimmed.chars().take_while(|c| *c == '#').count();
        if !(1..=6).contains(&hashes) {
            continue;
        }
        let rest = &trimmed[hashes..];
        if !(rest.starts_with([' ', '\t']) || rest.trim().is_empty()) {
            continue;
        }
        let text = rest.trim().trim_end_matches('#').trim_end();
        let base = slug(&heading_text(text));
        let n = seen.entry(base.clone()).or_insert(0);
        let id = if *n == 0 {
            base.clone()
        } else {
            format!("{base}-{n}")
        };
        *n += 1;
        if id == want {
            let end = start + line.trim_end_matches(['\n', '\r']).len();
            return Some(start..end);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_links_resolve_against_the_files_folder() {
        assert_eq!(
            resolve("docs/guide/a.md", "b.md").as_deref(),
            Some("docs/guide/b.md")
        );
        assert_eq!(
            resolve("docs/guide/a.md", "../img/x.png").as_deref(),
            Some("docs/img/x.png")
        );
        assert_eq!(
            resolve("docs/a.md", "./c%20d.md#top").as_deref(),
            Some("docs/c d.md")
        );
        assert_eq!(
            resolve("docs/a.md", "/README.md").as_deref(),
            Some("README.md")
        );
        assert_eq!(resolve("README.md", "../outside.md"), None);
    }

    #[test]
    fn links_are_told_apart() {
        assert_eq!(
            classify(Some("a.md"), "#Install"),
            Link::Anchor("Install".into())
        );
        assert_eq!(
            classify(Some("a.md"), "https://x.dev"),
            Link::Web("https://x.dev".into())
        );
        assert_eq!(
            classify(Some("a.md"), "mailto:me@x.dev"),
            Link::Other("mailto:me@x.dev".into())
        );
        assert_eq!(
            classify(Some("docs/a.md"), "../src/main.rs#L12"),
            Link::File {
                path: "src/main.rs".into(),
                line: Some(12)
            }
        );
        assert_eq!(classify(Some("a.md"), "../x"), Link::Outside("../x".into()));
    }

    #[test]
    fn headings_slug_as_on_github() {
        assert_eq!(slug("Hello, World!"), "hello-world");
        assert_eq!(slug("API `v2` — notes"), "api-v2--notes");
        let src = "# Title\n\n```\n# not a heading\n```\n## Install it\n## Install it\n";
        let range = heading_range(src, "install-it-1").unwrap();
        assert_eq!(&src[range], "## Install it");
        assert_eq!(heading_range(src, "not-a-heading"), None);
        assert_eq!(&src[heading_range(src, "title").unwrap()], "# Title");
        let linked = "## See [the docs](x.md)\n";
        assert!(heading_range(linked, "see-the-docs").is_some());
    }
}
