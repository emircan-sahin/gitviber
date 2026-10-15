//! The element picker, the console, the network log and an agent's tab's clipboard on a page's
//! view (scripts/picker.js, console.js, network.js, clipboard.js): the picker in GitViber's own
//! content world, which the page can't see or post into; the rest in the page's, where its
//! logging, its requests and its clipboard calls are.

use super::{emit, main_thread, url_text, view, Delegate, View, WebView, GONE};
use crate::browser::console::{Counts, Entry};
use crate::browser::network;
use crate::browser::picks::{self, Bounds, Pick};
use crate::browser::{cut, epoch_ms};
use crate::state::Res;
use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, NSObject, ProtocolObject};
use objc2::{
    define_class, msg_send, sel, ClassType, DefinedClass, MainThreadMarker, MainThreadOnly, Message,
};
use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSImage};
use objc2_foundation::{
    ns_string, NSArray, NSDictionary, NSError, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString,
    NSUUID,
};
use objc2_web_kit::{
    WKContentWorld, WKPreferences, WKScriptMessage, WKScriptMessageHandler,
    WKSnapshotConfiguration, WKUserContentController, WKUserScript, WKUserScriptInjectionTime,
    WKWebView,
};
use serde::Serialize;
use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::AppHandle;

const PICKER: &str = include_str!("../scripts/picker.js");
const CONSOLE: &str = include_str!("../scripts/console.js");
const NETWORK: &str = include_str!("../scripts/network.js");
const COMPONENTS: &str = include_str!("../scripts/components.js");
const CLIPBOARD: &str = include_str!("../scripts/clipboard.js");
/// In GitViber's world, in every frame of an agent's tab: whether a password field has focus,
/// for its panel (panel.rs `settle`). The page can't post to it.
const SECRET: &str = r#"(() => {
  const post = () => {
    let el = document.activeElement;
    while (el?.shadowRoot?.activeElement) el = el.shadowRoot.activeElement;
    const secret = el?.localName === "input" && el.type === "password";
    window.webkit?.messageHandlers?.gvSecret?.postMessage(secret ? "1" : "0");
  };
  addEventListener("focusin", post, true);
  addEventListener("focusout", () => setTimeout(post), true);
})();"#;

thread_local! {
    /// Settings → Browser's console switch: off, pages load without console.js.
    static CAPTURE: Cell<bool> = const { Cell::new(true) };
}

/// Whether pages keep their console (Settings → Browser).
pub(super) fn capturing() -> bool {
    CAPTURE.get()
}

/// GitViber's own content world: scripts there share the page's DOM, not its JavaScript.
pub(super) fn ours(mtm: MainThreadMarker) -> Retained<WKContentWorld> {
    unsafe { WKContentWorld::worldWithName(ns_string!("gitviber"), mtm) }
}

struct MessagesIvars {
    /// The tab's id, looked up in the registry as messages come: holding the view itself would
    /// keep it alive, as its user content controller holds this.
    id: String,
}

define_class!(
    /// gvPick from the picker's world, gvConsole and gvNetwork from the page's.
    // SAFETY: NSObject has no subclassing requirements, and Messages has no Drop.
    #[unsafe(super(NSObject))]
    #[name = "GitViberBrowserMessages"]
    #[thread_kind = MainThreadOnly]
    #[ivars = MessagesIvars]
    struct Messages;

    unsafe impl NSObjectProtocol for Messages {}

    unsafe impl WKScriptMessageHandler for Messages {
        #[unsafe(method(userContentController:didReceiveScriptMessage:))]
        fn receive(&self, _controller: &WKUserContentController, message: &WKScriptMessage) {
            let body = unsafe { message.body() };
            let Some(text) = body.downcast_ref::<NSString>().map(|s| s.to_string()) else {
                return;
            };
            let Some(v) = view(&self.ivars().id) else {
                return;
            };
            match unsafe { message.name() }.to_string().as_str() {
                "gvConsole" if CAPTURE.get() => logged(&v.delegate, &text),
                "gvNetwork" => {
                    v.delegate.ivars().network.borrow_mut().add(&text);
                }
                "gvPick" => picked(&v, &text),
                "gvSecret" => {
                    v.delegate.ivars().secret.set(text == "1");
                    super::panel::settle(&v);
                }
                _ => {}
            }
        }
    }
);

