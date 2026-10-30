//! GitViber: frames paced by a `CAMetalDisplayLink` (macOS 14+, iOS 17+).
//!
//! On macOS 26, `-[CAMetalLayer nextDrawable]` called after a `CVDisplayLink` tick blocks the
//! main thread for about a refresh (33 ms at 60 Hz): the compositor deadline moved close to
//! vsync, so the frame slips. The link hands out a drawable early enough to make the deadline.

use foreign_types::{ForeignType, ForeignTypeRef};
pub use metal::MetalDrawable;
use objc2::{
    AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send,
    rc::Retained,
    runtime::{AnyClass, ProtocolObject},
};
use objc2_foundation::{NSObject, NSObjectProtocol, NSRunLoop, NSRunLoopCommonModes};
use objc2_quartz_core::{
    CAMetalDisplayLink, CAMetalDisplayLinkDelegate, CAMetalDisplayLinkUpdate, CAMetalLayer,
};

struct DelegateIvars {
    callback: Box<dyn Fn(MetalDrawable)>,
}

define_class!(
    // SAFETY: `NSObject` has no subclassing requirements and the class has no `Drop` impl.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = DelegateIvars]
    struct GitViberMetalDisplayLinkDelegate;

    unsafe impl NSObjectProtocol for GitViberMetalDisplayLinkDelegate {}

    unsafe impl CAMetalDisplayLinkDelegate for GitViberMetalDisplayLinkDelegate {
        #[unsafe(method(metalDisplayLink:needsUpdate:))]
        fn needs_update(&self, _link: &CAMetalDisplayLink, update: &CAMetalDisplayLinkUpdate) {
            // The callback may drop the frame source, and with it the source's retain on us.
            let this = self.retain();
            let drawable = Retained::into_raw(update.drawable());
            // SAFETY: a `CAMetalDrawable`; `into_raw` hands its +1 retain to the wrapper.
            let drawable = unsafe { MetalDrawable::from_ptr(drawable.cast()) };
            (this.ivars().callback)(drawable);
        }
    }
);

impl GitViberMetalDisplayLinkDelegate {
    fn new(mtm: MainThreadMarker, callback: Box<dyn Fn(MetalDrawable)>) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(DelegateIvars { callback });
        // SAFETY: `init` is `NSObject`'s designated initializer.
        unsafe { msg_send![super(this), init] }
    }
}

/// A layer's `CAMetalDisplayLink` on the main run loop, calling back with each frame's
/// drawable. Dropping it invalidates the link; the layer can then use `nextDrawable` again.
pub struct MetalFrameSource {
    link: Retained<CAMetalDisplayLink>,
    // The link holds its delegate weakly.
    _delegate: Retained<GitViberMetalDisplayLinkDelegate>,
}

impl MetalFrameSource {
    /// Whether this system has `CAMetalDisplayLink` (macOS 14+, iOS 17+).
    pub fn is_available() -> bool {
        AnyClass::get(c"CAMetalDisplayLink").is_some()
    }

    /// `None` where `CAMetalDisplayLink` is unavailable. The link runs at the rate the system
    /// picks for the display (it follows ProMotion, Low Power Mode and thermal state).
    pub fn new(
        layer: &metal::MetalLayerRef,
        mtm: MainThreadMarker,
        callback: impl Fn(MetalDrawable) + 'static,
    ) -> Option<Self> {
        if !Self::is_available() {
            return None;
        }
        // SAFETY: metal-rs' `CAMetalLayer` is the same Objective-C object.
        let layer = unsafe { &*layer.as_ptr().cast::<CAMetalLayer>() };
        let delegate = GitViberMetalDisplayLinkDelegate::new(mtm, Box::new(callback));
        let link = CAMetalDisplayLink::initWithMetalLayer(CAMetalDisplayLink::alloc(), layer);
        link.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        // SAFETY: we are on the main thread (`mtm`), and the main run loop outlives the link.
        unsafe { link.addToRunLoop_forMode(&NSRunLoop::mainRunLoop(), NSRunLoopCommonModes) };
        Some(Self {
            link,
            _delegate: delegate,
        })
    }

    /// A paused link calls back with nothing and takes no drawables.
    pub fn set_paused(&self, paused: bool) {
        self.link.setPaused(paused);
    }
}

impl Drop for MetalFrameSource {
    fn drop(&mut self) {
        self.link.invalidate();
    }
}
