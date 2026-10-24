//! Trusted input for an agent's tab (browser/input.rs plans it): NSEvents handed straight to the
//! page's web view, as AppKit would hand them, so the page takes them as the user's
//! (`isTrusted`, user activation). Never posted to the window server: the real cursor, the
//! user's focus and the app's activation stay as they are.

use super::{agent, main_thread, page_tools, view, Delegate, View, WebView, GONE};
use crate::input::{self, Edit, Half, KeyStep, Keys, Pointer};
use block2::{DynBlock, RcBlock};
use gitviber_core::Res;
use objc2::encode::{Encoding, RefEncode};
use objc2::rc::{Retained, Weak};
use objc2::runtime::{AnyClass, AnyObject, Imp, Sel};
use objc2::{msg_send, sel, ClassType, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSEvent, NSEventModifierFlags, NSEventType, NSScreen, NSWindow,
};
use objc2_foundation::{
    NSArray, NSObjectProtocol, NSPoint, NSProcessInfo, NSRange, NSString, NSTimer, NSURL,
};
use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::path::PathBuf;
use std::ptr::NonNull;
use std::sync::OnceLock;

/// An awake tab sits in its panel, or in GitViber's window when on show, whatever else is on
/// screen: with no window at all, GitViber's own is gone.
const NOT_DRAWN: &str =
    "The page isn't drawn now: GitViber's window is closed. Open it, then run the command again.";

/// Without WebKit's own wait for the mouse events it holds, about a frame between events, so
/// it doesn't coalesce them.
const PACE_SECS: f64 = 0.016;

thread_local! {
    /// The buttons WebKit reads as held while one of our events goes in (`real_buttons`).
    static BUTTONS: Cell<Option<usize>> = const { Cell::new(None) };
    /// Our key downs and presses, for as long as WebKit holds them: it lets each go once it
    /// has answered it, and with that the entry is dead.
    static SENT: RefCell<Vec<Sent>> = RefCell::default();
    static NUMBER: Cell<isize> = const { Cell::new(0) };
}

/// An event of ours WebKit may hand back: its tab, and for a key what the Edit menu does when
/// no page takes it.
struct Sent {
    event: Weak<NSEvent>,
    web: Option<Weak<WebView>>,
    edit: Option<Edit>,
}

fn remember(event: &NSEvent, web: Option<&WebView>, edit: Option<Edit>) {
    SENT.with_borrow_mut(|sent| {
        sent.retain(|s| s.event.load().is_some());
        sent.push(Sent {
            event: Weak::new(event),
            web: web.map(Weak::new),
            edit,
        });
    });
}

fn is(sent: &Sent, event: &NSEvent) -> bool {
    sent.event.load().is_some_and(|e| std::ptr::eq(&*e, event))
}

/// Whether `event` is one of ours, this very object, as WebKit hands it back.
pub(super) fn ours(event: &NSEvent) -> bool {
    SENT.with_borrow(|sent| sent.iter().any(|s| is(s, event)))
}

/// Ours, handed back: no longer looked for, its tab and edit to the caller.
fn answered(event: &NSEvent) -> Option<Sent> {
    SENT.with_borrow_mut(|sent| {
        let at = sent.iter().position(|s| is(s, event))?;
        Some(sent.swap_remove(at))
    })
}

/// `+[NSEvent pressedMouseButtons]` as it was: WebKit gives pointer events the buttons the real
/// mouse holds, none while an agent's press or drag goes in, so ours stand in for them then.
static REAL_BUTTONS: OnceLock<Imp> = OnceLock::new();

type PressedButtons = extern "C-unwind" fn(&AnyClass, Sel) -> usize;

extern "C-unwind" fn pressed_buttons(class: &AnyClass, sel: Sel) -> usize {
    if let Some(held) = BUTTONS.with(Cell::get) {
        return held;
    }
    let Some(&original) = REAL_BUTTONS.get() else {
        return 0;
    };
    // SAFETY: the class method's own implementation, of this same signature.
    let original = unsafe { std::mem::transmute::<Imp, PressedButtons>(original) };
    original(class, sel)
}

