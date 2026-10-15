//! Downloads in an agent's tab, as WKDownloads: saved in the tab's own folder (downloads.rs),
//! told to its agent, and gone with the tab. A user's tab still hands them to the system
//! browser (mod.rs `download`).

use super::{agent, Delegate};
use crate::browser::downloads::{self, Outcome};
use crate::browser::notes;
use block2::DynBlock;
use objc2::rc::Retained;
use objc2::runtime::NSObject;
use objc2::{define_class, msg_send, DefinedClass, MainThreadOnly, Message};
use objc2_foundation::{
    ns_string, NSData, NSError, NSHTTPURLResponse, NSObjectProtocol, NSString, NSURLResponse, NSURL,
};
use objc2_web_kit::{
    WKDownload, WKDownloadDelegate, WKNavigationActionPolicy, WKNavigationResponse,
    WKNavigationResponsePolicy,
};
use std::cell::{OnceCell, RefCell};
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::{Instant, SystemTime};
use tauri::{AppHandle, Manager};

thread_local! {
    /// Each download going, with what saves it: WebKit holds its delegate weakly.
    static GOING: RefCell<Vec<(Retained<WKDownload>, Retained<Saver>)>> = RefCell::default();
    /// The app's cache folder, once a download needed it, and the tabs with a folder in it.
    static CACHE: OnceCell<PathBuf> = const { OnceCell::new() };
    static FOLDERS: RefCell<HashSet<String>> = RefCell::default();
}

/// A link with `download`: saved for an agent, else left to the system browser.
pub(super) fn on_link(delegate: &Delegate, url: &str) -> WKNavigationActionPolicy {
    if agent::routed(&delegate.ivars().id) {
        return WKNavigationActionPolicy::Download;
    }
    delegate.download(url);
    WKNavigationActionPolicy::Cancel
}

/// A page's file an agent's tab saves: one it can't show, or one sent as an attachment (a CSV
/// export it would show as text). None for anything else, as for a user's tab.
pub(super) fn on_response(
    delegate: &Delegate,
    response: &WKNavigationResponse,
) -> Option<WKNavigationResponsePolicy> {
    if !agent::routed(&delegate.ivars().id) || !unsafe { response.isForMainFrame() } {
        return None;
    }
    let sent = unsafe { response.response() };
    let attachment = sent
        .downcast_ref::<NSHTTPURLResponse>()
        .and_then(|r| r.valueForHTTPHeaderField(ns_string!("Content-Disposition")))
        .is_some_and(|d| downloads::attachment(&d.to_string()));
    let shows = unsafe { response.canShowMIMEType() };
    (attachment || !shows).then_some(WKNavigationResponsePolicy::Download)
}

/// A navigation became `download`: saved by a Saver of the tab's.
pub(super) fn began(delegate: &Delegate, download: &WKDownload) {
    let d = delegate.ivars();
    let saver = Saver::alloc(delegate.mtm()).set_ivars(SaverIvars {
        id: d.id.clone(),
        app: d.app.clone(),
        path: RefCell::default(),
        name: RefCell::new(file_of(download)),
        mime: RefCell::default(),
        refused: RefCell::default(),
    });
    let saver: Retained<Saver> = unsafe { msg_send![super(saver), init] };
    unsafe { download.setDelegate(Some(objc2::runtime::ProtocolObject::from_ref(&*saver))) };
    GOING.with_borrow_mut(|g| g.push((download.retain(), saver)));
}

/// The download's file name until the server suggests one: its address's last part.
fn file_of(download: &WKDownload) -> String {
    let url = unsafe { download.originalRequest() }.and_then(|r| r.URL());
    let last = url
        .and_then(|u| u.lastPathComponent())
        .map(|c| c.to_string())
        .filter(|c| !c.is_empty() && c != "/");
    downloads::file_name(&last.unwrap_or_default())
}

/// The tab closed: its downloads stop, and its folder goes.
pub(super) fn forget(id: &str) {
    let going: Vec<_> = GOING.with_borrow_mut(|g| {
        let (theirs, rest) = std::mem::take(g)
            .into_iter()
            .partition(|(_, s)| s.ivars().id == id);
        *g = rest;
        theirs
    });
    for (download, _) in going {
        unsafe { download.cancel(None) };
    }
    downloads::state().forget(id);
    if FOLDERS.with_borrow_mut(|f| f.remove(id)) {
        if let Some(cache) = CACHE.with(|c| c.get().cloned()) {
            let _ = std::fs::remove_dir_all(downloads::base(&cache).join(id));
        }
    }
}

/// Every agent tab went (the app page reloaded): theirs too.
pub(super) fn forget_all() {
    let ids: Vec<String> = FOLDERS.with_borrow(|f| f.iter().cloned().collect());
    let going: Vec<String> =
        GOING.with_borrow(|g| g.iter().map(|(_, s)| s.ivars().id.clone()).collect());
    for id in ids.into_iter().chain(going) {
        forget(&id);
    }
}

