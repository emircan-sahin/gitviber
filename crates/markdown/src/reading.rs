//! Reading typography, for a document a whole pane shows (a file's or a diff's preview, a vault
//! note): body text sized from the code font, a column of comfortable width, and keys that zoom
//! the text on its own. Text inside other pages (a pull request, a comment) keeps the interface's
//! size.

use gitviber_ui::Settings;
use gitviber_ui::keymap::markdown;
use gpui_kit::base::text::TextViewStyle;
use gpui_kit::{
    AnyElement, App, FontWeight, InteractiveElement, IntoElement, ParentElement as _, Pixels,
    StyleRefinement, Styled, Window, div, px, relative, rems,
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

/// Between paragraphs, in ems of the body: the text view's is in rems, the interface's size.
const PARAGRAPH_GAP_EM: f32 = 0.75;

/// The column in ems of the body: about 75 characters of prose a line, at any zoom.
const MEASURE_EM: f32 = 40.;

/// Code blocks, tables and diagrams get this much: about 860px at the default size.
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

/// The body's size, line height and weight on `el`, which its text inherits. Regular, not the
/// interface's medium: at reading size medium read as bold beside a done task's dimmed text.
pub fn typography<E: Styled>(el: E, cx: &App) -> E {
    el.text_size(text_size(cx))
        .line_height(relative(LINE_HEIGHT))
        .font_weight(FontWeight::NORMAL)
}

/// A reading view's text style: prose at [`measure`], paragraphs spaced for the body's size, code
/// blocks at the size its inline code is.
pub fn style(window: &Window, cx: &App) -> TextViewStyle {
    let size = text_size(cx);
    let code = size / BODY_PER_CODE;
    TextViewStyle::from_theme(&gpui_kit::base::Theme::global(cx))
        .with_measure(measure(cx))
        .with_paragraph_gap(rems(PARAGRAPH_GAP_EM * (size / window.rem_size())))
        .with_code_block(StyleRefinement::default().text_size(code))
}

/// The zoom keys (bound in [`KEY_CONTEXT`]) step the reading size, which persists; the code
/// editors and diffs keep theirs.
pub fn zoom_keys<E: InteractiveElement>(el: E) -> E {
    el.on_action(|_: &markdown::ZoomIn, _, cx| Settings::step_reading_size(cx, 1))
        .on_action(|_: &markdown::ZoomOut, _, cx| Settings::step_reading_size(cx, -1))
        .on_action(|_: &markdown::ZoomReset, _, cx| Settings::step_reading_size(cx, 0))
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;
    use std::sync::{Arc, Mutex};

    use gpui_kit::{
        AppContext as _, Bounds, Context, DevicePixels, Entity, Font, FontId, FontMetrics, FontRun,
        GlyphId, IntoElement, LineLayout, NoopTextSystem, PlatformTextSystem, Render,
        RenderGlyphParams, Size, TestAppContext, TestDispatcher, TextRenderingMode, Window, div,
    };

    use gitviber_ui::ActiveAppTheme as _;

    use super::*;
    use crate::{MarkdownBase, MarkdownView};

    /// The test platform's text system, keeping the weight of each line's runs as shaped.
    #[derive(Default)]
    struct Shaped {
        fonts: Mutex<Vec<FontWeight>>,
        lines: Mutex<Vec<(String, Vec<FontWeight>)>>,
    }

    impl PlatformTextSystem for Shaped {
        fn add_fonts(&self, fonts: Vec<Cow<'static, [u8]>>) -> gpui_kit::Result<()> {
            NoopTextSystem.add_fonts(fonts)
        }
        fn all_font_names(&self) -> Vec<String> {
            NoopTextSystem.all_font_names()
        }
        fn font_id(&self, font: &Font) -> gpui_kit::Result<FontId> {
            let mut fonts = self.fonts.lock().unwrap();
            fonts.push(font.weight);
            Ok(FontId(fonts.len() - 1))
        }
        fn font_metrics(&self, id: FontId) -> FontMetrics {
            NoopTextSystem.font_metrics(id)
        }
        fn typographic_bounds(&self, id: FontId, glyph: GlyphId) -> gpui_kit::Result<Bounds<f32>> {
            NoopTextSystem.typographic_bounds(id, glyph)
        }
        fn advance(&self, id: FontId, glyph: GlyphId) -> gpui_kit::Result<Size<f32>> {
            NoopTextSystem.advance(id, glyph)
        }
        fn glyph_for_char(&self, id: FontId, ch: char) -> Option<GlyphId> {
            NoopTextSystem.glyph_for_char(id, ch)
        }
        fn glyph_raster_bounds(
            &self,
            params: &RenderGlyphParams,
        ) -> gpui_kit::Result<Bounds<DevicePixels>> {
            NoopTextSystem.glyph_raster_bounds(params)
        }
        fn rasterize_glyph(
            &self,
            params: &RenderGlyphParams,
            bounds: Bounds<DevicePixels>,
        ) -> gpui_kit::Result<(Size<DevicePixels>, Vec<u8>)> {
            NoopTextSystem.rasterize_glyph(params, bounds)
        }
        fn recommended_rendering_mode(&self, id: FontId, size: Pixels) -> TextRenderingMode {
            NoopTextSystem.recommended_rendering_mode(id, size)
        }
        fn layout_line(&self, text: &str, size: Pixels, runs: &[FontRun]) -> LineLayout {
            let fonts = self.fonts.lock().unwrap();
            let weights = runs.iter().map(|r| fonts[r.font_id.0]).collect();
            self.lines.lock().unwrap().push((text.into(), weights));
            NoopTextSystem.layout_line(text, size, runs)
        }
    }

    /// At the interface's weight, as the workspace's view is.
    struct Workspace(Entity<MarkdownView>);

    impl Render for Workspace {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let weight = cx.app_theme().fonts.ui_weight;
            div().size_full().font_weight(weight).child(self.0.clone())
        }
    }

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

    /// The text as drawn, not only the style it inherits: a task's too, an open and a done one.
    #[test]
    fn a_documents_text_is_drawn_at_the_regular_weight() {
        let shaped = Arc::new(Shaped::default());
        let mut cx =
            TestAppContext::build_with_text_system(TestDispatcher::new(0), None, shaped.clone());
        gitviber_ui::test_support::config_dir("markdown-reading-drawn");
        cx.update(gitviber_ui::init);
        let (_, cx) = cx.add_window_view(|window, cx| {
            let doc = "Plain words\n\n- [ ] open task\n- [x] done task\n";
            Workspace(cx.new(|cx| MarkdownView::new(doc, MarkdownBase::default(), window, cx)))
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
        let lines = shaped.lines.lock().unwrap();
        for text in ["Plain words", "open task", "done task"] {
            let (_, weights) = lines
                .iter()
                .find(|(line, _)| line.contains(text))
                .unwrap_or_else(|| panic!("{text:?} wasn't drawn: {lines:?}"));
            assert_eq!(weights, &[FontWeight::NORMAL], "{text}");
        }
    }
}