fn stand_in_for_buttons() {
    if REAL_BUTTONS.get().is_some() {
        return;
    }
    let Some(method) = NSEvent::class().class_method(sel!(pressedMouseButtons)) else {
        return;
    };
    let ours: PressedButtons = pressed_buttons;
    // SAFETY: an implementation of the same signature, set on the main thread with the
    // original kept first.
    let _ = REAL_BUTTONS.set(method.implementation());
    unsafe { method.set_implementation(std::mem::transmute::<PressedButtons, Imp>(ours)) };
}

/// Our buttons stand in for the real mouse's while this lives, and not a moment longer.
struct Holding;

impl Holding {
    fn buttons(buttons: usize) -> Self {
        stand_in_for_buttons();
        BUTTONS.set(Some(buttons));
        Self
    }
}

impl Drop for Holding {
    fn drop(&mut self) {
        BUTTONS.set(None);
    }
}

/// `-[NSApplication sendEvent:]` as it was, on the class that has it (tao's TaoApp).
static SEND_EVENT: OnceLock<Imp> = OnceLock::new();

type SendEvent = extern "C-unwind" fn(&AnyObject, Sel, &NSEvent);

/// A key down of ours no page took comes back here: WebKit sends it on as the app's (its
/// doneWithKeyEvent), to the menus and the key window. The page had it already; the user's
/// terminal mustn't type it, nor a ⌘W close the user's pane. What the Edit menu does for it
/// is done in its own page instead.
extern "C-unwind" fn send_event(app: &AnyObject, sel: Sel, event: &NSEvent) {
    if event.r#type() == NSEventType::KeyDown {
        if let Some(sent) = answered(event) {
            if let (Some(edit), Some(web)) = (sent.edit, sent.web.and_then(|w| w.load())) {
                edit_page(&web, edit);
            }
            return;
        }
    }
    if let Some(&original) = SEND_EVENT.get() {
        // SAFETY: the method's own implementation, of this same signature.
        let original = unsafe { std::mem::transmute::<Imp, SendEvent>(original) };
        original(app, sel, event);
    }
}

/// The class whose own `sel` a message to an instance of `class` runs.
fn owner(class: &AnyClass, sel: Sel) -> Option<&AnyClass> {
    let mut at = Some(class);
    while let Some(c) = at {
        if c.instance_methods().iter().any(|m| m.name() == sel) {
            return Some(c);
        }
        at = c.superclass();
    }
    None
}

/// Keys of ours dropped as they come back, from the first one on.
fn drop_resent_keys(mtm: MainThreadMarker) {
    if SEND_EVENT.get().is_some() {
        return;
    }
    let app = NSApplication::sharedApplication(mtm);
    let sel = sel!(sendEvent:);
    let Some(class) = owner(app.class(), sel) else {
        return;
    };
    let Some(method) = class.instance_method(sel) else {
        return;
    };
    #[cfg(debug_assertions)]
    eprintln!(
        "browser: agents' unanswered keys stop at {}'s sendEvent:",
        class.name().to_string_lossy()
    );
    let ours: SendEvent = send_event;
    // SAFETY: an implementation of the same signature, set on the main thread, where events
    // come, with the original kept first.
    let _ = SEND_EVENT.set(method.implementation());
    unsafe { method.set_implementation(std::mem::transmute::<SendEvent, Imp>(ours)) };
}

fn uptime() -> f64 {
    NSProcessInfo::processInfo().systemUptime()
}

/// The modifiers an agent holds down on the tab, on each of its events.
fn held(web: &WebView) -> NSEventModifierFlags {
    NSEventModifierFlags(web.ivars().held.borrow().flags() as usize)
}

