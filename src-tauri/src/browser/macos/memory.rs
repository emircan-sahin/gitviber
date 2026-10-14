//! The system's memory pressure: hidden views park at once, rather than when their timers say.
//! Each is a WebContent process, and a parked one loads again when its tab shows.

use super::{begin_park, VIEWS};
use block2::{DynBlock, RcBlock};
use std::cell::Cell;

/// A dispatch object of libdispatch's, which only its C interface names.
#[repr(C)]
struct Dispatch {
    _private: [u8; 0],
}

/// DISPATCH_MEMORYPRESSURE_WARN and _CRITICAL: not NORMAL, which says it's over.
const PRESSED: usize = 0x2 | 0x4;

extern "C" {
    static _dispatch_main_q: Dispatch;
    static _dispatch_source_type_memorypressure: Dispatch;
    fn dispatch_source_create(
        kind: *const Dispatch,
        handle: usize,
        mask: usize,
        queue: *const Dispatch,
    ) -> *mut Dispatch;
    fn dispatch_source_set_event_handler(source: *mut Dispatch, handler: &DynBlock<dyn Fn()>);
    fn dispatch_resume(object: *mut Dispatch);
}

thread_local! {
    static WATCHING: Cell<bool> = const { Cell::new(false) };
}

/// From the first view on, for the life of the app: on the main queue, where the views are.
pub(super) fn watch() {
    if WATCHING.replace(true) {
        return;
    }
    let park = RcBlock::new(|| {
        for (id, since) in VIEWS.with_borrow(|r| r.hidden()) {
            begin_park(&id, since);
        }
    });
    unsafe {
        let source = dispatch_source_create(
            &raw const _dispatch_source_type_memorypressure,
            0,
            PRESSED,
            &raw const _dispatch_main_q,
        );
        if source.is_null() {
            return;
        }
        // libdispatch copies the block, and the source lives as long as the app.
        dispatch_source_set_event_handler(source, &park);
        dispatch_resume(source);
    }
}
