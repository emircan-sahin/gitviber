//! PDFs, audio and video in the app on macOS: PDFKit's PDFView and AVKit's AVPlayerView, native
//! views GPUI can't draw over, laid beside the window's own view on the bounds the media view
//! painted, a frame after it painted them. One its tab didn't paint is out of sight: hidden, its
//! player paused. One something of the window's covers (a menu, a dialog) steps aside until it's
//! uncovered. Closing the view takes its native view out and lets it go.

use std::collections::HashMap;
use std::path::Path;
use std::ptr::NonNull;

use gpui_kit::{
    App, Bounds, EntityId, Global, Hitbox, Pixels, WeakEntity, Window, WindowId, point, px,
};
use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyClass, AnyObject};
use objc2::{MainThreadMarker, msg_send};
use objc2_app_kit::{NSView, NSWindowOrderingMode};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString, NSURL};

use crate::media::{MediaKind, MediaView};

// The app links neither on its own.
#[link(name = "PDFKit", kind = "framework")]
unsafe extern "C" {}
#[link(name = "AVKit", kind = "framework")]
unsafe extern "C" {}
#[link(name = "AVFoundation", kind = "framework")]
unsafe extern "C" {}

/// A file's native viewer: the view, and the player behind a video's or a song's.
pub struct NativeMedia {
    view: Retained<NSView>,
    player: Option<Retained<AnyObject>>,
}

fn new_view(class: &std::ffi::CStr) -> Option<Retained<AnyObject>> {
    let cls = AnyClass::get(class)?;
    // SAFETY: an NSView subclass, allocated and inited as AppKit does.
    unsafe {
        let alloc: Allocated<AnyObject> = msg_send![cls, alloc];
        msg_send![alloc, initWithFrame: NSRect::ZERO]
    }
}

impl NativeMedia {
    /// A viewer for `path`, or None off the main thread or when the framework isn't there.
    pub fn open(path: &Path, kind: MediaKind) -> Option<Self> {
        MainThreadMarker::new()?;
        let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
        let (view, player) = match kind {
            MediaKind::Pdf => {
                let doc_cls = AnyClass::get(c"PDFDocument")?;
                // SAFETY: PDFKit's documented initializers and setters.
                let view = unsafe {
                    let alloc: Allocated<AnyObject> = msg_send![doc_cls, alloc];
                    let doc: Option<Retained<AnyObject>> = msg_send![alloc, initWithURL: &*url];
                    let view = new_view(c"PDFView")?;
                    let _: () = msg_send![&*view, setDocument: doc.as_deref()];
                    let _: () = msg_send![&*view, setAutoScales: true];
                    view
                };
                (view, None)
            }
            MediaKind::Audio | MediaKind::Video => {
                let player_cls = AnyClass::get(c"AVPlayer")?;
                // SAFETY: AVFoundation's and AVKit's documented constructors and setters.
                unsafe {
                    let player: Option<Retained<AnyObject>> =
                        msg_send![player_cls, playerWithURL: &*url];
                    let player = player?;
                    let view = new_view(c"AVPlayerView")?;
                    let _: () = msg_send![&*view, setPlayer: &*player];
                    (view, Some(player))
                }
            }
            MediaKind::Image => return None,
        };
        // SAFETY: both classes are NSView subclasses.
        let view: Retained<NSView> = unsafe { Retained::cast_unchecked(view) };
        view.setHidden(true);
        Some(NativeMedia { view, player })
    }

    /// Lays the view over `rect` (logical points from the GPUI view's top left).
    fn place(&self, rect: Bounds<Pixels>, gpui: &NSView) {
        // SAFETY: AppKit, on the main thread (paint and frame callbacks run there).
        let Some(parent) = (unsafe { gpui.superview() }) else {
            return;
        };
        if !unsafe { self.view.superview() }.is_some_and(|s| std::ptr::eq(&*s, &*parent)) {
            self.view.removeFromSuperview();
            parent.addSubview_positioned_relativeTo(&self.view, NSWindowOrderingMode::Above, None);
        }
        let (x, y, w, h) = (
            half(rect.origin.x),
            half(rect.origin.y),
            half(rect.size.width),
            half(rect.size.height),
        );
        let y = if gpui.isFlipped() {
            y
        } else {
            gpui.bounds().size.height - y - h
        };
        let frame = gpui.convertRect_toView(
            NSRect::new(NSPoint::new(x, y), NSSize::new(w, h)),
            Some(&parent),
        );
        if self.view.frame() != frame {
            self.view.setFrame(frame);
        }
        self.view.setHidden(false);
    }

