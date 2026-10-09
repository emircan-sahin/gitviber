//! The browser tab's native view (see mod.rs): a host view laid over the app page, holding a
//! WKWebView, and the delegate that keeps the page to web URLs and tells the app page its state.
//! Everything here runs on the main thread, where AppKit's views belong: the registry is that
//! thread's own, and no callback takes a borrow of it, as WebKit calls back from within calls.

use super::keys::{self, Key, Route};
use super::Corners;
use super::{
    host_key, is_loopback, loadable, policy, root_key, Created, Failed, Go, PageState, Parked,
    Policy, Rect, Registry, Screen,
};
use crate::state::Res;
use block2::{DynBlock, RcBlock};
use objc2::encode::{Encoding, RefEncode};
use objc2::rc::{Retained, Weak};
use objc2::runtime::{AnyObject, Bool, NSObject, ProtocolObject};
use objc2::{
    available, define_class, msg_send, sel, ClassType, DefinedClass, MainThreadMarker,
    MainThreadOnly,
};
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSAutoresizingMaskOptions, NSBitmapImageFileType,
    NSBitmapImageRep, NSColor, NSEvent, NSEventModifierFlags, NSImage, NSImageCompressionFactor,
    NSModalResponse, NSModalResponseOK, NSOpenPanel, NSView, NSWindow, NSWindowOrderingMode,
    NSWorkspace,
};
use objc2_foundation::{
    ns_string, NSArray, NSDataBase64EncodingOptions, NSDate, NSDictionary, NSError,
    NSKeyValueChangeKey, NSKeyValueObservingOptions, NSNumber, NSObjectNSDelayedPerforming,
    NSObjectNSKeyValueObserverRegistration, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString,
    NSURLAuthenticationChallenge, NSURLAuthenticationMethodServerTrust, NSURLCredential,
    NSURLRequest, NSURLSessionAuthChallengeDisposition as Disposition, NSURL, NSUUID,
};
use objc2_web_kit::{
    WKFrameInfo, WKNavigation, WKNavigationAction, WKNavigationActionPolicy, WKNavigationDelegate,
    WKNavigationResponse, WKNavigationResponsePolicy, WKNavigationType, WKOpenPanelParameters,
    WKSnapshotConfiguration, WKUIDelegate, WKUserContentController, WKWebView,
    WKWebViewConfiguration, WKWebsiteDataStore, WKWindowFeatures,
};
use serde::Serialize;
use std::cell::{Cell, OnceCell, RefCell};
use std::collections::HashSet;
use std::ffi::c_void;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, Url};

/// The browser's website data, apart from the app page's own (WebKit's default store), so
/// clearing it can't touch the app's settings. A made-up id; macOS 14 keeps the store by it.
const STORE: [u8; 16] = [
    0x6f, 0x1d, 0x2a, 0x93, 0x5c, 0x4e, 0x4b, 0x7a, 0x9e, 0x31, 0x0b, 0x8d, 0x52, 0xc6, 0xa4, 0x17,
];

/// What the address bar and buttons show, observed on the web view.
const OBSERVED: [&str; 6] = [
    "URL",
    "title",
    "loading",
    "estimatedProgress",
    "canGoBack",
    "canGoForward",
];

/// A burst of changes (progress ticks during a load) goes to the page as one event.
const COALESCE_SECS: f64 = 0.1;

/// alert() and confirm() a page may show in DIALOG_WINDOW; past them they answer by themselves,
/// so `while (true) alert()` can't hold the window. A window, not a load: an app that routes
/// with pushState never loads again.
const DIALOGS: usize = 3;
const DIALOG_WINDOW: Duration = Duration::from_secs(10);

/// Asked as an agent finishes: whether a dev server reloads the page on a change by itself
/// (Vite, webpack, Next, Turbopack, Parcel). One that does isn't reloaded, which would lose its
/// state for nothing.
const HMR_PROBE: &str = r#"!!(document.querySelector('script[src*="/@vite/client"],script[src*="react-refresh"],script[src*="webpack-dev-server"],script[src*="hot-update"],script[src*="hmr-client"]') || window.__vite_plugin_react_preamble_installed__ || Object.keys(window).some((k) => /^(webpackHotUpdate|__turbopack|TURBOPACK|parcelHotUpdate)/.test(k)))"#;

const GONE: &str = "This browser tab is closed.";

#[derive(Clone)]
struct View {
    host: Retained<Host>,
    web: Retained<WebView>,
    delegate: Retained<Delegate>,
}

thread_local! {
    static VIEWS: RefCell<Registry<View>> = const { RefCell::new(Registry::new()) };
    /// macOS 13 has no store by id: one in memory for every tab, gone when the app quits.
    static EPHEMERAL: OnceCell<Retained<WKWebsiteDataStore>> = const { OnceCell::new() };
    /// A hidden view parks after this many minutes (browserParkAfterMin); 0 never.
    static PARK_AFTER: Cell<u32> = const { Cell::new(10) };
}

fn view(id: &str) -> Option<View> {
    VIEWS.with_borrow(|r| r.get(id).cloned())
}

fn main_thread() -> Res<MainThreadMarker> {
    MainThreadMarker::new().ok_or_else(|| "The browser runs on the main thread.".into())
}

fn emit<T: Serialize + Clone>(app: &AppHandle, event: &str, payload: T) {
    let _ = app.emit_to("main", event, payload);
}

#[derive(Serialize, Clone)]
struct Id<'a> {
    id: &'a str,
}

#[derive(Serialize, Clone)]
struct Download<'a> {
    id: &'a str,
    url: &'a str,
}

/// SecTrustRef, which only the Security framework's crate types; passed through as is.
#[repr(C)]
struct SecTrust {
    _private: [u8; 0],
}

// SAFETY: SecTrustRef is a pointer to the opaque struct __SecTrust, as Security's headers declare.
unsafe impl RefEncode for SecTrust {
    const ENCODING_REF: Encoding = Encoding::Pointer(&Encoding::Struct("__SecTrust", &[]));
}

