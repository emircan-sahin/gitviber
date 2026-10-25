//! What the SVG and image previews share: a picture fitted to its view or zoomed (the diff's
//! zoom model), pinch or ⌘-scroll zooming around the pointer, a drag, the wheel or the arrows
//! moving one bigger than the view, a double-click going between fit and 1:1, a checkerboard
//! behind what's see-through on the theme's background or a light or dark one, and the bar
//! under it: the size, the zoom, Fit / 1:1 and the background.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use gitviber_diff::model::zoom::{ACTUAL, FIT, Frame, TIP, Zoom};
use gitviber_ui::ActiveAppTheme as _;
use gitviber_ui::keymap::media;
use gitviber_ui::widgets::{Toggle as _, segmented};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::{
    AnyElement, App, Bounds, Context, CursorStyle, Div, FocusHandle, Hsla, ImageSource,
    InteractiveElement as _, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent,
    MouseMoveEvent, ObjectFit, ParentElement as _, PinchEvent, Pixels, Point, RenderImage,
    ScrollWheelEvent, Stateful, Styled as _, StyledImage as _, Window, canvas, div, fill, img,
    point, prelude::FluentBuilder as _, px, size,
};

/// An arrow key moves the picture this far.
const STEP: f32 = 48.;
/// A checkerboard square, in view pixels.
const SQUARE: f32 = 8.;

/// A picture drawn: `part` of it (x, y, width, height in its own pixels).
pub(crate) struct Drawn {
    pub image: Arc<RenderImage>,
    pub part: [f32; 4],
}

/// A preview's zoom and what its pointer is doing.
pub(crate) struct Zoomer {
    pub zoom: Zoom,
    contrast: bool,
    /// The stage's bounds, as last painted.
    pub stage: Rc<Cell<Bounds<Pixels>>>,
    /// The pointer while a drag moves the picture.
    drag: Option<Point<Pixels>>,
    /// Set as the stage paints: an animation waits while its tab is hidden.
    pub painted: Rc<Cell<bool>>,
    pub focus: FocusHandle,
}

impl Zoomer {
    pub fn new(cx: &mut App) -> Self {
        Zoomer {
            zoom: FIT,
            contrast: false,
            stage: Rc::new(Cell::new(Bounds::default())),
            drag: None,
            painted: Rc::default(),
            focus: cx.focus_handle(),
        }
    }
}

pub(crate) trait Zoomable: Sized + 'static {
    /// Fitting may enlarge it past its own size: a drawing's does, an image's pixels don't.
    const GROW: bool;
    fn zoomer(&self) -> &Zoomer;
    fn zoomer_mut(&mut self) -> &mut Zoomer;
    /// Its own size, once known.
    fn natural(&self) -> Option<(f32, f32)>;

    /// How it sits in the stage as last painted.
    fn frame(&self) -> Option<Frame> {
        let natural = self.natural()?;
        let room = self.zoomer().stage.get().size;
        let room = (f32::from(room.width), f32::from(room.height));
        (room.0 > 0. && room.1 > 0. && natural.0 > 0. && natural.1 > 0.).then_some(Frame {
            room,
            natural,
            grow: Self::GROW,
        })
    }

    fn set_zoom(&mut self, zoom: Zoom, cx: &mut Context<Self>) {
        self.zoomer_mut().zoom = zoom;
        cx.notify();
    }

    /// Zooms by `factor`, keeping the point `at` of the stage (its middle if None) in place.
    fn zoom_at(&mut self, factor: f32, at: Option<(f32, f32)>, cx: &mut Context<Self>) {
        if let Some(f) = self.frame() {
            let at = at.unwrap_or((f.room.0 / 2., f.room.1 / 2.));
            let zoom = f.zoomed(self.zoomer().zoom, factor, at);
            self.set_zoom(zoom, cx);
        }
    }

    fn pan_by(&mut self, delta: (f32, f32), cx: &mut Context<Self>) {
        if let Some(f) = self.frame() {
            let zoom = f.panned(self.zoomer().zoom, delta);
            self.set_zoom(zoom, cx);
        }
    }
}

/// Where `position` (the window's) is in the stage.
fn in_stage<T: Zoomable>(this: &T, position: Point<Pixels>) -> (f32, f32) {
    let at = position - this.zoomer().stage.get().origin;
    (at.x.into(), at.y.into())
}