struct SaverIvars {
    id: String,
    app: AppHandle,
    /// Where it's written, once WebKit asked.
    path: RefCell<Option<PathBuf>>,
    name: RefCell<String>,
    mime: RefCell<String>,
    /// Why no place was given it, when none was: WebKit only says it was cancelled.
    refused: RefCell<Option<String>>,
}

define_class!(
    /// Saves one download of an agent's tab, and tells how it ended.
    // SAFETY: NSObject has no subclassing requirements, and Saver has no Drop.
    #[unsafe(super(NSObject))]
    #[name = "GitViberBrowserDownload"]
    #[thread_kind = MainThreadOnly]
    #[ivars = SaverIvars]
    struct Saver;

    unsafe impl NSObjectProtocol for Saver {}

    unsafe impl WKDownloadDelegate for Saver {
        #[unsafe(method(download:decideDestinationUsingResponse:suggestedFilename:completionHandler:))]
        fn destination(
            &self,
            _download: &WKDownload,
            response: &NSURLResponse,
            suggested: &NSString,
            done: &DynBlock<dyn Fn(*mut NSURL)>,
        ) {
            let mime = response.MIMEType().map(|m| m.to_string());
            *self.ivars().mime.borrow_mut() = mime.unwrap_or_else(|| "unknown type".into());
            match self.place(&suggested.to_string()) {
                Ok(path) => {
                    let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
                    downloads::state().started(&self.ivars().id, path.clone());
                    *self.ivars().path.borrow_mut() = Some(path);
                    done.call((Retained::as_ptr(&url).cast_mut(),));
                }
                Err(why) => {
                    *self.ivars().refused.borrow_mut() = Some(why);
                    done.call((std::ptr::null_mut(),));
                }
            }
        }

        #[unsafe(method(downloadDidFinish:))]
        fn finished(&self, download: &WKDownload) {
            let path = self.ivars().path.borrow().clone();
            let outcome = match &path {
                Some(path) => Outcome::Done {
                    name: self.ivars().name.borrow().clone(),
                    path: path.clone(),
                    size: std::fs::metadata(path).map_or(0, |m| m.len()),
                    mime: self.ivars().mime.borrow().clone(),
                },
                None => self.failed_as("it was given no file".into()),
            };
            self.ended(download, outcome);
        }

        #[unsafe(method(download:didFailWithError:resumeData:))]
        fn failed(&self, download: &WKDownload, error: &NSError, _resume: Option<&NSData>) {
            const CANCELLED: isize = -999;
            let why = match (self.ivars().refused.take(), error.code()) {
                (Some(why), _) => why,
                (None, CANCELLED) => "cancelled".into(),
                (None, _) => error.localizedDescription().to_string(),
            };
            // Nothing half written is left to read.
            if let Some(path) = self.ivars().path.borrow().as_ref() {
                let _ = std::fs::remove_file(path);
            }
            let outcome = self.failed_as(why);
            self.ended(download, outcome);
        }
    }
);

impl Saver {
    /// A free path in the tab's folder for the file the server suggests.
    fn place(&self, suggested: &str) -> Result<PathBuf, String> {
        let d = self.ivars();
        let cache = match CACHE.with(|c| c.get().cloned()) {
            Some(cache) => cache,
            None => {
                let cache = d.app.path().app_cache_dir().map_err(|e| e.to_string())?;
                CACHE.with(|c| c.get_or_init(|| cache).clone())
            }
        };
        let folder = downloads::tab_folder(&cache, &d.id, SystemTime::now())
            .map_err(|e| format!("couldn't make its folder: {e}"))?;
        FOLDERS.with_borrow_mut(|f| f.insert(d.id.clone()));
        let name = downloads::file_name(suggested);
        let path = downloads::free_path(&folder, &name, |p| p.exists());
        *d.name.borrow_mut() = path
            .file_name()
            .map_or(name, |n| n.to_string_lossy().into_owned());
        Ok(path)
    }

    fn failed_as(&self, why: String) -> Outcome {
        Outcome::Failed {
            name: self.ivars().name.borrow().clone(),
            why,
        }
    }

    /// Told to a `wait --download`, else in the pane's next reply; let go of either way.
    fn ended(&self, download: &WKDownload, outcome: Outcome) {
        let d = self.ivars();
        GOING.with_borrow_mut(|g| g.retain(|(w, _)| !std::ptr::eq(&**w, download)));
        let path = d.path.borrow().clone();
        let waited =
            downloads::state().ended(&d.id, path.as_deref(), outcome.clone(), Instant::now());
        if !waited {
            if let Some((pty, name)) = agent::pane_of(&d.id) {
                notes::note(pty, Some((&d.id, &name)), outcome.note());
            }
        }
    }
}