#[link(name = "Security", kind = "framework")]
extern "C" {
    fn SecTrustEvaluateWithError(trust: *mut SecTrust, error: *mut *mut c_void) -> bool;
}

/// CGColorRef, for the host's layer.
#[repr(C)]
struct CGColor {
    _private: [u8; 0],
}

// SAFETY: CGColorRef is a pointer to the opaque struct CGColor, as CoreGraphics declares it.
unsafe impl RefEncode for CGColor {
    const ENCODING_REF: Encoding = Encoding::Pointer(&Encoding::Struct("CGColor", &[]));
}

define_class!(
    /// Holds the web view over the app page. Opaque: a see-through window would otherwise show
    /// the desktop behind a page that hasn't painted yet.
    // SAFETY: NSView has no subclassing requirements, and Host has no Drop.
    #[unsafe(super(NSView))]
    #[name = "GitViberBrowserHost"]
    struct Host;

    impl Host {
        /// Keys the page didn't use would go on up to the window's content view, which hands
        /// them to the menu bar: a plain J on a page would run Next Changed File.
        #[unsafe(method(keyDown:))]
        fn key_down(&self, _event: &NSEvent) {}
    }
);

impl Host {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        paint(&this, &NSColor::windowBackgroundColor());
        this
    }
}

fn paint(view: &NSView, color: &NSColor) {
    view.setWantsLayer(true);
    unsafe {
        let color: *mut CGColor = msg_send![color, CGColor];
        let layer: *mut AnyObject = msg_send![view, layer];
        if !layer.is_null() {
            let _: () = msg_send![layer, setBackgroundColor: color];
        }
    }
}

/// Rounds `corners` of a layer-backed view, what it holds clipped to them.
fn round(view: &NSView, radius: f64, corners: Corners) {
    // CACornerMask, in the layer's coordinates, which run up from the bottom like the view's.
    const BOTTOM_LEFT: usize = 1;
    const BOTTOM_RIGHT: usize = 2;
    const TOP_LEFT: usize = 4;
    const TOP_RIGHT: usize = 8;
    let mask = [
        (corners.top_left, TOP_LEFT),
        (corners.top_right, TOP_RIGHT),
        (corners.bottom_right, BOTTOM_RIGHT),
        (corners.bottom_left, BOTTOM_LEFT),
    ]
    .into_iter()
    .filter_map(|(on, bit)| on.then_some(bit))
    .sum::<usize>();
    let radius = if mask == 0 { 0.0 } else { radius };
    unsafe {
        let layer: *mut AnyObject = msg_send![view, layer];
        if !layer.is_null() {
            let _: () = msg_send![layer, setCornerRadius: radius];
            let _: () = msg_send![layer, setMaskedCorners: mask];
            let _: () = msg_send![layer, setMasksToBounds: Bool::new(radius > 0.0)];
        }
    }
}

/// Whether WebKit here lets a page see another pixel ratio (a private call).
fn can_set_dpr(web: &WKWebView) -> bool {
    web.respondsToSelector(sel!(_setOverrideDeviceScaleFactor:))
}

struct WebIvars {
    id: String,
    app: AppHandle,
}

define_class!(
    /// The page's view: the app's chords go to the app (see keys.rs), and taking focus says so.
    // SAFETY: WKWebView allows subclassing; WebView only overrides two NSResponder methods,
    // calling super, and its ivars need no Drop of their own.
    #[unsafe(super(WKWebView))]
    #[name = "GitViberBrowserWebView"]
    #[ivars = WebIvars]
    struct WebView;

    impl WebView {
        /// AppKit offers every ⌘ key to each view in the window; only the focused page answers.
        #[unsafe(method(performKeyEquivalent:))]
        fn perform_key_equivalent(&self, event: &NSEvent) -> Bool {
            if !self.focused() {
                return Bool::NO;
            }
            let key = key_of(&self.ivars().id, event);
            match keys::route_now(&key) {
                Route::Page => unsafe { msg_send![super(self), performKeyEquivalent: event] },
                Route::System => Bool::NO,
                Route::App => {
                    emit(&self.ivars().app, "browser-key", key);
                    Bool::YES
                }
            }
        }

        #[unsafe(method(becomeFirstResponder))]
        fn become_first_responder(&self) -> Bool {
            let took: Bool = unsafe { msg_send![super(self), becomeFirstResponder] };
            if took.as_bool() {
                emit(&self.ivars().app, "browser-focus", Id { id: &self.ivars().id });
            }
            took
        }
    }
);

impl WebView {
    fn focused(&self) -> bool {
        let responder = self.window().and_then(|w| w.firstResponder());
        responder.is_some_and(|r| std::ptr::eq(Retained::as_ptr(&r).cast(), self))
    }
}

fn key_of(id: &str, event: &NSEvent) -> Key {
    let flags = event.modifierFlags();
    let typed = event
        .charactersIgnoringModifiers()
        .map(|s| s.to_string())
        .unwrap_or_default();
    Key {
        id: id.into(),
        key: keys::name(&typed),
        code: keys::code(event.keyCode()),
        meta_key: flags.contains(NSEventModifierFlags::Command),
        ctrl_key: flags.contains(NSEventModifierFlags::Control),
        alt_key: flags.contains(NSEventModifierFlags::Option),
        shift_key: flags.contains(NSEventModifierFlags::Shift),
        repeat: event.isARepeat(),
    }
}

struct DelegateIvars {
    id: String,
    app: AppHandle,
    web: Weak<WebView>,
    /// A `browser-state` is on its way (COALESCE_SECS).
    pending: Cell<bool>,
    /// Loopback hosts (host_key) whose self-signed certificate was let through.
    trusted: RefCell<HashSet<String>>,
    /// Where the page is going, kept while it loads: a failed load doesn't leave it in `URL`.
    going: RefCell<String>,
    failed: RefCell<Option<Failed>>,
    /// When the last few alert() and confirm() showed.
    dialogs: RefCell<Vec<Instant>>,
    /// The page has loaded something since the view was made: a parked page's picture can go.
    committed: Cell<bool>,
    /// The hide's park timer pending (gvParkDue:), its argument, to cancel it by.
    timer: RefCell<Option<Retained<NSNumber>>>,
    /// When the view last went out of its tab's sight, for a timer set again (`configure`).
    hidden_at: Cell<Option<Instant>>,
    /// The device it shows (device mode), as last placed.
    screen: Cell<Option<Screen>>,
    /// The page's process ended once since the user last sent it somewhere: the next time it
    /// stays down, or a page that kills it on load would reload forever.
    crashed: Cell<bool>,
}

