//! Reading typography, for a document a whole pane shows (a file's preview, a vault note): body
//! text sized from the code font, a column of comfortable width, and keys that zoom the text.
//! Text inside other pages (a pull request, a comment) keeps the interface's size.

use gitviber_ui::ActiveAppTheme as _;
use gitviber_ui::Settings;
use gitviber_ui::keymap::markdown;
use gpui_kit::{App, InteractiveElement, Pixels, Styled, relative};

/// A reading view's key context: a rendered document's (scroll and find keys), and the one the
/// zoom keys bind in.
pub const KEY_CONTEXT: &str = "MarkdownView MarkdownReading";

/// The body over the code font: inline code, drawn at 7/8 of the body, comes out at the code
/// font's own size, and zooming the code font zooms the reading.
const BODY_PER_CODE: f32 = 8. / 7.;

const LINE_HEIGHT: f32 = 1.6;

/// The column in ems of the body: about 75 characters of prose a line, at any zoom.
const MEASURE_EM: f32 = 40.;

/// The body text's size: 15.4px at the default 13.5px code font.
pub fn text_size(cx: &App) -> Pixels {
    cx.app_theme().fonts.code_size * BODY_PER_CODE
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

/// The zoom keys (bound in [`KEY_CONTEXT`]) step the code font size, which persists.
pub fn zoom_keys<E: InteractiveElement>(el: E) -> E {
    el.on_action(|_: &markdown::ZoomIn, _, cx| Settings::step_code_font(cx, 1))
        .on_action(|_: &markdown::ZoomOut, _, cx| Settings::step_code_font(cx, -1))
        .on_action(|_: &markdown::ZoomReset, _, cx| Settings::step_code_font(cx, 0))
}