/// The picker's and the console's handlers on a new view's user content controller, and its
/// page scripts (`add_scripts`). Only an agent's tab gets the network log: no one else asks for
/// it, and the user's own pages shouldn't pay for copying every response.
/// `agent`: an agent's tab's, with what its clipboard script answers to.
pub(super) fn install(
    content: &WKUserContentController,
    id: &str,
    agent: Option<&str>,
    mtm: MainThreadMarker,
) {
    let messages = Messages::alloc(mtm).set_ivars(MessagesIvars { id: id.into() });
    let messages: Retained<Messages> = unsafe { msg_send![super(messages), init] };
    let handler = ProtocolObject::from_ref(&*messages);
    unsafe {
        content.addScriptMessageHandler_contentWorld_name(
            handler,
            &ours(mtm),
            ns_string!("gvPick"),
        );
        let page = WKContentWorld::pageWorld(mtm);
        content.addScriptMessageHandler_contentWorld_name(handler, &page, ns_string!("gvConsole"));
        if agent.is_some() {
            content.addScriptMessageHandler_contentWorld_name(
                handler,
                &page,
                ns_string!("gvNetwork"),
            );
            content.addScriptMessageHandler_contentWorld_name(
                handler,
                &ours(mtm),
                ns_string!("gvSecret"),
            );
        }
    }
    add_scripts(content, agent, mtm);
}

/// network.js in an agent's tab, whatever the console switch says (an agent asks for its
/// network itself), clipboard.js in each of its frames, answering to `agent`'s key only, and
/// SECRET; console.js while the console is on.
fn add_scripts(content: &WKUserContentController, agent: Option<&str>, mtm: MainThreadMarker) {
    let page = unsafe { WKContentWorld::pageWorld(mtm) };
    if let Some(key) = agent {
        add_script(content, NETWORK, true, &page, mtm);
        let key = serde_json::to_string(key).unwrap_or_default();
        let clipboard = CLIPBOARD.replacen("__GV_KEY__", &key, 1);
        add_script(content, &clipboard, false, &page, mtm);
        add_script(content, SECRET, false, &ours(mtm), mtm);
    }
    if CAPTURE.get() {
        add_script(content, CONSOLE, true, &page, mtm);
    }
}

fn add_script(
    content: &WKUserContentController,
    source: &str,
    main_frame: bool,
    world: &WKContentWorld,
    mtm: MainThreadMarker,
) {
    unsafe {
        let script = WKUserScript::initWithSource_injectionTime_forMainFrameOnly_inContentWorld(
            WKUserScript::alloc(mtm),
            &NSString::from_str(source),
            WKUserScriptInjectionTime::AtDocumentStart,
            main_frame,
            world,
        );
        content.addUserScript(&script);
    }
}

/// Before the view goes: the controller holds its handlers and scripts strongly.
pub(super) fn uninstall(content: &WKUserContentController) {
    unsafe {
        content.removeAllScriptMessageHandlers();
        content.removeAllUserScripts();
    }
}

/// The console switch: pages load with console.js or without, from their next load.
pub fn capture(on: bool, views: &[View]) {
    let Ok(mtm) = main_thread() else {
        return;
    };
    if CAPTURE.replace(on) == on {
        return;
    }
    for v in views {
        let content = unsafe { v.web.configuration().userContentController() };
        // There's no taking one script out: all go, and what stays comes back.
        unsafe { content.removeAllUserScripts() };
        let routed = super::agent::routed(&v.delegate.ivars().id);
        let key = routed.then_some(&*v.web.ivars().clipboard);
        add_scripts(&content, key, mtm);
    }
}