define_class!(
    /// Navigation, UI and state for one page: keeps it to web URLs, answers its dialogs, and
    /// reports what the address bar shows.
    // SAFETY: NSObject has no subclassing requirements, and Delegate has no Drop.
    #[unsafe(super(NSObject))]
    #[name = "GitViberBrowserDelegate"]
    #[thread_kind = MainThreadOnly]
    #[ivars = DelegateIvars]
    struct Delegate;

    impl Delegate {
        #[unsafe(method(observeValueForKeyPath:ofObject:change:context:))]
        fn observe(
            &self,
            _key: Option<&NSString>,
            _object: Option<&AnyObject>,
            _change: Option<&NSDictionary<NSKeyValueChangeKey, AnyObject>>,
            _context: *mut c_void,
        ) {
            self.changed();
        }

        /// A park timer (`hide`): `since` is the hide that set it.
        #[unsafe(method(gvParkDue:))]
        fn park_due(&self, since: &NSNumber) {
            self.ivars().timer.take();
            let (id, since) = (&self.ivars().id, since.unsignedLongLongValue());
            if PARK_AFTER.get() > 0 && VIEWS.with_borrow(|r| r.still_hidden(id, since)) {
                begin_park(id, since);
            }
        }

        #[unsafe(method(gvSendState))]
        fn send_state(&self) {
            self.ivars().pending.set(false);
            if let Some(web) = self.ivars().web.load() {
                emit(&self.ivars().app, "browser-state", self.state(&web));
            }
        }
    }

    unsafe impl NSObjectProtocol for Delegate {}

    unsafe impl WKNavigationDelegate for Delegate {
        #[unsafe(method(webView:decidePolicyForNavigationAction:decisionHandler:))]
        fn decide_action(
            &self,
            _web: &WKWebView,
            action: &WKNavigationAction,
            decide: &DynBlock<dyn Fn(WKNavigationActionPolicy)>,
        ) {
            let url = url_text(unsafe { action.request().URL() });
            let main = unsafe { action.targetFrame() }.is_none_or(|f| unsafe { f.isMainFrame() });
            let clicked = unsafe { action.navigationType() } == WKNavigationType::LinkActivated;
            let decision = match policy(&url, main, clicked) {
                // A link with `download`: nothing here saves files.
                Policy::Allow if unsafe { action.shouldPerformDownload() } => {
                    self.download(&url);
                    WKNavigationActionPolicy::Cancel
                }
                Policy::Allow => {
                    if clicked && main {
                        self.ivars().crashed.set(false);
                    }
                    WKNavigationActionPolicy::Allow
                }
                Policy::External => {
                    open_external(&url);
                    WKNavigationActionPolicy::Cancel
                }
                Policy::Cancel => WKNavigationActionPolicy::Cancel,
            };
            decide.call((decision,));
        }

        /// A file the page can't show (a zip, a dmg) would download: left to the system browser.
        #[unsafe(method(webView:decidePolicyForNavigationResponse:decisionHandler:))]
        fn decide_response(
            &self,
            _web: &WKWebView,
            response: &WKNavigationResponse,
            decide: &DynBlock<dyn Fn(WKNavigationResponsePolicy)>,
        ) {
            if unsafe { response.canShowMIMEType() } {
                return decide.call((WKNavigationResponsePolicy::Allow,));
            }
            if unsafe { response.isForMainFrame() } {
                self.download(&url_text(unsafe { response.response().URL() }));
            }
            decide.call((WKNavigationResponsePolicy::Cancel,));
        }

        #[unsafe(method(webView:didStartProvisionalNavigation:))]
        fn did_start(&self, web: &WKWebView, _navigation: Option<&WKNavigation>) {
            *self.ivars().going.borrow_mut() = url_text(unsafe { web.URL() });
            self.ivars().failed.take();
            self.changed();
        }

        #[unsafe(method(webView:didCommitNavigation:))]
        fn did_commit(&self, _web: &WKWebView, _navigation: Option<&WKNavigation>) {
            self.ivars().failed.take();
            self.ivars().committed.set(true);
            self.changed();
        }

        #[unsafe(method(webView:didFailProvisionalNavigation:withError:))]
        fn did_fail(&self, _web: &WKWebView, _navigation: Option<&WKNavigation>, error: &NSError) {
            // Cancelled: a new load replaced it, or the policy above stopped it.
            const CANCELLED: isize = -999;
            const STOPPED_BY_POLICY: isize = 102;
            if matches!(error.code(), CANCELLED | STOPPED_BY_POLICY) {
                return;
            }
            let url = self.ivars().going.borrow().clone();
            self.fail(url, error.localizedDescription().to_string());
        }

        /// A dev server's self-signed certificate passes on this machine only, marked insecure.
        #[unsafe(method(webView:didReceiveAuthenticationChallenge:completionHandler:))]
        fn challenge(
            &self,
            _web: &WKWebView,
            challenge: &NSURLAuthenticationChallenge,
            answer: &DynBlock<dyn Fn(Disposition, *mut NSURLCredential)>,
        ) {
            match self.trust(challenge) {
                Some(credential) => answer.call((Disposition::UseCredential, credential)),
                None => answer.call((Disposition::PerformDefaultHandling, std::ptr::null_mut())),
            }
        }

        /// The page's process crashed or was reclaimed: loaded again once, rather than blank.
        #[unsafe(method(webViewWebContentProcessDidTerminate:))]
        fn process_ended(&self, web: &WKWebView) {
            if self.ivars().crashed.replace(true) {
                let message = "The page stopped again after loading once more.".to_string();
                self.fail(url_text(unsafe { web.URL() }), message);
            } else {
                unsafe { web.reload() };
            }
        }
    }

    unsafe impl WKUIDelegate for Delegate {
        /// A link to a new window (target=_blank, window.open) opens here: tabs come from the app.
        #[unsafe(method_id(webView:createWebViewWithConfiguration:forNavigationAction:windowFeatures:))]
        fn new_window(
            &self,
            web: &WKWebView,
            _configuration: &WKWebViewConfiguration,
            action: &WKNavigationAction,
            _features: &WKWindowFeatures,
        ) -> Option<Retained<WKWebView>> {
            unsafe { web.loadRequest(&action.request()) };
            None
        }

        #[unsafe(method(webView:runJavaScriptAlertPanelWithMessage:initiatedByFrame:completionHandler:))]
        fn alert(
            &self,
            web: &WKWebView,
            message: &NSString,
            frame: &WKFrameInfo,
            done: &DynBlock<dyn Fn()>,
        ) {
            if !self.may_ask(web) {
                return done.call(());
            }
            let done = done.copy();
            sheet(web, message, frame, false, move |_| done.call(()));
        }

        #[unsafe(method(webView:runJavaScriptConfirmPanelWithMessage:initiatedByFrame:completionHandler:))]
        fn confirm(
            &self,
            web: &WKWebView,
            message: &NSString,
            frame: &WKFrameInfo,
            done: &DynBlock<dyn Fn(Bool)>,
        ) {
            if !self.may_ask(web) {
                return done.call((Bool::NO,));
            }
            let done = done.copy();
            sheet(web, message, frame, true, move |ok| done.call((Bool::new(ok),)));
        }

        /// prompt() answers as if cancelled: a text field in a sheet isn't worth it here.
        #[unsafe(method(webView:runJavaScriptTextInputPanelWithPrompt:defaultText:initiatedByFrame:completionHandler:))]
        fn prompt(
            &self,
            _web: &WKWebView,
            _prompt: &NSString,
            _default: Option<&NSString>,
            _frame: &WKFrameInfo,
            done: &DynBlock<dyn Fn(*mut NSString)>,
        ) {
            done.call((std::ptr::null_mut(),));
        }

        /// An <input type=file>: the open panel, as a sheet on the app's window.
        #[unsafe(method(webView:runOpenPanelWithParameters:initiatedByFrame:completionHandler:))]
        fn open_panel(
            &self,
            web: &WKWebView,
            parameters: &WKOpenPanelParameters,
            _frame: &WKFrameInfo,
            done: &DynBlock<dyn Fn(*mut NSArray<NSURL>)>,
        ) {
            let Some(window) = web.window() else {
                return done.call((std::ptr::null_mut(),));
            };
            let panel = NSOpenPanel::openPanel(self.mtm());
            panel.setCanChooseFiles(true);
            panel.setAllowsMultipleSelection(unsafe { parameters.allowsMultipleSelection() });
            panel.setCanChooseDirectories(unsafe { parameters.allowsDirectories() });
            let done = done.copy();
            let chosen = panel.clone();
            let block = RcBlock::new(move |response: NSModalResponse| {
                // Held until WebKit has them.
                let urls = (response == NSModalResponseOK).then(|| chosen.URLs());
                let urls_ptr = urls.as_ref().map_or(std::ptr::null_mut(), |u| Retained::as_ptr(u).cast_mut());
                done.call((urls_ptr,));
            });
            panel.beginSheetModalForWindow_completionHandler(&window, &block);
        }
    }
);

