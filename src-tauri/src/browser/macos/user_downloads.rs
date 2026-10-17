//! Downloads in a user's tab, as WKDownloads: saved in the Downloads folder (user_downloads.rs),
//! reported to the app page as they go and when they end, and cancelled from there. An agent's
//! tab saves its files apart (downloads.rs).

use super::{downloads::file_of, emit, Delegate};
use crate::browser::downloads;
use crate::browser::user_downloads::{Claims, Progress, State};
use block2::DynBlock;
use objc2::rc::Retained;
use objc2::runtime::{NSObject, ProtocolObject};
use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadOnly, Message};
use objc2_foundation::{
    NSData, NSError, NSObjectNSDelayedPerforming, NSObjectProtocol, NSProgressReporting, NSString,
    NSURLResponse, NSURL,
};
use objc2_web_kit::{WKDownload, WKDownloadDelegate};
use std::cell::{Cell, RefCell};
use std::ffi::CString;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

/// How often a download going says how far it is.
const TICK_SECS: f64 = 0.25;

thread_local! {
    /// Each download going, with what saves it: WebKit holds its delegate weakly.
    static GOING: RefCell<Vec<Retained<Saver>>> = RefCell::default();
    static CLAIMS: RefCell<Claims> = RefCell::default();
    static NEXT: Cell<u64> = const { Cell::new(1) };
}

/// A navigation of user tab `delegate` became `download`: saved by a Saver of its own.
pub(super) fn began(delegate: &Delegate, download: &WKDownload) {
    let d = delegate.ivars();
    let key = NEXT.replace(NEXT.get() + 1);
    let saver = Saver::alloc(delegate.mtm()).set_ivars(SaverIvars {
        tab: d.id.clone(),
        key,
        app: d.app.clone(),
        download: RefCell::new(Some(download.retain())),
        path: RefCell::default(),
        name: RefCell::new(file_of(download)),
        refused: RefCell::default(),
        quiet: Cell::new(false),
    });
    let saver: Retained<Saver> = unsafe { msg_send![super(saver), init] };
    unsafe { download.setDelegate(Some(ProtocolObject::from_ref(&*saver))) };
    saver.say(State::Going, None);
    saver.tick_later();
    GOING.with_borrow_mut(|g| g.push(saver));
}

/// Whether tab `tab` has a download going: its page parks only once they end, or its view's
/// going would take them along.
pub(super) fn going(tab: &str) -> bool {
    GOING.with_borrow(|g| g.iter().any(|s| s.ivars().tab == tab))
}

/// The user cancelled download `key`; it ends as WebKit reports it cancelled.
pub(super) fn cancel(key: u64) {
    let download = GOING.with_borrow(|g| {
        g.iter()
            .find(|s| s.ivars().key == key)
            .and_then(|s| s.ivars().download.borrow().clone())
    });
    if let Some(download) = download {
        unsafe { download.cancel(None) };
    }
}

/// The tab closed: its downloads stop, unreported (the app page forgot the tab), and their
/// partial files go.
pub(super) fn forget(tab: &str) {
    let theirs: Vec<_> =
        GOING.with_borrow(|g| g.iter().filter(|s| s.ivars().tab == tab).cloned().collect());
    for saver in theirs {
        saver.ivars().quiet.set(true);
        if let Some(download) = saver.ivars().download.borrow().clone() {
            unsafe { download.cancel(None) };
        }
    }
}

/// Every tab went (the app page reloaded).
pub(super) fn forget_all() {
    let tabs: Vec<_> = GOING.with_borrow(|g| g.iter().map(|s| s.ivars().tab.clone()).collect());
    for tab in tabs {
        forget(&tab);
    }
}

struct SaverIvars {
    tab: String,
    key: u64,
    app: AppHandle,
    /// Until it ends.
    download: RefCell<Option<Retained<WKDownload>>>,
    /// Where it's written, once WebKit asked.
    path: RefCell<Option<PathBuf>>,
    name: RefCell<String>,
    /// Why no place was given it, when none was: WebKit only says it was cancelled.
    refused: RefCell<Option<String>>,
    /// Its tab is gone: nothing is told.
    quiet: Cell<bool>,
}

