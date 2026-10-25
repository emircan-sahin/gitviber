//! Markdown beyond what gpui-component draws: GitHub's alerts and Obsidian's callouts
//! (`> [!NOTE]`), and which blocks are diagrams or math (drawn in diagrams.rs; their source shows
//! under a label until then). The parsing is plain functions on the source, so tests run
//! without a window.

use gpui_kit::component::text::markdown_ast as mdast;

/// GitHub's five alert types.
pub const GITHUB_ALERTS: [&str; 5] = ["note", "tip", "important", "warning", "caution"];

/// Obsidian's callout aliases, by the type each draws as.
const ALIASES: &[(&str, &str)] = &[
    ("summary", "abstract"),
    ("tldr", "abstract"),
    ("hint", "tip"),
    ("important", "tip"),
    ("check", "success"),
    ("done", "success"),
    ("help", "question"),
    ("faq", "question"),
    ("caution", "warning"),
    ("attention", "warning"),
    ("fail", "failure"),
    ("missing", "failure"),
    ("error", "danger"),
    ("cite", "quote"),
];

/// Which markdown a document is: what its blockquotes can be, and what else it reads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Callouts {
    /// A pull request's or an issue's text, as GitHub shows it: its five alerts (`[!TYPE]` alone
    /// on the first line), no frontmatter, and images only from GitHub's own hosts.
    GitHub,
    /// A file in a repo: GitHub's alerts, and frontmatter as its properties.
    #[default]
    Repo,
    /// An Obsidian note's: any type, a title after it, `+`/`-` folding.
    Obsidian,
}

/// A blockquote that opens with a type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Callout {
    /// "gh-note".. for GitHub's alerts, Obsidian's type (aliases folded in) otherwise.
    pub kind: String,
    pub title: String,
    /// The quote's markdown after the type line, its `>` taken off.
    pub body: String,
    /// "+" or "-" (Obsidian's folding), "" when it doesn't fold.
    pub fold: String,
}

fn title_case(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| {
            f.to_uppercase()
                .chain(c)
                .collect::<String>()
                .replace('-', " ")
        })
        .unwrap_or_default()
}

/// The quote's lines with their `>` marker (and one space after it) taken off.
fn unquote(source: &str) -> Vec<&str> {
    source
        .lines()
        .map(|line| {
            let t = line.trim_start();
            match t.strip_prefix('>') {
                Some(rest) => rest.strip_prefix(' ').unwrap_or(rest),
                None => line,
            }
        })
        .collect()
}

/// A blockquote's source as a callout, if its first line names one.
pub fn parse_callout(quote_source: &str, flavor: Callouts) -> Option<Callout> {
    let lines = unquote(quote_source);
    let first = lines.first()?.trim_start();
    let rest = first.strip_prefix("[!")?;
    let close = rest.find(']')?;
    let inside = &rest[..close];
    // [!type|metadata]
    let written = inside.split('|').next()?.trim();
    if written.is_empty()
        || !written
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    {
        return None;
    }
    let written = written.to_lowercase();
    let mut after = &rest[close + 1..];
    let fold = match after.chars().next() {
        Some(c @ ('+' | '-')) => {
            after = &after[1..];
            c.to_string()
        }
        _ => String::new(),
    };
    let title_text = after.trim();
    let body = lines[1..].join("\n");
    match flavor {
        Callouts::GitHub | Callouts::Repo => {
            if !GITHUB_ALERTS.contains(&written.as_str())
                || !fold.is_empty()
                || !title_text.is_empty()
            {
                return None;
            }
            Some(Callout {
                kind: format!("gh-{written}"),
                title: title_case(&written),
                body,
                fold,
            })
        }
        Callouts::Obsidian => {
            let kind = ALIASES
                .iter()
                .find(|(a, _)| *a == written)
                .map(|(_, k)| k.to_string())
                .unwrap_or(written.clone());
            let title = if title_text.is_empty() {
                title_case(&written)
            } else {
                title_text.to_string()
            };
            Some(Callout {
                kind,
                title,
                body,
                fold,
            })
        }
    }
}

