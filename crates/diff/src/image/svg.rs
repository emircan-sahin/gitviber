//! An SVG's Preview: both versions, fitted or at 1:1, sharing one zoom so they stay lined up.
//! Scroll zooms around the pointer, a drag (or the arrows) moves a drawing bigger than its
//! room, a double-click fits, and a checkerboard on the theme's background or a light or dark
//! one shows what's see-through. Zoomed past the whole drawing's pixels, only the part on screen
//! is drawn again, at the display's density and off the main thread.

use super::*;
use crate::model::zoom::{FIT, Frame};
use gitviber_ui::keymap::media;
use gpui_kit::{
    Corners, CursorStyle, Hsla, KeyDownEvent, ScrollWheelEvent, fill, point,
    prelude::FluentBuilder as _, rgb,
};
use std::time::Duration;

/// Zooming or moving settles this long before the part on screen is drawn again.
const SETTLE: Duration = Duration::from_millis(90);
/// A checkerboard square, in view pixels.
const SQUARE: f32 = 8.;
/// An arrow key moves the drawing this far.
const STEP: f32 = 48.;

/// A zoomed side's part on screen (x, y, width, height of the drawing at its own size), drawn
/// sharp.
pub(crate) struct Crop {
    image: Arc<RenderImage>,
    part: [f32; 4],
}

/// The part and the pixels a crop was asked for, rounded.
pub(crate) type CropKey = [i32; 6];

/// A crop's pixels' size and the BGRA of its middle one (tests).
pub type CropShown = ((u32, u32), [u8; 4]);

/// What one side paints into its room.
struct Layer {
    zoom: Zoom,
    /// What's fitted and moved: the side's drawing, or the frame both share.
    frame: (f32, f32),
    natural: (f32, f32),
    image: Arc<RenderImage>,
    crop: Option<(Arc<RenderImage>, [f32; 4])>,
    /// The share of the room's width shown (swipe).
    clip: Option<(f32, f32)>,
    /// The backdrop under the frame: its color, its squares'.
    checker: Option<(Option<Hsla>, Hsla)>,
    room: Rc<Cell<Bounds<Pixels>>>,
}

impl Layer {
    fn paint(&self, bounds: Bounds<Pixels>, window: &mut Window) {
        self.room.set(bounds);
        let f = Frame {
            room: (bounds.size.width.into(), bounds.size.height.into()),
            natural: self.frame,
        };
        if f.room.0 <= 0. || f.room.1 <= 0. {
            return;
        }
        let [x, y, w, h] = f.drawn(self.zoom);
        let s = f.scale(self.zoom);
        let origin = bounds.origin + point(px(x), px(y));
        let clip = match self.clip {
            Some((a, b)) => Bounds::new(
                bounds.origin + point(bounds.size.width * a, px(0.)),
                size(bounds.size.width * (b - a), bounds.size.height),
            ),
            None => bounds,
        };
        if let Some(colors) = self.checker {
            let frame = Bounds::new(origin, size(px(w), px(h)));
            paint_checkerboard(frame.intersect(&clip), origin, colors, window);
        }
        let (nw, nh) = self.natural;
        let whole = Bounds::new(origin, size(px(nw * s), px(nh * s)));
        let none = Corners::default();
        window
            .paint_image(clip, whole, none, self.image.clone(), 0, false)
            .ok();
        if let Some((image, [px0, py0, pw, ph])) = &self.crop {
            let part = Bounds::new(
                origin + point(px(px0 * s), px(py0 * s)),
                size(px(pw * s), px(ph * s)),
            );
            window
                .paint_image(clip, part, none, image.clone(), 0, false)
                .ok();
        }
    }
}