define_class!(
    /// Saves one download of a user's tab, and tells how it goes and ends.
    // SAFETY: NSObject has no subclassing requirements, and Saver has no Drop.
    #[unsafe(super(NSObject))]
    #[name = "GitViberUserDownload"]
    #[thread_kind = MainThreadOnly]
    #[ivars = SaverIvars]
    struct Saver;

    unsafe impl NSObjectProtocol for Saver {}

    impl Saver {
        #[unsafe(method(gvTick))]
        fn tick(&self) {
            if self.ivars().download.borrow().is_some() {
                self.say(State::Going, None);
                self.tick_later();
            }
        }
    }

    unsafe impl WKDownloadDelegate for Saver {
        #[unsafe(method(download:decideDestinationUsingResponse:suggestedFilename:completionHandler:))]
        fn destination(
            &self,
            _download: &WKDownload,
            _response: &NSURLResponse,
            suggested: &NSString,
            done: &DynBlock<dyn Fn(*mut NSURL)>,
        ) {
            match self.place(&suggested.to_string()) {
                Ok(path) => {
                    let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
                    *self.ivars().path.borrow_mut() = Some(path);
                    self.say(State::Going, None);
                    done.call((Retained::as_ptr(&url).cast_mut(),));
                }
                Err(why) => {
                    *self.ivars().refused.borrow_mut() = Some(why);
                    done.call((std::ptr::null_mut(),));
                }
            }
        }

        #[unsafe(method(downloadDidFinish:))]
        fn finished(&self, _download: &WKDownload) {
            let path = self.ivars().path.borrow().clone();
            match path {
                Some(path) => {
                    quarantine(&path);
                    self.end(State::Done, None);
                }
                None => self.end(State::Failed, Some("it was given no file".into())),
            }
        }

        #[unsafe(method(download:didFailWithError:resumeData:))]
        fn failed(&self, _download: &WKDownload, error: &NSError, _resume: Option<&NSData>) {
            const CANCELLED: isize = -999;
            // NSURLErrorCannotCreateFile: what's at its path isn't its own.
            const NOT_MADE: isize = -3000;
            // Nothing half written is left; only a file it made.
            if let Some(path) = self
                .ivars()
                .path
                .borrow()
                .as_ref()
                .filter(|_| error.code() != NOT_MADE)
            {
                let _ = std::fs::remove_file(path);
            }
            match (self.ivars().refused.take(), error.code()) {
                (Some(why), _) => self.end(State::Failed, Some(why)),
                (None, CANCELLED) => self.end(State::Cancelled, None),
                (None, _) => self.end(State::Failed, Some(error.localizedDescription().to_string())),
            }
        }
    }
);

impl Saver {
    /// A free path in the Downloads folder for the file the server suggests.
    fn place(&self, suggested: &str) -> Result<PathBuf, String> {
        let folder = self
            .ivars()
            .app
            .path()
            .download_dir()
            .map_err(|_| "there is no Downloads folder".to_string())?;
        let name = downloads::file_name(suggested);
        let path = CLAIMS.with_borrow_mut(|c| c.claim(&folder, &name));
        *self.ivars().name.borrow_mut() = path
            .file_name()
            .map_or(name, |n| n.to_string_lossy().into_owned());
        Ok(path)
    }

    fn tick_later(&self) {
        unsafe { self.performSelector_withObject_afterDelay(sel!(gvTick), None, TICK_SECS) };
    }

    /// Where it is now, told to the app page.
    fn say(&self, state: State, why: Option<&str>) {
        let d = self.ivars();
        if d.quiet.get() {
            return;
        }
        let (received, total) = d.download.borrow().as_ref().map_or((0, -1), |w| {
            let p = w.progress();
            (p.completedUnitCount(), p.totalUnitCount())
        });
        let path = d.path.borrow();
        let path = path.as_deref().map(Path::to_string_lossy);
        emit(
            &d.app,
            "browser-download",
            Progress {
                tab: &d.tab,
                key: d.key,
                name: &d.name.borrow(),
                state,
                received,
                total,
                path: path.as_deref(),
                why,
            },
        );
    }

    /// It ended: told, and let go of.
    fn end(&self, state: State, why: Option<String>) {
        let d = self.ivars();
        if let Some(path) = d.path.borrow().as_deref() {
            CLAIMS.with_borrow_mut(|c| c.release(path));
        }
        self.say(state, why.as_deref());
        d.download.take();
        unsafe { NSObject::cancelPreviousPerformRequestsWithTarget(self) };
        GOING.with_borrow_mut(|g| g.retain(|s| s.ivars().key != d.key));
    }
}

/// Marks a finished file as from the web, as a browser does, so Gatekeeper checks what the user
/// opens: WebKit leaves it to the app (no LSFileQuarantineEnabled, which would mark every file
/// the app writes). Not over a mark already there.
fn quarantine(path: &Path) {
    const NAME: &[u8] = b"com.apple.quarantine\0";
    let Ok(file) = CString::new(path.to_string_lossy().as_bytes()) else {
        return;
    };
    let name = NAME.as_ptr().cast();
    let seen = unsafe { libc::getxattr(file.as_ptr(), name, std::ptr::null_mut(), 0, 0, 0) };
    if seen >= 0 {
        return;
    }
    let at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    // Flags 0081: downloaded, not yet opened.
    let value = format!("0081;{at:x};GitViber;");
    unsafe {
        libc::setxattr(
            file.as_ptr(),
            name,
            value.as_ptr().cast(),
            value.len(),
            0,
            0,
        )
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_finished_file_is_marked_as_from_the_web_once() {
        let file = std::env::temp_dir().join(format!("gitviber-quarantine-{}", std::process::id()));
        std::fs::write(&file, b"x").unwrap();
        let read = || {
            let path = CString::new(file.to_string_lossy().as_bytes()).unwrap();
            let mut buf = [0u8; 128];
            let n = unsafe {
                libc::getxattr(
                    path.as_ptr(),
                    c"com.apple.quarantine".as_ptr(),
                    buf.as_mut_ptr().cast(),
                    buf.len(),
                    0,
                    0,
                )
            };
            (n >= 0).then(|| String::from_utf8_lossy(&buf[..n as usize]).into_owned())
        };
        assert_eq!(read(), None);
        quarantine(&file);
        let mark = read().expect("marked");
        assert!(
            mark.starts_with("0081;") && mark.ends_with(";GitViber;"),
            "{mark}"
        );
        quarantine(&file);
        assert_eq!(read().as_deref(), Some(mark.as_str()));
        std::fs::remove_file(file).unwrap();
    }
}
