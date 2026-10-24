//! A rendered SVG file: fit to the view or 1:1, scroll to zoom around the pointer, drag (or the
//! arrows) to pan, double-click to fit, and a light or dark checkerboard behind it for art drawn
//! in a color that vanishes on the theme's. Drawn from the text, so typing not saved yet shows.
//!
//! Only the part on screen is rasterized, at the display's density, off the main thread: the
//! root element's viewBox is rewritten to that part, so a zoomed drawing stays sharp and its
//! pixels never outgrow the view. The image is this view's own, dropped when replaced.

use std::cell::Cell;
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gitviber_ui::ActiveAppTheme as _;
use gitviber_ui::keymap::media;
use gitviber_ui::memory::Live;
use gitviber_ui::widgets::{Toggle as _, segmented};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::{
    App, AppContext as _, Bounds, Context, CursorStyle, DevicePixels, FocusHandle, Hsla,
    ImageSource, InteractiveElement as _, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent,
    MouseMoveEvent, ObjectFit, ParentElement as _, Pixels, Point, Render, RenderImage,
    ScrollWheelEvent, SharedString, Styled as _, StyledImage as _, SvgSize, Task, Window, canvas,
    div, fill, img, point, prelude::FluentBuilder as _, px, size,
};

/// Margin around a drawing fitted to its view.
const PAD: f32 = 16.;
/// The longest side zooming may show, as the old viewer's.
const MAX_DRAWN: f32 = 16384.;
/// A checkerboard square, in view pixels.
const SQUARE: f32 = 8.;
/// Zooming or panning settles this long before the sharp picture is drawn again.
const SETTLE: Duration = Duration::from_millis(90);

/// How the drawing is zoomed. `scale` is view pixels per SVG pixel, None to fit the view;
/// (u, v) is the point of the drawing, as fractions of its size, held at the view's center.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Zoom {
    pub scale: Option<f32>,
    pub u: f32,
    pub v: f32,
}

pub(crate) const FIT: Zoom = Zoom {
    scale: None,
    u: 0.5,
    v: 0.5,
};

/// How far a drawing of `natural` size, which fits its view at `fit`, may zoom out and in.
pub(crate) fn zoom_limits(natural: (f32, f32), fit: f32) -> (f32, f32) {
    let min = fit.min(1.) / 2.;
    let max = (fit.max(1.) * 16.).min(MAX_DRAWN / natural.0.max(natural.1));
    (min, min.max(max))
}

/// Where along a `room`-long view a drawing `size` long starts: centered if it fits, else never
/// leaving a gap.
pub(crate) fn place(room: f32, size: f32, u: f32) -> f32 {
    if size <= room {
        (room - size) / 2.
    } else {
        (room / 2. - u * size).clamp(room - size, 0.)
    }
}

/// `u` limited to what place() can show, so panning past an edge doesn't build up.
fn clamp_center(room: f32, size: f32, u: f32) -> f32 {
    if size <= room {
        return 0.5;
    }
    let half = room / 2. / size;
    u.clamp(half, 1. - half)
}

/// Along one axis: the new center when zooming from `size` to `next`, keeping the point under
/// `at` in place.
pub(crate) fn zoom_axis(room: f32, size: f32, next: f32, u: f32, at: f32) -> f32 {
    let under = (at - place(room, size, u)) / size;
    clamp_center(room, next, (room / 2. - at) / next + under)
}

/// Along one axis: the new center after dragging the drawing by `delta` pixels.
pub(crate) fn pan_axis(room: f32, size: f32, u: f32, delta: f32) -> f32 {
    clamp_center(room, size, u - delta / size)
}

