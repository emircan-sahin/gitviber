//! The element picker and the console on a page's view (scripts/picker.js, console.js): the
//! picker in GitViber's own content world, which the page can't see or post into; the console
//! in the page's, where its logging is.

use super::{emit, main_thread, url_text, view, Delegate, View, GONE};
use crate::browser::console::{Counts, Entry};
use crate::browser::cut;
use crate::browser::picks::{self, Bounds, Pick};
use crate::state::Res;
use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, ProtocolObject};
use objc2::{define_class, msg_send, DefinedClass, MainThreadMarker, MainThreadOnly, Message};
use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSImage};
use objc2_foundation::{
    ns_string, NSDictionary, NSError, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString, NSUUID,
};
use objc2_web_kit::{
    WKContentWorld, WKScriptMessage, WKScriptMessageHandler, WKSnapshotConfiguration,
    WKUserContentController, WKUserScript, WKUserScriptInjectionTime, WKWebView,
};
use serde::Serialize;
use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tauri::AppHandle;

const PICKER: &str = include_str!("../scripts/picker.js");
const CONSOLE: &str = include_str!("../scripts/console.js");
const COMPONENTS: &str = include_str!("../scripts/components.js");

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
    /// gvPick from the picker's world, gvConsole from the page's.
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
                "gvPick" => picked(&v, &text),
                _ => {}
            }
        }
    }
);

/// The picker's and the console's handlers on a new view's user content controller, and
/// console.js when the console is on.
pub(super) fn install(content: &WKUserContentController, id: &str, mtm: MainThreadMarker) {
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
    }
    if CAPTURE.get() {
        add_console(content, mtm);
    }
}

fn add_console(content: &WKUserContentController, mtm: MainThreadMarker) {
    unsafe {
        let script = WKUserScript::initWithSource_injectionTime_forMainFrameOnly_inContentWorld(
            WKUserScript::alloc(mtm),
            &NSString::from_str(CONSOLE),
            WKUserScriptInjectionTime::AtDocumentStart,
            true,
            &WKContentWorld::pageWorld(mtm),
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
        // console.js is a view's only user script.
        unsafe { content.removeAllUserScripts() };
        if on {
            add_console(&content, mtm);
        }
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
    let ts = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0.0, |t| t.as_millis() as f64);
    d.console.borrow_mut().loaded(url, ts);
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
}