/// Squares of `colors.1` from `origin` over `visible`, on `colors.0` if there's one.
fn paint_checkerboard(
    visible: Bounds<Pixels>,
    origin: Point<Pixels>,
    colors: (Option<Hsla>, Hsla),
    window: &mut Window,
) {
    if visible.is_empty() {
        return;
    }
    if let Some(back) = colors.0 {
        window.paint_quad(fill(visible, back));
    }
    let at = |v: Pixels, from: Pixels| f32::from(v - from) / SQUARE;
    let (c0, c1) = (
        at(visible.left(), origin.x).floor() as i32,
        at(visible.right(), origin.x).ceil() as i32,
    );
    let (r0, r1) = (
        at(visible.top(), origin.y).floor() as i32,
        at(visible.bottom(), origin.y).ceil() as i32,
    );
    let sq = px(SQUARE);
    for r in r0..r1 {
        // Every other square of the row, starting on the row's own parity.
        let first = c0 + (r + c0).rem_euclid(2);
        for c in (first..c1).step_by(2) {
            let square = Bounds::new(origin + point(sq * c as f32, sq * r as f32), size(sq, sq))
                .intersect(&visible);
            if !square.is_empty() {
                window.paint_quad(fill(square, colors.1));
            }
        }
    }
}

/// The backdrop's color, if it has one, and its squares'.
fn checker(backdrop: Backdrop, foreground: Hsla) -> (Option<Hsla>, Hsla) {
    match backdrop {
        Backdrop::Theme => (None, foreground.opacity(0.08)),
        Backdrop::Light => (Some(rgb(0xffffff).into()), rgb(0xe8e8e8).into()),
        Backdrop::Dark => (Some(rgb(0x181818).into()), rgb(0x272727).into()),
    }
}

impl ImageCompare {
    pub(super) fn drop_crops(&mut self, cx: &mut App) {
        self.asked = [None, None];
        self._crop = Task::ready(());
        for crop in self.crops.iter_mut().filter_map(Option::take) {
            cx.drop_image(crop.image, None);
        }
    }

    fn natural(&self, i: usize) -> Option<(f32, f32)> {
        match &self.sides[i] {
            Side::Ready(v) => v.natural,
            _ => None,
        }
    }

    /// The frame two versions laid over each other share: as big as the bigger of them.
    fn overlay_box(&self) -> Option<(f32, f32)> {
        let (a, b) = (self.natural(0)?, self.natural(1)?);
        Some((a.0.max(b.0), a.1.max(b.1)))
    }

    /// Side `i`'s frame as last drawn, and its room.
    fn frame(&self, i: usize, overlay: bool) -> Option<(Frame, Bounds<Pixels>)> {
        let (room, natural) = if overlay {
            (self.stage.get(), self.overlay_box()?)
        } else {
            (self.rooms[i].get(), self.natural(i)?)
        };
        let size = (room.size.width.into(), room.size.height.into());
        (size.0 > 0. && size.1 > 0.).then_some((
            Frame {
                room: size,
                natural,
            },
            room,
        ))
    }

    /// The side the keys zoom and move: After, unless there's only Before.
    fn key_side(&self) -> usize {
        if self.natural(1).is_some() { 1 } else { 0 }
    }

    fn overlaid(&self, cx: &App) -> bool {
        self.mode(cx) != ImageMode::Side
    }

    fn zoom_at(&mut self, i: usize, factor: f32, at: Option<(f32, f32)>, cx: &mut Context<Self>) {
        let Some((f, _)) = self.frame(i, self.overlaid(cx)) else {
            return;
        };
        let at = at.unwrap_or((f.room.0 / 2., f.room.1 / 2.));
        self.zoom = f.zoomed(self.zoom, factor, at);
        cx.notify();
    }

    fn pan_by(&mut self, i: usize, delta: (f32, f32), cx: &mut Context<Self>) {
        let Some((f, _)) = self.frame(i, self.overlaid(cx)) else {
            return;
        };
        self.zoom = f.panned(self.zoom, delta);
        cx.notify();
    }

    fn wheel(&mut self, i: usize, e: &ScrollWheelEvent, window: &Window, cx: &mut Context<Self>) {
        let Some((_, room)) = self.frame(i, self.overlaid(cx)) else {
            return;
        };
        cx.stop_propagation();
        let dy: f32 = e.delta.pixel_delta(window.line_height()).y.into();
        // A pinch comes as ⌃ and the wheel with small steps.
        let k = if e.modifiers.control { 0.01 } else { 0.002 };
        let at = e.position - room.origin;
        self.zoom_at(i, (dy * k).exp(), Some((at.x.into(), at.y.into())), cx);
    }

