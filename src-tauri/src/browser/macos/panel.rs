//! Where an agent's awake tab draws while out of its tab's sight: a panel of its own outside
//! every screen. WebKit stops a page whose window isn't visible (the main window minimized, the
//! app hidden), and takes one in a window that isn't key as inactive (no :hover); the panel is
//! neither, and never key in fact, so it takes no keys or focus from the user.
//! One per tab, as each has its own size (`device WxH`) and its own first responder: two pages
//! sharing a window couldn't both be focused.

use super::{content_view, View};
use objc2::rc::Retained;
use objc2::runtime::Bool;
use objc2::{define_class, msg_send, ClassType, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAutoresizingMaskOptions, NSBackingStoreType, NSColor, NSPanel, NSScreen,
    NSWindowAnimationBehavior, NSWindowCollectionBehavior, NSWindowOrderingMode,
    NSWindowSharingType, NSWindowStyleMask,
};
use objc2_foundation::{NSObjectProtocol, NSPoint, NSRect, NSSize};

/// Between the screens and the panel: a screen added at their edge later doesn't reach it.
const MARGIN: f64 = 10_000.0;

define_class!(
    /// Reports itself key without becoming key, as WebKitTestRunner's window does: WebKit draws
    /// the page as active (hover, focus) while AppKit's key window stays the user's.
    // SAFETY: NSPanel has no subclassing requirements, and Panel has no Drop.
    #[unsafe(super(NSPanel))]
    #[name = "GitViberBrowserPanel"]
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
            Bool::YES
        }
    }
);

impl Panel {
    fn new(mtm: MainThreadMarker, size: NSSize) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        let style = NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel;
        let this: Retained<Self> = unsafe {
            msg_send![super(this), initWithContentRect: frame(mtm, size), styleMask: style, backing: NSBackingStoreType::Buffered, defer: false]
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
        // Out of Mission Control, which would otherwise shrink it, and of ⌘`.
        this.setCollectionBehavior(
            NSWindowCollectionBehavior::Transient
                | NSWindowCollectionBehavior::IgnoresCycle
                | NSWindowCollectionBehavior::Stationary
                | NSWindowCollectionBehavior::CanJoinAllSpaces,
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

/// The view drawn in its panel at `size`, made for it the first time, with the page its first
/// responder there: focused as a page in the key window is.
pub(super) fn hold(v: &View, size: NSSize) {
    let mtm = v.host.mtm();
    let mut slot = v.delegate.ivars().panel.borrow_mut();
    let panel = slot.get_or_insert_with(|| Panel::new(mtm, size)).clone();
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
}

/// The view back in the main window, hidden there until it's placed, and its panel gone.
pub(super) fn release(v: &View) {
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
    panel.orderOut(None);
}

/// Whether `web` draws in a panel: no sheet, focus or menu of the user's belongs there.
pub(super) fn in_panel(web: &super::WebView) -> bool {
    web.window()
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