/// An HTML block holding a `<details>`, GitHub's collapsed section: shut unless it says `open`,
/// its summary a line that opens it. The HTML around it is drawn above and below.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Details {
    pub before: String,
    /// The summary's HTML, as a paragraph; "Details" when it has none, as browsers say.
    pub summary: String,
    /// What it hides: markdown and HTML.
    pub body: String,
    pub after: String,
    pub open: bool,
}

/// Where the tag `name` (lowercase) next opens or closes in `lower` from `from`: (at, closing).
fn next_tag(lower: &str, name: &str, from: usize) -> Option<(usize, bool)> {
    let mut at = from;
    while let Some(i) = lower[at..].find('<') {
        let start = at + i;
        let rest = &lower[start + 1..];
        let (closing, rest) = match rest.strip_prefix('/') {
            Some(r) => (true, r),
            None => (false, rest),
        };
        if rest.strip_prefix(name).is_some_and(|r| {
            r.chars()
                .next()
                .is_none_or(|c| c.is_ascii_whitespace() || c == '>' || c == '/')
        }) {
            return Some((start, closing));
        }
        at = start + 1;
    }
    None
}

/// The end of the tag starting at `at`: past its `>`, or the end.
fn tag_end(html: &str, at: usize) -> usize {
    html[at..].find('>').map_or(html.len(), |i| at + i + 1)
}

/// The first `<details>` element in an HTML block, and what's around it.
pub fn parse_details(html: &str) -> Option<Details> {
    let lower = html.to_ascii_lowercase();
    let (start, _) = std::iter::successors(next_tag(&lower, "details", 0), |(at, _)| {
        next_tag(&lower, "details", at + 1)
    })
    .find(|(_, closing)| !closing)?;
    let open_end = tag_end(html, start);
    let open = lower[start + "<details".len()..open_end]
        .trim_end_matches(['>', '/'])
        .split(|c: char| c.is_ascii_whitespace())
        .any(|attr| attr == "open" || attr.starts_with("open="));
    // Its own closing tag, past any nested ones; unclosed, it runs to the end.
    let mut depth = 0usize;
    let mut at = open_end;
    let (close, close_end) = loop {
        match next_tag(&lower, "details", at) {
            Some((i, true)) if depth == 0 => break (i, tag_end(html, i)),
            Some((i, closing)) => {
                if closing {
                    depth -= 1;
                } else {
                    depth += 1;
                }
                at = i + 1;
            }
            None => break (html.len(), html.len()),
        }
    };
    let inner = &html[open_end..close];
    let lead = inner.len() - inner.trim_start().len();
    let summary = next_tag(&lower[open_end..close], "summary", 0)
        .filter(|(i, closing)| *i == lead && !closing)
        .map(|(i, _)| {
            let text_start = tag_end(inner, i);
            let text_end = next_tag(&lower[open_end..close], "summary", text_start)
                .filter(|(_, closing)| *closing)
                .map_or(inner.len(), |(e, _)| e);
            (
                text_start,
                text_end,
                tag_end(inner, text_end).min(inner.len()),
            )
        });
    let (summary, body) = match summary {
        Some((s, e, past)) => (inner[s..e].trim(), &inner[past..]),
        None => ("", inner),
    };
    let summary = if summary.is_empty() {
        "Details"
    } else {
        summary
    };
    Some(Details {
        before: html[..start].to_string(),
        summary: format!("<p>{}</p>", summary.replace(['\n', '\r'], " ")),
        body: body.to_string(),
        after: html[close_end..].to_string(),
        open,
    })
}

/// HTML's text with its tags taken out, for find.
pub fn html_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A diagram or a formula: its picture once drawn, its source under a label until then (and
/// when it can't be drawn).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceBlock {
    pub kind: DiagramKind,
    pub source: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiagramKind {
    Mermaid,
    Math,
}

impl DiagramKind {
    pub fn label(self) -> &'static str {
        match self {
            DiagramKind::Mermaid => "Mermaid diagram",
            DiagramKind::Math => "Math",
        }
    }
}

