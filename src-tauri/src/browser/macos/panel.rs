//! Where an agent's awake tab draws while out of its tab's sight: a panel of its own outside
//! every screen. WebKit stops a page whose window isn't visible (the main window minimized, the
//! app hidden), and takes one in a window that isn't key as inactive (no :hover); the panel is
//! neither, and never key in fact, so it takes no keys or focus from the user. One per tab, as
//! each has its own size (`device WxH`) and its own first responder: two pages sharing a window
//! couldn't both be focused. A released panel or two are kept to be used again rather than
//! made anew each time a tab goes out of sight.

use super::{content_view, View};
use objc2::rc::Retained;
use objc2::runtime::Bool;
use objc2::{define_class, msg_send, ClassType, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAutoresizingMaskOptions, NSBackingStoreType, NSColor, NSPanel, NSScreen, NSView,
    NSWindowAnimationBehavior, NSWindowCollectionBehavior, NSWindowDidBecomeKeyNotification,
    NSWindowDidResignKeyNotification, NSWindowOrderingMode, NSWindowSharingType, NSWindowStyleMask,
};
use objc2_foundation::{NSNotificationCenter, NSObjectProtocol, NSPoint, NSRect, NSSize};
use std::cell::{Cell, RefCell};

/// Between the screens and the panel: a screen added at their edge later doesn't reach it.
const MARGIN: f64 = 10_000.0;
/// Released panels kept to be used again.
const SPARE: usize = 2;
/// _NSBackingStoreUnbuffered: rendering checked off every screen, page and rAF at full rate.
const UNBUFFERED: usize = 3;

thread_local! {
    static SPARES: RefCell<Vec<Retained<Panel>>> = RefCell::default();
}

pub(super) struct PanelIvars {
    /// Whether it reports itself key (`settle`).
    key: Cell<bool>,
}

define_class!(
    /// Reports itself key without becoming key: WebKit then draws the page as active (hover,
    /// focus), while AppKit's key window, and with it the user's keys, stay where they are.
    // SAFETY: NSPanel has no subclassing requirements, and Panel has no Drop.
    #[unsafe(super(NSPanel))]
    #[name = "GitViberBrowserPanel"]
    #[ivars = PanelIvars]
    pub(super) struct Panel;

    impl Panel {
        #[unsafe(method(canBecomeKeyWindow))]
        fn can_become_key(&self) -> Bool {
            Bool::NO
        }

        #[unsafe(method(canBecomeMainWindow))]
        fn can_become_main(&self) -> Bool {
            Bool::NO
        }

        #[unsafe(method(isKeyWindow))]
        fn is_key(&self) -> Bool {
            Bool::new(self.ivars().key.get())
        }

        /// No window of the user's: VoiceOver, window switchers and tilers pass it by.
        #[unsafe(method(isAccessibilityElement))]
        fn is_accessibility_element(&self) -> Bool {
            Bool::NO
        }

        #[unsafe(method(accessibilityIsIgnored))]
        fn accessibility_is_ignored(&self) -> Bool {
            Bool::YES
        }
    }
);

impl Panel {
    fn new(mtm: MainThreadMarker, size: NSSize) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(PanelIvars {
            key: Cell::new(false),
        });
        let style = NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel;
        // No buffer of its own (unbuffered, private to AppKit), its device made as it's ordered
        // in: the page draws in its own layers, which a window this far out never shows.
        let backing = NSBackingStoreType(UNBUFFERED);
        let this: Retained<Self> = unsafe {
            msg_send![super(this), initWithContentRect: frame(mtm, size), styleMask: style, backing: backing, defer: true]
        };
        unsafe { this.setReleasedWhenClosed(false) };
        this.setHasShadow(false);
        this.setOpaque(false);
        this.setBackgroundColor(Some(&NSColor::clearColor()));
        this.setIgnoresMouseEvents(true);
        this.setAlphaValue(0.0);
        this.setSharingType(NSWindowSharingType::None);
        this.setHidesOnDeactivate(false);
        // Drawn on while the app is hidden (⌘H).
        this.setCanHide(false);
        // Out of Mission Control, which would otherwise shrink it, and of ⌘`. Not on all
        // Spaces: a window there could keep ⌘Tab from going to GitViber's own, and with
        // occlusion detection off the page draws on whichever Space the panel is.
        this.setCollectionBehavior(
            NSWindowCollectionBehavior::Transient
                | NSWindowCollectionBehavior::IgnoresCycle
                | NSWindowCollectionBehavior::Stationary,
        );
        this.setExcludedFromWindowsMenu(true);
        this.setAnimationBehavior(NSWindowAnimationBehavior::None);
        this
    }
}

/// Left of and below every screen.
fn frame(mtm: MainThreadMarker, size: NSSize) -> NSRect {
    let screens: Vec<NSRect> = NSScreen::screens(mtm).iter().map(|s| s.frame()).collect();
    let (x, y) = outside(
        (size.width, size.height),
        screens
            .iter()
            .map(|r| (r.origin.x, r.origin.y, r.size.width, r.size.height)),
    );
    NSRect::new(NSPoint::new(x, y), size)
}

