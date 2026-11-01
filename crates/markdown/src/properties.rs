//! A document's properties (its frontmatter) above the text: one row per key, each with an icon
//! for what its value is, lists as pills, links that open. Read from the YAML Obsidian writes
//! (`key: value`, `key: [a, b]`, `key:` with `- item` lines under it, and items that are maps
//! such as `- url: …` with `title: …` under it); anything fancier shows as written. A repo file's
//! frontmatter shows the same way.

use gitviber_ui::ActiveAppTheme as _;
use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::{
    App, ElementId, FontWeight, InteractiveElement as _, IntoElement, ParentElement as _,
    SharedString, StatefulInteractiveElement as _, Styled as _, div, prelude::FluentBuilder as _,
    px, relative,
};

use crate::reading;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    Text(String),
    List(Vec<Item>),
}

/// A list's item: its text, and the address it opens when it is or has one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub text: String,
    pub link: Option<String>,
}

impl Item {
    fn new(text: String) -> Item {
        let link = is_url(&text).then(|| text.clone());
        Item { text, link }
    }

    /// A map in a list (`- url: …` and `title: …` under it): its title, opening its address.
    fn record(fields: &[(String, String)]) -> Item {
        let named = |names: &[&str]| {
            fields
                .iter()
                .find(|(k, _)| names.contains(&k.as_str()))
                .map(|(_, v)| v.clone())
        };
        let link = named(&["url", "link", "href"])
            .filter(|v| is_url(v))
            .or_else(|| fields.iter().map(|(_, v)| v).find(|v| is_url(v)).cloned());
        let text = named(&["title", "name", "label"])
            .or_else(|| link.clone())
            .unwrap_or_else(|| {
                let pairs: Vec<_> = fields.iter().map(|(k, v)| format!("{k}: {v}")).collect();
                pairs.join(", ")
            });
        Item { text, link }
    }
}

fn is_url(s: &str) -> bool {
    s.starts_with("https://") || s.starts_with("http://")
}

/// `2026-10-29`, with or without a time after it.
fn is_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() >= 10
        && b[..10].iter().enumerate().all(|(i, c)| {
            matches!(i, 4 | 7) == (*c == b'-') && (matches!(i, 4 | 7) || c.is_ascii_digit())
        })
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

/// `key: value` as a map's field, the key a plain word.
fn field(s: &str) -> Option<(String, String)> {
    let (key, value) = s.split_once(':')?;
    let key = key.trim();
    let plain_key = !key.is_empty()
        && key
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '-');
    (plain_key && (value.is_empty() || value.starts_with(' ')))
        .then(|| (key.to_string(), plain(&unquote(value))))
}

/// The properties in `front` (the block with its `---` lines), in order.
pub fn parse(front: &str) -> Vec<(String, Value)> {
    let mut out: Vec<(String, Value)> = Vec::new();
    // The fields of the map the last list item is, while its lines go on.
    let mut record: Option<Vec<(String, String)>> = None;
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
            record = field(item).map(|f| vec![f]);
            items.push(match &record {
                Some(fields) => Item::record(fields),
                None => Item::new(plain(&unquote(item))),
            });
            continue;
        }
        if indented {
            if let (Some(fields), Some((_, Value::List(items)))) = (&mut record, out.last_mut())
                && let Some(f) = field(t)
            {
                fields.push(f);
                if let Some(last) = items.last_mut() {
                    *last = Item::record(fields);
                }
                continue;
            }
            // A nested map or a folded string: shown as it's written, after the key it's under.
            match out.last_mut() {
                Some((_, Value::Text(v))) => {
                    if !v.is_empty() {
                        v.push(' ');
                    }
                    v.push_str(t);
                }
                Some((_, v)) if *v == Value::List(vec![]) => *v = Value::Text(t.to_string()),
                _ => {}
            }
            continue;
        }
        record = None;
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
                    .map(Item::new)
                    .collect(),
            )
        } else {
            Value::Text(plain(&unquote(value)))
        };
        out.push((key.trim().to_string(), v));
    }
    out
}

impl Value {
    /// As it reads: a list's items with commas between.
    pub fn text(&self) -> String {
        match self {
            Value::Text(s) => s.clone(),
            Value::List(items) => {
                let texts: Vec<_> = items.iter().map(|i| i.text.as_str()).collect();
                texts.join(", ")
            }
        }
    }

    /// What the row's icon says the value is.
    fn icon(&self, key: &str) -> IconName {
        match self {
            _ if key == "tags" => IconName::Tag,
            Value::List(_) => IconName::List,
            Value::Text(s) if is_url(s) => IconName::Link,
            Value::Text(s) if is_date(s) => IconName::Calendar,
            Value::Text(s) if s == "true" || s == "false" => IconName::SquareCheck,
            Value::Text(s) if s.parse::<f64>().is_ok() => IconName::Hash,
            Value::Text(_) => IconName::TextAlignStart,
        }
    }
}

/// `https://www.bls.gov/news…` as `bls.gov`, beside a link's title.
fn host(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let host = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    host.strip_prefix("www.").unwrap_or(host)
}

