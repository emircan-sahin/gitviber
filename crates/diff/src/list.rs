//! A vertical list that lays out only the items on screen. Every item's height is known up
//! front (a code line, a file header, the room a file not loaded yet keeps), so the scroll
//! height is exact and a rebuilt list can put the same content back at the top.

use gpui_kit::base::ScrollbarHandle;
use gpui_kit::{
    AnyElement, App, AvailableSpace, Bounds, ContentMask, DispatchPhase, Element, ElementId,
    EntityId, GlobalElementId, Hitbox, HitboxBehavior, InspectorElementId, IntoElement, LayoutId,
    Pixels, Point, ScrollWheelEvent, Size, Style, Window, point, px, relative, size,
};
use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;

/// Where each item starts, and where the last one ends.
#[derive(Clone, Debug, Default)]
pub struct Heights {
    tops: Vec<Pixels>,
}

impl Heights {
    pub fn uniform(count: usize, height: Pixels) -> Self {
        Heights {
            tops: (0..=count).map(|i| height * i as f32).collect(),
        }
    }

    pub fn from_heights(heights: impl IntoIterator<Item = Pixels>) -> Self {
        let mut tops = vec![px(0.)];
        let mut at = px(0.);
        for h in heights {
            at += h;
            tops.push(at);
        }
        Heights { tops }
    }

    pub fn len(&self) -> usize {
        self.tops.len().saturating_sub(1)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn total(&self) -> Pixels {
        self.tops.last().copied().unwrap_or_default()
    }

    pub fn top(&self, ix: usize) -> Pixels {
        self.tops[ix.min(self.len())]
    }

    pub fn height(&self, ix: usize) -> Pixels {
        self.top(ix + 1) - self.top(ix)
    }

    /// The item at `y`: the last one starting at or above it.
    pub fn index_at(&self, y: Pixels) -> usize {
        self.tops
            .partition_point(|t| *t <= y)
            .saturating_sub(1)
            .min(self.len().saturating_sub(1))
    }
}

/// How `scroll_to` places an item.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    Top,
    Center,
    /// Only if it's out of view, at the nearer edge.
    Nearest,
}

#[derive(Default)]
struct Inner {
    top: Pixels,
    left: Pixels,
    max_left: Pixels,
    viewport: Bounds<Pixels>,
    total: Pixels,
    pending: Option<(usize, Pixels, Place)>,
}

/// The list's scroll position, shared by the list, its owner and the scrollbars.
#[derive(Clone, Default)]
pub struct Scroll(Rc<RefCell<Inner>>);

impl Scroll {
    pub fn top(&self) -> Pixels {
        self.0.borrow().top
    }

    pub fn left(&self) -> Pixels {
        self.0.borrow().left
    }

    pub fn viewport(&self) -> Bounds<Pixels> {
        self.0.borrow().viewport
    }

    pub fn set_top(&self, top: Pixels) {
        self.0.borrow_mut().top = top.max(px(0.));
    }

    /// How far the content can scroll sideways.
    pub fn set_max_left(&self, max: Pixels) {
        let mut s = self.0.borrow_mut();
        s.max_left = max.max(px(0.));
        s.left = s.left.min(s.max_left);
    }

    /// Scrolls item `ix` into view at the next layout; `offset` pixels into it.
    pub fn scroll_to(&self, ix: usize, offset: Pixels, place: Place) {
        self.0.borrow_mut().pending = Some((ix, offset, place));
    }
}

impl ScrollbarHandle for Scroll {
    fn viewport_bounds(&self) -> Bounds<Pixels> {
        self.0.borrow().viewport
    }

    fn offset(&self) -> Point<Pixels> {
        let s = self.0.borrow();
        point(-s.left, -s.top)
    }

    fn set_offset(&self, offset: Point<Pixels>) {
        let mut s = self.0.borrow_mut();
        let max_top = (s.total - s.viewport.size.height).max(px(0.));
        s.top = (-offset.y).clamp(px(0.), max_top);
        s.left = (-offset.x).clamp(px(0.), s.max_left);
    }

    fn content_size(&self) -> Size<Pixels> {
        let s = self.0.borrow();
        size(s.viewport.size.width + s.max_left, s.total)
    }
}

type RenderItems = Box<dyn FnMut(Range<usize>, &mut Window, &mut App) -> Vec<AnyElement>>;

pub struct VList {
    id: ElementId,
    scroll: Scroll,
    heights: Rc<Heights>,
    /// The view to redraw when the wheel scrolls.
    view: EntityId,
    render: RenderItems,
    /// Told when an item lays out taller or shorter than its height says (one with blocks
    /// under its line); the owner gives it that height from the next frame.
    measured: Option<Measured>,
}

