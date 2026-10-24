//! A changed image's two versions: side by side, as a swipe (a divider between them) or as an
//! onion skin (after faded in over before). Both are decoded once off the main thread and held
//! by this view only; closing it takes their textures out of every window's atlas. An SVG's
//! Preview is drawn here too, from the texts its diff has, fitted or at 1:1, on the theme's
//! background or a light or dark one.

use crate::source::DiffSource;
use crate::style::{DiffOptions, DiffStyle, ImageMode, Layout};
use gitviber_core::Core;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::{
    App, AppContext as _, Bounds, Context, DevicePixels, Hsla, Image, ImageFormat, ImageSource,
    InteractiveElement as _, IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent, ObjectFit,
    ParentElement as _, Pixels, Render, RenderImage, Rgba, StatefulInteractiveElement as _,
    Styled as _, StyledImage as _, Subscription, SvgRenderer, SvgSize, Task, Window, canvas, div,
    img, prelude::FluentBuilder as _, px, relative, size,
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
    /// An SVG's own size, which 1:1 shows it at; its pixels are drawn for a fitted view.
    natural: Option<(f32, f32)>,
}

/// How an SVG preview is sized.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Zoom {
    #[default]
    Fit,
    Actual,
}

/// What an SVG preview is drawn on: art in the theme's own color vanishes on the theme's.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Backdrop {
    #[default]
    Theme,
    Light,
    Dark,
}

/// An SVG's longer side is drawn this many device pixels long, whatever its own size: sharp
/// when fitted to a pane, and two of them stay under ~45 MB.
const SVG_MIN_SIDE: f32 = 1600.;
const SVG_MAX_SIDE: f32 = 2400.;

fn rasterize_svg(svg: &SvgRenderer, text: &str, hash: u64) -> Side {
    let Some((w, h)) = crate::model::svg::intrinsic_size(text) else {
        return Side::Failed("Not an SVG".into());
    };
    let long = w.max(h);
    let k = (long * 2.).clamp(SVG_MIN_SIDE, SVG_MAX_SIDE) / long;
    let px = |v: f32| DevicePixels((v * k).round().max(1.) as i32);
    let exact = SvgSize::ExactSize(size(px(w), px(h)));
    match svg
        .parse_svg(text.as_bytes())
        .and_then(|parsed| svg.render_parsed(&parsed, exact))
    {
        Ok(image) => Side::Ready(Version {
            image,
            bytes: text.len(),
            hash,
            natural: Some((w, h)),
        }),
        Err(e) => Side::Failed(format!("Could not draw this SVG: {e}")),
    }
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
    /// Drawn from texts handed in (`set_svg_texts`), not read from git.
    svg: bool,
    zoom: Zoom,
    backdrop: Backdrop,
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
            svg: false,
            zoom: Zoom::Fit,
            backdrop: Backdrop::Theme,
            _load: Task::ready(()),
            _release: cx.on_release(|this, cx| this.drop_images(cx)),
        };
        view.load(cx);
        view
    }

    /// An SVG's preview, drawn from the texts `set_svg_texts` gives it.
    pub fn svg(source: DiffSource, cx: &mut Context<Self>) -> Self {
        ImageCompare {
            core: crate::core(cx),
            source,
            sides: [Side::Loading, Side::Loading],
            divider: 0.5,
            opacity: 0.5,
            dragging: false,
            stage: Rc::default(),
            svg: true,
            zoom: Zoom::Fit,
            backdrop: Backdrop::Theme,
            _load: Task::ready(()),
            _release: cx.on_release(|this, cx| this.drop_images(cx)),
        }
    }

    /// Reads both versions again (the file changed on disk).
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        if !self.svg {
            self.load(cx);
        }
    }

    /// The SVG's text before and after (None: not on that side); the same text isn't drawn again.
    pub fn set_svg_texts(&mut self, texts: [Option<String>; 2], cx: &mut Context<Self>) {
        let svg = cx.svg_renderer();
        let shown = [0, 1].map(|i| match &self.sides[i] {
            Side::Ready(v) => Some(v.hash),
            _ => None,
        });
        self._load = cx.spawn(async move |this, cx| {
            let sides = cx
                .background_spawn(async move {
                    let [before, after] = texts;
                    [(before, shown[0]), (after, shown[1])].map(|(text, shown)| match text {
                        None => Side::Missing,
                        Some(t) if shown == Some(hash(t.as_bytes())) => Side::Same,
                        Some(t) if t.len() > MAX_BYTES => {
                            Side::Failed("Too large to preview".into())
                        }
                        Some(t) => rasterize_svg(&svg, &t, hash(t.as_bytes())),
                    })
                })
                .await;
            this.update(cx, |this, cx| this.take_sides(sides, cx)).ok();
        });
    }

    pub fn zoom(&self) -> Zoom {
        self.zoom
    }

    pub fn set_zoom(&mut self, zoom: Zoom, cx: &mut Context<Self>) {
        self.zoom = zoom;
        cx.notify();
    }

    pub fn backdrop(&self) -> Backdrop {
        self.backdrop
    }

    pub fn set_backdrop(&mut self, backdrop: Backdrop, cx: &mut Context<Self>) {
        self.backdrop = backdrop;
        cx.notify();
    }

    /// Both versions are there to lay one over the other.
    pub fn has_both(&self) -> bool {
        !matches!(self.sides, [Side::Missing, _] | [_, Side::Missing])
    }

    fn take_sides(&mut self, sides: [Side; 2], cx: &mut Context<Self>) {
        for (i, side) in sides.into_iter().enumerate() {
            if matches!(side, Side::Same) {
                continue;
            }
            if let Side::Ready(old) = std::mem::replace(&mut self.sides[i], side) {
                cx.drop_image(old.image, None);
            }
        }
        cx.notify();
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
                                                natural: None,
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
            this.update(cx, |this, cx| this.take_sides(sides, cx)).ok();
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
            let (w, h) = v.natural.unwrap_or_else(|| {
                let s = v.image.size(0);
                (s.width.0 as f32, s.height.0 as f32)
            });
            format!("{}×{} · {}", w.round(), h.round(), bytes(v.bytes))
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
    // A fitted SVG fills its place: its pixels were drawn for that.
    let fit = if v.natural.is_some() {
        ObjectFit::Contain
    } else {
        ObjectFit::ScaleDown
    };
    Some(
        img(ImageSource::Render(v.image.clone()))
            .size_full()
            .object_fit(fit),
    )
}