/// An agent's tab's clipboard script (clipboard.js) asked `ask`: `on` or `off`, its calls kept
/// to the page or the user's again; `copy`, `cut` or `paste` on the page's own clipboard. `done`
/// gets its answer: for a paste, the text, unless the page took the paste event.
pub(super) fn clipboard(web: &WebView, ask: &str, done: impl FnOnce(Option<String>) + 'static) {
    let mtm = web.mtm();
    let (key, ask) = (
        NSString::from_str(&web.ivars().clipboard),
        NSString::from_str(ask),
    );
    let arguments = NSDictionary::<NSString, AnyObject>::from_slices(
        &[ns_string!("key"), ns_string!("ask")],
        &[&*key, &*ask],
    );
    let done = Cell::new(Some(done));
    let block = RcBlock::new(move |value: *mut AnyObject, _error: *mut NSError| {
        // SAFETY: WebKit's answer, an object or nil, alive for this call.
        let text = unsafe { value.as_ref() }
            .and_then(|v| v.downcast_ref::<NSString>())
            .map(|s| s.to_string());
        if let Some(done) = done.take() {
            done(text);
        }
    });
    // `window` can't be redefined by the page, nor the script's own property on it.
    let body = ns_string!(
        "return typeof window.__gvClipboard === 'function' ? window.__gvClipboard(key, ask) ?? null : null;"
    );
    unsafe {
        web.callAsyncJavaScript_arguments_inFrame_inContentWorld_completionHandler(
            body,
            Some(&arguments),
            None,
            &WKContentWorld::pageWorld(mtm),
            Some(&block),
        )
    };
}

/// The page's own clipboard calls kept to it while an agent's command runs (`on`), the user's
/// again after.
pub(super) fn guard_clipboard(web: &WebView, on: bool) {
    web.ivars().guarded.set(on);
    clipboard(web, if on { "on" } else { "off" }, |_| {});
}

/// WebKit's async Clipboard API off for the page (`navigator.clipboard`), in every document of
/// it, about:blank frames too: a native click's user activation outlasts the command that
/// gave it. Private, so only where WebKit has the feature; whether it's off, read back.
pub(super) fn no_async_clipboard(preferences: &WKPreferences) -> bool {
    let class = WKPreferences::class();
    let can = class.metaclass().responds_to(sel!(_features))
        && preferences.respondsToSelector(sel!(_setEnabled:forFeature:))
        && preferences.respondsToSelector(sel!(_isEnabledForFeature:));
    if !can {
        return false;
    }
    let features: Option<Retained<NSArray<AnyObject>>> = unsafe { msg_send![class, _features] };
    let named = |f: &AnyObject| {
        let key: Option<Retained<NSString>> = unsafe { msg_send![f, key] };
        key.is_some_and(|k| k.to_string() == "AsyncClipboardAPIEnabled")
    };
    let Some(feature) = features.and_then(|all| all.iter().find(|f| named(f))) else {
        return false;
    };
    unsafe {
        let _: () = msg_send![preferences, _setEnabled: Bool::NO, forFeature: &*feature];
        let on: Bool = msg_send![preferences, _isEnabledForFeature: &*feature];
        !on.as_bool()
    }
}

#[derive(Serialize, Clone)]
struct Logged<'a> {
    id: &'a str,
    #[serde(flatten)]
    counts: Counts,
}

fn logged(delegate: &Delegate, json: &str) {
    if delegate.ivars().console.borrow_mut().add(json) {
        delegate.console_changed();
    }
}

/// `browser-console`, after a burst of lines has settled (Delegate's gvSendConsole).
pub(super) fn send_counts(delegate: &Delegate) {
    let d = delegate.ivars();
    let counts = d.console.borrow().counts();
    emit(&d.app, "browser-console", Logged { id: &d.id, counts });
}

/// A new page committed: the console marks where it began, and a pick on the last one ends.
pub(super) fn loaded(delegate: &Delegate, url: &str) {
    let d = delegate.ivars();
    d.console.borrow_mut().loaded(url, epoch_ms());
    if d.picking.take().is_some() {
        send_pick(&d.app, &d.id, None);
    }
}

pub fn console_entries(id: &str) -> Res<Vec<Entry>> {
    main_thread()?;
    let v = view(id).ok_or(GONE)?;
    let entries = v.delegate.ivars().console.borrow().entries();
    Ok(entries)
}

pub fn console_clear(id: &str) -> Res<()> {
    main_thread()?;
    let v = view(id).ok_or(GONE)?;
    v.delegate.ivars().console.borrow_mut().clear();
    send_counts(&v.delegate);
    Ok(())
}

