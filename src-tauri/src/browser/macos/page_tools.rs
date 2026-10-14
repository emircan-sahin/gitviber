//! The element picker, the console and the network log on a page's view (scripts/picker.js,
//! console.js, network.js): the picker in GitViber's own content world, which the page can't see
//! or post into; the console and the network log in the page's, where its logging and its
//! requests are.

use super::{emit, main_thread, url_text, view, Delegate, View, GONE};
use crate::browser::console::{Counts, Entry};
use crate::browser::marks::{Picture, Pixels};
use crate::browser::network;
use crate::browser::picks::{self, Bounds, Pick};
use crate::browser::{cut, epoch_ms};
use crate::state::Res;
use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, ProtocolObject};
use objc2::{
    define_class, msg_send, AllocAnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, Message,
};
use objc2_app_kit::{
    NSBitmapImageFileType, NSBitmapImageRep, NSCompositingOperation, NSDeviceRGBColorSpace,
    NSGraphicsContext, NSImage, NSImageCompressionFactor, NSImageInterpolation,
};
use objc2_foundation::{
    ns_string, NSDictionary, NSError, NSNumber, NSObjectProtocol, NSPoint, NSRect, NSSize,
    NSString, NSTimer, NSUUID,
};
use objc2_web_kit::{
    WKContentWorld, WKScriptMessage, WKScriptMessageHandler, WKSnapshotConfiguration,
    WKUserContentController, WKUserScript, WKUserScriptInjectionTime, WKWebView,
};
use serde::Serialize;
use std::cell::Cell;
use std::ptr::NonNull;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::AppHandle;

const PICKER: &str = include_str!("../scripts/picker.js");
const CONSOLE: &str = include_str!("../scripts/console.js");
const NETWORK: &str = include_str!("../scripts/network.js");
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
                _ => {}
            }
        }
    }
);

/// The picker's and the console's handlers on a new view's user content controller, and its
/// page scripts (`add_scripts`). Only an agent's tab gets the network log: no one else asks for
/// it, and the user's own pages shouldn't pay for copying every response.
pub(super) fn install(
    content: &WKUserContentController,
    id: &str,
    agent: bool,
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
        if agent {
            content.addScriptMessageHandler_contentWorld_name(
                handler,
                &page,
                ns_string!("gvNetwork"),
            );
        }
    }
    add_scripts(content, agent, mtm);
}

/// network.js in an agent's tab, whatever the console switch says (an agent asks for its
/// network itself); console.js while the console is on.
fn add_scripts(content: &WKUserContentController, agent: bool, mtm: MainThreadMarker) {
    if agent {
        add_script(content, NETWORK, mtm);
    }
    if CAPTURE.get() {
        add_script(content, CONSOLE, mtm);
    }
}

