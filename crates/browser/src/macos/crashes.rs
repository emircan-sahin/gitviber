//! A page whose WebContent process ended (crashes.rs): loaded again, its agent told in the
//! pane's next reply, and neither parked nor closed while that reload runs.

use super::{agent, begin_park, url_text, Delegate, View};
use crate::crashes::{self, Ended};
use crate::notes;
use block2::RcBlock;
use gitviber_core::Res;
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
    // The page that saw the keys go down is gone: a key held for it can't be let go there, and
    // would be refused as held already once the page is back.
    if let Some(web) = d.web.load() {
        web.ivars().held.borrow_mut().release_all();
    }
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

/// A park of a view mid-recovery waits for it, tried again shortly: true when held. Not while
/// memory is short, when a page that just ended of it is the one to let go.
pub(super) fn hold_park(v: &View, id: &str, since: u64) -> bool {
    if !recovering(v) || memory_pressed() {
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

fn memory_pressed() -> bool {
    let mut level: libc::c_int = 0;
    let mut size = std::mem::size_of::<libc::c_int>();
    let read = unsafe {
        libc::sysctlbyname(
            c"kern.memorystatus_vm_pressure_level".as_ptr(),
            (&raw mut level).cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    read == 0 && crashes::pressed(level)
}
