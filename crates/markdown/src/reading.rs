//! Reading typography, for a document a whole pane shows (a file's or a diff's preview, a vault
//! note): body text sized from the code font, a column of comfortable width, and keys that zoom
//! the text on its own. Text inside other pages (a pull request, a comment) keeps the interface's
//! size.

use gitviber_ui::Settings;
use gitviber_ui::keymap::markdown;
use gpui_kit::base::text::TextViewStyle;
use gpui_kit::{App, InteractiveElement, Pixels, StyleRefinement, Styled, px, relative};

/// A reading view's key context: a rendered document's (scroll and find keys), and the one the
/// zoom keys bind in.
pub const KEY_CONTEXT: &str = "MarkdownView MarkdownReading";

/// The body over the code font: inline code, drawn at 7/8 of the body, comes out at the code
/// font's own size, and changing the code font moves the reading along.
const BODY_PER_CODE: f32 = 8. / 7.;

const LINE_HEIGHT: f32 = 1.6;

/// The column in ems of the body: about 75 characters of prose a line, at any zoom.
const MEASURE_EM: f32 = 40.;

/// The body text's size: 15.4px at the default 13.5px code font, before any zoom.
pub fn text_size(cx: &App) -> Pixels {
    px(Settings::get(cx).reading_code_size() * BODY_PER_CODE)
}

/// The widest the text column gets.
pub fn measure(cx: &App) -> Pixels {
    text_size(cx) * MEASURE_EM
}

/// The body's size and line height on `el`, which its text inherits.
pub fn typography<E: Styled>(el: E, cx: &App) -> E {
    el.text_size(text_size(cx))
        .line_height(relative(LINE_HEIGHT))
}

/// A reading view's text style: its code blocks at the size its inline code is.
pub fn style(cx: &App) -> TextViewStyle {
    let code = text_size(cx) / BODY_PER_CODE;
    TextViewStyle::from_theme(&gpui_kit::base::Theme::global(cx))
        .with_code_block(StyleRefinement::default().text_size(code))
}

/// The zoom keys (bound in [`KEY_CONTEXT`]) step the reading size, which persists; the code
/// editors and diffs keep theirs.
pub fn zoom_keys<E: InteractiveElement>(el: E) -> E {
    el.on_action(|_: &markdown::ZoomIn, _, cx| Settings::step_reading_size(cx, 1))
        .on_action(|_: &markdown::ZoomOut, _, cx| Settings::step_reading_size(cx, -1))
        .on_action(|_: &markdown::ZoomReset, _, cx| Settings::step_reading_size(cx, 0))
}