/// ⌘ (or ⌃) and the wheel zoom around the pointer; the wheel alone moves a picture bigger than
/// the stage, and otherwise scrolls what holds the preview.
fn wheel<T: Zoomable>(this: &mut T, e: &ScrollWheelEvent, window: &Window, cx: &mut Context<T>) {
    let Some(f) = this.frame() else { return };
    let d = e.delta.pixel_delta(window.line_height());
    let (dx, dy): (f32, f32) = (d.x.into(), d.y.into());
    if e.modifiers.platform || e.modifiers.control {
        cx.stop_propagation();
        // ⌃ and the wheel come as small steps, as a pinch does elsewhere.
        let k = if e.modifiers.control { 0.01 } else { 0.002 };
        let at = in_stage(this, e.position);
        this.zoom_at((dy * k).exp(), Some(at), cx);
    } else if f.pannable(this.zoomer().zoom) {
        cx.stop_propagation();
        this.pan_by((dx, dy), cx);
    }
}

/// The light or dark backdrop's color, if one is on, and its squares'.
fn backdrop(contrast: bool, cx: &App) -> (Option<Hsla>, Hsla) {
    let t = cx.app_theme();
    let rgb = |c: u32| -> Hsla { gpui_kit::rgb(c).into() };
    match (contrast, t.dark) {
        (false, _) => (None, t.foreground.opacity(0.08)),
        (true, true) => (Some(rgb(0xffffff)), rgb(0xe8e8e8)),
        (true, false) => (Some(rgb(0x181818)), rgb(0x272727)),
    }
}

/// The checkerboard behind the picture (`picture`, window coordinates), only where it's in
/// `clip`: squares start at the picture's corner, the way a background starts at its box's.
fn paint_checkerboard(
    picture: Bounds<Pixels>,
    clip: Bounds<Pixels>,
    (base, square): (Option<Hsla>, Hsla),
    window: &mut Window,
) {
    let shown = picture.intersect(&clip);
    if shown.is_empty() {
        return;
    }
    if let Some(base) = base {
        window.paint_quad(fill(shown, base));
    }
    let at = |a: Pixels, o: Pixels| f32::from(a - o) / SQUARE;
    let (c0, c1) = (
        at(shown.left(), picture.left()).floor() as i64,
        at(shown.right(), picture.left()).ceil() as i64,
    );
    let (r0, r1) = (
        at(shown.top(), picture.top()).floor() as i64,
        at(shown.bottom(), picture.top()).ceil() as i64,
    );
    let sq = px(SQUARE);
    for r in r0..r1 {
        // The top right and bottom left of each 2 × 2.
        let first = c0 + (r + c0 + 1).rem_euclid(2);
        for c in (first..c1).step_by(2) {
            let cell = Bounds::new(
                picture.origin + point(sq * c as f32, sq * r as f32),
                size(sq, sq),
            )
            .intersect(&shown);
            if !cell.is_empty() {
                window.paint_quad(fill(cell, square));
            }
        }
    }
}

/// Why the picture can't be drawn, over the stage.
pub(crate) fn problem(title: &'static str, detail: String, cx: &App) -> AnyElement {
    let t = cx.app_theme();
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
                .child(title),
        )
        .child(
            div()
                .max_w(px(560.))
                .text_center()
                .text_size(px(11.5))
                .text_color(t.subtle)
                .child(detail),
        )
        .into_any_element()
}