    fn press(&mut self, i: usize, e: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        if e.click_count == 2 {
            self.pan = None;
            return self.set_zoom(FIT, cx);
        }
        let overlay = self.overlaid(cx);
        if self
            .frame(i, overlay)
            .is_some_and(|(f, _)| f.pannable(self.zoom))
        {
            self.pan = Some(e.position);
            cx.notify();
        } else if overlay {
            self.dragging = true;
            self.drag(e.position.x, cx);
        }
    }

    fn moved(&mut self, i: usize, e: &MouseMoveEvent, cx: &mut Context<Self>) {
        if e.pressed_button != Some(MouseButton::Left) {
            return;
        }
        if let Some(last) = self.pan {
            self.pan = Some(e.position);
            let d = e.position - last;
            self.pan_by(i, (d.x.into(), d.y.into()), cx);
        } else if self.dragging {
            self.drag(e.position.x, cx);
        }
    }

    fn released(&mut self, cx: &mut Context<Self>) {
        self.dragging = false;
        if self.pan.take().is_some() {
            cx.notify();
        }
    }

    /// What each zoomed side should have drawn sharp now, asked for once the zoom settles. A
    /// side the whole drawing's pixels still cover gives its crop back.
    fn sync_crops(&mut self, overlay: bool, window: &Window, cx: &mut Context<Self>) {
        let density = window.scale_factor();
        let want = [0, 1].map(|i| {
            let Side::Ready(v) = &self.sides[i] else {
                return None;
            };
            let natural = v.natural?;
            let (f, _) = self.frame(i, overlay)?;
            let s = f.scale(self.zoom);
            if natural.0 * s * density <= v.image.size(0).width.0 as f32 * 1.1 {
                return None;
            }
            let [x, y, _, _] = f.drawn(self.zoom);
            let (x0, y0) = ((-x / s).max(0.), (-y / s).max(0.));
            let (x1, y1) = (
                ((f.room.0 - x) / s).min(natural.0),
                ((f.room.1 - y) / s).min(natural.1),
            );
            if x1 <= x0 || y1 <= y0 {
                return None;
            }
            let part = [x0, y0, x1 - x0, y1 - y0];
            let device = (
                ((x1 - x0) * s * density).ceil() as u32,
                ((y1 - y0) * s * density).ceil() as u32,
            );
            let key = [
                (x0 * 64.) as i32,
                (y0 * 64.) as i32,
                (part[2] * 64.) as i32,
                (part[3] * 64.) as i32,
                device.0 as i32,
                device.1 as i32,
            ];
            Some((key, part, device))
        });
        for (i, w) in want.iter().enumerate() {
            if w.is_none()
                && let Some(crop) = self.crops[i].take()
            {
                cx.drop_image(crop.image, None);
            }
        }
        let keys = want.map(|w| w.map(|(key, ..)| key));
        if keys == self.asked {
            return;
        }
        self.asked = keys;
        let jobs: Vec<_> = (0..2)
            .filter_map(|i| {
                let (_, part, device) = want[i]?;
                Some((i, self.texts[i].clone()?, part, device))
            })
            .collect();
        if jobs.is_empty() {
            self._crop = Task::ready(());
            return;
        }
        let svg = cx.svg_renderer();
        self._crop = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SETTLE).await;
            let drawn = cx
                .background_spawn(async move {
                    jobs.into_iter()
                        .filter_map(|(i, text, part, (w, h))| {
                            let text = crate::model::svg::crop(&text, part, (w, h))?;
                            let exact = SvgSize::ExactSize(size(
                                DevicePixels(w as i32),
                                DevicePixels(h as i32),
                            ));
                            let parsed = svg.parse_svg(text.as_bytes()).ok()?;
                            let image = svg.render_parsed(&parsed, exact).ok()?;
                            Some((i, Crop { image, part }))
                        })
                        .collect::<Vec<_>>()
                })
                .await;
            this.update(cx, |this, cx| {
                for (i, crop) in drawn {
                    if let Some(old) = this.crops[i].replace(crop) {
                        cx.drop_image(old.image, None);
                    }
                }
                cx.notify();
            })
            .ok();
        });
    }

    fn layer(
        &self,
        i: usize,
        overlay: bool,
        clip: Option<(f32, f32)>,
        checker: Option<(Option<Hsla>, Hsla)>,
    ) -> Option<AnyElement> {
        let Side::Ready(v) = &self.sides[i] else {
            return None;
        };
        let natural = v.natural?;
        let layer = Layer {
            zoom: self.zoom,
            frame: if overlay {
                self.overlay_box()?
            } else {
                natural
            },
            natural,
            image: v.image.clone(),
            crop: self.crops[i].as_ref().map(|c| (c.image.clone(), c.part)),
            clip,
            checker,
            room: if overlay {
                self.stage.clone()
            } else {
                self.rooms[i].clone()
            },
        };
        Some(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| layer.paint(bounds, window),
            )
            .absolute()
            .size_full()
            .into_any_element(),
        )
    }

    /// Onion skin's After, faded in from 0 to 100% along a short track.
    fn fade_track(&self, style: &DiffStyle, cx: &mut Context<Self>) -> AnyElement {
        let c = &style.colors;
        let track = self.fade.clone();
        let set = |this: &mut Self, x: Pixels, cx: &mut Context<Self>| {
            let b = this.fade.get();
            if b.size.width > px(0.) {
                this.opacity = ((x - b.origin.x) / b.size.width).clamp(0., 1.);
                cx.notify();
            }
        };
        h_flex()
            .gap_2()
            .child(format!("After at {}%", (self.opacity * 100.).round()))
            .child(
                div()
                    .id("svg-fade")
                    .relative()
                    .w(px(120.))
                    .h(px(14.))
                    .cursor_ew_resize()
                    .child(
                        canvas(move |b, _, _| track.set(b), |_, _, _, _| {})
                            .absolute()
                            .size_full(),
                    )
                    .child(
                        div()
                            .absolute()
                            .top(px(5.))
                            .w_full()
                            .h(px(4.))
                            .rounded_full()
                            .bg(c.border)
                            .child(
                                div()
                                    .h_full()
                                    .w(relative(self.opacity))
                                    .rounded_full()
                                    .bg(c.primary),
                            ),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            set(this, e.position.x, cx)
                        }),
                    )
                    .on_mouse_move(cx.listener(move |this, e: &MouseMoveEvent, _, cx| {
                        if e.pressed_button == Some(MouseButton::Left) {
                            set(this, e.position.x, cx)
                        }
                    })),
            )
            .into_any_element()
    }

    /// A room's pointer: the wheel zooms, a drag moves the drawing (or the divider and the fade
    /// when it fits), a double-click fits.
    fn room(
        &self,
        el: gpui_kit::Stateful<gpui_kit::Div>,
        i: usize,
        overlay: bool,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Stateful<gpui_kit::Div> {
        let pannable = self
            .frame(i, overlay)
            .is_some_and(|(f, _)| f.pannable(self.zoom));
        let cursor = match (pannable, self.pan.is_some()) {
            (true, true) => CursorStyle::ClosedHand,
            (true, false) => CursorStyle::OpenHand,
            (false, _) if overlay => CursorStyle::ResizeLeftRight,
            _ => CursorStyle::Arrow,
        };
        el.overflow_hidden()
            .cursor(cursor)
            .on_scroll_wheel(cx.listener(move |this, e: &ScrollWheelEvent, window, cx| {
                this.wheel(i, e, window, cx)
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                    this.press(i, e, window, cx)
                }),
            )
            .on_mouse_move(cx.listener(move |this, e: &MouseMoveEvent, _, cx| this.moved(i, e, cx)))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.released(cx)),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.released(cx)),
            )
    }

    /// Each side's sharp part: its pixels' size, and the BGRA of the middle one (tests).
    #[doc(hidden)]
    pub fn crops(&self) -> [Option<CropShown>; 2] {
        self.crops.each_ref().map(|c| {
            let c = c.as_ref()?;
            let s = c.image.size(0);
            let (w, h) = (s.width.0 as usize, s.height.0 as usize);
            let at = (h / 2 * w + w / 2) * 4;
            let px = c.image.as_bytes(0)?.get(at..at + 4)?;
            Some(((w as u32, h as u32), [px[0], px[1], px[2], px[3]]))
        })
    }

    pub(super) fn render_svg(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let style = DiffStyle::get(cx).clone();
        let c = &style.colors;
        let mode = self.mode(cx);
        let overlay = mode != ImageMode::Side;
        self.sync_crops(overlay, window, cx);
        let checker = checker(self.backdrop, c.foreground);
        let middle = (mode == ImageMode::Onion).then(|| self.fade_track(&style, cx));
        let bar = self.bar(&style, middle);
        let stage = match mode {
            ImageMode::Side => {
                // The two versions stack in the unified layout, as the diff's lines do.
                let stacked = DiffOptions::get(cx).layout == Layout::Unified;
                (if stacked { v_flex() } else { h_flex() })
                    .flex_1()
                    .min_h_0()
                    .gap_4()
                    .p_4()
                    .children((0..2).map(|i| {
                        let cell = div()
                            .id(("svg-side", i))
                            .relative()
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            .map(|el| if stacked { el.w_full() } else { el.h_full() });
                        match &self.sides[i] {
                            Side::Missing => cell
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_color(c.subtle)
                                .child("Not on this side")
                                .into_any_element(),
                            _ => self
                                .room(cell, i, false, cx)
                                .children(self.layer(i, false, None, Some(checker)))
                                .into_any_element(),
                        }
                    }))
                    .into_any_element()
            }
            ImageMode::Swipe => {
                let d = self.divider;
                let el = div().id("svg-stage").relative().flex_1().min_h_0().m_4();
                self.room(el, 1, true, cx)
                    .children(self.layer(0, true, Some((0., d)), Some(checker)))
                    .children(self.layer(1, true, Some((d, 1.)), Some(checker)))
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .h_full()
                            .left(relative(d))
                            .w(px(2.))
                            .bg(c.primary),
                    )
                    // The divider's handle moves it even while a drag moves the drawing.
                    .child(
                        div()
                            .id("svg-divider")
                            .absolute()
                            .top_0()
                            .h_full()
                            .left(relative(d))
                            .ml(px(-8.))
                            .w(px(16.))
                            .cursor_ew_resize()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, e: &MouseDownEvent, _, cx| {
                                    cx.stop_propagation();
                                    this.dragging = true;
                                    this.drag(e.position.x, cx);
                                }),
                            ),
                    )
                    .into_any_element()
            }
            ImageMode::Onion => {
                let el = div().id("svg-stage").relative().flex_1().min_h_0().m_4();
                self.room(el, 1, true, cx)
                    .children(self.layer(0, true, None, Some(checker)))
                    .child(
                        div()
                            .absolute()
                            .size_full()
                            .opacity(self.opacity)
                            .children(self.layer(1, true, None, None)),
                    )
                    .into_any_element()
            }
        };
        v_flex()
            .id("svg-preview")
            .key_context("ImageView")
            .track_focus(&self.focus)
            .size_full()
            .bg(c.background)
            .text_color(c.foreground)
            .child(bar)
            .child(stage)
            .on_action(cx.listener(|this, _: &media::ZoomIn, _, cx| {
                this.zoom_at(this.key_side(), 1.25, None, cx)
            }))
            .on_action(cx.listener(|this, _: &media::ZoomOut, _, cx| {
                this.zoom_at(this.key_side(), 0.8, None, cx)
            }))
            .on_action(cx.listener(|this, _: &media::Fit, _, cx| this.set_zoom(FIT, cx)))
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| {
                let m = &e.keystroke.modifiers;
                if m.platform || m.control || m.alt || m.shift {
                    return;
                }
                let delta = match e.keystroke.key.as_str() {
                    "left" => (STEP, 0.),
                    "right" => (-STEP, 0.),
                    "up" => (0., STEP),
                    "down" => (0., -STEP),
                    _ => return,
                };
                cx.stop_propagation();
                this.pan_by(this.key_side(), delta, cx);
            }))
    }
}