/// One pointer event at `at` (CSS px; where the pointer last was when None, else the middle of
/// the page): `done` gets the point once WebKit has handled it.
pub fn agent_pointer(
    id: &str,
    what: Pointer,
    at: Option<(f64, f64)>,
    done: Box<dyn FnOnce((f64, f64))>,
) -> Res<()> {
    let mtm = main_thread()?;
    // Letting a button go still goes, so a tab the user took isn't left mid-press.
    if !matches!(what, Pointer::Up { .. }) {
        agent::yours(id)?;
    }
    let v = view(id).ok_or(GONE)?;
    super::crashes::check(&v)?;
    let web = &v.web;
    let window = web.window().ok_or(NOT_DRAWN)?;
    let zoom = unsafe { web.pageZoom() };
    let size = web.bounds().size;
    let at = at
        .or(v.delegate.ivars().pointer.get())
        .unwrap_or((size.width / zoom / 2.0, size.height / zoom / 2.0));
    let (x, y) = input::to_view(at, zoom, size.height, web.isFlipped());
    let location = web.convertPoint_toView(NSPoint::new(x, y), None);
    let flags = held(web);
    let event = match what {
        Pointer::Wheel { dy } => wheel(mtm, &window, location, dy, flags)?,
        _ => mouse(what, location, flags, window.windowNumber())
            .ok_or("AppKit made no mouse event.")?,
    };
    // A press is what WebKit starts a drag from (`refuse_drag`), and isn't the user's.
    if matches!(what, Pointer::Down { .. }) {
        remember(&event, None, None);
    }
    v.delegate.ivars().pointer.set(Some(at));
    let holding = Holding::buttons(what.buttons());
    unsafe {
        match what {
            Pointer::Move if web.respondsToSelector(sel!(_simulateMouseMove:)) => {
                // WebKit drops a move AppKit's tracking area didn't route; its testing entry
                // point takes it as a real one.
                let _: () = msg_send![&**web, _simulateMouseMove: &*event];
            }
            Pointer::Move => web.mouseMoved(&event),
            Pointer::Down { right: false, .. } => web.mouseDown(&event),
            Pointer::Down { right: true, .. } => {
                let menus = &web.ivars().menus;
                menus.set(menus.get() + 1);
                web.rightMouseDown(&event);
            }
            Pointer::Up { right: false, .. } => web.mouseUp(&event),
            Pointer::Up { right: true, .. } => web.rightMouseUp(&event),
            Pointer::Drag => web.mouseDragged(&event),
            Pointer::Wheel { .. } => web.scrollWheel(&event),
        }
    }
    drop(holding);
    after_pending(web, move || done(at));
    Ok(())
}

fn mouse(
    what: Pointer,
    location: NSPoint,
    flags: NSEventModifierFlags,
    window: isize,
) -> Option<Retained<NSEvent>> {
    let kind = match what {
        Pointer::Move => NSEventType::MouseMoved,
        Pointer::Down { right: false, .. } => NSEventType::LeftMouseDown,
        Pointer::Down { right: true, .. } => NSEventType::RightMouseDown,
        Pointer::Up { right: false, .. } => NSEventType::LeftMouseUp,
        Pointer::Up { right: true, .. } => NSEventType::RightMouseUp,
        Pointer::Drag => NSEventType::LeftMouseDragged,
        Pointer::Wheel { .. } => return None,
    };
    let clicks = match what {
        Pointer::Down { clicks, .. } | Pointer::Up { clicks, .. } => isize::from(clicks),
        _ => 0,
    };
    let pressure = if what.buttons() > 0 { 1.0 } else { 0.0 };
    let number = NUMBER.with(|n| {
        n.set(n.get() + 1);
        n.get()
    });
    NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
        kind, location, flags, uptime(), window, None, number, clicks, pressure,
    )
}

/// Key events on the tab, as `keys` asks, with the keys its agents hold (input.rs `Held`). What
/// the Edit menu does for a ⌘ key comes after, and only if the page didn't take the key
/// (`send_event`).
pub fn agent_keys(id: &str, keys: Keys) -> Res<()> {
    let mtm = main_thread()?;
    // As a button: a key let go still goes.
    if !matches!(keys, Keys::Up(_) | Keys::Press(_, Half::Up)) {
        agent::yours(id)?;
    }
    drop_resent_keys(mtm);
    let v = view(id).ok_or(GONE)?;
    super::crashes::check(&v)?;
    let window = v.web.window().ok_or(NOT_DRAWN)?;
    let mut held = v.web.ivars().held.borrow_mut();
    let (steps, edit) = match &keys {
        Keys::Press(chord, half) => held.press(chord, *half)?,
        Keys::Down(name) => (held.down(name)?, None),
        Keys::Up(name) => (held.up(name)?, None),
    };
    drop(held);
    send_keys(&v.web, &window, steps, edit)
}