/// The stage: the checkerboard behind the picture, `pictures` over it at their parts, and
/// `over` on top.
pub(crate) fn stage<'a, T: Zoomable>(
    this: &T,
    pictures: impl IntoIterator<Item = &'a Drawn>,
    over: Option<AnyElement>,
    cx: &mut Context<T>,
) -> Stateful<Div> {
    let t = cx.app_theme().clone();
    let z = this.zoomer();
    let frame = this.frame();
    let placed = frame.map(|f| (f.drawn(z.zoom), f.scale(z.zoom)));
    let colors = backdrop(z.contrast, cx);
    let (stage, painted, me) = (z.stage.clone(), z.painted.clone(), cx.weak_entity());
    let picture = placed.map(|(d, _)| d);
    let measure = canvas(
        |_, _, _| {},
        move |bounds, _, window, cx| {
            painted.set(true);
            if stage.get() != bounds {
                stage.set(bounds);
                // Laid out anew at this size.
                let me = me.clone();
                cx.defer(move |cx| _ = me.update(cx, |_, cx| cx.notify()));
            }
            if let Some([x, y, w, h]) = picture {
                let p = Bounds::new(bounds.origin + point(px(x), px(y)), size(px(w), px(h)));
                paint_checkerboard(p, bounds, colors, window);
            }
        },
    )
    .absolute()
    .size_full();
    let images: Vec<_> = match placed {
        Some(([x, y, _, _], s)) => pictures
            .into_iter()
            .map(|d| {
                let [px0, py0, pw, ph] = d.part;
                img(ImageSource::Render(d.image.clone()))
                    .absolute()
                    .left(px(x + px0 * s))
                    .top(px(y + py0 * s))
                    .w(px(pw * s))
                    .h(px(ph * s))
                    .object_fit(ObjectFit::Fill)
            })
            .collect(),
        None => vec![],
    };
    let pannable = frame.is_some_and(|f| f.pannable(z.zoom));
    let dragging = z.drag.is_some();
    div()
        .id("picture-stage")
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
        .on_scroll_wheel(
            cx.listener(|this: &mut T, e: &ScrollWheelEvent, window, cx| {
                wheel(this, e, window, cx)
            }),
        )
        .on_pinch(cx.listener(|this: &mut T, e: &PinchEvent, _, cx| {
            cx.stop_propagation();
            let at = in_stage(this, e.position);
            this.zoom_at((1. + e.delta).max(0.1), Some(at), cx);
        }))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this: &mut T, e: &MouseDownEvent, window, cx| {
                window.focus(&this.zoomer().focus, cx);
                let Some(f) = this.frame() else { return };
                let zoom = this.zoomer().zoom;
                if e.click_count >= 2 {
                    this.zoomer_mut().drag = None;
                    let at = in_stage(this, e.position);
                    return this.set_zoom(f.toggled(zoom, at), cx);
                }
                if f.pannable(zoom) {
                    this.zoomer_mut().drag = Some(e.position);
                    cx.notify();
                }
            }),
        )
        .on_mouse_move(cx.listener(|this: &mut T, e: &MouseMoveEvent, _, cx| {
            let Some(from) = this.zoomer().drag else {
                return;
            };
            if e.pressed_button != Some(MouseButton::Left) {
                this.zoomer_mut().drag = None;
                return cx.notify();
            }
            this.zoomer_mut().drag = Some(e.position);
            let d = e.position - from;
            this.pan_by((d.x.into(), d.y.into()), cx);
        }))
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(|this: &mut T, _, _, cx| {
                this.zoomer_mut().drag = None;
                cx.notify();
            }),
        )
        .on_mouse_up_out(
            MouseButton::Left,
            cx.listener(|this: &mut T, _, _, cx| {
                if this.zoomer_mut().drag.take().is_some() {
                    cx.notify();
                }
            }),
        )
        .child(measure)
        .children(images)
        .children(over)
}

/// The bar under the stage: `details` (the size), the zoom, Fit / 1:1 and the background.
pub(crate) fn bar<T: Zoomable>(this: &T, details: Option<String>, cx: &mut Context<T>) -> Div {
    let t = cx.app_theme().clone();
    let z = this.zoomer();
    let label = this
        .frame()
        .map(|f| format!("{}%", (f.scale(z.zoom) * 100.).round()));
    let contrast_tip = match (z.contrast, t.dark) {
        (true, _) => "Theme background",
        (false, true) => "Light background",
        (false, false) => "Dark background",
    };
    let zoom_button = |id: &'static str, label: &'static str, to: Zoom, cx: &mut Context<T>| {
        Button::new(id)
            .ghost()
            .xsmall()
            .label(label)
            .tooltip(TIP)
            .toggle(z.zoom.scale == to.scale, cx)
            .on_click(cx.listener(move |this: &mut T, _, _, cx| this.set_zoom(to, cx)))
    };
    let buttons = [
        zoom_button("picture-fit", "Fit", FIT, cx),
        zoom_button("picture-actual", "1:1", ACTUAL, cx),
    ];
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
        .children(details)
        .child(div().flex_1())
        .child(div().min_w(px(40.)).text_center().children(label))
        .child(segmented(buttons, cx))
        .child(
            Button::new("picture-contrast")
                .ghost()
                .xsmall()
                .icon(Icon::new(IconName::Contrast))
                .tooltip(contrast_tip)
                .toggle(z.contrast, cx)
                .on_click(cx.listener(|this: &mut T, _, _, cx| {
                    let z = this.zoomer_mut();
                    z.contrast = !z.contrast;
                    cx.notify();
                })),
        )
}

/// The preview's root: its focus, + / − / 0 and the arrows.
pub(crate) fn root<T: Zoomable>(this: &T, cx: &mut Context<T>) -> Stateful<Div> {
    div()
        .id("picture")
        .key_context("ImageView")
        .track_focus(&this.zoomer().focus)
        .size_full()
        .flex()
        .flex_col()
        .on_action(
            cx.listener(|this: &mut T, _: &media::ZoomIn, _, cx| this.zoom_at(1.25, None, cx)),
        )
        .on_action(
            cx.listener(|this: &mut T, _: &media::ZoomOut, _, cx| this.zoom_at(0.8, None, cx)),
        )
        .on_action(cx.listener(|this: &mut T, _: &media::Fit, _, cx| this.set_zoom(FIT, cx)))
        .on_key_down(cx.listener(|this: &mut T, e: &KeyDownEvent, _, cx| {
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
            this.pan_by(delta, cx);
        }))
}
