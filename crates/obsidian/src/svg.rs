//! An SVG in a vault drawn from its text, so the preview shows typing not saved yet. It's
//! rasterized off the main thread into an image this view owns: dropped from the window when
//! another replaces it or the view goes, never kept in gpui's global image cache.

use std::sync::Arc;

use gitviber_ui::ActiveAppTheme as _;
use gitviber_ui::memory::Live;
use gpui_kit::{
    App, AppContext as _, Context, ImageSource, IntoElement, ObjectFit, ParentElement as _, Render,
    RenderImage, SharedString, Styled as _, StyledImage as _, Task, Window, div, img, px,
};

pub struct SvgPreview {
    source: Arc<str>,
    image: Option<Arc<RenderImage>>,
    error: Option<SharedString>,
    scale: f32,
    _render: Task<()>,
    _live: Live,
}

impl SvgPreview {
    pub fn new(source: String, window: &mut Window, cx: &mut Context<Self>) -> Self {
        cx.on_release(|this: &mut Self, cx| this.drop_image(cx))
            .detach();
        let mut this = SvgPreview {
            source: "".into(),
            image: None,
            error: None,
            scale: window.scale_factor(),
            _render: Task::ready(()),
            _live: Live::new("SvgPreview"),
        };
        this.set_source(source, cx);
        this
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    /// Draws other text (typed into, or changed on disk).
    pub fn set_source(&mut self, source: String, cx: &mut Context<Self>) {
        if *self.source == *source && (self.image.is_some() || self.error.is_some()) {
            return;
        }
        self.source = source.into();
        let (renderer, bytes, scale) = (cx.svg_renderer(), self.source.clone(), self.scale);
        self._render = cx.spawn(async move |this, cx| {
            // gpui caps the raster at 8192 pixels a side.
            let drawn = cx
                .background_spawn(
                    async move { renderer.render_single_frame(bytes.as_bytes(), scale) },
                )
                .await;
            _ = this.update(cx, |this, cx| {
                this.drop_image(cx);
                match drawn {
                    Ok(image) => this.image = Some(image),
                    Err(e) => this.error = Some(format!("This SVG can't be drawn: {e}").into()),
                }
                cx.notify();
            });
        });
    }

    fn drop_image(&mut self, cx: &mut App) {
        self.error = None;
        if let Some(image) = self.image.take() {
            cx.drop_image(image, None);
        }
    }
}

impl Render for SvgPreview {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.app_theme();
        let body = match (&self.image, &self.error) {
            (Some(image), _) => img(ImageSource::Render(image.clone()))
                .max_w_full()
                .max_h_full()
                .size_full()
                .object_fit(ObjectFit::ScaleDown)
                .into_any_element(),
            (None, Some(e)) => div()
                .text_size(px(12.))
                .text_color(t.subtle)
                .child(e.clone())
                .into_any_element(),
            (None, None) => div().into_any_element(),
        };
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .p_4()
            .bg(t.code(t.background))
            .child(body)
    }
}