/// The keys its agents held let go: the pane that held them closed.
pub(super) fn release_keys(v: &View) {
    let steps = v.web.ivars().held.borrow_mut().release_all();
    if let Some(window) = v.web.window() {
        let _ = send_keys(&v.web, &window, steps, None);
    }
}

fn send_keys(web: &WebView, window: &NSWindow, steps: Vec<KeyStep>, edit: Option<Edit>) -> Res<()> {
    for step in steps {
        key(web, window, step, edit)?;
    }
    Ok(())
}

fn key(web: &WebView, window: &NSWindow, step: KeyStep, edit: Option<Edit>) -> Res<()> {
    let (kind, code, flags, chars, bare) = match &step {
        KeyStep::Flags { code, flags } => (NSEventType::FlagsChanged, *code, *flags, "", ""),
        KeyStep::Down(s) => (NSEventType::KeyDown, s.code, s.flags, &*s.chars, &*s.bare),
        KeyStep::Up(s) => (NSEventType::KeyUp, s.code, s.flags, &*s.chars, &*s.bare),
    };
    let event = NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(
        kind,
        NSPoint::ZERO,
        NSEventModifierFlags(flags as usize),
        uptime(),
        window.windowNumber(),
        None,
        &NSString::from_str(chars),
        &NSString::from_str(bare),
        false,
        code,
    )
    .ok_or("AppKit made no key event.")?;
    match step {
        KeyStep::Flags { .. } => web.flagsChanged(&event),
        KeyStep::Down(_) => {
            remember(&event, Some(web), edit);
            // An arrow moves the caret by lines in a contenteditable only through the window's
            // text input, which sees it as the first responder's.
            if (0x7B..=0x7E).contains(&code) && web.focused() {
                window.sendEvent(&event);
            } else {
                web.keyDown(&event);
            }
        }
        KeyStep::Up(_) => web.keyUp(&event),
    }
    Ok(())
}

/// What the Edit menu would do for a ⌘ key no page took, on the page's own web view: AppKit
/// sends it no menu action for a key it didn't see. Copy, cut and paste go by the tab's own
/// clipboard (scripts/clipboard.js), never the user's; none reaches a frame of another origin,
/// whose copies are its own and which the page may not read.
fn edit_page(web: &WebView, edit: Edit) {
    let undo = &web.ivars().undo;
    match edit {
        Edit::SelectAll => {
            let _: () = unsafe { msg_send![web, selectAll: None::<&AnyObject>] };
        }
        Edit::Undo => undo.undo(),
        Edit::Redo => undo.redo(),
        Edit::Copy => page_tools::clipboard(web, "copy", |_| {}),
        Edit::Cut => page_tools::clipboard(web, "cut", |_| {}),
        // A paste event with the text, which the page may take; else the text goes in as typed.
        // The script answers nothing for a frame of another origin: nothing goes in.
        Edit::Paste => {
            let target = Weak::new(web);
            page_tools::clipboard(web, "paste", move |text| {
                if let (Some(web), Some(text)) = (target.load(), text.filter(|t| !t.is_empty())) {
                    insert(&web, &text);
                }
            });
        }
    }
}

/// NSNotFound: at the caret, in place of what's selected.
const CARET: NSRange = NSRange::new(isize::MAX as usize, 0);

fn insert(web: &WebView, text: &str) {
    let text = NSString::from_str(text);
    let _: () = unsafe { msg_send![web, insertText: &*text, replacementRange: CARET] };
}

/// `text` put in at the page's caret as an input method puts it (NSTextInputClient), a
/// character at a time: the page gets its input events, whatever the user's keyboard or input
/// source. A line break goes in as text too, in order with the rest: a key event's would come
/// back from WebKit later than the text after it.
pub fn agent_text(id: &str, text: &str) -> Res<()> {
    main_thread()?;
    agent::yours(id)?;
    let v = view(id).ok_or(GONE)?;
    super::crashes::check(&v)?;
    for c in input::as_typed(text).chars() {
        insert(&v.web, c.encode_utf8(&mut [0; 4]));
    }
    Ok(())
}