/// The root `<svg …>` start tag's byte range, past the prolog, comments and doctype.
fn root_tag(text: &str) -> Option<Range<usize>> {
    let mut at = 0;
    loop {
        let rest = &text[at..];
        let start = at + rest.find('<')?;
        let tail = &text[start..];
        let skip = if tail.starts_with("<?") {
            tail.find("?>").map(|e| e + 2)
        } else if tail.starts_with("<!--") {
            tail.find("-->").map(|e| e + 3)
        } else if tail.starts_with("<!") {
            // A doctype's internal subset may hold '>'.
            let close = tail
                .find('[')
                .filter(|b| tail.find('>').is_some_and(|g| *b < g));
            match close {
                Some(_) => tail.find("]>").map(|e| e + 2),
                None => tail.find('>').map(|e| e + 1),
            }
        } else {
            None
        };
        if let Some(skip) = skip {
            at = start + skip;
            continue;
        }
        let name_end = tail[1..]
            .find(|c: char| c.is_whitespace() || c == '>' || c == '/')
            .map_or(tail.len(), |e| e + 1);
        let name = &tail[1..name_end];
        if name != "svg" && !name.ends_with(":svg") {
            return None;
        }
        let mut quote = None;
        for (i, c) in tail.char_indices() {
            match (quote, c) {
                (None, '"' | '\'') => quote = Some(c),
                (Some(q), c) if c == q => quote = None,
                (None, '>') => return Some(start..start + i + 1),
                _ => {}
            }
        }
        return None;
    }
}

/// A start tag's attributes: name, value, and the byte range of the whole `name="value"`.
fn attributes(tag: &str) -> Vec<(&str, &str, Range<usize>)> {
    static ATTR: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r#"([A-Za-z_:][\w:.-]*)\s*=\s*(?:"([^"]*)"|'([^']*)')"#).unwrap()
    });
    ATTR.captures_iter(tag)
        .filter_map(|c| {
            let all = c.get(0)?;
            let value = c.get(2).or(c.get(3))?;
            Some((c.get(1)?.as_str(), value.as_str(), all.range()))
        })
        .collect()
}

/// A length in CSS pixels; None for a percentage or what doesn't parse.
fn length(text: &str) -> Option<f32> {
    let t = text.trim();
    let split = t
        .find(|c: char| c.is_ascii_alphabetic() || c == '%')
        .unwrap_or(t.len());
    let n: f32 = t[..split].trim().parse().ok()?;
    let per = match t[split..].trim() {
        "" | "px" => 1.,
        "pt" => 4. / 3.,
        "pc" => 16.,
        "in" => 96.,
        "cm" => 96. / 2.54,
        "mm" => 96. / 25.4,
        "em" => 16.,
        "ex" => 8.,
        _ => return None,
    };
    (n.is_finite() && n > 0.).then_some(n * per)
}

/// What the root element says about its size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Root {
    /// The size it draws at on its own, in CSS pixels.
    pub natural: (f32, f32),
    /// Its viewBox: user space shown in `natural`.
    pub view_box: Option<[f32; 4]>,
    /// preserveAspectRatio="none": the viewBox stretches to the size.
    pub stretch: bool,
}

/// The root's size the way the renderer reads it: width and height, else the viewBox's (one
/// missing follows its aspect), else 100 × 100.
pub(crate) fn root_size(text: &str) -> Option<Root> {
    let range = root_tag(text)?;
    let attrs = attributes(&text[range]);
    let get = |name: &str| attrs.iter().find(|a| a.0 == name).map(|a| a.1);
    let view_box = get("viewBox").and_then(|v| {
        let n: Vec<f32> = v
            .split(|c: char| c.is_whitespace() || c == ',')
            .filter(|s| !s.is_empty())
            .filter_map(|s| s.parse().ok())
            .collect();
        (n.len() == 4 && n[2] > 0. && n[3] > 0.).then(|| [n[0], n[1], n[2], n[3]])
    });
    let (w, h) = (
        get("width").and_then(length),
        get("height").and_then(length),
    );
    let natural = match (w, h, view_box) {
        (Some(w), Some(h), _) => (w, h),
        (Some(w), None, Some(b)) => (w, w * b[3] / b[2]),
        (None, Some(h), Some(b)) => (h * b[2] / b[3], h),
        (None, None, Some(b)) => (b[2], b[3]),
        (w, h, None) => (w.unwrap_or(100.), h.unwrap_or(100.)),
    };
    let stretch = get("preserveAspectRatio").is_some_and(|p| p.trim() == "none");
    Some(Root {
        natural,
        view_box,
        stretch,
    })
}