fn add_script(content: &WKUserContentController, source: &str, mtm: MainThreadMarker) {
    unsafe {
        let script = WKUserScript::initWithSource_injectionTime_forMainFrameOnly_inContentWorld(
            WKUserScript::alloc(mtm),
            &NSString::from_str(source),
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
        // There's no taking one script out: all go, and what stays comes back.
        unsafe { content.removeAllUserScripts() };
        add_scripts(&content, super::agent::routed(&v.delegate.ivars().id), mtm);
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

/// The alert() or confirm() open on the tab's page, waiting for the user, if one is.
pub fn agent_dialog(id: &str) -> Res<Option<&'static str>> {
    main_thread()?;
    let v = view(id).ok_or(GONE)?;
    Ok(v.delegate.ivars().asking.get())
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
    let mut points = None;
    if let Some(rect) = rect {
        let size = web.bounds().size;
        let zoom = unsafe { web.pageZoom() };
        let Some(r) = picks::picture_rect(rect, zoom, (size.width, size.height)) else {
            return done(None);
        };
        points = Some(r);
    }
    snap(web, points, after_updates, done);
}

/// The view as drawn, or `rect` of it (its points).
fn snap(
    web: &WKWebView,
    rect: Option<Bounds>,
    after_updates: bool,
    done: impl FnOnce(Option<Retained<NSImage>>) + 'static,
) {
    let config = unsafe { WKSnapshotConfiguration::new(web.mtm()) };
    unsafe { config.setAfterScreenUpdates(after_updates) };
    if let Some(r) = rect {
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

/// How long an agent's picture waits for the next screen update before it takes the page as
/// drawn: a view that isn't drawing may never get one.
const UPDATE_WAIT: f64 = 2.0;

type Taken = Box<dyn FnOnce(Option<Retained<NSImage>>)>;

/// An agent's screenshot (marks.rs): `rect` of the viewport after the next screen update, or as
/// drawn once UPDATE_WAIT has gone, redrawn at exactly its size in pixels whatever the screen's
/// scale (a Retina view gives twice that), its labels drawn on; a PNG or a JPEG.
pub fn agent_picture(
    id: &str,
    picture: Picture,
    done: Box<dyn FnOnce(Option<Vec<u8>>)>,
) -> Res<()> {
    main_thread()?;
    let v = view(id).ok_or(GONE)?;
    let zoom = unsafe { v.web.pageZoom() };
    let size = v.web.bounds().size;
    let r = picture.rect;
    let (x0, y0) = ((r.x * zoom).max(0.0), (r.y * zoom).max(0.0));
    let (x1, y1) = (
        ((r.x + r.w) * zoom).min(size.width),
        ((r.y + r.h) * zoom).min(size.height),
    );
    if x1 <= x0 || y1 <= y0 {
        done(None);
        return Ok(());
    }
    let points = Bounds {
        x: x0,
        y: y0,
        w: x1 - x0,
        h: y1 - y0,
    };
    let taken: Taken =
        Box::new(move |image| done(image.as_deref().and_then(|i| redrawn(i, &picture))));
    let first = Rc::new(Cell::new(Some(taken)));
    let now = first.clone();
    snap(&v.web, Some(points), true, move |image| {
        if let Some(taken) = now.take() {
            taken(image);
        }
    });
    let web = v.web.clone();
    let late = RcBlock::new(move |_: NonNull<NSTimer>| {
        if let Some(taken) = first.take() {
            snap(&web, Some(points), false, taken);
        }
    });
    unsafe { NSTimer::scheduledTimerWithTimeInterval_repeats_block(UPDATE_WAIT, false, &late) };
    Ok(())
}

/// `image` drawn into a bitmap of `picture.size` pixels, its labels on it, encoded.
fn redrawn(image: &NSImage, picture: &Picture) -> Option<Vec<u8>> {
    let (w, h) = picture.size;
    let rep = unsafe {
        NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
            NSBitmapImageRep::alloc(),
            std::ptr::null_mut(),
            w as isize,
            h as isize,
            8,
            4,
            true,
            false,
            NSDeviceRGBColorSpace,
            0,
            0,
        )
    }?;
    let context = NSGraphicsContext::graphicsContextWithBitmapImageRep(&rep)?;
    NSGraphicsContext::saveGraphicsState_class();
    NSGraphicsContext::setCurrentContext(Some(&context));
    context.setImageInterpolation(NSImageInterpolation::High);
    let all = NSRect::new(NSPoint::ZERO, NSSize::new(f64::from(w), f64::from(h)));
    image.drawInRect_fromRect_operation_fraction(
        all,
        NSRect::ZERO,
        NSCompositingOperation::Copy,
        1.0,
    );
    context.flushGraphics();
    NSGraphicsContext::restoreGraphicsState_class();
    let data = rep.bitmapData();
    if !picture.labels.is_empty() && !data.is_null() {
        let stride = rep.bytesPerRow() as usize;
        // SAFETY: the bitmap's own buffer, `stride` bytes a row for its `h` rows, held by `rep`.
        let data = unsafe { std::slice::from_raw_parts_mut(data, stride * h as usize) };
        let big = f64::from(w) >= picture.rect.w;
        let mut pixels = Pixels {
            data,
            width: w,
            height: h,
            stride,
        };
        pixels.draw(&picture.labels, big);
    }
    let (kind, properties) = if picture.jpeg {
        let quality = NSNumber::new_f64(0.85);
        let key = unsafe { NSImageCompressionFactor };
        (
            NSBitmapImageFileType::JPEG,
            NSDictionary::<NSString, AnyObject>::from_slices(&[key], &[&*quality]),
        )
    } else {
        (NSBitmapImageFileType::PNG, NSDictionary::new())
    };
    let data = unsafe { rep.representationUsingType_properties(kind, &properties) }?;
    Some(data.to_vec())
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
