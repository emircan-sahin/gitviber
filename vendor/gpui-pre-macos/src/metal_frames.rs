//! GitViber: a window's `CAMetalDisplayLink` (macOS 14+), shared with GPUI's frame waker so a
//! wake reaches the link without the window's lock.
//!
//! The link pauses after an update in which GPUI neither drew, presented, nor asked for another
//! frame, so an idle window takes no drawables and does no work. GPUI still decides every frame
//! (presenting for a while after high-rate input included); `wake` resumes the link.

use gpui_apple::display_link::MetalFrameSource;
use std::cell::{Cell, RefCell};

#[derive(Default)]
pub(crate) struct MetalFrames {
    source: RefCell<Option<MetalFrameSource>>,
    /// Whether GPUI asked for a frame since the current update began.
    woken: Cell<bool>,
}

impl MetalFrames {
    /// Dropping the old source invalidates its link.
    pub fn replace(&self, source: Option<MetalFrameSource>) {
        *self.source.borrow_mut() = source;
    }

    pub fn is_attached(&self) -> bool {
        self.source.borrow().is_some()
    }

    pub fn wake(&self) {
        self.woken.set(true);
        // A source being replaced starts unpaused, so skipping it is safe.
        if let Ok(source) = self.source.try_borrow()
            && let Some(source) = source.as_ref()
        {
            source.set_paused(false);
        }
    }

    pub fn begin_frame(&self) {
        self.woken.set(false);
    }

    /// Ends an update; pauses the link when GPUI neither used the drawable nor asked for another
    /// frame meanwhile. Returns whether it paused.
    pub fn end_frame(&self, used_drawable: bool) -> bool {
        let pause = !used_drawable && !self.woken.get();
        if pause && let Some(source) = self.source.borrow().as_ref() {
            source.set_paused(true);
        }
        pause
    }
}

#[cfg(test)]
mod gitviber_tests {
    use super::*;

    #[test]
    fn pauses_only_when_gpui_neither_draws_nor_asks_again() {
        let frames = MetalFrames::default();

        frames.begin_frame();
        assert!(
            !frames.end_frame(true),
            "a frame that drew keeps the link running"
        );

        frames.begin_frame();
        frames.wake();
        assert!(
            !frames.end_frame(false),
            "a wake during the update (a throttled frame's retry) keeps it running"
        );

        frames.wake();
        frames.begin_frame();
        assert!(
            frames.end_frame(false),
            "a wake before the update was served by it"
        );

        frames.begin_frame();
        assert!(frames.end_frame(false), "an idle update pauses the link");
    }

    #[test]
    fn a_detached_window_has_no_link_to_wake() {
        let frames = MetalFrames::default();
        assert!(!frames.is_attached());
        frames.wake();
        frames.replace(None);
        assert!(!frames.is_attached());
    }
}