/// `text` with its root showing only `part` (CSS pixels of the drawing at its natural size),
/// `device` pixels big.
pub(crate) fn cropped(
    text: &str,
    root: &Root,
    part: [f32; 4],
    device: (u32, u32),
) -> Option<String> {
    let range = root_tag(text)?;
    let tag = &text[range.clone()];
    let (nw, nh) = root.natural;
    // From the drawing's pixels to the root's user space, as the renderer maps it (centered meet).
    let to_user = |x: f32, y: f32| -> (f32, f32, f32, f32) {
        let Some([bx, by, bw, bh]) = root.view_box else {
            return (x, y, 1., 1.);
        };
        if root.stretch {
            return (bx + x * bw / nw, by + y * bh / nh, bw / nw, bh / nh);
        }
        let s = (nw / bw).min(nh / bh);
        let (tx, ty) = ((nw - bw * s) / 2., (nh - bh * s) / 2.);
        (bx + (x - tx) / s, by + (y - ty) / s, 1. / s, 1. / s)
    };
    let (ux, uy, sx, sy) = to_user(part[0], part[1]);
    let view_box = format!("{ux} {uy} {} {}", part[2] * sx, part[3] * sy);
    let mut out = String::with_capacity(text.len() + 128);
    out.push_str(&text[..range.start]);
    let mut last = 0;
    for (name, _, at) in attributes(tag) {
        if matches!(name, "width" | "height" | "viewBox" | "preserveAspectRatio") {
            out.push_str(&tag[last..at.start]);
            last = at.end;
        }
    }
    let close = if tag.ends_with("/>") { 2 } else { 1 };
    out.push_str(&tag[last..tag.len() - close]);
    out.push_str(&format!(
        " width=\"{}\" height=\"{}\" viewBox=\"{view_box}\" preserveAspectRatio=\"none\"",
        device.0, device.1
    ));
    out.push_str(&tag[tag.len() - close..]);
    out.push_str(&text[range.end..]);
    Some(out)
}

fn no_root(root: Option<Root>) -> Option<SharedString> {
    root.is_none()
        .then(|| "It has no <svg> element at its root".into())
}

/// The picture drawn last: `part` of the drawing (CSS pixels at its natural size).
struct Drawn {
    image: Arc<RenderImage>,
    part: [f32; 4],
}

/// The view's layout as of a frame: the drawing's place in the view, in view pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Layout {
    room: (f32, f32),
    fit: f32,
    scale: f32,
    origin: (f32, f32),
}

pub struct SvgPreview {
    source: Arc<str>,
    root: Option<Root>,
    zoom: Zoom,
    contrast: bool,
    /// The view's bounds, as last painted.
    stage: Rc<Cell<Bounds<Pixels>>>,
    drawn: Option<Drawn>,
    /// The part and device size asked of the renderer last.
    asked: Option<([f32; 4], (u32, u32))>,
    broken: Option<SharedString>,
    drag: Option<Point<Pixels>>,
    focus: FocusHandle,
    _render: Task<()>,
    _live: Live,
}