fn natural(v: &Side) -> Option<(f32, f32)> {
    match v {
        Side::Ready(v) => v.natural,
        _ => None,
    }
}

fn backdrop_color(b: Backdrop) -> Option<Hsla> {
    let rgb = |n: u32| -> Hsla {
        Rgba {
            r: (n >> 16) as f32 / 255.,
            g: ((n >> 8) & 0xff) as f32 / 255.,
            b: (n & 0xff) as f32 / 255.,
            a: 1.,
        }
        .into()
    };
    match b {
        Backdrop::Theme => None,
        Backdrop::Light => Some(rgb(0xffffff)),
        Backdrop::Dark => Some(rgb(0x181818)),
    }
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
        let bg = if self.svg {
            backdrop_color(self.backdrop)
        } else {
            None
        };
        let actual = self.svg && self.zoom == Zoom::Actual;
        // At 1:1 the two lie in one frame, as large as the larger of them.
        let frame = actual.then(|| {
            let [a, b] = [&self.sides[0], &self.sides[1]].map(|s| natural(s).unwrap_or((0., 0.)));
            (a.0.max(b.0), a.1.max(b.1))
        });
        let overlay = |el: gpui_kit::Div| match frame {
            Some((w, h)) => div().flex_1().min_h_0().p_4().flex().child(
                el.relative()
                    .flex_none()
                    .m_auto()
                    .w(px(w))
                    .h(px(h))
                    .when_some(bg, |el, bg| el.bg(bg)),
            ),
            None => el
                .relative()
                .flex_1()
                .min_h_0()
                .m_4()
                .when_some(bg, |el, bg| el.bg(bg)),
        };
        let stage = match mode {
            ImageMode::Side => {
                // An SVG's two versions stack in the unified layout, as its diff's lines do.
                let stacked = self.svg && DiffOptions::get(cx).layout == Layout::Unified;
                (if stacked { v_flex() } else { h_flex() })
                    .flex_1()
                    .min_h_0()
                    .gap_4()
                    .p_4()
                    .children(self.sides.iter().enumerate().map(|(i, s)| {
                        let cell = div()
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            .map(|el| if stacked { el.w_full() } else { el.h_full() });
                        match (s, natural(s)) {
                            (Side::Missing, _) => cell
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_color(c.subtle)
                                .child("Not on this side"),
                            (_, Some((w, h))) if actual => cell.child(
                                div()
                                    .id(("svg-actual", i))
                                    .size_full()
                                    .overflow_scroll()
                                    .flex()
                                    .child(
                                        div()
                                            .m_auto()
                                            .flex_none()
                                            .w(px(w))
                                            .h(px(h))
                                            .when_some(bg, |el, bg| el.bg(bg))
                                            .children(picture(s)),
                                    ),
                            ),
                            _ => cell.when_some(bg, |el, bg| el.bg(bg)).children(picture(s)),
                        }
                    }))
            }
            ImageMode::Swipe => {
                let d = self.divider.max(0.001);
                overlay(
                    div()
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
                        ),
                )
            }
            ImageMode::Onion => overlay(
                div()
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
                    .when(frame.is_some() && mode != ImageMode::Side, |el| {
                        el.overflow_scroll()
                    })
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
