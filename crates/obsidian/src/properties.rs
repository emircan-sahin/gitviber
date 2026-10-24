//! A note's properties (its frontmatter) as Obsidian shows them above the text: one row per
//! key, lists as pills. Read from the YAML Obsidian writes (`key: value`, `key: [a, b]`, and
//! `key:` with `- item` lines under it); anything fancier shows as written.

use gitviber_ui::ActiveAppTheme as _;
use gpui_kit::{App, IntoElement, ParentElement as _, SharedString, Styled as _, div, px};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    Text(String),
    List(Vec<String>),
}

fn unquote(s: &str) -> String {
    let s = s.trim();
    for q in ['"', '\''] {
        if s.len() >= 2 && s.starts_with(q) && s.ends_with(q) {
            return s[1..s.len() - 1].to_string();
        }
    }
    s.to_string()
}

/// `[[Note]]` in a value reads as its name, as Obsidian shows it.
fn plain(s: &str) -> String {
    match s.strip_prefix("[[").and_then(|r| r.strip_suffix("]]")) {
        Some(inner) => inner.rsplit('|').next().unwrap_or(inner).to_string(),
        None => s.to_string(),
    }
}

/// The properties in `front` (the block with its `---` lines), in order.
pub fn parse(front: &str) -> Vec<(String, Value)> {
    let mut out: Vec<(String, Value)> = Vec::new();
    let lines: Vec<&str> = front.lines().collect();
    let body = lines.get(1..lines.len().saturating_sub(1)).unwrap_or(&[]);
    for line in body {
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let indented = line.starts_with([' ', '\t']);
        let t = line.trim();
        if let Some(item) = t.strip_prefix("- ").or((t == "-").then_some(""))
            && (indented || t.starts_with('-'))
            && let Some((_, Value::List(items))) = out.last_mut()
        {
            items.push(plain(&unquote(item)));
            continue;
        }
        if indented {
            // A nested map or a folded string: shown as it's written, after the key it's under.
            if let Some((_, Value::Text(v))) = out.last_mut() {
                if !v.is_empty() {
                    v.push(' ');
                }
                v.push_str(t);
            }
            continue;
        }
        let Some((key, value)) = t.split_once(':') else {
            continue;
        };
        let value = value.trim();
        let v = if value.is_empty() {
            Value::List(vec![])
        } else if let Some(inner) = value.strip_prefix('[').and_then(|v| v.strip_suffix(']')) {
            Value::List(
                inner
                    .split(',')
                    .map(|s| plain(&unquote(s)))
                    .filter(|s| !s.is_empty())
                    .collect(),
            )
        } else {
            Value::Text(plain(&unquote(value)))
        };
        out.push((key.trim().to_string(), v));
    }
    out
}

/// The properties as rows above the note.
pub fn element(props: &[(String, Value)], cx: &App) -> impl IntoElement + use<> {
    let t = cx.app_theme();
    let rows = props.iter().map(|(key, value)| {
        let value = match value {
            Value::Text(s) => div()
                .min_w_0()
                .text_color(t.foreground)
                .child(SharedString::from(s.clone())),
            Value::List(items) => {
                div()
                    .flex()
                    .flex_wrap()
                    .gap_1()
                    .children(items.iter().map(|item| {
                        let tag = key == "tags";
                        div()
                            .px_2()
                            .rounded_full()
                            .text_size(px(12.))
                            .bg(t.primary.opacity(0.12))
                            .text_color(if tag { t.primary } else { t.foreground })
                            .child(SharedString::from(if tag {
                                format!("#{item}")
                            } else {
                                item.clone()
                            }))
                    }))
            }
        };
        div()
            .flex()
            .items_start()
            .gap_3()
            .py(px(3.))
            .child(
                div()
                    .w(px(140.))
                    .flex_none()
                    .truncate()
                    .text_color(t.muted_foreground)
                    .child(SharedString::from(key.clone())),
            )
            .child(div().flex_1().min_w_0().child(value))
    });
    div()
        .mt_2()
        .mb_4()
        .pb_3()
        .border_b_1()
        .border_color(t.border)
        .flex()
        .flex_col()
        .text_size(px(13.))
        .child(
            div()
                .pb_1()
                .text_size(px(11.5))
                .text_color(t.subtle)
                .child("Properties"),
        )
        .children(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn obsidians_property_forms() {
        let front = "---\ntags: [daily, \"gitviber\"]\naliases:\n  - Start\n  - Home page\nstatus: draft\nup: \"[[Plan|The plan]]\"\nnested:\n  a: 1\n# a comment\nempty:\n---\n";
        let p = parse(front);
        assert_eq!(
            p,
            vec![
                (
                    "tags".into(),
                    Value::List(vec!["daily".into(), "gitviber".into()])
                ),
                (
                    "aliases".into(),
                    Value::List(vec!["Start".into(), "Home page".into()])
                ),
                ("status".into(), Value::Text("draft".into())),
                ("up".into(), Value::Text("The plan".into())),
                ("nested".into(), Value::List(vec![])),
                ("empty".into(), Value::List(vec![])),
            ]
        );
        assert!(parse("---\n---\n").is_empty());
    }
}