/// The properties as rows above the text, in the interface font a step under the body.
pub fn element(props: &[(String, Value)], cx: &App) -> impl IntoElement + use<> {
    let t = cx.app_theme();
    let size = reading::text_size(cx) * 0.85;
    let ink = t.foreground.opacity(0.85);
    let link_color = t.primary;
    let link = move |id: ElementId, url: String, label: SharedString| {
        div()
            .id(id)
            .min_w_0()
            .truncate()
            .cursor_pointer()
            .text_color(link_color)
            .hover(|d| d.underline())
            .on_click(move |_, _, cx| cx.open_url(&url))
            .child(label)
    };
    let rows = props.iter().enumerate().map(|(row, (key, value))| {
        let value = match value {
            Value::Text(s) if is_url(s) => link(
                ElementId::NamedInteger("property".into(), row as u64),
                s.clone(),
                SharedString::from(s.clone()),
            )
            .into_any_element(),
            Value::Text(s) => div()
                .min_w_0()
                .text_color(ink)
                .child(SharedString::from(s.clone()))
                .into_any_element(),
            // Links, one a line: a pill each would run past the column.
            Value::List(items) if items.iter().any(|i| i.link.is_some()) => div()
                .flex()
                .flex_col()
                .min_w_0()
                .children(items.iter().enumerate().map(|(ix, item)| {
                    let row_el = div().flex().items_baseline().gap_2().min_w_0();
                    match &item.link {
                        Some(url) => row_el
                            .child(link(
                                ElementId::NamedInteger(
                                    format!("property-{row}").into(),
                                    ix as u64,
                                ),
                                url.clone(),
                                SharedString::from(item.text.clone()),
                            ))
                            .when(item.text != *url, |d| {
                                d.child(
                                    div()
                                        .flex_none()
                                        .text_size(size * 0.85)
                                        .text_color(t.subtle)
                                        .child(SharedString::from(host(url).to_string())),
                                )
                            }),
                        None => row_el.child(
                            div()
                                .truncate()
                                .text_color(ink)
                                .child(SharedString::from(item.text.clone())),
                        ),
                    }
                }))
                .into_any_element(),
            Value::List(items) => {
                let tags = key == "tags";
                div()
                    .flex()
                    .flex_wrap()
                    .gap_1()
                    .min_w_0()
                    .children(items.iter().map(|item| {
                        div()
                            .max_w_full()
                            .truncate()
                            .px_2()
                            .rounded_full()
                            .text_size(size * 0.9)
                            .bg(t.primary.opacity(0.12))
                            .text_color(if tags { t.primary } else { ink })
                            .child(SharedString::from(if tags {
                                format!("#{}", item.text)
                            } else {
                                item.text.clone()
                            }))
                    }))
                    .into_any_element()
            }
        };
        div()
            .flex()
            .items_start()
            .gap_3()
            .py(px(3.))
            .child(
                div()
                    .w(size * 9.)
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_color(t.muted_foreground)
                    // The icon sits on the key's first line.
                    .h(size * 1.5)
                    .child(
                        Icon::new(props[row].1.icon(key))
                            .size(size * 0.95)
                            .text_color(t.subtle),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .child(SharedString::from(key.clone())),
                    ),
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
        .font_family(t.fonts.ui.clone())
        .font_weight(FontWeight::NORMAL)
        .text_size(size)
        .line_height(relative(1.5))
        .child(
            div()
                .pb_1()
                .text_size(size * 0.85)
                .text_color(t.subtle)
                .child("Properties"),
        )
        .children(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items(texts: &[&str]) -> Value {
        Value::List(texts.iter().map(|t| Item::new(t.to_string())).collect())
    }

    #[test]
    fn obsidians_property_forms() {
        let front = "---\ntags: [daily, \"gitviber\"]\naliases:\n  - Start\n  - Home page\nstatus: draft\nup: \"[[Plan|The plan]]\"\nnested:\n  a: 1\n# a comment\nempty:\n---\n";
        let p = parse(front);
        assert_eq!(
            p,
            vec![
                ("tags".into(), items(&["daily", "gitviber"])),
                ("aliases".into(), items(&["Start", "Home page"])),
                ("status".into(), Value::Text("draft".into())),
                ("up".into(), Value::Text("The plan".into())),
                ("nested".into(), Value::Text("a: 1".into())),
                ("empty".into(), Value::List(vec![])),
            ]
        );
        assert!(parse("---\n---\n").is_empty());
    }

    /// A list of maps reads as titled links, the key after it a key again.
    #[test]
    fn a_list_of_maps_reads_as_titled_links() {
        let front = "---\nsources:\n  - url: \"https://www.bls.gov/cpi.htm\"\n    title: \"BLS: CPI\"\n  - url: https://example.com/x\n  - note: plain\nlang: tr\n---\n";
        let link = |text: &str, url: &str| Item {
            text: text.into(),
            link: Some(url.into()),
        };
        assert_eq!(
            parse(front),
            vec![
                (
                    "sources".into(),
                    Value::List(vec![
                        link("BLS: CPI", "https://www.bls.gov/cpi.htm"),
                        link("https://example.com/x", "https://example.com/x"),
                        Item {
                            text: "note: plain".into(),
                            link: None
                        },
                    ])
                ),
                ("lang".into(), Value::Text("tr".into())),
            ]
        );
        assert_eq!(host("https://www.bls.gov/cpi.htm"), "bls.gov");
    }

    #[test]
    fn each_kind_of_value_has_its_icon() {
        let text = |s: &str| Value::Text(s.into());
        assert_eq!(text("2026-10-29").icon("date"), IconName::Calendar);
        assert_eq!(text("https://x.com").icon("liveUrl"), IconName::Link);
        assert_eq!(text("12").icon("n"), IconName::Hash);
        assert_eq!(text("true").icon("done"), IconName::SquareCheck);
        assert_eq!(text("draft").icon("status"), IconName::TextAlignStart);
        assert_eq!(items(&["a"]).icon("tags"), IconName::Tag);
        assert_eq!(items(&["a"]).icon("aliases"), IconName::List);
        assert!(!is_date("2026-1-29"));
    }
}