impl Delegate {
    fn new(mtm: MainThreadMarker, id: &str, app: AppHandle, web: &WebView) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(DelegateIvars {
            id: id.into(),
            app,
            web: Weak::new(web),
            pending: Cell::new(false),
            trusted: RefCell::default(),
            going: RefCell::default(),
            failed: RefCell::default(),
            dialogs: RefCell::default(),
            committed: Cell::new(false),
            timer: RefCell::default(),
            hidden_at: Cell::new(None),
            screen: Cell::new(None),
            crashed: Cell::new(false),
        });
        unsafe { msg_send![super(this), init] }
    }

    fn changed(&self) {
        if !self.ivars().pending.replace(true) {
            unsafe {
                self.performSelector_withObject_afterDelay(sel!(gvSendState), None, COALESCE_SECS)
            };
        }
    }

    fn fail(&self, url: String, message: String) {
        *self.ivars().failed.borrow_mut() = Some(Failed { url, message });
        self.changed();
    }

    /// A dialog only from the page on show in its tab (a toast over it doesn't count), and only
    /// a few at a time; the rest answer at once.
    fn may_ask(&self, web: &WKWebView) -> bool {
        if web.window().is_none() || !VIEWS.with_borrow(|r| r.shown(&self.ivars().id)) {
            return false;
        }
        let now = Instant::now();
        let mut shown = self.ivars().dialogs.borrow_mut();
        shown.retain(|&at| now.duration_since(at) < DIALOG_WINDOW);
        let ok = shown.len() < DIALOGS;
        if ok {
            shown.push(now);
        }
        ok
    }

    /// The credential that lets a loopback server's own certificate through, remembering it.
    /// One the system already trusts (mkcert's) goes the usual way, and isn't insecure.
    fn trust(&self, challenge: &NSURLAuthenticationChallenge) -> Option<*mut NSURLCredential> {
        let space = challenge.protectionSpace();
        let host = space.host().to_string();
        let server_trust = unsafe { NSURLAuthenticationMethodServerTrust };
        if !space.authenticationMethod().isEqualToString(server_trust) || !is_loopback(&host) {
            return None;
        }
        let trust: *mut SecTrust = unsafe { msg_send![&*space, serverTrust] };
        if trust.is_null() || unsafe { SecTrustEvaluateWithError(trust, std::ptr::null_mut()) } {
            return None;
        }
        self.ivars().trusted.borrow_mut().insert(host_key(&host));
        self.changed();
        Some(unsafe { msg_send![NSURLCredential::class(), credentialForTrust: trust] })
    }

    fn download(&self, url: &str) {
        emit(
            &self.ivars().app,
            "browser-download",
            Download {
                id: &self.ivars().id,
                url,
            },
        );
    }

    fn state(&self, web: &WKWebView) -> PageState {
        let url = url_text(unsafe { web.URL() });
        let insecure = Url::parse(&url).is_ok_and(|u| {
            u.scheme() == "https"
                && u.host_str()
                    .is_some_and(|h| self.ivars().trusted.borrow().contains(&host_key(h)))
        });
        PageState {
            id: self.ivars().id.clone(),
            title: unsafe { web.title() }.map_or_else(String::new, |t| t.to_string()),
            url,
            loading: unsafe { web.isLoading() },
            progress: unsafe { web.estimatedProgress() },
            can_back: unsafe { web.canGoBack() },
            can_forward: unsafe { web.canGoForward() },
            insecure,
            failed: self.ivars().failed.borrow().clone(),
            committed: self.ivars().committed.get(),
        }
    }
}