/// `f` on the view's network log (network.js), as `gitviber browser network` reads it.
pub fn network_log<T>(id: &str, f: impl FnOnce(&mut network::Log) -> T) -> Res<T> {
    main_thread()?;
    let v = view(id).ok_or(GONE)?;
    let out = f(&mut v.delegate.ivars().network.borrow_mut());
    Ok(out)
}

#[derive(Serialize, Clone)]
struct Picked<'a> {
    id: &'a str,
    /// None: cancelled, or the page moved on.
    pick: Option<Pick>,
}

fn send_pick(app: &AppHandle, id: &str, pick: Option<Pick>) {
    emit(app, "browser-picked", Picked { id, pick });
}

/// The picker on (with a new tag for the element it picks) or off.
pub fn pick(id: &str, on: bool) -> Res<()> {
    let mtm = main_thread()?;
    let v = view(id).ok_or(GONE)?;
    let script = if on {
        // Not one a page could guess and put on another element ahead of the picked one.
        let tag = NSUUID::new().UUIDString().to_string();
        let start = format!("{PICKER}\nwindow.__gvPicker.start({tag:?});");
        *v.delegate.ivars().picking.borrow_mut() = Some(tag);
        start
    } else {
        v.delegate.ivars().picking.take();
        "window.__gvPicker?.stop();".to_string()
    };
    unsafe {
        v.web
            .evaluateJavaScript_inFrame_inContentWorld_completionHandler(
                &NSString::from_str(&script),
                None,
                &ours(mtm),
                None,
            )
    };
    Ok(())
}

/// How long a pick waits on its components and picture: a view closed mid-pick, or a page that
/// never answers, still gets the pick as it is.
const PICK_WAIT: Duration = Duration::from_secs(3);

/// A pick on its way, sent once: by its last step, or by the wait running out first.
#[derive(Clone)]
struct Delivery {
    id: String,
    app: AppHandle,
    sent: Arc<AtomicBool>,
}

impl Delivery {
    fn send(&self, pick: Pick) {
        if !self.sent.swap(true, Ordering::SeqCst) {
            send_pick(&self.app, &self.id, Some(pick));
        }
    }
}

/// The picker's message: the element, then its React components from the page's own world, then
/// its picture; `browser-picked` once all are in, or PICK_WAIT after the click.
fn picked(v: &View, json: &str) {
    let d = v.delegate.ivars();
    let Some(tag) = d.picking.borrow().clone() else {
        return;
    };
    // The picker's nonce, then the pick's own number (picker.js).
    let ours = |pick: &Pick| {
        pick.nonce
            .rsplit_once(':')
            .is_some_and(|(nonce, _)| nonce == tag)
    };
    let mut pick = match picks::read(json) {
        Ok(Some(pick)) if ours(&pick) => pick,
        _ => {
            d.picking.take();
            return send_pick(&d.app, &d.id, None);
        }
    };
    // A ⇧-click's pick leaves the picker on for the next.
    if !pick.more {
        d.picking.take();
    }
    pick.url = url_text(unsafe { v.web.URL() });
    let mark = pick.nonce.clone();
    pick.number = mark
        .rsplit_once(':')
        .and_then(|(_, n)| n.parse().ok())
        .unwrap_or(0);
    let delivery = Delivery {
        id: d.id.clone(),
        app: d.app.clone(),
        sent: Arc::default(),
    };
    let late = (delivery.clone(), pick.clone());
    std::thread::spawn(move || {
        std::thread::sleep(PICK_WAIT);
        late.0.send(late.1);
    });
    let Ok(mtm) = main_thread() else {
        return;
    };
    let web = v.web.clone();
    let named = RcBlock::new(move |names: *mut AnyObject, _error: *mut NSError| {
        // SAFETY: WebKit's answer, an object or nil, alive for this call.
        let names = unsafe { names.as_ref() }
            .and_then(|n| n.downcast_ref::<NSString>())
            .and_then(|n| serde_json::from_str::<Vec<String>>(&n.to_string()).ok())
            .unwrap_or_default();
        let mut pick = pick.clone();
        pick.components = names.into_iter().take(5).map(|n| cut(&n, 80)).collect();
        picked_picture(&web, pick, delivery.clone());
    });
    // The pick's own tag on its element, not the picker's.
    let mark = NSString::from_str(&mark);
    let arguments =
        NSDictionary::<NSString, AnyObject>::from_slices(&[ns_string!("nonce")], &[&*mark]);
    unsafe {
        v.web
            .callAsyncJavaScript_arguments_inFrame_inContentWorld_completionHandler(
                &NSString::from_str(COMPONENTS),
                Some(&arguments),
                None,
                &WKContentWorld::pageWorld(mtm),
                Some(&named),
            )
    };
}

