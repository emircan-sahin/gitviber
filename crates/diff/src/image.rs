//! A changed image's two versions: side by side, as a swipe (a divider between them) or as an
//! onion skin (after faded in over before). Both are decoded once off the main thread and held
//! by this view only; closing it takes their textures out of every window's atlas.

use crate::source::DiffSource;
use crate::style::{DiffOptions, DiffStyle, ImageMode};
use gitviber_core::Core;
use gpui_kit::component::h_flex;
use gpui_kit::{
    App, AppContext as _, Bounds, Context, Image, ImageFormat, ImageSource,
    InteractiveElement as _, IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent, ObjectFit,
    ParentElement as _, Pixels, Render, RenderImage, Styled as _, StyledImage as _, Subscription,
    Task, Window, canvas, div, img, prelude::FluentBuilder as _, px, relative,
};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

/// The image formats the compare view decodes.
pub fn image_format(path: &str) -> Option<ImageFormat> {
    let ext = path.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase())?;
    Some(match ext.as_str() {
        "png" => ImageFormat::Png,
        "jpg" | "jpeg" => ImageFormat::Jpeg,
        "gif" => ImageFormat::Gif,
        "webp" => ImageFormat::Webp,
        "bmp" => ImageFormat::Bmp,
        "ico" => ImageFormat::Ico,
        "tif" | "tiff" => ImageFormat::Tiff,
        _ => return None,
    })
}

struct Version {
    image: Arc<RenderImage>,
    bytes: usize,
    /// Of the file's bytes: the same again isn't decoded again.
    hash: u64,
}

/// Past this the compressed file isn't decoded, and past this many pixels the decoded one isn't
/// kept: a 10000×10000 screenshot is 400 MB as pixels.
const MAX_BYTES: usize = 64 * 1024 * 1024;
const MAX_PIXELS: u64 = 40_000_000;

fn hash(bytes: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut h);
    h.finish()
}

enum Side {
    Loading,
    Missing,
    Ready(Version),
    Failed(String),
    /// What a refresh found: the same bytes as the version shown.
    Same,
}

pub struct ImageCompare {
    core: Arc<Core>,
    source: DiffSource,
    sides: [Side; 2],
    /// Swipe's divider, and onion skin's opacity of After: kept apart, as a divider near the
    /// edge isn't a faint onion skin.
    divider: f32,
    opacity: f32,
    dragging: bool,
    stage: Rc<Cell<Bounds<Pixels>>>,
    _load: Task<()>,
    _release: Subscription,
}

impl ImageCompare {
    pub fn new(source: DiffSource, cx: &mut Context<Self>) -> Self {
        let mut view = ImageCompare {
            core: crate::core(cx),
            source,
            sides: [Side::Loading, Side::Loading],
            divider: 0.5,
            opacity: 0.5,
            dragging: false,
            stage: Rc::default(),
            _load: Task::ready(()),
            _release: cx.on_release(|this, cx| this.drop_images(cx)),
        };
        view.load(cx);
        view
    }

    /// Reads both versions again (the file changed on disk).
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.load(cx);
    }

    fn drop_images(&mut self, cx: &mut App) {
        for side in &mut self.sides {
            if let Side::Ready(v) = std::mem::replace(side, Side::Loading) {
                cx.drop_image(v.image, None);
            }
        }
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        let Some(format) = image_format(&self.source.path) else {
            return;
        };
        let (core, source) = (self.core.clone(), self.source.clone());
        let svg = cx.svg_renderer();
        let shown = [0, 1].map(|i| match &self.sides[i] {
            Side::Ready(v) => Some(v.hash),
            _ => None,
        });
        self._load = cx.spawn(async move |this, cx| {
            let sides = cx
                .background_spawn(async move {
                    let c = &source.compare;
                    [(true, shown[0]), (false, shown[1])].map(|(original, shown)| {
                        let bytes = gitviber_core::api::files::media(
                            &core,
                            c.kind().to_string(),
                            source.path.clone(),
                            source.old_path.clone(),
                            c.sha().map(str::to_string),
                            c.base().map(str::to_string),
                            original,
                        );
                        match bytes {
                            Ok(b) if b.is_empty() => Side::Missing,
                            Ok(b) if shown == Some(hash(&b)) => Side::Same,
                            Ok(b) if b.len() > MAX_BYTES => {
                                Side::Failed("Too large to preview".into())
                            }
                            Ok(b) => {
                                let (len, h) = (b.len(), hash(&b));
                                match Image::from_bytes(format, b).to_image_data(svg.clone()) {
                                    Ok(image) => {
                                        let size = image.size(0);
                                        if size.width.0 as u64 * size.height.0 as u64 > MAX_PIXELS {
                                            Side::Failed(format!(
                                                "{}×{} is too large to preview",
                                                size.width.0, size.height.0
                                            ))
                                        } else {
                                            Side::Ready(Version {
                                                image,
                                                bytes: len,
                                                hash: h,
                                            })
                                        }
                                    }
                                    Err(e) => Side::Failed(e.to_string()),
                                }
                            }
                            // A side that isn't there (added, deleted) reads as an error.
                            Err(_) => Side::Missing,
                        }
                    })
                })
                .await;
            this.update(cx, |this, cx| {
                for (i, side) in sides.into_iter().enumerate() {
                    if matches!(side, Side::Same) {
                        continue;
                    }
                    if let Side::Ready(old) = std::mem::replace(&mut this.sides[i], side) {
                        cx.drop_image(old.image, None);
                    }
                }
                cx.notify();
            })
            .ok();
        });
    }

    fn drag(&mut self, x: Pixels, cx: &mut Context<Self>) {
        let b = self.stage.get();
        if b.size.width <= px(0.) {
            return;
        }
        let at = ((x - b.origin.x) / b.size.width).clamp(0., 1.);
        if DiffOptions::get(cx).image == ImageMode::Onion {
            self.opacity = at;
        } else {
            self.divider = at;
        }
        cx.notify();
    }
}