fn url_text(url: Option<Retained<NSURL>>) -> String {
    url.and_then(|u| u.absoluteString())
        .map(|s| s.to_string())
        .unwrap_or_default()
}

/// A web page's address, ready to load; anything else is refused before a view is made.
fn web_page(url: &str) -> Res<Retained<NSURL>> {
    let page = loadable(url)
        .then(|| NSURL::URLWithString(&NSString::from_str(url)))
        .flatten();
    page.ok_or_else(|| format!("Not a web page: {url}"))
}

fn open_external(url: &str) {
    if let Some(url) = NSURL::URLWithString(&NSString::from_str(url)) {
        NSWorkspace::sharedWorkspace().openURL(&url);
    }
}

/// alert() and confirm() as a sheet on the app's window, named for the page's host.
fn sheet(
    web: &WKWebView,
    message: &NSString,
    frame: &WKFrameInfo,
    cancel: bool,
    done: impl Fn(bool) + 'static,
) {
    let (Some(window), Some(mtm)) = (web.window(), MainThreadMarker::new()) else {
        return done(false);
    };
    let host = unsafe { frame.request().URL() }
        .and_then(|u| u.host())
        .map_or_else(|| "This page".into(), |h| h.to_string());
    let alert = NSAlert::new(mtm);
    alert.setMessageText(&NSString::from_str(&format!("{host} says")));
    alert.setInformativeText(message);
    alert.addButtonWithTitle(ns_string!("OK"));
    if cancel {
        alert.addButtonWithTitle(ns_string!("Cancel"));
    }
    let block =
        RcBlock::new(move |response: NSModalResponse| done(response == NSAlertFirstButtonReturn));
    alert.beginSheetModalForWindow_completionHandler(&window, Some(&block));
}

fn data_store(mtm: MainThreadMarker) -> Retained<WKWebsiteDataStore> {
    if available!(macos = 14.0) {
        unsafe { WKWebsiteDataStore::dataStoreForIdentifier(&NSUUID::from_bytes(STORE), mtm) }
    } else {
        EPHEMERAL.with(|s| {
            s.get_or_init(|| unsafe { WKWebsiteDataStore::nonPersistentDataStore(mtm) })
                .clone()
        })
    }
}

/// The app page's own web view, beside the hosts in the window's content view.
fn app_page(host: &NSView) -> Option<Retained<NSView>> {
    let parent = unsafe { host.superview() }?;
    // Bound first: in edition 2021 the subviews array, a temporary, would outlive the return.
    let found = parent
        .subviews()
        .iter()
        .find(|v| v.isKindOfClass(WKWebView::class()));
    found
}

/// Focus back to the app page, when it was in this view's page.
fn release_focus(view: &View) {
    if view.web.focused() {
        if let (Some(window), Some(page)) = (view.web.window(), app_page(&view.host)) {
            window.makeFirstResponder(Some(&page));
        }
    }
}

fn load(web: &WKWebView, url: &NSURL) {
    unsafe { web.loadRequest(&NSURLRequest::requestWithURL(url)) };
}