type Measured = Box<dyn FnMut(usize, Pixels, &mut Window, &mut App)>;

impl VList {
    pub fn on_measure(
        mut self,
        f: impl FnMut(usize, Pixels, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.measured = Some(Box::new(f));
        self
    }
}

/// A list of `heights.len()` items; `render` draws those of a range, in order.
pub fn vlist(
    id: impl Into<ElementId>,
    scroll: &Scroll,
    heights: Rc<Heights>,
    view: EntityId,
    render: impl FnMut(Range<usize>, &mut Window, &mut App) -> Vec<AnyElement> + 'static,
) -> VList {
    VList {
        id: id.into(),
        scroll: scroll.clone(),
        heights,
        view,
        render: Box::new(render),
        measured: None,
    }
}

impl IntoElement for VList {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

pub struct Frame {
    items: Vec<AnyElement>,
    hitbox: Option<Hitbox>,
}

impl Element for VList {
    type RequestLayoutState = ();
    type PrepaintState = Frame;

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
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        style.min_size.height = px(0.).into();
        style.flex_grow = 1.;
        (window.request_layout(style, None, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Frame {
        let heights = self.heights.clone();
        let range = {
            let mut s = self.scroll.0.borrow_mut();
            s.viewport = bounds;
            s.total = heights.total();
            let view_h = bounds.size.height;
            let max_top = (s.total - view_h).max(px(0.));
            if let Some((ix, offset, place)) = s.pending.take()
                && ix < heights.len()
            {
                let (top, h) = (heights.top(ix) + offset, heights.height(ix));
                s.top = match place {
                    Place::Top => top,
                    Place::Center => top - (view_h - h) / 2.,
                    Place::Nearest if top < s.top => top,
                    Place::Nearest if top + h > s.top + view_h => top + h - view_h,
                    Place::Nearest => s.top,
                };
            }
            s.top = s.top.clamp(px(0.), max_top);
            if heights.is_empty() {
                0..0
            } else {
                heights.index_at(s.top)..(heights.index_at(s.top + view_h) + 1).min(heights.len())
            }
        };
        let top = self.scroll.top();
        let mut items = if range.is_empty() {
            vec![]
        } else {
            (self.render)(range.clone(), window, cx)
        };
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            for (item, ix) in items.iter_mut().zip(range) {
                let space = size(
                    AvailableSpace::Definite(bounds.size.width),
                    AvailableSpace::MinContent,
                );
                let laid = item.layout_as_root(space, window, cx);
                if (laid.height - heights.height(ix)).abs() > px(0.5)
                    && let Some(f) = self.measured.as_mut()
                {
                    f(ix, laid.height, window, cx);
                }
                item.prepaint_at(
                    bounds.origin + point(px(0.), heights.top(ix) - top),
                    window,
                    cx,
                );
            }
        });
        let hitbox = Some(window.insert_hitbox(bounds, HitboxBehavior::Normal));
        Frame { items, hitbox }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        frame: &mut Frame,
        window: &mut Window,
        cx: &mut App,
    ) {
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            for item in &mut frame.items {
                item.paint(window, cx);
            }
        });
        let Some(hitbox) = frame.hitbox.clone() else {
            return;
        };
        let scroll = self.scroll.clone();
        let view = self.view;
        let line = window.line_height();
        window.on_mouse_event(move |event: &ScrollWheelEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble || !hitbox.should_handle_scroll(window) {
                return;
            }
            let delta = event.delta.pixel_delta(line);
            let mut s = scroll.0.borrow_mut();
            let before = (s.top, s.left);
            let max_top = (s.total - s.viewport.size.height).max(px(0.));
            // One axis at a time, as a trackpad's sideways swipe drifts.
            if delta.x.abs() > delta.y.abs() {
                s.left = (s.left - delta.x).clamp(px(0.), s.max_left);
            } else {
                s.top = (s.top - delta.y).clamp(px(0.), max_top);
            }
            if (s.top, s.left) != before {
                cx.notify(view);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_items_by_offset() {
        let h = Heights::from_heights([px(10.), px(20.), px(10.)]);
        assert_eq!((h.len(), h.total()), (3, px(40.)));
        assert_eq!(
            (
                h.index_at(px(0.)),
                h.index_at(px(9.)),
                h.index_at(px(10.)),
                h.index_at(px(39.)),
                h.index_at(px(99.))
            ),
            (0, 0, 1, 2, 2)
        );
        assert_eq!((h.top(2), h.height(1)), (px(30.), px(20.)));
        assert_eq!(Heights::uniform(0, px(5.)).index_at(px(3.)), 0);
    }
}