/// The element's picture, saved as a PNG in the app's cache, then the pick sent.
fn picked_picture(web: &WKWebView, pick: Pick, delivery: Delivery) {
    picture(web, Some(pick.bounds), true, move |image| {
        let png = image.as_deref().and_then(png);
        let mut pick = pick;
        // Written off the main thread: a slow disk shouldn't hold the window.
        std::thread::spawn(move || {
            pick.screenshot = png.and_then(|png| picks::save_png(&delivery.app, &png));
            delivery.send(pick);
        });
    });
}

/// The page as drawn, or the part of it `rect` names (CSS px from the viewport's top left, with
/// a margin); after the next screen update, or as it is now, which a view about to hide needs.
/// None when that part doesn't show, or WebKit gives no picture.
pub(super) fn picture(
    web: &WKWebView,
    rect: Option<Bounds>,
    after_updates: bool,
    done: impl FnOnce(Option<Retained<NSImage>>) + 'static,
) {
    let config = unsafe { WKSnapshotConfiguration::new(web.mtm()) };
    unsafe { config.setAfterScreenUpdates(after_updates) };
    if let Some(rect) = rect {
        let size = web.bounds().size;
        let zoom = unsafe { web.pageZoom() };
        let Some(r) = picks::picture_rect(rect, zoom, (size.width, size.height)) else {
            return done(None);
        };
        unsafe { config.setRect(NSRect::new(NSPoint::new(r.x, r.y), NSSize::new(r.w, r.h))) };
    }
    let done = Cell::new(Some(done));
    let block = RcBlock::new(move |image: *mut NSImage, _error: *mut NSError| {
        if let Some(done) = done.take() {
            // SAFETY: WebKit's picture, or nil, alive for this call.
            done(unsafe { image.as_ref() }.map(|i| i.retain()));
        }
    });
    unsafe { web.takeSnapshotWithConfiguration_completionHandler(Some(&config), &block) };
}

pub(super) fn png(image: &NSImage) -> Option<Vec<u8>> {
    let tiff = image.TIFFRepresentation()?;
    let bitmap = NSBitmapImageRep::imageRepWithData(&tiff)?;
    let none = NSDictionary::new();
    let data =
        unsafe { bitmap.representationUsingType_properties(NSBitmapImageFileType::PNG, &none) }?;
    Some(data.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use objc2::ClassType;

    /// Registering the class checks its method against WKScriptMessageHandler's (debug builds).
    #[test]
    fn the_message_handler_matches_its_protocol() {
        let _ = Messages::class();
    }

    /// The private feature switches `no_async_clipboard` uses, checked where WebKit has them.
    #[test]
    fn the_feature_switches_match_their_declarations() {
        let class = WKPreferences::class();
        if class.metaclass().responds_to(sel!(_features)) {
            class
                .metaclass()
                .verify_sel::<(), *mut AnyObject>(sel!(_features))
                .unwrap();
            class
                .verify_sel::<(Bool, &AnyObject), ()>(sel!(_setEnabled:forFeature:))
                .unwrap();
            class
                .verify_sel::<(&AnyObject,), Bool>(sel!(_isEnabledForFeature:))
                .unwrap();
        }
    }

    /// The tab's key goes in once, as a JSON string, and nothing else answers for it.
    #[test]
    fn the_clipboard_script_answers_to_its_tabs_key_only() {
        assert_eq!(CLIPBOARD.matches("__GV_KEY__").count(), 1);
        let key = serde_json::to_string("6F1D2A93-made-up").unwrap();
        let script = CLIPBOARD.replacen("__GV_KEY__", &key, 1);
        assert!(script.contains("const key = \"6F1D2A93-made-up\";"));
    }
}