/// The view for tab `id` of worktree `root`, loading `url`; hidden until placed. One that is
/// already open stays as it is; a parked one opens again where it was, its picture returned.
pub fn create(
    window: &tauri::WebviewWindow,
    id: &str,
    root: &str,
    url: &str,
    ua: Option<&str>,
) -> Res<Created> {
    let mtm = main_thread()?;
    if let Some(v) = view(id) {
        let page = v.delegate.state(&v.web);
        return Ok(Created {
            page,
            snapshot: None,
            dpr: can_set_dpr(&v.web),
        });
    }
    // Where it parked, else where the tab says; the parked entry goes only once a view is made.
    let parked_at = VIEWS.with_borrow(|r| r.parked(id).and_then(|p| web_page(&p.url).ok()));
    let page = match parked_at {
        Some(page) => page,
        None => web_page(url)?,
    };
    let ns_window = window
        .ns_window()
        .map_err(|e| e.to_string())?
        .cast::<NSWindow>();
    let content = unsafe { ns_window.as_ref() }
        .and_then(|w| w.contentView())
        .ok_or("The window isn't ready.")?;
    let parked = VIEWS.with_borrow_mut(|r| r.unpark(id));

    let config = unsafe { WKWebViewConfiguration::new(mtm) };
    unsafe {
        config.setWebsiteDataStore(&data_store(mtm));
        // Its own, so none of the app page's scripts (Tauri's IPC) reach this page.
        config.setUserContentController(&WKUserContentController::new(mtm));
        let preferences = config.preferences();
        preferences.setJavaScriptCanOpenWindowsAutomatically(false);
        // Inspect Element in the page's menu; private, so only where WebKit still has it.
        if preferences.respondsToSelector(sel!(_setDeveloperExtrasEnabled:)) {
            let _: () = msg_send![&*preferences, _setDeveloperExtrasEnabled: Bool::YES];
        }
    }
    let web = WebView::alloc(mtm).set_ivars(WebIvars {
        id: id.into(),
        app: window.app_handle().clone(),
    });
    let web: Retained<WebView> =
        unsafe { msg_send![super(web), initWithFrame: NSRect::ZERO, configuration: &*config] };
    if available!(macos = 13.3) {
        unsafe { web.setInspectable(true) };
    }
    web.setAutoresizingMask(
        NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
    );
    let delegate = Delegate::new(mtm, id, window.app_handle().clone(), &web);
    unsafe {
        web.setNavigationDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        web.setUIDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        for key in OBSERVED {
            web.addObserver_forKeyPath_options_context(
                &delegate,
                &NSString::from_str(key),
                NSKeyValueObservingOptions::empty(),
                std::ptr::null_mut(),
            );
        }
    }

    let host = Host::new(mtm);
    host.setHidden(true);
    // Kept to the top as the window resizes, until the page places it again.
    host.setAutoresizingMask(NSAutoresizingMaskOptions::ViewMinYMargin);
    host.addSubview(&web);
    content.addSubview_positioned_relativeTo(&host, NSWindowOrderingMode::Above, None);
    if let Some(ua) = ua {
        unsafe { web.setCustomUserAgent(Some(&NSString::from_str(ua))) };
    }
    load(&web, &page);
    #[cfg(debug_assertions)]
    log_process(&web, "made");

    let state = delegate.state(&web);
    let view = View {
        host,
        web,
        delegate,
    };
    let dpr = can_set_dpr(&view.web);
    VIEWS.with_borrow_mut(|r| r.insert(id, &root_key(root), view));
    Ok(Created {
        page: state,
        snapshot: parked.and_then(|p| p.snapshot),
        dpr,
    })
}

/// The WebContent process behind a view, so a debug build shows each one going with its view.
#[cfg(debug_assertions)]
fn log_process(web: &WKWebView, what: &str) {
    if web.respondsToSelector(sel!(_webProcessIdentifier)) {
        let pid: i32 = unsafe { msg_send![web, _webProcessIdentifier] };
        eprintln!("browser: view {what}, web content process {pid}");
    }
}

/// Shows the view at `rect`, in the app page's points, as `screen` when it shows a device.
pub fn place(id: &str, rect: Rect, screen: Option<Screen>) -> Res<()> {
    main_thread()?;
    let screen = screen.map(Screen::check).transpose()?;
    let v = view(id).ok_or(GONE)?;
    let page = app_page(&v.host).ok_or(GONE)?;
    let parent = unsafe { v.host.superview() }.ok_or(GONE)?;
    // From the page's top left, in its own coordinates whichever way up they run.
    let y = if page.isFlipped() {
        rect.y
    } else {
        page.bounds().size.height - rect.y - rect.h
    };
    let rect = NSRect::new(NSPoint::new(rect.x, y), NSSize::new(rect.w, rect.h));
    let frame = page.convertRect_toView(rect, Some(&parent));
    let moved = v.host.frame() != frame;
    if moved {
        v.host.setFrame(frame);
    }
    if v.delegate.ivars().screen.replace(screen) != screen || moved {
        shape(&v, screen);
    }
    v.host.setHidden(false);
    VIEWS.with_borrow_mut(|r| r.show(id));
    stop_timer(&v);
    Ok(())
}

/// The view as a device's screen, or as itself (None). The page lays out at the device's width
/// through its zoom, which keeps text sharp where scaling the view's bounds would blur it; the
/// zoom comes from the width as placed, rounded, so the layout is exactly the device's.
fn shape(v: &View, screen: Option<Screen>) {
    let width = v.host.frame().size.width;
    let zoom = screen.and_then(|s| s.zoom(width)).unwrap_or(1.0);
    unsafe {
        if v.web.pageZoom() != zoom {
            v.web.setPageZoom(zoom);
        }
    }
    // WebKit multiplies it by the zoom, so it's set divided by it.
    if can_set_dpr(&v.web) {
        let scale = screen.and_then(|s| s.dpr).map_or(0.0, |dpr| dpr / zoom);
        let _: () = unsafe { msg_send![&*v.web, _setOverrideDeviceScaleFactor: scale] };
    }
    let every = Corners {
        top_left: true,
        top_right: true,
        bottom_right: true,
        bottom_left: true,
    };
    let radius = screen.map_or(0.0, |s| s.radius);
    round(&v.host, radius, screen.map_or(every, |s| s.corners));
}

/// The device's user agent, or WebKit's own (None); a change loads the page again, as a server
/// may answer another one.
pub fn set_agent(id: &str, ua: Option<&str>) -> Res<()> {
    main_thread()?;
    let v = view(id).ok_or(GONE)?;
    let now = unsafe { v.web.customUserAgent() }.map(|s| s.to_string());
    if now.as_deref().filter(|s| !s.is_empty()) != ua {
        unsafe {
            v.web
                .setCustomUserAgent(ua.map(NSString::from_str).as_deref())
        };
        if loadable(&url_text(unsafe { v.web.URL() })) {
            unsafe { drop(v.web.reload()) };
        }
    }
    Ok(())
}

/// Hidden, WebKit slows its timers and animation frames. Out of its tab's sight (not just
/// `aside` for something drawn over it), it parks past the cap or once its timer is up.
pub fn hide(id: &str, aside: bool) {
    let (Ok(_), Some(v)) = (main_thread(), view(id)) else {
        return;
    };
    release_focus(&v);
    if aside {
        v.host.setHidden(true);
        return;
    }
    // Its picture while it still shows: hidden, WebKit may never give one. It parks with it.
    let picture_of = id.to_string();
    take_snapshot(&v.web, move |shot| {
        VIEWS.with_borrow_mut(|r| r.set_picture(&picture_of, shot));
    });
    v.host.setHidden(true);
    let Some((since, over)) = VIEWS.with_borrow_mut(|r| r.hide(id)) else {
        return;
    };
    v.delegate.ivars().hidden_at.set(Some(Instant::now()));
    start_timer(&v, since, Duration::ZERO);
    for (id, since) in over {
        begin_park(&id, since);
    }
}