/// Mermaid fences, `$$` math and ```math fences: drawn as pictures (diagrams.rs).
pub fn parse_source_block(node: &mdast::Node) -> Option<SourceBlock> {
    match node {
        mdast::Node::Code(code)
            if code
                .lang
                .as_deref()
                .is_some_and(|l| l.eq_ignore_ascii_case("mermaid")) =>
        {
            Some(SourceBlock {
                kind: DiagramKind::Mermaid,
                source: code.value.clone(),
            })
        }
        mdast::Node::Code(code)
            if code
                .lang
                .as_deref()
                .is_some_and(|l| l.eq_ignore_ascii_case("math")) =>
        {
            Some(SourceBlock {
                kind: DiagramKind::Math,
                source: code.value.clone(),
            })
        }
        mdast::Node::Math(math) => Some(SourceBlock {
            kind: DiagramKind::Math,
            source: math.value.clone(),
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn github_alerts_need_their_type_alone_on_the_line() {
        let c = parse_callout("> [!NOTE]\n> Useful **info**.\n> More.", Callouts::GitHub).unwrap();
        assert_eq!(c.kind, "gh-note");
        assert_eq!(c.title, "Note");
        assert_eq!(c.body, "Useful **info**.\nMore.");
        assert!(parse_callout("> [!WARNING]\n> x", Callouts::GitHub).is_some());
        assert!(parse_callout("> [!NOTE] Title\n> x", Callouts::GitHub).is_none());
        assert!(parse_callout("> [!BUG]\n> x", Callouts::GitHub).is_none());
        assert!(parse_callout("> Just a quote", Callouts::GitHub).is_none());
    }

    #[test]
    fn obsidian_callouts_take_titles_aliases_and_folding() {
        let c = parse_callout("> [!faq]- Why?\n> Because.", Callouts::Obsidian).unwrap();
        assert_eq!(
            (
                c.kind.as_str(),
                c.title.as_str(),
                c.fold.as_str(),
                c.body.as_str()
            ),
            ("question", "Why?", "-", "Because.")
        );
        let c = parse_callout("> [!my-type|wide]\n> x", Callouts::Obsidian).unwrap();
        assert_eq!((c.kind.as_str(), c.title.as_str()), ("my-type", "My type"));
    }

    #[test]
    fn details_split_into_summary_body_and_what_is_around() {
        let d = parse_details(
            "<p>x</p>\n<DETAILS Open>\n<summary>Commits <b>3</b></summary>\n<ul><li>a</li></ul>\n<details><summary>In</summary>i</details>\n</details>\n<br>",
        )
        .unwrap();
        assert_eq!(
            d,
            Details {
                before: "<p>x</p>\n".into(),
                summary: "<p>Commits <b>3</b></p>".into(),
                body: "\n<ul><li>a</li></ul>\n<details><summary>In</summary>i</details>\n".into(),
                after: "\n<br>".into(),
                open: true,
            }
        );
        // No summary: the browser's word; never closed: to the end.
        let d = parse_details("<details>\nbody").unwrap();
        assert_eq!(
            (d.summary.as_str(), d.body.as_str(), d.open),
            ("<p>Details</p>", "\nbody", false)
        );
        assert!(parse_details("<detailsx>no</detailsx>").is_none());
        assert_eq!(html_text("<p>a <b>b</b>\n c</p>"), "a b c");
    }

    #[test]
    fn nested_quote_lines_keep_their_inner_marker() {
        let c = parse_callout("> [!TIP]\n> > inner\n> text", Callouts::GitHub).unwrap();
        assert_eq!(c.body, "> inner\ntext");
    }
}

/// Inline math as GitHub and Obsidian read it: `$` with no space inside either end and no digit
/// right after the closing one, so "$5 and $10" stays text. `after`: the source past the node.
pub fn is_inline_math(value: &str, after: &str) -> bool {
    !value.is_empty()
        && !value.starts_with(char::is_whitespace)
        && !value.ends_with(char::is_whitespace)
        && !after.starts_with(|c: char| c.is_ascii_digit())
}

#[cfg(test)]
mod math_tests {
    use super::is_inline_math;

    #[test]
    fn dollars_in_prose_stay_text() {
        assert!(is_inline_math("x^2", " and more"));
        assert!(!is_inline_math("5 and ", "10"));
        assert!(!is_inline_math(" x", ""));
        assert!(!is_inline_math("x", "5"));
    }
}
