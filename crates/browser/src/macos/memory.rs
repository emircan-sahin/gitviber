//! The system's memory pressure: hidden views park at once, rather than when their timers say.
//! Each is a WebContent process, and a parked one loads again when its tab shows. A warning
//! parks agents' sleeping tabs only; the user's go too once it's critical.

use super::{agent, begin_park, VIEWS};
use block2::{DynBlock, RcBlock};
use std::cell::Cell;

/// A dispatch object of libdispatch's, which only its C interface names.
#[repr(C)]
struct Dispatch {
    _private: [u8; 0],
}

/// DISPATCH_MEMORYPRESSURE_WARN and _CRITICAL: not NORMAL, which says it's over.
const WARN: usize = 0x2;
const CRITICAL: usize = 0x4;

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
    fn dispatch_source_get_data(source: *mut Dispatch) -> usize;
}

thread_local! {
    /// The source, once made: it lives as long as the app.
    static SOURCE: Cell<*mut Dispatch> = const { Cell::new(std::ptr::null_mut()) };
}

/// From the first view on, for the life of the app: on the main queue, where the views are.
pub(super) fn watch() {
    if !SOURCE.get().is_null() {
        return;
    }
    let park = RcBlock::new(|| {
        let critical = unsafe { dispatch_source_get_data(SOURCE.get()) } & CRITICAL != 0;
        for (id, since) in VIEWS.with_borrow(|r| r.hidden()) {
            if critical || agent::routed(&id) {
                begin_park(&id, since);
            }
        }
    });
    unsafe {
        let source = dispatch_source_create(
            &raw const _dispatch_source_type_memorypressure,
            0,
            WARN | CRITICAL,
            &raw const _dispatch_main_q,
        );
        if source.is_null() {
            return;
        }
        SOURCE.set(source);
        // libdispatch copies the block, and the source lives as long as the app.
        dispatch_source_set_event_handler(source, &park);
        dispatch_resume(source);
    }
}
