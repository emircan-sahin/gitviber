//! The strip beside a diff's lines, in place of its scrollbar: where the changes and find's
//! matches are over the whole file, each side on its half, and the part in view. A click scrolls
//! there and a drag keeps scrolling, as a scrollbar's thumb does.

use crate::list::{Heights, Scroll};
use crate::model::Side;
use crate::model::overview::{ItemMarks, Mark};
use crate::style::DiffStyle;
use gpui_kit::{
    App, Bounds, DispatchPhase, Element, ElementId, EntityId, GlobalElementId, Hitbox,
    HitboxBehavior, Hsla, InspectorElementId, IntoElement, LayoutId, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, ScrollWheelEvent, Size, Style, Window, fill, point, px,
    relative, size,
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// As wide as the old diff's overview: two halves of 7.
pub const WIDTH: Pixels = px(14.);
/// The shortest a change and a match are drawn, so one line of a long file still shows.
const MIN_CHANGE: Pixels = px(2.);
const MIN_FOUND: Pixels = px(3.);
const MIN_THUMB: Pixels = px(10.);

/// Each kind's spans over the content, top to bottom, in its pixels.
#[derive(Default)]
pub struct Marks {
    spans: [Vec<(Pixels, Pixels)>; 4],
    /// The quads last drawn, for the strip's size and the content's height they were scaled to:
    /// marks are scaled once per change, not every frame.
    drawn: RefCell<Option<Drawn>>,
}

struct Drawn {
    strip: Size<Pixels>,
    content: Pixels,
    quads: Vec<(Mark, Bounds<Pixels>)>,
}

impl Marks {
    /// `mark` over `top..bottom`, below the spans of its kind already in.
    pub fn push(&mut self, mark: Mark, top: Pixels, bottom: Pixels) {
        let spans = &mut self.spans[mark as usize];
        match spans.last_mut() {
            Some(last) if last.1 >= top => last.1 = last.1.max(bottom),
            _ => spans.push((top, bottom)),
        }
    }

    /// `other`'s spans, `dy` further down.
    pub fn extend(&mut self, other: &Marks, dy: Pixels) {
        for mark in Mark::ALL {
            for &(top, bottom) in other.spans(mark) {
                self.push(mark, top + dy, bottom + dy);
            }
        }
    }

    /// The items' marks, each item where `heights` puts it.
    pub fn of_items(heights: &Heights, items: impl Iterator<Item = (usize, ItemMarks)>) -> Self {
        let mut out = Marks::default();
        for (ix, marks) in items {
            for mark in Mark::ALL {
                if marks[mark as usize] {
                    out.push(mark, heights.top(ix), heights.top(ix + 1));
                }
            }
        }
        out
    }

    pub fn spans(&self, mark: Mark) -> &[(Pixels, Pixels)] {
        &self.spans[mark as usize]
    }

    /// Every kind's spans, by `Mark as usize` (tests).
    pub fn all_spans(&self) -> [Vec<(Pixels, Pixels)>; 4] {
        self.spans.clone()
    }

    fn scaled(&self, strip: Size<Pixels>, content: Pixels) -> Vec<(Mark, Bounds<Pixels>)> {
        let k = strip.height / content;
        let half = strip.width / 2.;
        let mut out = vec![];
        for mark in Mark::ALL {
            let min = match mark {
                Mark::Removed | Mark::Added => MIN_CHANGE,
                Mark::FoundOld | Mark::FoundNew => MIN_FOUND,
            };
            let x = if mark.side() == Side::Old {
                px(0.)
            } else {
                half
            };
            let mut push = |(top, bottom): (Pixels, Pixels)| {
                out.push((mark, Bounds::new(point(x, top), size(half, bottom - top))))
            };
            // Spans closer than a pixel apart draw as one quad.
            let mut run: Option<(Pixels, Pixels)> = None;
            for &(top, bottom) in &self.spans[mark as usize] {
                let mut y = (top * k, (bottom * k).max(top * k + min));
                if y.1 > strip.height {
                    y = ((strip.height - min).max(px(0.)), strip.height);
                }
                match &mut run {
                    Some(r) if y.0 <= r.1 + px(1.) => r.1 = r.1.max(y.1),
                    _ => {
                        if let Some(r) = run.replace(y) {
                            push(r);
                        }
                    }
                }
            }
            if let Some(r) = run {
                push(r);
            }
        }
        out
    }

    fn paint(
        &self,
        bounds: Bounds<Pixels>,
        content: Pixels,
        colors: &[Hsla; 4],
        window: &mut Window,
    ) {
        let mut drawn = self.drawn.borrow_mut();
        let stale = drawn
            .as_ref()
            .is_none_or(|d| d.strip != bounds.size || d.content != content);
        if stale {
            *drawn = Some(Drawn {
                strip: bounds.size,
                content,
                quads: self.scaled(bounds.size, content),
            });
        }
        for (mark, quad) in drawn.iter().flat_map(|d| &d.quads) {
            let quad = Bounds::new(bounds.origin + quad.origin, quad.size);
            window.paint_quad(fill(quad, colors[*mark as usize]));
        }
    }
}

/// The overview of a list scrolled by `scroll`, whose content is `content` tall; `view` redraws
/// when it scrolls.
pub fn overview(
    marks: Rc<Marks>,
    content: Pixels,
    scroll: &Scroll,
    view: EntityId,
    style: &DiffStyle,
    cx: &App,
) -> Overview {
    let c = &style.colors;
    let found = crate::style::find_colors(cx).mark;
    Overview {
        id: ElementId::named_usize("diff-overview", view.as_u64() as usize),
        marks,
        content,
        scroll: scroll.clone(),
        view,
        colors: [c.removed, c.added, found, found],
        thumb: c.foreground,
    }
}

pub struct Overview {
    id: ElementId,
    marks: Rc<Marks>,
    content: Pixels,
    scroll: Scroll,
    view: EntityId,
    colors: [Hsla; 4],
    thumb: Hsla,
}

/// A drag's hold on the thumb (how far below its top), and whether the pointer is over the strip.
#[derive(Default)]
struct Pointer {
    grab: Cell<Option<Pixels>>,
    hover: Cell<bool>,
}

/// Where the view's part of the content is on a strip `height` tall: top and bottom.
fn thumb(top: Pixels, view_h: Pixels, k: f32, height: Pixels) -> (Pixels, Pixels) {
    let h = (view_h * k).max(MIN_THUMB).min(height);
    let t = (top * k).min(height - h).max(px(0.));
    (t, t + h)
}

impl IntoElement for Overview {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for Overview {
    type RequestLayoutState = ();
    type PrepaintState = Hitbox;

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = WIDTH.into();
        style.size.height = relative(1.).into();
        style.flex_shrink = 0.;
        (window.request_layout(style, None, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        _: &mut App,
    ) -> Hitbox {
        window.insert_hitbox(bounds, HitboxBehavior::Normal)
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        hitbox: &mut Hitbox,
        window: &mut Window,
        _: &mut App,
    ) {
        let Some(id) = id else { return };
        let pointer = window.with_element_state(id, |p: Option<Rc<Pointer>>, _| {
            let p = p.unwrap_or_default();
            (p.clone(), p)
        });
        let view_h = bounds.size.height;
        if view_h <= px(0.) {
            return;
        }
        // Content shorter than the view keeps its marks where its lines are.
        let content = self.content.max(view_h);
        self.marks.paint(bounds, content, &self.colors, window);
        let k = view_h / content;
        let scrolls = self.content > view_h + px(0.5);
        if scrolls {
            let (t0, t1) = thumb(self.scroll.top(), view_h, k, view_h);
            let alpha = if pointer.grab.get().is_some() {
                0.5
            } else if pointer.hover.get() {
                0.36
            } else {
                0.18
            };
            window.paint_quad(fill(
                Bounds::new(
                    bounds.origin + point(px(0.), t0),
                    size(bounds.size.width, t1 - t0),
                ),
                self.thumb.opacity(alpha),
            ));
        }
        let (scroll, view, max_top) = (
            self.scroll.clone(),
            self.view,
            (self.content - view_h).max(px(0.)),
        );
        // The thumb's top at `y` in the strip.
        let scroll_to = move |scroll: &Scroll, y: Pixels| {
            scroll.set_top((y / k).clamp(px(0.), max_top));
        };
        {
            let (hitbox, pointer, scroll) = (hitbox.clone(), pointer.clone(), scroll.clone());
            window.on_mouse_event(move |e: &MouseDownEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble
                    || e.button != MouseButton::Left
                    || !hitbox.is_hovered(window)
                {
                    return;
                }
                cx.stop_propagation();
                if !scrolls {
                    return;
                }
                let y = e.position.y - bounds.origin.y;
                let (t0, t1) = thumb(scroll.top(), view_h, k, view_h);
                let grab = if (t0..t1).contains(&y) {
                    y - t0
                } else {
                    (t1 - t0) / 2.
                };
                scroll_to(&scroll, y - grab);
                pointer.grab.set(Some(grab));
                window.capture_pointer(hitbox.id);
                cx.notify(view);
            });
        }
        {
            let (hitbox, pointer, scroll) = (hitbox.clone(), pointer.clone(), scroll.clone());
            window.on_mouse_event(move |e: &MouseMoveEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble {
                    return;
                }
                if let Some(grab) = pointer.grab.get() {
                    if e.pressed_button == Some(MouseButton::Left) {
                        scroll_to(&scroll, e.position.y - bounds.origin.y - grab);
                    } else {
                        pointer.grab.set(None);
                    }
                    cx.notify(view);
                }
                let hover = hitbox.is_hovered(window);
                if pointer.hover.replace(hover) != hover {
                    cx.notify(view);
                }
            });
        }
        {
            let pointer = pointer.clone();
            window.on_mouse_event(move |e: &MouseUpEvent, phase, window, cx| {
                if phase == DispatchPhase::Bubble
                    && e.button == MouseButton::Left
                    && pointer.grab.take().is_some()
                {
                    window.release_pointer();
                    cx.notify(view);
                }
            });
        }
        // The wheel over the strip scrolls the lines beside it.
        let hitbox = hitbox.clone();
        window.on_mouse_event(move |e: &ScrollWheelEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble || !hitbox.should_handle_scroll(window) {
                return;
            }
            let dy = e.delta.pixel_delta(window.line_height()).y;
            let top = scroll.top();
            let to = (top - dy).clamp(px(0.), max_top);
            if to != top {
                scroll.set_top(to);
                cx.notify(view);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spans_merge_and_scale_with_a_floor() {
        let mut m = Marks::default();
        m.push(Mark::Added, px(0.), px(10.));
        m.push(Mark::Added, px(10.), px(20.));
        m.push(Mark::Added, px(500.), px(510.));
        m.push(Mark::FoundOld, px(990.), px(1000.));
        assert_eq!(
            m.spans(Mark::Added),
            &[(px(0.), px(20.)), (px(500.), px(510.))]
        );
        // 1000px of content on a 100px strip: a tenth.
        let quads = m.scaled(size(px(14.), px(100.)), px(1000.));
        let added: Vec<_> = quads
            .iter()
            .filter(|(k, _)| *k == Mark::Added)
            .map(|(_, b)| (b.origin.x, b.origin.y, b.size.height))
            .collect();
        assert_eq!(
            added,
            vec![(px(7.), px(0.), px(2.)), (px(7.), px(50.), px(2.))]
        );
        // A match on the last line still shows, inside the strip, on the old half.
        let found = quads.iter().find(|(k, _)| *k == Mark::FoundOld).unwrap().1;
        assert_eq!(
            (found.origin.x, found.origin.y, found.size.height),
            (px(0.), px(97.), px(3.))
        );
    }

    #[test]
    fn the_thumb_keeps_a_size_to_grab_and_stays_inside() {
        assert_eq!(thumb(px(0.), px(100.), 0.5, px(100.)), (px(0.), px(50.)));
        assert_eq!(
            thumb(px(10_000.), px(100.), 0.001, px(100.)),
            (px(10.), px(20.))
        );
        assert_eq!(
            thumb(px(99_900.), px(100.), 0.001, px(100.)),
            (px(90.), px(100.))
        );
    }
}