fn details(v: &Side) -> String {
    match v {
        Side::Ready(v) => {
            let s = v.image.size(0);
            format!("{}×{} · {}", s.width.0, s.height.0, bytes(v.bytes))
        }
        Side::Failed(e) => e.clone(),
        _ => String::new(),
    }
}

fn bytes(n: usize) -> String {
    match n {
        n if n >= 1 << 20 => format!("{:.1} MB", n as f64 / (1 << 20) as f64),
        n if n >= 1 << 10 => format!("{:.1} KB", n as f64 / 1024.),
        n => format!("{n} B"),
    }
}

fn picture(v: &Side) -> Option<gpui_kit::Img> {
    let Side::Ready(v) = v else { return None };
    Some(
        img(ImageSource::Render(v.image.clone()))
            .size_full()
            .object_fit(ObjectFit::ScaleDown),
    )
}

impl Render for ImageCompare {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let style = DiffStyle::get(cx).clone();
        let c = &style.colors;
        let both = matches!(self.sides, [Side::Ready(_), Side::Ready(_)]);
        let mode = if both {
            DiffOptions::get(cx).image
        } else {
            ImageMode::Side
        };
        let label = |text: &'static str, color, d: String| {
            h_flex()
                .gap_2()
                .child(div().text_color(color).child(text))
                .child(div().text_color(c.subtle).child(d))
        };
        let bar = h_flex()
            .h(px(28.))
            .flex_none()
            .px_3()
            .gap_2()
            .border_b_1()
            .border_color(c.border)
            .text_size(style.ui_font_size - px(0.5))
            .child(label("Before", c.removed, details(&self.sides[0])))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .justify_center()
                    .text_color(c.subtle)
                    .when(mode == ImageMode::Onion, |el| {
                        el.child(format!(
                            "After at {}% · drag to fade",
                            (self.opacity * 100.).round()
                        ))
                    }),
            )
            .child(label("After", c.added, details(&self.sides[1])));
        let stage_bounds = self.stage.clone();
        let measure = canvas(
            move |bounds, _, _| stage_bounds.set(bounds),
            |_, _, _, _| {},
        )
        .absolute()
        .size_full();
        let stage = match mode {
            ImageMode::Side => {
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .gap_4()
                    .p_4()
                    .children(self.sides.iter().map(|s| {
                        div().flex_1().h_full().children(picture(s)).when(
                            matches!(s, Side::Missing),
                            |el| {
                                el.flex()
                                    .items_center()
                                    .justify_center()
                                    .text_color(c.subtle)
                                    .child("Not on this side")
                            },
                        )
                    }))
            }
            ImageMode::Swipe => {
                let d = self.divider.max(0.001);
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .m_4()
                    .child(measure)
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .left_0()
                            .h_full()
                            .w(relative(d))
                            .overflow_hidden()
                            .child(
                                div()
                                    .h_full()
                                    .w(relative(1. / d))
                                    .children(picture(&self.sides[0])),
                            ),
                    )
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .right_0()
                            .h_full()
                            .w(relative(1. - self.divider))
                            .overflow_hidden()
                            .child(
                                div()
                                    .absolute()
                                    .top_0()
                                    .right_0()
                                    .h_full()
                                    .w(relative(1. / (1. - self.divider).max(0.001)))
                                    .children(picture(&self.sides[1])),
                            ),
                    )
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .h_full()
                            .left(relative(self.divider))
                            .w(px(2.))
                            .bg(c.primary),
                    )
            }
            ImageMode::Onion => div()
                .relative()
                .flex_1()
                .min_h_0()
                .m_4()
                .child(measure)
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full()
                        .children(picture(&self.sides[0])),
                )
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full()
                        .opacity(self.opacity)
                        .children(picture(&self.sides[1])),
                ),
        };
        div()
            .id("image-compare")
            .size_full()
            .flex()
            .flex_col()
            .bg(c.background)
            .text_color(c.foreground)
            .child(bar)
            .child(
                stage
                    .id("image-stage")
                    .cursor_ew_resize()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, e: &MouseDownEvent, _, cx| {
                            this.dragging = true;
                            this.drag(e.position.x, cx);
                        }),
                    )
                    .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, _, cx| {
                        if this.dragging && e.pressed_button == Some(MouseButton::Left) {
                            this.drag(e.position.x, cx);
                        }
                    }))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _, _, _| this.dragging = false),
                    )
                    .on_mouse_up_out(
                        MouseButton::Left,
                        cx.listener(|this, _, _, _| this.dragging = false),
                    ),
            )
    }
}