/// The hide numbered `since`'s one park timer, never a poll, from `elapsed` into it: a show
/// stops it, a later hide replaces it, and with parking off there is none.
fn start_timer(v: &View, since: u64, elapsed: Duration) {
    stop_timer(v);
    let minutes = PARK_AFTER.get();
    if minutes == 0 {
        return;
    }
    let after = Duration::from_secs(u64::from(minutes) * 60).saturating_sub(elapsed);
    let since = NSNumber::new_u64(since);
    unsafe {
        v.delegate.performSelector_withObject_afterDelay(
            sel!(gvParkDue:),
            Some(&since),
            after.as_secs_f64(),
        )
    };
    *v.delegate.ivars().timer.borrow_mut() = Some(since);
}

fn stop_timer(v: &View) {
    if let Some(since) = v.delegate.ivars().timer.take() {
        unsafe {
            NSObject::cancelPreviousPerformRequestsWithTarget_selector_object(
                &v.delegate,
                sel!(gvParkDue:),
                Some(&since),
            )
        };
    }
}

/// How many hidden views stay alive, and after how many minutes one parks (0: never).
pub fn configure(live_hidden: u32, park_after_min: u32) {
    if main_thread().is_err() {
        return;
    }
    PARK_AFTER.set(park_after_min);
    // The pages already hidden go by the new timing, from when each hid.
    for (id, since) in VIEWS.with_borrow(|r| r.hidden()) {
        if let Some(v) = view(&id) {
            let elapsed = v.delegate.ivars().hidden_at.get().map(|at| at.elapsed());
            start_timer(&v, since, elapsed.unwrap_or_default());
        }
    }
    // Settings bound it (BROWSER_LIVE_HIDDEN); this only keeps a stray value in reach.
    let live_hidden = usize::try_from(live_hidden.min(4)).unwrap_or(4);
    for (id, since) in VIEWS.with_borrow_mut(|r| r.set_live_hidden(live_hidden)) {
        begin_park(&id, since);
    }
}

/// Parks a hidden view at once, with the picture taken as it hid (or none): never waiting on
/// WebKit, which may not draw a hidden view at all.
fn begin_park(id: &str, since: u64) {
    let Some(v) = view(id) else {
        return;
    };
    let parked = Parked {
        url: url_text(unsafe { v.web.URL() }),
        snapshot: None,
    };
    if let Some(v) = VIEWS.with_borrow_mut(|r| r.park(id, since, parked)) {
        let app = v.delegate.ivars().app.clone();
        destroy(v);
        emit(&app, "browser-parked", Id { id });
    }
}

pub fn close(id: &str) {
    if main_thread().is_ok() {
        if let Some(v) = VIEWS.with_borrow_mut(|r| r.remove(id)) {
            destroy(v);
        }
    }
}

/// A worktree removed (`root` as root_key wrote it before the folder went): its pages go too.
pub fn close_root(root: &str) {
    if main_thread().is_ok() {
        VIEWS
            .with_borrow_mut(|r| r.remove_root(root))
            .into_iter()
            .for_each(destroy);
    }
}

/// The app page reloaded: its tabs come back as new views.
pub fn close_all() {
    if main_thread().is_ok() {
        VIEWS
            .with_borrow_mut(|r| r.take_all())
            .into_iter()
            .for_each(destroy);
    }
}

/// In this order: a page left loading, observed, or with a delegate would keep its web content
/// process alive.
fn destroy(v: View) {
    unsafe {
        v.web.stopLoading();
        NSObject::cancelPreviousPerformRequestsWithTarget(&v.delegate);
        for key in OBSERVED {
            v.web
                .removeObserver_forKeyPath(&v.delegate, &NSString::from_str(key));
        }
        v.web.setNavigationDelegate(None);
        v.web.setUIDelegate(None);
    }
    release_focus(&v);
    v.host.removeFromSuperview();
    #[cfg(debug_assertions)]
    log_process(&v.web, "closed");
}

/// The user sends the page somewhere: a crash after this may reload it again.
fn sent(v: &View) {
    v.delegate.ivars().crashed.set(false);
}

pub fn navigate(id: &str, url: &str) -> Res<()> {
    main_thread()?;
    let page = web_page(url)?;
    let v = view(id).ok_or(GONE)?;
    sent(&v);
    load(&v.web, &page);
    Ok(())
}

pub fn go(id: &str, to: Go) -> Res<()> {
    main_thread()?;
    let v = view(id).ok_or(GONE)?;
    sent(&v);
    let web = &v.web;
    unsafe {
        match to {
            Go::Back => drop(web.goBack()),
            Go::Forward => drop(web.goForward()),
            Go::Reload => drop(web.reload()),
            Go::HardReload => drop(web.reloadFromOrigin()),
            Go::Stop => web.stopLoading(),
        }
    }
    Ok(())
}

/// An agent finished in `dir`: the pages of its worktree load again, but for one a dev server
/// already reloads by itself, or one that never loaded.
pub fn agent_done(dir: &str) {
    if main_thread().is_err() {
        return;
    }
    let root = root_key(dir);
    let views: Vec<View> = VIEWS.with_borrow(|r| r.within(&root).into_iter().cloned().collect());
    for v in views {
        if v.delegate.ivars().failed.borrow().is_some() {
            continue;
        }
        // Asked now, of the page as it is: reloaded when no dev server does it already.
        let web = Weak::new(&*v.web);
        let reload = RcBlock::new(move |found: *mut AnyObject, _error: *mut NSError| {
            // SAFETY: WebKit's answer, an object or nil, alive for this call.
            let found = unsafe { found.as_ref() };
            let hot = found
                .and_then(|f| f.downcast_ref::<NSNumber>())
                .is_some_and(|n| n.boolValue());
            if let (false, Some(web)) = (hot, web.load()) {
                unsafe { drop(web.reload()) };
            }
        });
        let probe = NSString::from_str(HMR_PROBE);
        unsafe {
            v.web
                .evaluateJavaScript_completionHandler(&probe, Some(&reload))
        };
    }
}

