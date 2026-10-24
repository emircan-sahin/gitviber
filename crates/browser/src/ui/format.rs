//! What goes to an agent from a browser tab: picked elements, or the page's errors. Plain text,
//! short, in the order an agent would look. And how a download reads under the address.

use crate::console::Entry;
use serde::Deserialize;
use std::collections::BTreeMap;

/// An element the picker picked (scripts/picker.js), as the backend reports it.
#[derive(Deserialize, Clone, Debug, PartialEq)]
pub struct Pick {
    pub selector: String,
    pub tag: String,
    pub html: String,
    pub text: String,
    #[serde(rename = "box")]
    pub bounds: crate::picks::Bounds,
    #[serde(default)]
    pub styles: BTreeMap<String, String>,
    #[serde(default)]
    pub components: Vec<String>,
    #[serde(default)]
    pub screenshot: Option<String>,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub more: bool,
    #[serde(default)]
    pub number: u32,
}

/// The device a page was shown as, for an agent reading a pick.
#[derive(Clone, Debug, PartialEq)]
pub struct ShownAs {
    pub name: String,
    pub viewport: (f64, f64),
}

/// What the page wrote, on one line: a newline in it can't start a line an agent reads as ours.
fn flat(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// 143.765625px reads as 143.77px.
fn rounded(v: &str) -> String {
    let mut out = String::new();
    let mut rest = v;
    while let Some(at) = rest.find("px") {
        let (before, after) = rest.split_at(at);
        let start = before
            .char_indices()
            .rev()
            .take_while(|(_, c)| c.is_ascii_digit() || *c == '.' || *c == '-')
            .last()
            .map_or(before.len(), |(i, _)| i);
        let number = &before[start..];
        out.push_str(&before[..start]);
        match number.parse::<f64>() {
            Ok(n) if number.contains('.') => {
                let r = (n * 100.0).round() / 100.0;
                out.push_str(&format!("{r}"));
            }
            _ => out.push_str(number),
        }
        out.push_str("px");
        rest = &after[2..];
    }
    out.push_str(rest);
    out
}

/// The page's address, and the device it showed as: for the agent to see it the same way.
fn page_of(url: &str, device: Option<&ShownAs>) -> String {
    match device {
        Some(d) => format!(
            "{} as {} ({}×{})",
            flat(url),
            d.name,
            d.viewport.0.round(),
            d.viewport.1.round()
        ),
        None => flat(url),
    }
}

/// The element, where it is and what it looks like, the screenshot's path, then the note.
pub fn format_pick(pick: &Pick, note: &str, device: Option<&ShownAs>) -> String {
    let styles = pick
        .styles
        .iter()
        .filter(|(_, v)| !v.is_empty() && !["none", "normal", "auto", "0px"].contains(&v.as_str()))
        .map(|(k, v)| format!("{}: {}", flat(k), rounded(&flat(v))))
        .collect::<Vec<_>>()
        .join("; ");
    let b = pick.bounds;
    let mut lines = vec![
        format!("Picked an element on {}", page_of(&pick.url, device)),
        format!("- selector: {}", flat(&pick.selector)),
    ];
    if !pick.components.is_empty() {
        let names: Vec<String> = pick.components.iter().map(|c| flat(c)).collect();
        lines.push(format!("- React: {}", names.join(" < ")));
    }
    if !pick.text.is_empty() {
        let quoted = serde_json::to_string(&pick.text).unwrap_or_default();
        lines.push(format!("- text: {quoted}"));
    }
    lines.push(format!(
        "- box: {}×{} at {},{}",
        b.w.round(),
        b.h.round(),
        b.x.round(),
        b.y.round()
    ));
    if !styles.is_empty() {
        lines.push(format!("- styles: {styles}"));
    }
    lines.push(format!("- html: {}", flat(&pick.html)));
    if let Some(shot) = pick.screenshot.as_deref().filter(|s| !s.is_empty()) {
        lines.push(format!("- screenshot: {shot}"));
    }
    if !note.trim().is_empty() {
        lines.push(format!("Note: {}", note.trim()));
    }
    lines.join("\n")
}

/// A few elements ⇧-clicked together, each as format_pick has it, under the page they're on.
pub fn format_picks(picks: &[Pick], device: Option<&ShownAs>) -> String {
    if let [one] = picks {
        return format_pick(one, "", device);
    }
    let Some(first) = picks.first() else {
        return String::new();
    };
    let mut out = vec![format!(
        "Picked {} elements on {}",
        picks.len(),
        page_of(&first.url, device)
    )];
    for (i, p) in picks.iter().enumerate() {
        let body = format_pick(p, "", device);
        let mut lines = vec![format!("Element {}:", i + 1)];
        lines.extend(body.lines().skip(1).map(str::to_string));
        out.push(lines.join("\n"));
    }
    out.join("\n")
}

/// Stack lines kept under each error: where it was thrown, and a little of how it got there.
const STACK_LINES: usize = 3;
/// Errors sent at most, the latest.
pub const ERRORS: usize = 20;

/// The page's latest errors, each once with how often, oldest first; empty when there are none.
pub fn format_errors(entries: &[Entry], page: Option<(&str, Option<&ShownAs>)>) -> String {
    // Each message once, in the order of its latest time.
    let mut seen: Vec<(&Entry, usize)> = Vec::new();
    for e in entries.iter().filter(|e| e.level == "error") {
        let times = match seen.iter().position(|(was, _)| was.msg == e.msg) {
            Some(at) => seen.remove(at).1,
            None => 0,
        };
        seen.push((e, times + 1));
    }
    let latest = &seen[seen.len().saturating_sub(ERRORS)..];
    if latest.is_empty() {
        return String::new();
    }
    let mut pages: Vec<String> = latest.iter().map(|(e, _)| flat(&e.url)).collect();
    pages.sort();
    pages.dedup();
    let head = match page {
        Some((url, device)) => format!("Errors from the page at {}:", page_of(url, device)),
        None if pages.len() == 1 => format!("Errors on {}:", pages[0]),
        None => "Errors from the page:".into(),
    };
    let mut out = vec![head];
    for (entry, times) in latest {
        let stack: Vec<String> = entry
            .stack
            .lines()
            .map(flat)
            .filter(|l| !l.is_empty() && !entry.msg.contains(l.as_str()))
            .take(STACK_LINES)
            .map(|l| format!("    {l}"))
            .collect();
        // Its own page whenever that isn't the one named above.
        let elsewhere = match page {
            Some((url, _)) => flat(&entry.url) != flat(url),
            None => pages.len() > 1,
        };
        let where_ = if elsewhere {
            format!(" ({})", flat(&entry.url))
        } else {
            String::new()
        };
        let count = if *times > 1 {
            format!("(×{times}) ")
        } else {
            String::new()
        };
        let mut lines = vec![format!("- {count}{}{where_}", flat(&entry.msg))];
        lines.extend(stack);
        out.push(lines.join("\n"));
    }
    out.join("\n")
}

/// What a tab hands its worktree's agent: picked elements, or the page's errors.
#[derive(Clone, Debug, PartialEq)]
pub struct AskAbout {
    /// As the question's label names it: "SaveButton <button>", "3 elements", "these errors".
    pub what: String,
    /// Shown above the question: what goes along with it.
    pub summary: String,
    pub content: String,
    /// The agent session's name.
    pub name: String,
}

fn label(p: &Pick) -> String {
    match p.components.first() {
        Some(c) => format!("{c} <{}>", p.tag),
        None => format!("<{}>", p.tag),
    }
}

/// Picked elements as the question names them: one by its components, tag and text.
pub fn picks_about(picks: &[Pick], device: Option<&ShownAs>) -> Option<AskAbout> {
    let one = picks.first()?;
    let what = if picks.len() == 1 {
        label(one)
    } else {
        format!("{} elements", picks.len())
    };
    let summary = if picks.len() == 1 {
        let text = if one.text.is_empty() {
            String::new()
        } else if one.text.chars().count() > 40 {
            format!(" \"{}…\"", one.text.chars().take(39).collect::<String>())
        } else {
            format!(" \"{}\"", one.text)
        };
        let components = if one.components.is_empty() {
            String::new()
        } else {
            format!("{} · ", one.components.join(" < "))
        };
        format!("{components}<{}>{text}", one.tag)
    } else {
        picks.iter().map(label).collect::<Vec<_>>().join(" · ")
    };
    Some(AskAbout {
        name: format!("Browser: {what}"),
        what,
        summary,
        content: format_picks(picks, device),
    })
}

/// The page's errors (format_errors), as the question names them.
pub fn errors_about(errors: &str) -> AskAbout {
    let count = errors.lines().filter(|l| l.starts_with("- ")).count();
    AskAbout {
        what: "these errors".into(),
        summary: format!(
            "{count} {} from the page's console",
            if count == 1 { "error" } else { "errors" }
        ),
        content: errors.into(),
        name: "Browser: console errors".into(),
    }
}

pub fn format_bytes(n: i64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = n as f64;
    let mut i = 0;
    // 1023.6 KB would round to "1024 KB": past 1023.5 it's the next unit's.
    while v >= if i == 0 { 1024.0 } else { 1023.5 } && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        return format!("{n} B");
    }
    if v < 9.95 {
        format!("{v:.1} {}", UNITS[i])
    } else {
        format!("{v:.0} {}", UNITS[i])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pick() -> Pick {
        Pick {
            selector: "#save".into(),
            tag: "button".into(),
            html: "<button id=\"save\"\n  class=\"btn\">Save</button>".into(),
            text: "Save".into(),
            bounds: crate::picks::Bounds {
                x: 10.4,
                y: 20.6,
                w: 80.0,
                h: 30.0,
            },
            styles: [
                ("display", "inline-flex"),
                ("margin", "0px"),
                ("font-weight", "600"),
                ("width", "143.765625px"),
            ]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
            components: vec!["SaveButton".into(), "Form".into(), "App".into()],
            screenshot: Some("/tmp/picks/pick-1.png".into()),
            url: "http://localhost:5173/settings".into(),
            more: false,
            number: 1,
        }
    }

    #[test]
    fn a_pick_reads_as_where_what_how_it_looks_its_picture_then_the_note() {
        let device = ShownAs {
            name: "iPhone 16 Pro".into(),
            viewport: (402.0, 778.0),
        };
        assert_eq!(
            format_pick(&pick(), "  make it blue  ", Some(&device)),
            [
                "Picked an element on http://localhost:5173/settings as iPhone 16 Pro (402×778)",
                "- selector: #save",
                "- React: SaveButton < Form < App",
                "- text: \"Save\"",
                "- box: 80×30 at 10,21",
                "- styles: display: inline-flex; font-weight: 600; width: 143.77px",
                "- html: <button id=\"save\" class=\"btn\">Save</button>",
                "- screenshot: /tmp/picks/pick-1.png",
                "Note: make it blue",
            ]
            .join("\n")
        );
        let bare = Pick {
            components: vec![],
            text: String::new(),
            screenshot: None,
            styles: BTreeMap::new(),
            ..pick()
        };
        assert_eq!(
            format_pick(&bare, " ", None),
            [
                "Picked an element on http://localhost:5173/settings",
                "- selector: #save",
                "- box: 80×30 at 10,21",
                "- html: <button id=\"save\" class=\"btn\">Save</button>",
            ]
            .join("\n")
        );
    }

    fn entry(level: &str, msg: &str, url: &str) -> Entry {
        Entry {
            level: level.into(),
            msg: msg.into(),
            stack: String::new(),
            url: url.into(),
            ts: 0.0,
            frame: None,
        }
    }

    #[test]
    fn errors_go_once_each_with_how_often_latest_last() {
        let page = "http://localhost:5173/";
        let entries = [
            entry("error", "a", page),
            entry("warn", "w", page),
            entry("error", "b", page),
            entry("error", "a", page),
            entry("error", "c", "http://localhost:5173/old"),
        ];
        assert_eq!(
            format_errors(&entries, Some((page, None))),
            "Errors from the page at http://localhost:5173/:\n- b\n- (×2) a\n- c (http://localhost:5173/old)"
        );
        assert_eq!(format_errors(&entries[1..2], None), "");
        assert_eq!(
            errors_about("x\n- a\n- b").summary,
            "2 errors from the page's console"
        );
    }

    #[test]
    fn bytes_read_in_their_unit() {
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1536), "1.5 KB");
        assert_eq!(format_bytes(1024 * 1023 + 700), "1.0 MB");
        assert_eq!(format_bytes(20 * 1024 * 1024), "20 MB");
    }
}