impl SvgPreview {
    pub fn new(source: &str, cx: &mut Context<Self>) -> Self {
        cx.on_release(|this: &mut Self, cx| this.drop_image(cx))
            .detach();
        let root = root_size(source);
        SvgPreview {
            source: source.into(),
            broken: no_root(root),
            root,
            zoom: FIT,
            contrast: false,
            stage: Rc::new(Cell::new(Bounds::default())),
            drawn: None,
            asked: None,
            drag: None,
            focus: cx.focus_handle(),
            _render: Task::ready(()),
            _live: Live::new("SvgPreview"),
        }
    }

    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus
    }

    /// Draws other text (typed into, or changed on disk); the zoom stays.
    pub fn set_source(&mut self, source: &str, cx: &mut Context<Self>) {
        if *self.source == *source {
            return;
        }
        self.source = source.into();
        self.root = root_size(source);
        self.asked = None;
        self.broken = no_root(self.root);
        cx.notify();
    }

    fn drop_image(&mut self, cx: &mut App) {
        if let Some(d) = self.drawn.take() {
            cx.drop_image(d.image, None);
        }
    }

    fn layout(&self) -> Option<Layout> {
        let natural = self.root?.natural;
        let room = self.stage.get().size;
        let room = (f32::from(room.width), f32::from(room.height));
        if room.0 <= 0. || room.1 <= 0. {
            return None;
        }
        let fit = ((room.0 - 2. * PAD) / natural.0)
            .min((room.1 - 2. * PAD) / natural.1)
            .max(0.);
        // Clamped here too: the file can change under a zoom chosen for a much smaller drawing.
        let scale = self
            .zoom
            .scale
            .unwrap_or(fit)
            .min(zoom_limits(natural, fit).1);
        let (w, h) = (natural.0 * scale, natural.1 * scale);
        Some(Layout {
            room,
            fit,
            scale,
            origin: (place(room.0, w, self.zoom.u), place(room.1, h, self.zoom.v)),
        })
    }

    /// Zooms by `factor`, keeping the point (x, y) of the view where it is.
    fn zoom_at(&mut self, factor: f32, x: f32, y: f32, cx: &mut Context<Self>) {
        let (Some(l), Some(root)) = (self.layout(), self.root) else {
            return;
        };
        let (n, z) = (root.natural, self.zoom);
        let (min, max) = zoom_limits(n, l.fit);
        let to = (l.scale * factor).clamp(min, max);
        self.zoom = Zoom {
            scale: Some(to),
            u: zoom_axis(l.room.0, n.0 * l.scale, n.0 * to, z.u, x),
            v: zoom_axis(l.room.1, n.1 * l.scale, n.1 * to, z.v, y),
        };
        cx.notify();
    }

    fn pan_by(&mut self, dx: f32, dy: f32, cx: &mut Context<Self>) {
        let (Some(l), Some(root)) = (self.layout(), self.root) else {
            return;
        };
        let n = root.natural;
        self.zoom = Zoom {
            scale: Some(l.scale),
            u: pan_axis(l.room.0, n.0 * l.scale, self.zoom.u, dx),
            v: pan_axis(l.room.1, n.1 * l.scale, self.zoom.v, dy),
        };
        cx.notify();
    }

    fn set_zoom(&mut self, zoom: Zoom, cx: &mut Context<Self>) {
        self.zoom = zoom;
        cx.notify();
    }

    fn pannable(&self) -> bool {
        match (self.layout(), self.root) {
            (Some(l), Some(r)) => {
                r.natural.0 * l.scale > l.room.0 + 0.5 || r.natural.1 * l.scale > l.room.1 + 0.5
            }
            _ => false,
        }
    }

    /// Asks for the part on screen at the display's density, once the view settles.
    fn draw_visible(&mut self, l: &Layout, window: &Window, cx: &mut Context<Self>) {
        let Some(root) = self.root else { return };
        let n = root.natural;
        let (w, h) = (n.0 * l.scale, n.1 * l.scale);
        let (x0, y0) = (l.origin.0.max(0.), l.origin.1.max(0.));
        let (x1, y1) = (
            (l.origin.0 + w).min(l.room.0),
            (l.origin.1 + h).min(l.room.1),
        );
        if x1 <= x0 || y1 <= y0 || l.scale <= 0. {
            return;
        }
        let sf = window.scale_factor();
        let device = (
            ((x1 - x0) * sf).round().max(1.) as u32,
            ((y1 - y0) * sf).round().max(1.) as u32,
        );
        let part = [
            (x0 - l.origin.0) / l.scale,
            (y0 - l.origin.1) / l.scale,
            (x1 - x0) / l.scale,
            (y1 - y0) / l.scale,
        ];
        if self.asked == Some((part, device)) {
            return;
        }
        self.asked = Some((part, device));
        let (renderer, source, wait) =
            (cx.svg_renderer(), self.source.clone(), self.drawn.is_some());
        self._render = cx.spawn(async move |this, cx| {
            if wait {
                cx.background_executor().timer(SETTLE).await;
            }
            let drawn = cx
                .background_spawn(async move {
                    let text = cropped(&source, &root, part, device).unwrap_or_default();
                    let parsed = renderer.parse_svg(text.as_bytes())?;
                    let px = |n: u32| DevicePixels(n as i32);
                    renderer.render_parsed(
                        &parsed,
                        SvgSize::ExactSize(size(px(device.0), px(device.1))),
                    )
                })
                .await;
            _ = this.update(cx, |this, cx| {
                this.drop_image(cx);
                match drawn {
                    Ok(image) => {
                        this.broken = None;
                        this.drawn = Some(Drawn { image, part });
                    }
                    Err(e) => this.broken = Some(e.to_string().into()),
                }
                cx.notify();
            });
        });
    }

    fn backdrop(&self, cx: &App) -> (Option<Hsla>, Hsla) {
        let t = cx.app_theme();
        let rgb = |c: u32| -> Hsla { gpui_kit::rgb(c).into() };
        match (self.contrast, t.dark) {
            (false, _) => (None, t.foreground.opacity(0.08)),
            (true, true) => (Some(rgb(0xffffff)), rgb(0xe8e8e8)),
            (true, false) => (Some(rgb(0x181818)), rgb(0x272727)),
        }
    }
}

