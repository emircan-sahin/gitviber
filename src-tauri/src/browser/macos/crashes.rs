//! A page whose WebContent process ended (crashes.rs): loaded again, its agent told in the
//! pane's next reply, and neither parked nor closed while that reload runs.

use super::{agent, begin_park, url_text, Delegate, View};
use crate::browser::crashes::{self, Ended};
use crate::browser::notes;
use crate::state::Res;
use block2::RcBlock;
use objc2::DefinedClass;
use objc2_foundation::NSTimer;
use objc2_web_kit::WKWebView;
use std::ptr::NonNull;
use std::time::{Duration, Instant};

/// How soon a park held for a reload is tried again.
const RETRY: Duration = Duration::from_secs(2);

/// The process ended (`reason`: WebKit's private code, where it gave one): reloaded once, else
/// left stopped on its error.
pub(super) fn ended(delegate: &Delegate, web: &WKWebView, reason: Option<isize>) {
    let d = delegate.ivars();
    let why = crashes::reason(reason);
    let ended = d.crashes.borrow_mut().ended(why, Instant::now());
    let note = match ended {
        Ended::Nothing => return,
        Ended::Reload => {
            unsafe { drop(web.reload()) };
            crashes::reloaded(why)
        }
        Ended::Stop => {
            delegate.fail(url_text(unsafe { web.URL() }), crashes::keeps_crashing(why));
            crashes::stopped(why)
        }
    };
    if let Some((pty, name)) = agent::pane_of(&d.id) {
        notes::note(pty, Some((&d.id, &name)), note);
    }
}

/// Whether the view's reload after its process ended is still running.
pub(super) fn recovering(v: &View) -> bool {
    let loading = unsafe { v.web.isLoading() };
    v.delegate
        .ivars()
        .crashes
        .borrow()
        .recovering(Instant::now(), loading)
}

/// A park of a view mid-recovery waits for it, tried again shortly: true when held.
pub(super) fn hold_park(v: &View, id: &str, since: u64) -> bool {
    if !recovering(v) {
        return false;
    }
    let id = id.to_string();
    let retry = RcBlock::new(move |_: NonNull<NSTimer>| begin_park(&id, since));
    unsafe {
        NSTimer::scheduledTimerWithTimeInterval_repeats_block(RETRY.as_secs_f64(), false, &retry)
    };
    true
}

/// An agent's command on a page left stopped says why, rather than finding nothing.
pub(super) fn check(v: &View) -> Res<()> {
    match v.delegate.ivars().crashes.borrow().stopped() {
        Some(why) => Err(crashes::try_again(why)),
        None => Ok(()),
    }
}
