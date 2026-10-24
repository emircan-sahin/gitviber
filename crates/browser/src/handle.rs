//! What the browser needs of the app, where the Tauri build had its AppHandle: where its events
//! go, the terminals (a request names its pane), the app's folders, and the main thread, where
//! AppKit's views are.

use gitviber_core::pty::Ptys;
use gitviber_core::{Core, Res};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

/// One event the views send, named as the old page heard it ("browser-state", "browser-open"…),
/// its payload as it was serialized for the page. Sent on the main thread mostly; the sink hands
/// it to the UI thread itself.
#[derive(Clone, Debug)]
pub struct Event {
    pub name: &'static str,
    pub payload: serde_json::Value,
}

pub type Sink = Arc<dyn Fn(Event) + Send + Sync>;

#[derive(Clone)]
pub struct Handle(Arc<Inner>);

struct Inner {
    core: Arc<Core>,
    events: Sink,
    data: Option<PathBuf>,
    cache: Option<PathBuf>,
}

/// The app's folders, as `app.path()` named them.
pub struct Paths<'a>(&'a Inner);

impl Paths<'_> {
    /// ~/Library/Application Support/app.gitviber.desktop on macOS: saved sessions.
    pub fn app_data_dir(&self) -> Res<PathBuf> {
        self.0
            .data
            .clone()
            .ok_or_else(|| "The app's data folder can't be found.".into())
    }

    /// ~/Library/Caches/app.gitviber.desktop: downloads, pictures.
    pub fn app_cache_dir(&self) -> Res<PathBuf> {
        self.0
            .cache
            .clone()
            .ok_or_else(|| "The app's cache folder can't be found.".into())
    }

    /// ~/Downloads: what the user's own tabs download.
    pub fn download_dir(&self) -> Res<PathBuf> {
        std::env::var_os("HOME")
            .filter(|h| !h.is_empty())
            .map(|h| PathBuf::from(h).join("Downloads"))
            .filter(|d| d.is_dir())
            .ok_or_else(|| "There is no Downloads folder.".into())
    }
}

impl Handle {
    pub fn new(core: Arc<Core>, events: Sink) -> Self {
        Self::with_dirs(core, events, data_dir(), gitviber_core::paths::cache_dir())
    }

    /// With folders of the caller's: tests and demos keep off the user's.
    pub fn with_dirs(
        core: Arc<Core>,
        events: Sink,
        data: Option<PathBuf>,
        cache: Option<PathBuf>,
    ) -> Self {
        Self(Arc::new(Inner {
            core,
            events,
            data,
            cache,
        }))
    }

    pub fn emit<T: Serialize>(&self, name: &'static str, payload: T) {
        if let Ok(payload) = serde_json::to_value(payload) {
            (self.0.events)(Event { name, payload });
        }
    }

    pub fn ptys(&self) -> &Ptys {
        &self.0.core.ptys
    }

    pub fn core(&self) -> &Arc<Core> {
        &self.0.core
    }

    pub fn path(&self) -> Paths<'_> {
        Paths(&self.0)
    }

    /// `f` on the main thread, soon: AppKit's main queue, which the app's run loop drains.
    pub fn run_on_main_thread(&self, f: impl FnOnce() + Send + 'static) -> Res<()> {
        main_queue::run(Box::new(f));
        Ok(())
    }
}

/// The handle the app installed (`install`), for what has none passed to it.
static HANDLE: OnceLock<Handle> = OnceLock::new();

/// Once, at launch; a second is ignored.
pub fn install(handle: Handle) {
    let _ = HANDLE.set(handle);
}

pub fn installed() -> Option<&'static Handle> {
    HANDLE.get()
}

fn data_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)?;
    #[cfg(target_os = "macos")]
    let base = home.join("Library/Application Support");
    #[cfg(not(target_os = "macos"))]
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home.join(".local/share"));
    Some(base.join(gitviber_core::paths::IDENTIFIER))
}

#[cfg(target_os = "macos")]
mod main_queue {
    use std::ffi::c_void;

    #[repr(C)]
    struct Queue {
        _private: [u8; 0],
    }

    unsafe extern "C" {
        static _dispatch_main_q: Queue;
        fn dispatch_async_f(
            queue: *const Queue,
            context: *mut c_void,
            work: extern "C" fn(*mut c_void),
        );
    }

    type Work = Box<dyn FnOnce() + Send>;

    extern "C" fn trampoline(context: *mut c_void) {
        // SAFETY: `run` leaked exactly this box for this one call.
        let work = unsafe { Box::from_raw(context.cast::<Work>()) };
        work();
    }

    pub fn run(work: Work) {
        let context = Box::into_raw(Box::new(work)).cast::<c_void>();
        // SAFETY: the main queue lives as long as the process; the trampoline frees the context.
        unsafe { dispatch_async_f(&raw const _dispatch_main_q, context, trampoline) };
    }
}

/// Nothing there is bound to a thread: every call answers that the browser needs macOS.
#[cfg(not(target_os = "macos"))]
mod main_queue {
    pub fn run(work: Box<dyn FnOnce() + Send>) {
        work();
    }
}
