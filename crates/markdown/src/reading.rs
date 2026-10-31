//! Reading typography, for a document a whole pane shows (a file's or a diff's preview, a vault
//! note): body text sized from the code font, a column of comfortable width, and keys that zoom
//! the text on its own. Text inside other pages (a pull request, a comment) keeps the interface's
//! size.

use gitviber_ui::Settings;
use gitviber_ui::keymap::markdown;
use gpui_kit::base::text::TextViewStyle;
use gpui_kit::{
    AnyElement, App, InteractiveElement, IntoElement, ParentElement as _, Pixels, StyleRefinement,
    Styled, div, px, relative,
};

/// A reading view's key context: a rendered document's (scroll and find keys), and the one the
/// zoom keys bind in.
pub const KEY_CONTEXT: &str = "MarkdownView MarkdownReading";

/// Where the zoom keys bind, for a place beside a reading view that's part of it (its find bar).
pub const ZOOM_CONTEXT: &str = "MarkdownReading";

/// The body over the code font: inline code, drawn at 7/8 of the body, comes out at the code
/// font's own size, and changing the code font moves the reading along.
const BODY_PER_CODE: f32 = 8. / 7.;

const LINE_HEIGHT: f32 = 1.6;

/// The column in ems of the body: about 75 characters of prose a line, at any zoom.
const MEASURE_EM: f32 = 40.;

/// Code blocks, tables and diagrams get this much: 860px at the default size.
const WIDE_EM: f32 = 56.;

/// The body text's size: 15.4px at the default 13.5px code font, before any zoom.
pub fn text_size(cx: &App) -> Pixels {
    px(Settings::get(cx).reading_code_size() * BODY_PER_CODE)
}

/// The widest prose gets.
pub fn measure(cx: &App) -> Pixels {
    text_size(cx) * MEASURE_EM
}

/// The widest a code block, a table or a diagram gets, past the prose on both sides.
pub fn wide(cx: &App) -> Pixels {
    text_size(cx) * WIDE_EM
}

/// `el` at most [`measure`] wide, centered in what it's given.
pub fn in_measure(el: impl IntoElement, cx: &App) -> AnyElement {
    div()
        .w_full()
        .flex()
        .justify_center()
        .child(div().w_full().min_w_0().max_w(measure(cx)).child(el))
        .into_any_element()
}

/// The body's size and line height on `el`, which its text inherits.
pub fn typography<E: Styled>(el: E, cx: &App) -> E {
    el.text_size(text_size(cx))
        .line_height(relative(LINE_HEIGHT))
}

/// A reading view's text style: prose at [`measure`], code blocks at the size its inline code is.
pub fn style(cx: &App) -> TextViewStyle {
    let code = text_size(cx) / BODY_PER_CODE;
    TextViewStyle::from_theme(&gpui_kit::base::Theme::global(cx))
        .with_measure(measure(cx))
        .with_code_block(StyleRefinement::default().text_size(code))
}

/// The zoom keys (bound in [`KEY_CONTEXT`]) step the reading size, which persists; the code
/// editors and diffs keep theirs.
pub fn zoom_keys<E: InteractiveElement>(el: E) -> E {
    el.on_action(|_: &markdown::ZoomIn, _, cx| Settings::step_reading_size(cx, 1))
        .on_action(|_: &markdown::ZoomOut, _, cx| Settings::step_reading_size(cx, -1))
        .on_action(|_: &markdown::ZoomReset, _, cx| Settings::step_reading_size(cx, 0))
}
