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

/// Which callouts a blockquote can be.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Callouts {
    /// GitHub's five, `[!TYPE]` alone on the first line.
    #[default]
    GitHub,
    /// Obsidian's: any type, a title after it, `+`/`-` folding.
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
        Callouts::GitHub => {
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