/// The checkerboard behind the drawing (`drawing`, window coordinates), only where it's in
/// `clip`: squares start at the drawing's corner, the way a background starts at its box's.
fn paint_checkerboard(
    drawing: Bounds<Pixels>,
    clip: Bounds<Pixels>,
    (base, square): (Option<Hsla>, Hsla),
    window: &mut Window,
) {
    let shown = drawing.intersect(&clip);
    if shown.size.width <= px(0.) || shown.size.height <= px(0.) {
        return;
    }
    if let Some(base) = base {
        window.paint_quad(fill(shown, base));
    }
    let rel = |a: Pixels, o: Pixels| (f32::from(a - o) / SQUARE).floor() as i64;
    let (c0, c1) = (
        rel(shown.left(), drawing.left()),
        rel(shown.right(), drawing.left()),
    );
    let (r0, r1) = (
        rel(shown.top(), drawing.top()),
        rel(shown.bottom(), drawing.top()),
    );
    for r in r0..=r1 {
        for c in c0..=c1 {
            // The top right and bottom left of each 2 × 2.
            if (r + c) % 2 == 0 {
                continue;
            }
            let cell = Bounds::new(
                point(
                    drawing.left() + px(c as f32 * SQUARE),
                    drawing.top() + px(r as f32 * SQUARE),
                ),
                size(px(SQUARE), px(SQUARE)),
            )
            .intersect(&shown);
            if cell.size.width > px(0.) && cell.size.height > px(0.) {
                window.paint_quad(fill(cell, square));
            }
        }
    }
}

