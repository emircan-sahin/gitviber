//! What a diff's rows are drawn with, shared by the single view and the stacked one: a line's
//! code in its colors with its word changes marked, the gutters, the file header's pieces.

use crate::data::DiffData;
use crate::highlight::{Highlighted, LineStyles};
use crate::model::indent::Shown;
use crate::model::{ADDED, REMOVED, Row, Side, emphasis};
use crate::style::DiffStyle;
use gpui_kit::component::h_flex;
use gpui_kit::{
    AnyElement, Div, HighlightStyle, Hsla, IntoElement, ParentElement as _, Pixels, SharedString,
    Styled as _, StyledText, TextRun, Window, div, font, prelude::FluentBuilder as _, px,
};
use std::ops::Range;

/// What a row of each kind is drawn with.
pub struct Tone {
    pub line: Hsla,
    pub gutter: Hsla,
    pub emph: Option<Hsla>,
    pub sign: &'static str,
}

pub fn tone(k: u8, style: &DiffStyle) -> Tone {
    let c = &style.colors;
    match k {
        ADDED => Tone {
            line: c.add_bg,
            gutter: c.add_gutter,
            emph: Some(c.add_emph),
            sign: "+",
        },
        REMOVED => Tone {
            line: c.del_bg,
            gutter: c.del_gutter,
            emph: Some(c.del_emph),
            sign: "−",
        },
        _ => Tone {
            line: c.background,
            gutter: c.background,
            emph: None,
            sign: "",
        },
    }
}

/// Sizes of one frame's rows.
pub struct Metrics {
    pub line: Pixels,
    pub char_w: Pixels,
    pub number_w: Pixels,
    pub sign_w: Pixels,
    /// How far the code is scrolled sideways.
    pub left: Pixels,
}

impl Metrics {
    /// `lines`: the most lines either side has, for the number column's width.
    pub fn new(window: &Window, style: &DiffStyle, lines: u32, line: Pixels, left: Pixels) -> Self {
        let font_id = window
            .text_system()
            .resolve_font(&font(style.font_family.clone()));
        let char_w = window
            .text_system()
            .ch_advance(font_id, style.font_size)
            .unwrap_or(style.font_size * 0.6);
        let digits = lines.max(1).ilog10() as f32 + 2.;
        Metrics {
            line,
            char_w,
            number_w: char_w * digits + px(6.),
            sign_w: char_w * 2.5,
            left,
        }
    }

    /// How far code `widest` columns wide scrolls sideways in `columns` columns of `width`, each
    /// with `numbers` number gutters.
    pub fn max_left(&self, widest: usize, width: Pixels, columns: f32, numbers: f32) -> Pixels {
        let room = width / columns - self.number_w * numbers - self.sign_w - px(8.);
        self.char_w * widest as f32 + px(24.) - room
    }
}

/// A line's code in its colors, its word changes marked.
pub fn code(
    data: &DiffData,
    hl: &[Option<Highlighted>; 2],
    side: Side,
    row: &Row,
    style: &DiffStyle,
) -> StyledText {
    let line = if side == Side::Old { row.o } else { row.n };
    let text = data.side(side).line(line);
    let shown = Shown::new(&text, data.unit);
    let syntax: Option<LineStyles> = hl[side as usize]
        .as_ref()
        .and_then(|h| h.line(line, &style.syntax));
    let tokens: &[(Range<usize>, HighlightStyle)] = syntax.as_deref().unwrap_or(&[]);
    let emph = if row.k != 0 {
        emphasis::useful(&text, emphasis::utf16_to_bytes(&text, &row.e))
    } else {
        vec![]
    };
    let tone = tone(row.k, style);
    let base = font(style.font_family.clone());
    let mut runs = Vec::new();
    for (range, token, emphasized) in emphasis::cut(text.len(), tokens, &emph) {
        let len = shown.offset(range.end) - shown.offset(range.start);
        if len == 0 {
            continue;
        }
        let mut f = base.clone();
        if let Some(t) = token {
            if let Some(w) = t.font_weight {
                f.weight = w;
            }
            if let Some(s) = t.font_style {
                f.style = s;
            }
        }
        runs.push(TextRun {
            len,
            font: f,
            color: token
                .and_then(|t| t.color)
                .unwrap_or(style.colors.foreground),
            background_color: if emphasized { tone.emph } else { None },
            underline: None,
            strikethrough: None,
        });
    }
    StyledText::new(SharedString::from(shown.text)).with_runs(runs)
}

pub fn number(n: Option<u32>, bg: Hsla, color: Hsla, m: &Metrics) -> Div {
    div()
        .flex_none()
        .w(m.number_w)
        .h_full()
        .pr(px(6.))
        .bg(bg)
        .text_color(color)
        .flex()
        .justify_end()
        .children(n.map(|n| SharedString::from(n.to_string())))
}

pub fn sign(t: &Tone, style: &DiffStyle, m: &Metrics) -> Div {
    div()
        .flex_none()
        .w(m.sign_w)
        .text_color(style.colors.subtle)
        .flex()
        .justify_center()
        .child(t.sign)
}

/// The code, clipped to its column and moved by the sideways scroll.
pub fn code_cell(code: StyledText, m: &Metrics) -> Div {
    div()
        .relative()
        .flex_1()
        .min_w_0()
        .h_full()
        .overflow_hidden()
        .child(
            div()
                .absolute()
                .top_0()
                .left(px(4.) - m.left)
                .h(m.line)
                .whitespace_nowrap()
                .child(code),
        )
}

pub fn overlay(color: Hsla) -> Div {
    div().absolute().top_0().left_0().size_full().bg(color)
}

pub fn gap_label(count: u32) -> String {
    if count == 1 {
        "1 unchanged line".to_string()
    } else {
        format!("{count} unchanged lines")
    }
}

/// A path as the header shows it: its folder dimmed, its name.
pub fn path_label(path: &str, style: &DiffStyle) -> Div {
    let (dir, name) = match path.rsplit_once('/') {
        Some((d, n)) => (format!("{d}/"), n.to_string()),
        None => (String::new(), path.to_string()),
    };
    h_flex()
        .min_w_0()
        .overflow_hidden()
        .child(div().flex_none().text_color(style.colors.subtle).child(dir))
        .child(div().flex_none().child(name))
}

/// "+12 −3", each only when there are some.
pub fn line_counts(additions: u32, deletions: u32, style: &DiffStyle) -> Div {
    let c = &style.colors;
    h_flex()
        .flex_none()
        .gap_1()
        .when(additions > 0, |el| {
            el.child(div().text_color(c.added).child(format!("+{additions}")))
        })
        .when(deletions > 0, |el| {
            el.child(div().text_color(c.removed).child(format!("−{deletions}")))
        })
}

/// The status letter as a small colored pill.
pub fn status_pill(status: &str, style: &DiffStyle) -> AnyElement {
    let c = &style.colors;
    let (label, color) = match status {
        "A" | "?" => ("Added", c.added),
        "D" => ("Deleted", c.removed),
        "R" => ("Renamed", c.primary),
        "C" => ("Copied", c.primary),
        "U" => ("Conflict", c.destructive),
        _ => ("Modified", c.primary),
    };
    div()
        .flex_none()
        .px_1p5()
        .rounded_sm()
        .bg(color.opacity(0.18))
        .text_color(color)
        .text_size(style.ui_font_size - px(1.))
        .child(label)
        .into_any_element()
}