/// The next file chooser on the tab, while this command runs, gets `files` (an agent's
/// `upload`), the user's never.
pub fn agent_upload(id: &str, files: Vec<PathBuf>) -> Res<()> {
    main_thread()?;
    agent::yours(id)?;
    let v = view(id).ok_or(GONE)?;
    v.delegate.ivars().chosen.take();
    *v.delegate.ivars().upload.borrow_mut() = Some(files);
    Ok(())
}

/// How many files the chooser took, or why none, once it came.
pub fn agent_chosen(id: &str) -> Res<Option<Res<usize>>> {
    main_thread()?;
    let v = view(id).ok_or(GONE)?;
    let chosen = v.delegate.ivars().chosen.borrow().clone();
    Ok(chosen)
}

/// The upload over: no chooser after this gets its files.
pub fn agent_upload_done(id: &str) -> Res<()> {
    main_thread()?;
    let v = view(id).ok_or(GONE)?;
    v.delegate.ivars().upload.take();
    v.delegate.ivars().chosen.take();
    Ok(())
}

/// The page's file chooser answered with an agent's `files`: none where it takes one only.
pub(super) fn choose(
    delegate: &Delegate,
    files: Vec<PathBuf>,
    many: bool,
    done: &DynBlock<dyn Fn(*mut NSArray<NSURL>)>,
) {
    let outcome = if files.len() > 1 && !many {
        done.call((std::ptr::null_mut(),));
        Err(format!(
            "The page's chooser takes one file, not {}",
            files.len()
        ))
    } else {
        let urls: Vec<Retained<NSURL>> = files
            .iter()
            .map(|f| NSURL::fileURLWithPath(&NSString::from_str(&f.to_string_lossy())))
            .collect();
        let urls = NSArray::from_retained_slice(&urls);
        done.call((Retained::as_ptr(&urls).cast_mut(),));
        Ok(files.len())
    };
    *delegate.ivars().chosen.borrow_mut() = Some(outcome);
}

/// CGEventRef: AppKit makes no wheel event of its own, so one comes from CoreGraphics.
#[repr(C)]
struct CGEvent {
    _private: [u8; 0],
}

// SAFETY: CGEventRef is a pointer to the opaque struct __CGEvent, as CoreGraphics declares it.
unsafe impl RefEncode for CGEvent {
    const ENCODING_REF: Encoding = Encoding::Pointer(&Encoding::Struct("__CGEvent", &[]));
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CGPoint {
    x: f64,
    y: f64,
}

/// kCGScrollEventUnitPixel, and the fields that name the window under the pointer.
const PIXELS: u32 = 0;
const WINDOW_UNDER_POINTER: u32 = 91;
const WINDOW_THAT_HANDLES_IT: u32 = 92;

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventCreateScrollWheelEvent2(
        source: *const c_void,
        units: u32,
        wheel_count: u32,
        wheel1: i32,
        wheel2: i32,
        wheel3: i32,
    ) -> *mut CGEvent;
    fn CGEventSetLocation(event: *mut CGEvent, location: CGPoint);
    fn CGEventSetIntegerValueField(event: *mut CGEvent, field: u32, value: i64);
    fn CGEventSetFlags(event: *mut CGEvent, flags: u64);
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRelease(cf: *const c_void);
}

