//! Keeps each tab's page (a native view, which GPUI can't draw over) on the bounds its tab laid
//! out, a frame after GPUI drew them. A page whose tab wasn't drawn is out of sight and hides;
//! one that something of the window's covers (a menu, a dialog, a popover) steps aside, its
//! picture standing in, and comes back once nothing covers it.

use super::tab::BrowserTab;
use crate::{Rect, Screen};
use gpui::{point, px, App, Bounds, Global, Hitbox, Pixels, WeakEntity, Window, WindowId};
use std::collections::HashMap;

/// A tab's page area as it was painted this frame.
pub struct Painted {
    pub id: String,
    pub bounds: Bounds<Pixels>,
    pub hitbox: Hitbox,
    pub screen: Option<Screen>,
    /// A live page shows there: not one that failed, nor a parked page's picture.
    pub live: bool,
    pub tab: WeakEntity<BrowserTab>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Placed {
    At(Rect, Option<Screen>),
    /// Hidden while its tab shows something else there.
    Aside,
    /// Something of the window's is over it: its picture stands in.
    Covered,
}

#[derive(Default)]
struct Frame {
    painted: Vec<Painted>,
    scheduled: bool,
    placed: HashMap<String, Placed>,
}

#[derive(Default)]
struct Placer(HashMap<WindowId, Frame>);

impl Global for Placer {}

fn frame_mut(cx: &mut App, key: WindowId) -> Option<&mut Frame> {
    if !cx.has_global::<Placer>() {
        return None;
    }
    cx.global_mut::<Placer>().0.get_mut(&key)
}

/// Called from the tab's paint: placed once the frame is drawn.
pub fn painted(p: Painted, window: &mut Window, cx: &mut App) {
    let key = window.window_handle().window_id();
    let frame = cx.default_global::<Placer>().0.entry(key).or_default();
    frame.painted.push(p);
    if !frame.scheduled {
        frame.scheduled = true;
        window.on_next_frame(check);
    }
}

/// The page is gone from this window (its tab closed): nothing to place or hide any more.
pub fn forget(id: &str, cx: &mut App) {
    if cx.has_global::<Placer>() {
        for frame in cx.global_mut::<Placer>().0.values_mut() {
            frame.placed.remove(id);
        }
    }
}

/// Points to the half, as AppKit lays views out on a 2x screen.
fn half(v: Pixels) -> f64 {
    (f64::from(v) * 2.0).round() / 2.0
}

/// Whether anything drawn after the page's area takes the pointer somewhere over it: a menu, a
/// dialog's backdrop, a popover. Sampled on a grid fine enough for the smallest of them.
fn covered(p: &Painted, window: &Window) -> bool {
    const STEP: f32 = 24.;
    let b = p.bounds;
    let (w, h) = (f32::from(b.size.width), f32::from(b.size.height));
    let cols = ((w / STEP).ceil() as usize).max(1);
    let rows = ((h / STEP).ceil() as usize).max(1);
    for r in 0..=rows {
        for c in 0..=cols {
            let x = (c as f32 * w / cols as f32).clamp(1., (w - 1.).max(1.));
            let y = (r as f32 * h / rows as f32).clamp(1., (h - 1.).max(1.));
            let at = point(b.origin.x + px(x), b.origin.y + px(y));
            if !p.hitbox.is_hovered_at(at, window) {
                return true;
            }
        }
    }
    false
}

fn check(window: &mut Window, cx: &mut App) {
    let key = window.window_handle().window_id();
    let Some(frame) = frame_mut(cx, key) else {
        return;
    };
    frame.scheduled = false;
    let painted = std::mem::take(&mut frame.painted);
    let mut was = std::mem::take(&mut frame.placed);
    let Some(view) = super::native_view(window) else {
        return;
    };
    let mut now = HashMap::new();
    for p in painted {
        let before = was.remove(&p.id);
        let b = p.bounds;
        let small = b.size.width < px(2.) || b.size.height < px(2.);
        let next = if !p.live || small {
            Placed::Aside
        } else if covered(&p, window) {
            Placed::Covered
        } else {
            let rect = Rect {
                x: half(b.origin.x),
                y: half(b.origin.y),
                w: half(b.size.width),
                h: half(b.size.height),
            };
            Placed::At(rect, p.screen)
        };
        if before != Some(next) {
            apply(&p, before, next, view, cx);
        }
        now.insert(p.id, next);
    }
    // Not drawn this frame: its tab is out of sight.
    for (id, _) in was {
        crate::hide(&id, false);
    }
    if let Some(frame) = frame_mut(cx, key) {
        frame.placed = now;
    }
}

fn apply(
    p: &Painted,
    before: Option<Placed>,
    next: Placed,
    view: std::ptr::NonNull<std::ffi::c_void>,
    cx: &mut App,
) {
    match next {
        Placed::At(rect, screen) => {
            let placed = crate::place(&p.id, rect, screen, view).is_ok();
            if placed && before == Some(Placed::Covered) {
                let _ = p.tab.update(cx, |tab, cx| tab.uncovered(cx));
            }
        }
        Placed::Aside => crate::hide(&p.id, true),
        Placed::Covered => {
            // The picture first: hidden, the view would have none to give.
            let (id, tab) = (p.id.clone(), p.tab.clone());
            let (tx, rx) = futures::channel::oneshot::channel();
            crate::snapshot_then(&id, move |shot| drop(tx.send(shot)));
            cx.spawn(async move |cx| {
                let shot = rx.await.ok().flatten();
                let still = cx.update(|cx| covered_now(&id, cx));
                if still {
                    let _ = tab.update(cx, |tab, cx| tab.covered(shot, cx));
                    crate::hide(&id, true);
                }
            })
            .detach();
        }
    }
}

/// Whether the page is still covered once its picture came.
fn covered_now(id: &str, cx: &App) -> bool {
    cx.try_global::<Placer>().is_some_and(|p| {
        p.0.values()
            .any(|f| f.placed.get(id) == Some(&Placed::Covered))
    })
}
