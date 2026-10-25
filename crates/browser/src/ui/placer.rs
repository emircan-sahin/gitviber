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

/// A page as the last check left it, and its tab.
struct Shown {
    placed: Placed,
    tab: WeakEntity<BrowserTab>,
}

#[derive(Default)]
struct Frame {
    painted: Vec<Painted>,
    scheduled: bool,
    placed: HashMap<String, Shown>,
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
    schedule(frame, window);
}

fn schedule(frame: &mut Frame, window: &mut Window) {
    if !frame.scheduled {
        frame.scheduled = true;
        window.on_next_frame(check);
    }
}

/// The window shows something else where a page was (another tab, a maximized terminal), which
/// no browser tab paints to say: a check once that frame is drawn hides the page. Each workspace
/// change comes with a redraw.
pub fn check_after_draw(window: &mut Window, cx: &mut App) {
    let key = window.window_handle().window_id();
    if frame_mut(cx, key).is_none_or(|f| f.placed.is_empty()) {
        return;
    }
    // A next frame's callbacks run before that frame draws: the check waits one more.
    window.on_next_frame(move |window, cx| {
        if let Some(frame) = frame_mut(cx, key) {
            schedule(frame, window);
        }
    });
}

/// The page is gone from this window (its tab closed): nothing to place or hide any more.
pub fn forget(id: &str, cx: &mut App) {
    if cx.has_global::<Placer>() {
        for frame in cx.global_mut::<Placer>().0.values_mut() {
            frame.placed.remove(id);
        }
    }
}

/// Whether the page shows now, placed on its tab: one aside or covered is hidden.
pub fn is_shown(id: &str, cx: &App) -> bool {
    placed(id, cx).is_some_and(|p| matches!(p, Placed::At(..)))
}

fn placed(id: &str, cx: &App) -> Option<Placed> {
    cx.try_global::<Placer>()?
        .0
        .values()
        .find_map(|f| f.placed.get(id).map(|s| s.placed))
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
    let mut now = HashMap::new();
    let mut changed = Vec::new();
    // Drawn more than once since the last check (a key's dispatch draws first): the latest
    // counts. An earlier one's hitbox is gone, so it read as covered.
    for p in painted.into_iter().rev() {
        if now.contains_key(&p.id) {
            continue;
        }
        let before = was.remove(&p.id).map(|s| s.placed);
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
        now.insert(
            p.id.clone(),
            Shown {
                placed: next,
                tab: p.tab.clone(),
            },
        );
        if before != Some(next) {
            changed.push((p, before, next));
        }
    }
    // Kept before the tabs hear of it: one shown asks whether it is.
    if let Some(frame) = frame_mut(cx, key) {
        frame.placed = now;
    }
    for (p, before, next) in changed {
        apply(&p, before, next, window, cx);
    }
    // Not drawn this frame: its tab is out of sight.
    for (id, shown) in was {
        crate::hide(&id, false);
        let _ = shown.tab.update(cx, |tab, cx| tab.out_of_sight(cx));
    }
}

fn apply(p: &Painted, before: Option<Placed>, next: Placed, window: &mut Window, cx: &mut App) {
    match next {
        Placed::At(rect, screen) => {
            // A test's window has no view to lay it beside, and nothing else differs.
            let placed = super::native_view(window)
                .is_none_or(|view| crate::place(&p.id, rect, screen, view).is_ok());
            if placed && !matches!(before, Some(Placed::At(..))) {
                let _ = p.tab.update(cx, |tab, cx| tab.shown(window, cx));
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

/// Whether the page is still covered: once its picture came, and once that's decoded.
pub fn covered_now(id: &str, cx: &App) -> bool {
    placed(id, cx) == Some(Placed::Covered)
}