/// Where a window `size` big lies clear of every screen (x, y, w, h), with MARGIN to spare.
fn outside(size: (f64, f64), screens: impl Iterator<Item = (f64, f64, f64, f64)>) -> (f64, f64) {
    let (left, bottom) = screens.fold((0.0f64, 0.0f64), |(l, b), (x, y, _, _)| {
        (l.min(x), b.min(y))
    });
    (left - size.0 - MARGIN, bottom - size.1 - MARGIN)
}

/// The view drawn in its panel at `size`, a spare one or made for it the first time, with the
/// page its first responder there, active while an agent's command runs on it.
pub(super) fn hold(v: &View, size: NSSize) {
    let mtm = v.host.mtm();
    let mut slot = v.delegate.ivars().panel.borrow_mut();
    let panel = slot
        .get_or_insert_with(|| {
            SPARES
                .with_borrow_mut(Vec::pop)
                .unwrap_or_else(|| Panel::new(mtm, size))
        })
        .clone();
    drop(slot);
    if panel.contentRectForFrameRect(panel.frame()).size != size {
        panel.setFrame_display(frame(mtm, size), false);
    }
    let Some(content) = panel.contentView() else {
        return;
    };
    let inside = unsafe { v.host.superview() }
        .is_some_and(|p| Retained::as_ptr(&p) == Retained::as_ptr(&content));
    if !inside {
        content.addSubview(&v.host);
        v.host.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
    }
    v.host.setFrame(content.bounds());
    v.host.setHidden(false);
    if !panel.isVisible() {
        panel.orderBack(None);
    }
    if !v.web.focused() {
        panel.makeFirstResponder(Some(&v.web));
    }
    settle(v);
}

/// Its page active, as in the key window, unless a password field has focus with no agent's
/// command running: WebKit turns on secure keyboard entry for a focused password field in the
/// key window, and macOS holds it for the whole system while it's on. Kept active otherwise, so
/// the page sees no blur and focus at every command's edge.
pub(super) fn settle(v: &View) {
    let d = v.delegate.ivars();
    let idle = d.running.borrow().is_empty();
    key(v, !(d.secret.get() && idle));
}

/// Its page no longer active: before it leaves its panel, so no secure input stays on.
pub(super) fn resign(v: &View) {
    key(v, false);
}

/// WebKit hears it as the window becoming or ceasing to be key; only a change is posted.
fn key(v: &View, on: bool) {
    let Some(panel) = v.delegate.ivars().panel.borrow().clone() else {
        return;
    };
    if panel.ivars().key.replace(on) != on {
        let name = unsafe {
            if on {
                NSWindowDidBecomeKeyNotification
            } else {
                NSWindowDidResignKeyNotification
            }
        };
        unsafe {
            NSNotificationCenter::defaultCenter().postNotificationName_object(name, Some(&panel))
        };
    }
}

/// Out of sight and kept to be used again, if there's room for it.
pub(super) fn spare(panel: Retained<Panel>) {
    panel.orderOut(None);
    panel.ivars().key.set(false);
    SPARES.with_borrow_mut(|spares| {
        if spares.len() < SPARE {
            spares.push(panel);
        }
    });
}

/// The view back in the main window, hidden there until it's placed, and its panel spared.
pub(super) fn release(v: &View) {
    resign(v);
    let Some(panel) = v.delegate.ivars().panel.take() else {
        return;
    };
    v.host.setHidden(true);
    v.host.removeFromSuperview();
    // Kept to the top as the window resizes, as `create` sets it.
    v.host
        .setAutoresizingMask(NSAutoresizingMaskOptions::ViewMinYMargin);
    if let Some(content) = content_view(&v.delegate.ivars().app) {
        content.addSubview_positioned_relativeTo(&v.host, NSWindowOrderingMode::Above, None);
    }
    spare(panel);
}

/// Whether `view` draws in a panel: no sheet or focus of the user's belongs there.
pub(super) fn in_panel(view: &NSView) -> bool {
    view.window()
        .is_some_and(|w| w.isKindOfClass(Panel::class()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_panel_lies_clear_of_every_screen_whatever_their_layout() {
        let main = (0.0, 0.0, 1512.0, 982.0);
        // A second screen to the left and below the main one.
        let side = (-1920.0, -300.0, 1920.0, 1080.0);
        let (x, y) = outside((1280.0, 800.0), [main, side].into_iter());
        assert_eq!((x, y), (-1920.0 - 1280.0 - MARGIN, -300.0 - 800.0 - MARGIN));
        for (sx, sy, _, _) in [main, side] {
            assert!(x + 1280.0 < sx && y + 800.0 < sy);
        }
        // No screens at all (closed lid, none attached yet): still off the origin.
        assert_eq!(
            outside((400.0, 300.0), std::iter::empty()),
            (-400.0 - MARGIN, -300.0 - MARGIN)
        );
    }

    #[test]
    fn the_panel_class_registers() {
        let _ = Panel::class();
    }
}