/// Opens Web Inspector for the page; false where WebKit has no private way to (the page's
/// menu still has Inspect Element).
pub fn inspect(id: &str) -> Res<bool> {
    main_thread()?;
    let v = view(id).ok_or(GONE)?;
    if !v.web.respondsToSelector(sel!(_inspector)) {
        return Ok(false);
    }
    let inspector: *mut AnyObject = unsafe { msg_send![&*v.web, _inspector] };
    let Some(inspector) = (unsafe { inspector.as_ref() }) else {
        return Ok(false);
    };
    if !inspector.class().responds_to(sel!(show)) {
        return Ok(false);
    }
    let _: () = unsafe { msg_send![inspector, show] };
    Ok(true)
}

/// Every cookie, cache and storage of the browser's own data store; the app page's is another.
pub fn clear_data(app: &AppHandle) -> Res<()> {
    let (tx, rx) = std::sync::mpsc::channel();
    app.run_on_main_thread(move || {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let done = RcBlock::new(move || {
            let _ = tx.send(());
        });
        unsafe {
            data_store(mtm).removeDataOfTypes_modifiedSince_completionHandler(
                &WKWebsiteDataStore::allWebsiteDataTypes(mtm),
                &NSDate::distantPast(),
                &done,
            )
        };
    })
    .map_err(|e| e.to_string())?;
    rx.recv_timeout(Duration::from_secs(30))
        .map_err(|_| "Clearing the browser's data took too long.".to_string())
}

/// Focus to the page (`page`) or back to the app page, as ⌘L does for the address bar.
pub fn focus(id: &str, page: bool) {
    let (Ok(_), Some(v)) = (main_thread(), view(id)) else {
        return;
    };
    if page {
        if let Some(window) = v.web.window() {
            window.makeFirstResponder(Some(&v.web));
        }
    } else {
        release_focus(&v);
    }
}

/// The page as it shows, as a JPEG data URL: stands in for the view while something of the app
/// page's covers it. None when the view is gone or WebKit had none. Waits on the main thread.
pub fn snapshot(app: &AppHandle, id: String) -> Res<Option<String>> {
    let (tx, rx) = std::sync::mpsc::channel();
    app.run_on_main_thread(move || match view(&id) {
        Some(v) => take_snapshot(&v.web, move |shot| drop(tx.send(shot))),
        None => drop(tx.send(None)),
    })
    .map_err(|e| e.to_string())?;
    Ok(rx.recv_timeout(Duration::from_secs(2)).ok().flatten())
}

/// The page as drawn now: not after the next screen update, which a view about to be hidden
/// may never have.
fn take_snapshot(web: &WKWebView, done: impl Fn(Option<String>) + 'static) {
    let block = RcBlock::new(move |image: *mut NSImage, _error: *mut NSError| {
        done(unsafe { image.as_ref() }.and_then(jpeg));
    });
    unsafe {
        let now = WKSnapshotConfiguration::new(web.mtm());
        now.setAfterScreenUpdates(false);
        web.takeSnapshotWithConfiguration_completionHandler(Some(&now), &block)
    };
}

fn jpeg(image: &NSImage) -> Option<String> {
    let tiff = image.TIFFRepresentation()?;
    let bitmap = NSBitmapImageRep::imageRepWithData(&tiff)?;
    let quality = NSNumber::new_f64(0.7);
    let properties = NSDictionary::<NSString, AnyObject>::from_slices(
        &[unsafe { NSImageCompressionFactor }],
        &[&*quality],
    );
    let data = unsafe {
        bitmap.representationUsingType_properties(NSBitmapImageFileType::JPEG, &properties)
    }?;
    let base64 = data.base64EncodedStringWithOptions(NSDataBase64EncodingOptions::empty());
    Some(format!("data:image/jpeg;base64,{base64}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use objc2::runtime::AnyClass;
    use objc2_foundation::NSURLProtectionSpace;

    /// Each would only fail on the main thread with a page open: registering a class checks its
    /// methods against their protocols (debug builds), and raw messages are checked here.
    #[test]
    fn classes_and_raw_messages_match_their_declarations() {
        let _ = (Host::class(), WebView::class(), Delegate::class());
        let layer = AnyClass::get(c"CALayer").unwrap();
        layer
            .verify_sel::<(*mut CGColor,), ()>(sel!(setBackgroundColor:))
            .unwrap();
        NSColor::class()
            .verify_sel::<(), *mut CGColor>(sel!(CGColor))
            .unwrap();
        NSURLProtectionSpace::class()
            .verify_sel::<(), *mut SecTrust>(sel!(serverTrust))
            .unwrap();
        layer
            .verify_sel::<(f64,), ()>(sel!(setCornerRadius:))
            .unwrap();
        layer
            .verify_sel::<(Bool,), ()>(sel!(setMasksToBounds:))
            .unwrap();
        layer
            .verify_sel::<(usize,), ()>(sel!(setMaskedCorners:))
            .unwrap();
        // Private: checked where this macOS has it.
        let scale = sel!(_setOverrideDeviceScaleFactor:);
        if WKWebView::class().instance_method(scale).is_some() {
            WKWebView::class().verify_sel::<(f64,), ()>(scale).unwrap();
        }
        NSURLCredential::class()
            .metaclass()
            .verify_sel::<(*mut SecTrust,), *mut NSURLCredential>(sel!(credentialForTrust:))
            .unwrap();
    }

    #[test]
    fn only_web_pages_become_addresses() {
        assert!(web_page("http://localhost:5173/").is_ok());
        assert!(web_page("about:blank").is_ok());
        for no in ["javascript:alert(1)", "file:///etc/hosts", "not a url", ""] {
            assert!(web_page(no).is_err(), "{no}");
        }
    }
}