/// A wheel turn at `location` in `window`, in px (`dy` down); an error where AppKit doesn't
/// take the window from it, as then WebKit would scroll some other point.
fn wheel(
    mtm: MainThreadMarker,
    window: &objc2_app_kit::NSWindow,
    location: NSPoint,
    dy: f64,
    flags: NSEventModifierFlags,
) -> Res<Retained<NSEvent>> {
    let screen = window.convertPointToScreen(location);
    // CoreGraphics counts down from the top of the first screen.
    let top = NSScreen::screens(mtm)
        .firstObject()
        .map_or(0.0, |s| s.frame().size.height);
    let event = unsafe {
        let cg = CGEventCreateScrollWheelEvent2(std::ptr::null(), PIXELS, 1, -dy as i32, 0, 0);
        if cg.is_null() {
            return Err(input::NO_WHEEL.into());
        }
        CGEventSetLocation(
            cg,
            CGPoint {
                x: screen.x,
                y: top - screen.y,
            },
        );
        let number = i64::try_from(window.windowNumber()).unwrap_or_default();
        CGEventSetIntegerValueField(cg, WINDOW_UNDER_POINTER, number);
        CGEventSetIntegerValueField(cg, WINDOW_THAT_HANDLES_IT, number);
        CGEventSetFlags(cg, flags.0 as u64);
        let event: Option<Retained<NSEvent>> = msg_send![NSEvent::class(), eventWithCGEvent: cg];
        CFRelease(cg.cast());
        event
    };
    let event = event.ok_or(input::NO_WHEEL)?;
    let at = event.locationInWindow();
    let here = event.windowNumber() == window.windowNumber()
        && (at.x - location.x).abs() < 1.0
        && (at.y - location.y).abs() < 1.0;
    if here {
        Ok(event)
    } else {
        Err(input::NO_WHEEL.into())
    }
}

/// `then` once WebKit has handled the mouse events it holds (a private call, where WebKit has
/// it), else a frame later.
fn after_pending(web: &WebView, then: impl FnOnce() + 'static) {
    let then = Cell::new(Some(then));
    let block = RcBlock::new(move || {
        if let Some(then) = then.take() {
            then();
        }
    });
    if web.respondsToSelector(sel!(_doAfterProcessingAllPendingMouseEvents:)) {
        let _: () = unsafe { msg_send![web, _doAfterProcessingAllPendingMouseEvents: &*block] };
    } else {
        let later = RcBlock::new(move |_: NonNull<NSTimer>| block.call(()));
        unsafe { NSTimer::scheduledTimerWithTimeInterval_repeats_block(PACE_SECS, false, &later) };
    }
}