    fn hide(&self, pause: bool) {
        self.view.setHidden(true);
        if pause && let Some(player) = &self.player {
            // SAFETY: AVPlayer's pause.
            let _: () = unsafe { msg_send![&**player, pause] };
        }
    }
}

impl Drop for NativeMedia {
    fn drop(&mut self) {
        self.hide(true);
        self.view.removeFromSuperview();
    }
}

/// Points to the half, as AppKit lays views out on a 2x screen.
fn half(v: Pixels) -> f64 {
    (f64::from(v) * 2.0).round() / 2.0
}

/// The GPUI view of `window`, which the media views lie beside.
fn gpui_view(window: &Window) -> Option<NonNull<std::ffi::c_void>> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match HasWindowHandle::window_handle(window).ok()?.as_raw() {
        RawWindowHandle::AppKit(h) => Some(h.ns_view),
        _ => None,
    }
}

/// A media view's area as it was painted this frame.
pub struct Painted {
    pub id: EntityId,
    pub bounds: Bounds<Pixels>,
    pub hitbox: Hitbox,
    pub view: WeakEntity<MediaView>,
}

#[derive(Default)]
struct Frame {
    painted: Vec<Painted>,
    scheduled: bool,
    /// The views placed or covered after the last check.
    shown: HashMap<EntityId, WeakEntity<MediaView>>,
}

#[derive(Default)]
struct Placer(HashMap<WindowId, Frame>);
impl Global for Placer {}

/// Called from the media view's paint: placed once the frame is drawn.
pub fn painted(p: Painted, window: &mut Window, cx: &mut App) {
    let frame = cx
        .default_global::<Placer>()
        .0
        .entry(window.window_handle().window_id())
        .or_default();
    frame.painted.push(p);
    schedule(frame, window);
}

/// A check next frame though nothing painted: tabs switched, so a shown view may be out of sight.
pub fn check_soon(window: &mut Window, cx: &mut App) {
    let frame = cx
        .default_global::<Placer>()
        .0
        .entry(window.window_handle().window_id())
        .or_default();
    if !frame.shown.is_empty() {
        schedule(frame, window);
    }
}

fn schedule(frame: &mut Frame, window: &mut Window) {
    if !frame.scheduled {
        frame.scheduled = true;
        window.on_next_frame(check);
    }
}

/// Whether anything drawn after the area takes the pointer somewhere over it.
fn covered(p: &Painted, window: &Window) -> bool {
    const STEP: f32 = 24.;
    let b = p.bounds;
    let (w, h) = (f32::from(b.size.width), f32::from(b.size.height));
    let (cols, rows) = (
        ((w / STEP).ceil() as usize).max(1),
        ((h / STEP).ceil() as usize).max(1),
    );
    (0..=rows).any(|r| {
        (0..=cols).any(|c| {
            let x = (c as f32 * w / cols as f32).clamp(1., (w - 1.).max(1.));
            let y = (r as f32 * h / rows as f32).clamp(1., (h - 1.).max(1.));
            !p.hitbox
                .is_hovered_at(point(b.origin.x + px(x), b.origin.y + px(y)), window)
        })
    })
}

fn check(window: &mut Window, cx: &mut App) {
    let key = window.window_handle().window_id();
    let Some(frame) = cx
        .try_global::<Placer>()
        .is_some()
        .then(|| cx.global_mut::<Placer>().0.get_mut(&key))
        .flatten()
    else {
        return;
    };
    frame.scheduled = false;
    let painted = std::mem::take(&mut frame.painted);
    let mut was = std::mem::take(&mut frame.shown);
    let Some(gpui) = gpui_view(window) else {
        return;
    };
    // SAFETY: the window's handle names its live NSView, on this thread.
    let gpui: &NSView = unsafe { gpui.cast::<NSView>().as_ref() };
    let mut now = HashMap::new();
    for p in painted {
        was.remove(&p.id);
        let small = p.bounds.size.width < px(2.) || p.bounds.size.height < px(2.);
        let cover = small || covered(&p, window);
        if let Some(view) = p.view.upgrade() {
            if let Some(native) = view.read(cx).native() {
                if cover {
                    native.hide(false)
                } else {
                    native.place(p.bounds, gpui)
                }
            }
            now.insert(p.id, p.view);
        }
    }
    // Not painted this frame: its tab is out of sight.
    for (_, view) in was {
        if let Some(view) = view.upgrade()
            && let Some(native) = view.read(cx).native()
        {
            native.hide(true);
        }
    }
    if let Some(frame) = cx.global_mut::<Placer>().0.get_mut(&key) {
        frame.shown = now;
    }
}