impl Render for SvgPreview {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.app_theme().clone();
        let layout = self.layout();
        if let Some(l) = &layout {
            self.draw_visible(l, window, cx);
        }
        let natural = self.root.map(|r| r.natural);
        let backdrop = self.backdrop(cx);
        let stage = self.stage.clone();
        let me = cx.weak_entity();
        // The drawing's box and the picture's, relative to the view.
        let boxes = layout.zip(natural).map(|(l, n)| {
            let at =
                |x: f32, y: f32| point(px(l.origin.0 + x * l.scale), px(l.origin.1 + y * l.scale));
            let drawing = Bounds::new(at(0., 0.), size(px(n.0 * l.scale), px(n.1 * l.scale)));
            let picture = self.drawn.as_ref().map(|d| {
                let p = d.part;
                (
                    d.image.clone(),
                    Bounds::new(at(p[0], p[1]), size(px(p[2] * l.scale), px(p[3] * l.scale))),
                )
            });
            (drawing, picture)
        });
        let drawing = boxes.as_ref().map(|b| b.0);
        let measure = canvas(
            move |bounds, _, _| bounds,
            move |bounds, _, window, cx| {
                if stage.get() != bounds {
                    stage.set(bounds);
                    // Laid out anew at this size.
                    let me = me.clone();
                    cx.defer(move |cx| _ = me.update(cx, |_, cx| cx.notify()));
                }
                if let Some(d) = drawing {
                    let d = Bounds::new(bounds.origin + d.origin, d.size);
                    paint_checkerboard(d, bounds, backdrop, window);
                }
            },
        )
        .absolute()
        .size_full();
        let picture = boxes.and_then(|b| b.1).map(|(image, at)| {
            img(ImageSource::Render(image))
                .absolute()
                .left(at.origin.x)
                .top(at.origin.y)
                .w(at.size.width)
                .h(at.size.height)
                .object_fit(ObjectFit::Fill)
        });
        let pannable = self.pannable();
        let dragging = self.drag.is_some();
        let label = match self.zoom.scale {
            None => "Fit".to_string(),
            Some(_) => layout.map_or(String::new(), |l| format!("{}%", (l.scale * 100.).round())),
        };
        let actual = self.zoom.scale == Some(1.);
        let stage_el = div()
            .id("svg-stage")
            .relative()
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .bg(t.code(t.background))
            .when(pannable, |d| {
                d.cursor(if dragging {
                    CursorStyle::ClosedHand
                } else {
                    CursorStyle::OpenHand
                })
            })
            .on_scroll_wheel(cx.listener(|this, e: &ScrollWheelEvent, window, cx| {
                let origin = this.stage.get().origin;
                let delta = e.delta.pixel_delta(window.line_height());
                // Pinches come as ctrl+wheel with small steps.
                let k = if e.modifiers.control { 0.01 } else { 0.002 };
                let at = e.position - origin;
                this.zoom_at(
                    (f32::from(delta.y) * k).exp(),
                    f32::from(at.x),
                    f32::from(at.y),
                    cx,
                );
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, e: &MouseDownEvent, window, cx| {
                    this.focus.focus(window, cx);
                    if e.click_count >= 2 {
                        return this.set_zoom(FIT, cx);
                    }
                    if this.pannable() {
                        this.drag = Some(e.position);
                        cx.notify();
                    }
                }),
            )
            .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, _, cx| {
                let Some(from) = this.drag else { return };
                if e.pressed_button != Some(MouseButton::Left) {
                    this.drag = None;
                    return cx.notify();
                }
                this.drag = Some(e.position);
                let d = e.position - from;
                this.pan_by(f32::from(d.x), f32::from(d.y), cx);
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.drag = None;
                    cx.notify();
                }),
            )
            .child(measure)
            .children(picture)
            .when_some(self.broken.clone(), |d, problem| {
                d.child(
                    div()
                        .absolute()
                        .size_full()
                        .flex()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .p_4()
                        .gap_1()
                        .child(
                            div()
                                .text_size(px(12.5))
                                .text_color(t.muted_foreground)
                                .child("This SVG can't be rendered"),
                        )
                        .child(
                            div()
                                .max_w(px(560.))
                                .text_center()
                                .text_size(px(11.5))
                                .text_color(t.subtle)
                                .child(format!("{problem}. Switch to Code to see it.")),
                        ),
                )
            });
        let contrast_tip = match (self.contrast, t.dark) {
            (true, _) => "Theme background",
            (false, true) => "Light background",
            (false, false) => "Dark background",
        };
        div()
            .id("svg-preview")
            .key_context("ImageView")
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .flex_col()
            .on_action(cx.listener(|this, _: &media::ZoomIn, _, cx| {
                if let Some(l) = this.layout() {
                    this.zoom_at(1.25, l.room.0 / 2., l.room.1 / 2., cx)
                }
            }))
            .on_action(cx.listener(|this, _: &media::ZoomOut, _, cx| {
                if let Some(l) = this.layout() {
                    this.zoom_at(0.8, l.room.0 / 2., l.room.1 / 2., cx)
                }
            }))
            .on_action(cx.listener(|this, _: &media::Fit, _, cx| this.set_zoom(FIT, cx)))
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| {
                let m = &e.keystroke.modifiers;
                if m.platform || m.control || m.alt {
                    return;
                }
                let step = 48.;
                let (dx, dy) = match e.keystroke.key.as_str() {
                    "left" => (step, 0.),
                    "right" => (-step, 0.),
                    "up" => (0., step),
                    "down" => (0., -step),
                    _ => return,
                };
                cx.stop_propagation();
                this.pan_by(dx, dy, cx);
            }))
            .child(stage_el)
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .h(px(28.))
                    .border_t_1()
                    .border_color(t.border)
                    .text_size(px(11.5))
                    .text_color(t.muted_foreground)
                    .children(natural.map(|(w, h)| format!("{} × {}", w.round(), h.round())))
                    .child(div().flex_1())
                    .child(div().min_w(px(40.)).text_center().child(label))
                    .child(segmented(
                        [
                            Button::new("svg-fit")
                                .ghost()
                                .xsmall()
                                .label("Fit")
                                .tooltip("Scroll to zoom, drag to pan, double-click to fit")
                                .toggle(self.zoom.scale.is_none(), cx)
                                .on_click(cx.listener(|this, _, _, cx| this.set_zoom(FIT, cx))),
                            Button::new("svg-actual")
                                .ghost()
                                .xsmall()
                                .label("1:1")
                                .tooltip("Scroll to zoom, drag to pan, double-click to fit")
                                .toggle(actual, cx)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    let zoom = Zoom {
                                        scale: Some(1.),
                                        ..FIT
                                    };
                                    this.set_zoom(zoom, cx)
                                })),
                        ],
                        cx,
                    ))
                    .child(
                        Button::new("svg-contrast")
                            .ghost()
                            .xsmall()
                            .icon(Icon::new(IconName::Contrast))
                            .tooltip(contrast_tip)
                            .toggle(self.contrast, cx)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.contrast = !this.contrast;
                                cx.notify();
                            })),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_root_size_follows_width_height_and_view_box() {
        let size = |s: &str| root_size(s).map(|r| r.natural);
        let doc = "<?xml version=\"1.0\"?>\n<!-- x > y -->\n<!DOCTYPE svg [<!ENTITY a \"b>\">]>\n";
        assert_eq!(
            size(&format!("{doc}<svg width=\"24\" height=\"12pt\">")),
            Some((24., 16.))
        );
        assert_eq!(
            size("<svg viewBox='0 0 48 24' width='100%'/>"),
            Some((48., 24.))
        );
        assert_eq!(
            size("<svg viewBox=\"0,0,48,24\" width=\"24\">"),
            Some((24., 12.))
        );
        assert_eq!(
            size("<svg xmlns='http://www.w3.org/2000/svg'>"),
            Some((100., 100.))
        );
        assert_eq!(size("<html><svg width='5'>"), None);
        assert_eq!(size("<svg:svg width='5' height='6'>"), Some((5., 6.)));
    }

    #[test]
    fn a_crop_rewrites_only_the_roots_size() {
        let text = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"100\" height=\"50\" viewBox=\"0 0 200 100\"><rect width=\"9\"/></svg>";
        let root = root_size(text).unwrap();
        let out = cropped(text, &root, [50., 0., 50., 25.], (200, 100)).unwrap();
        assert_eq!(
            out,
            "<svg xmlns=\"http://www.w3.org/2000/svg\"    width=\"200\" height=\"100\" viewBox=\"100 0 100 50\" preserveAspectRatio=\"none\"><rect width=\"9\"/></svg>"
        );
        // No viewBox: user space is the drawing's pixels. A letterboxed one is centered.
        let plain = root_size("<svg width='10' height='10'/>").unwrap();
        let out = cropped(
            "<svg width='10' height='10'/>",
            &plain,
            [2., 3., 4., 5.],
            (8, 10),
        )
        .unwrap();
        assert!(
            out.ends_with("viewBox=\"2 3 4 5\" preserveAspectRatio=\"none\"/>"),
            "{out}"
        );
        let wide = "<svg width='200' height='100' viewBox='0 0 100 100'>";
        let out = cropped(
            wide,
            &root_size(wide).unwrap(),
            [50., 0., 100., 100.],
            (1, 1),
        )
        .unwrap();
        assert!(out.contains("viewBox=\"0 0 100 100\""), "{out}");
    }

    struct Host(Option<gpui_kit::Entity<SvgPreview>>);

    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().children(self.0.clone())
        }
    }

    #[gpui_kit::test]
    fn the_part_on_screen_is_drawn_and_given_back(cx: &mut gpui_kit::TestAppContext) {
        let dir = std::env::temp_dir().join(format!("gitviber-files-test-{}", std::process::id()));
        // SAFETY: the tests that read the config dir all point it at a temporary folder.
        unsafe { std::env::set_var("GITVIBER_CONFIG_DIR", &dir) };
        cx.update(gitviber_ui::init);
        let (host, cx) = cx.add_window_view(|_, _| Host(None));
        let snapshot = cx.update(|_, cx| cx.leak_detector_snapshot());
        let svg = "<svg xmlns='http://www.w3.org/2000/svg' width='40' height='20'><rect width='40' height='20' fill='red'/></svg>";
        for _ in 0..2 {
            let view = cx.update(|_, cx| cx.new(|cx| SvgPreview::new(svg, cx)));
            host.update(cx, |h, cx| {
                h.0 = Some(view.clone());
                cx.notify();
            });
            for _ in 0..3 {
                cx.update(|window, cx| window.draw(cx).clear(cx));
                cx.run_until_parked();
            }
            let (drawn, layout, broken) = view.read_with(cx, |v, _| {
                (
                    v.drawn.as_ref().map(|d| (d.image.size(0), d.part)),
                    v.layout(),
                    v.broken.clone(),
                )
            });
            assert_eq!(broken, None);
            let (pixels, part) = drawn.expect("drawn");
            let l = layout.unwrap();
            // Fit: the whole drawing, at its size on screen.
            assert_eq!(part, [0., 0., 40., 20.]);
            let sf = cx.update(|window, _| window.scale_factor());
            assert_eq!(i32::from(pixels.width), (40. * l.scale * sf).round() as i32);
            // Broken markup says so.
            view.update(cx, |v, cx| v.set_source("<svg><g></svg>", cx));
            cx.update(|window, cx| window.draw(cx).clear(cx));
            cx.executor().advance_clock(SETTLE * 2);
            cx.run_until_parked();
            assert!(view.read_with(cx, |v, _| v.broken.is_some()));
            host.update(cx, |h, cx| {
                h.0 = None;
                cx.notify();
            });
            drop(view);
            cx.update(|window, cx| window.draw(cx).clear(cx));
            cx.run_until_parked();
        }
        cx.update(|_, cx| cx.assert_no_new_leaks(&snapshot));
    }

    #[test]
    fn zooming_keeps_the_point_under_the_pointer() {
        // A 100-wide drawing in a 200 room, zoomed 1 → 4 at x = 120: the point there stays.
        let u = zoom_axis(200., 100., 400., 0.5, 120.);
        let left = place(200., 400., u);
        assert!(((120. - left) / 400. - 0.7).abs() < 1e-4, "{u} {left}");
        // Panning stops at the edge.
        assert_eq!(pan_axis(200., 400., 0.5, 10_000.), 0.25);
        assert_eq!(place(200., 100., 0.1), 50.);
        let (min, max) = zoom_limits((24., 24.), 20.);
        assert_eq!((min, max), (0.5, 320.));
        assert_eq!(zoom_limits((4000., 1000.), 0.25).1, 4.096);
    }
}
