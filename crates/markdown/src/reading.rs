//! Reading typography, for a document a whole pane shows (a file's preview, a vault note): body
//! text sized from the code font, a column of comfortable width, and keys that zoom the text.
//! Text inside other pages (a pull request, a comment) keeps the interface's size.

use gitviber_ui::ActiveAppTheme as _;
use gitviber_ui::Settings;
use gitviber_ui::keymap::markdown;
use gpui_kit::{App, FontWeight, InteractiveElement, Pixels, Styled, relative};

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

/// The body's size, line height and weight on `el`, which its text inherits. Regular, not the
/// interface's medium: at reading size medium read as bold beside a done task's dimmed text.
pub fn typography<E: Styled>(el: E, cx: &App) -> E {
    el.text_size(text_size(cx))
        .line_height(relative(LINE_HEIGHT))
        .font_weight(FontWeight::NORMAL)
}

/// The zoom keys (bound in [`KEY_CONTEXT`]) step the code font size, which persists.
pub fn zoom_keys<E: InteractiveElement>(el: E) -> E {
    el.on_action(|_: &markdown::ZoomIn, _, cx| Settings::step_code_font(cx, 1))
        .on_action(|_: &markdown::ZoomOut, _, cx| Settings::step_code_font(cx, -1))
        .on_action(|_: &markdown::ZoomReset, _, cx| Settings::step_code_font(cx, 0))
}

#[cfg(test)]
mod tests {
    use gpui_kit::{TestAppContext, div};

    use super::*;

    #[gpui_kit::test]
    fn the_body_reads_at_the_regular_weight(cx: &mut TestAppContext) {
        gitviber_ui::test_support::config_dir("markdown-reading-test");
        cx.update(gitviber_ui::init);
        let weight = cx.update(|cx| typography(div(), cx).style().text.font_weight);
        assert_eq!(weight, Some(FontWeight::NORMAL));
        assert_ne!(
            cx.update(|cx| cx.app_theme().fonts.ui_weight),
            FontWeight::NORMAL
        );
    }
}