/// An AppKit drag WebKit would start from one of our presses: it follows the real mouse, so it
/// ends at once where it began, before AppKit runs it; the page gets its dragend.
pub(super) fn refuse_drag(web: &WebView, event: &NSEvent) -> Option<Retained<AnyObject>> {
    let class = AnyClass::get(c"NSDraggingSession")?;
    let ends = sel!(draggingSession:endedAtPoint:operation:);
    if !web.respondsToSelector(ends) {
        return None;
    }
    let session: Retained<AnyObject> = unsafe { msg_send![class, new] };
    let at = event
        .window(web.mtm())
        .map_or(event.locationInWindow(), |w| {
            w.convertPointToScreen(event.locationInWindow())
        });
    let (web, ended) = (objc2::rc::Weak::new(web), session.clone());
    let end = RcBlock::new(move |_: NonNull<NSTimer>| {
        if let Some(web) = web.load() {
            // NSDragOperationNone.
            let _: () = unsafe {
                msg_send![&*web, draggingSession: &*ended, endedAtPoint: at, operation: 0usize]
            };
        }
    });
    unsafe { NSTimer::scheduledTimerWithTimeInterval_repeats_block(0.0, false, &end) };
    Some(session)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each would only fail on the main thread with a page open: checked here instead.
    #[test]
    fn raw_messages_match_their_declarations() {
        NSEvent::class()
            .metaclass()
            .verify_sel::<(*mut CGEvent,), *mut NSEvent>(sel!(eventWithCGEvent:))
            .unwrap();
        // Private: checked where this macOS has it.
        let web = objc2_web_kit::WKWebView::class();
        if web.instance_method(sel!(_simulateMouseMove:)).is_some() {
            web.verify_sel::<(&NSEvent,), ()>(sel!(_simulateMouseMove:))
                .unwrap();
        }
        let pending = sel!(_doAfterProcessingAllPendingMouseEvents:);
        if web.instance_method(pending).is_some() {
            web.verify_sel::<(&block2::Block<dyn Fn()>,), ()>(pending)
                .unwrap();
        }
        let ended = sel!(draggingSession:endedAtPoint:operation:);
        if web.instance_method(ended).is_some() {
            web.verify_sel::<(&AnyObject, NSPoint, usize), ()>(ended)
                .unwrap();
        }
        assert!(NSEvent::class()
            .class_method(sel!(pressedMouseButtons))
            .is_some());
        web.verify_sel::<(Option<&AnyObject>,), ()>(sel!(selectAll:))
            .unwrap();
        web.verify_sel::<(&NSString, NSRange), ()>(sel!(insertText:replacementRange:))
            .unwrap();
        let undo = AnyClass::get(c"NSUndoManager").unwrap();
        undo.verify_sel::<(), ()>(sel!(undo)).unwrap();
        undo.verify_sel::<(), ()>(sel!(redo)).unwrap();
    }

    /// The class `drop_resent_keys` swaps `sendEvent:` on is the one that has it, here
    /// NSApplication's own; in the app, tao's subclass, which has its own.
    #[test]
    fn the_send_event_swapped_is_the_class_that_has_it() {
        let app = NSApplication::class();
        let class = owner(app, sel!(sendEvent:)).unwrap();
        assert_eq!(class.name(), app.name());
        class
            .verify_sel::<(&NSEvent,), ()>(sel!(sendEvent:))
            .unwrap();
        assert!(owner(app, sel!(gvNoSuchMessage)).is_none());
    }

    fn key_event(kind: NSEventType, timestamp: f64) -> Retained<NSEvent> {
        NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(
            kind,
            NSPoint::ZERO,
            NSEventModifierFlags(0),
            timestamp,
            0,
            None,
            &NSString::from_str("w"),
            &NSString::from_str("w"),
            false,
            0x0D,
        )
        .unwrap()
    }

    /// Ours known again as the very object WebKit hands back; another event, even one alike
    /// in every field (the user's at the same instant), isn't.
    #[test]
    fn the_leak_guard_knows_ours_and_nothing_else() {
        let at = uptime();
        let sent = key_event(NSEventType::KeyDown, at);
        remember(&sent, None, None);
        assert!(ours(&sent));
        assert!(
            !ours(&key_event(NSEventType::KeyDown, at)),
            "alike, not ours"
        );
        assert!(!ours(&key_event(NSEventType::KeyUp, at)));
        // Handed back once, it's answered and no longer looked for.
        assert!(answered(&sent).is_some());
        assert!(!ours(&sent) && answered(&sent).is_none());
    }

    /// A key WebKit still holds stays ours however many events come after it (a long scroll,
    /// a drag, other tabs' agents), and once WebKit lets it go there's nothing left to keep.
    #[test]
    fn a_key_sent_just_before_a_long_scroll_is_still_known_as_ours() {
        use objc2::rc::autoreleasepool;
        let sent = autoreleasepool(|_| key_event(NSEventType::KeyDown, uptime()));
        remember(&sent, None, None);
        for _ in input::wheel(40_000, false) {
            autoreleasepool(|_| {
                let turn = mouse(Pointer::Move, NSPoint::ZERO, NSEventModifierFlags(0), 0);
                remember(&turn.unwrap(), None, None);
            });
        }
        assert!(ours(&sent));
        drop(sent);
        assert!(SENT.with_borrow_mut(|s| {
            s.retain(|s| s.event.load().is_some());
            s.is_empty()
        }));
    }

    /// Every Enter of a long multi-line burst is still known while WebKit holds it.
    #[test]
    fn every_enter_of_a_long_multiline_type_is_still_known_as_ours() {
        let downs: Vec<_> = (0..200)
            .map(|_| {
                let down = key_event(NSEventType::KeyDown, uptime());
                remember(&down, None, None);
                down
            })
            .collect();
        let forgotten = downs.iter().filter(|d| !ours(d)).count();
        assert_eq!(forgotten, 0, "{forgotten} of 200 Enters forgotten");
    }

    /// A panic out of an event's delivery leaves no agent buttons held for the app.
    #[test]
    fn held_buttons_end_with_the_event_even_on_a_panic() {
        let _ = std::panic::catch_unwind(|| {
            let _holding = Holding::buttons(1);
            assert_eq!(BUTTONS.with(Cell::get), Some(1));
            panic!("WebKit threw");
        });
        assert_eq!(BUTTONS.with(Cell::get), None);
    }
}
