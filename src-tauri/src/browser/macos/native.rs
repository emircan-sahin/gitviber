//! Trusted input for an agent's tab (browser/input.rs plans it): NSEvents handed straight to the
//! page's web view, as AppKit would hand them, so the page takes them as the user's
//! (`isTrusted`, user activation). Never posted to the window server: the real cursor, the
//! user's focus and the app's activation stay as they are.

use super::{ask, asked, main_thread, view, Delegate, WebView, GONE};
use crate::browser::input::{self, Edit, Half, KeyStep, Keys, Pointer};
use crate::state::Res;
use block2::{DynBlock, RcBlock};
use objc2::encode::{Encoding, RefEncode};
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Imp, Sel};
use objc2::{msg_send, sel, ClassType, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSEvent, NSEventModifierFlags, NSEventType, NSScreen, NSWindow,
};
use objc2_foundation::{
    NSArray, NSObjectProtocol, NSPoint, NSProcessInfo, NSRange, NSString, NSTimer, NSURL,
};
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::ffi::c_void;
use std::path::PathBuf;
use std::ptr::NonNull;
use std::sync::OnceLock;

/// Without WebKit's own wait for the mouse events it holds, about a frame between events, so
/// it doesn't coalesce them.
const PACE_SECS: f64 = 0.016;
/// Events of ours remembered, to know them again as WebKit hands them back: a long scroll's
/// wheel turns mustn't push out a key sent just before.
const REMEMBERED: usize = 256;

thread_local! {
    /// The buttons WebKit reads as held while one of our events goes in (`pressed`).
    static BUTTONS: Cell<Option<usize>> = const { Cell::new(None) };
    /// Our latest events, by type and timestamp.
    static OURS: RefCell<VecDeque<(usize, u64)>> = RefCell::default();
    static NUMBER: Cell<isize> = const { Cell::new(0) };
}

/// `+[NSEvent pressedMouseButtons]` as it was: WebKit gives pointer events the buttons the real
/// mouse holds, none while an agent's press or drag goes in, so ours stand in for them then.
static PRESSED: OnceLock<Imp> = OnceLock::new();

type Pressed = extern "C-unwind" fn(&AnyClass, Sel) -> usize;

extern "C-unwind" fn pressed(class: &AnyClass, sel: Sel) -> usize {
    if let Some(held) = BUTTONS.with(Cell::get) {
        return held;
    }
    let Some(&original) = PRESSED.get() else {
        return 0;
    };
    // SAFETY: the class method's own implementation, of this same signature.
    let original = unsafe { std::mem::transmute::<Imp, Pressed>(original) };
    original(class, sel)
}

fn stand_in_for_buttons() {
    if PRESSED.get().is_some() {
        return;
    }
    let Some(method) = NSEvent::class().class_method(sel!(pressedMouseButtons)) else {
        return;
    };
    let ours: Pressed = pressed;
    // SAFETY: an implementation of the same signature, set on the main thread with the
    // original kept first.
    let _ = PRESSED.set(method.implementation());
    unsafe { method.set_implementation(std::mem::transmute::<Pressed, Imp>(ours)) };
}

/// `-[NSApplication sendEvent:]` as it was.
static SEND_EVENT: OnceLock<Imp> = OnceLock::new();

type SendEvent = extern "C-unwind" fn(&AnyObject, Sel, &NSEvent);

/// A key of ours no page handled comes back here: WebKit sends it on as the app's (its
/// doneWithKeyEvent), to the key window and the menus. The page had it already; the user's
/// terminal mustn't type it, nor a ⌘W close the user's pane.
extern "C-unwind" fn send_event(app: &AnyObject, sel: Sel, event: &NSEvent) {
    let key = matches!(
        event.r#type(),
        NSEventType::KeyDown | NSEventType::KeyUp | NSEventType::FlagsChanged
    );
    if key && ours(event) {
        return;
    }
    if let Some(&original) = SEND_EVENT.get() {
        // SAFETY: the method's own implementation, of this same signature.
        let original = unsafe { std::mem::transmute::<Imp, SendEvent>(original) };
        original(app, sel, event);
    }
}

/// Keys of ours dropped as they come back, from the first one on.
fn drop_resent_keys(mtm: MainThreadMarker) {
    if SEND_EVENT.get().is_some() {
        return;
    }
    let app = NSApplication::sharedApplication(mtm);
    let Some(method) = app.class().instance_method(sel!(sendEvent:)) else {
        return;
    };
    let ours: SendEvent = send_event;
    // SAFETY: an implementation of the same signature, set on the main thread, where events
    // come, with the original kept first.
    let _ = SEND_EVENT.set(method.implementation());
    unsafe { method.set_implementation(std::mem::transmute::<SendEvent, Imp>(ours)) };
}

fn uptime() -> f64 {
    NSProcessInfo::processInfo().systemUptime()
}

fn remember(event: &NSEvent) {
    OURS.with_borrow_mut(|ours| {
        if ours.len() == REMEMBERED {
            ours.pop_front();
        }
        ours.push_back((event.r#type().0, event.timestamp().to_bits()));
    });
}

/// Whether `event` is one of ours, as WebKit hands it back.
pub(super) fn ours(event: &NSEvent) -> bool {
    let key = (event.r#type().0, event.timestamp().to_bits());
    OURS.with_borrow(|ours| ours.contains(&key))
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
    let v = view(id).ok_or(GONE)?;
    let web = &v.web;
    let window = web.window().ok_or("The page isn't drawn now.")?;
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
    remember(&event);
    v.delegate.ivars().pointer.set(Some(at));
    stand_in_for_buttons();
    BUTTONS.set(Some(what.buttons()));
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
    BUTTONS.set(None);
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

/// Key events on the tab, as `keys` asks, with the keys its agents hold (input.rs `Held`); then
/// what the Edit menu does for a ⌘ key, but for a copy, cut or paste, left to the caller: those
/// read the page's selection first (`agent_clipboard`).
pub fn agent_keys(id: &str, keys: Keys) -> Res<Option<Edit>> {
    let mtm = main_thread()?;
    drop_resent_keys(mtm);
    let v = view(id).ok_or(GONE)?;
    let window = v.web.window().ok_or("The page isn't drawn now.")?;
    let mut held = v.web.ivars().held.borrow_mut();
    let (steps, edit) = match &keys {
        Keys::Press(chord, half) => held.press(chord, *half)?,
        Keys::Down(name) => (held.down(name)?, None),
        Keys::Up(name) => (held.up(name)?, None),
    };
    drop(held);
    for step in steps {
        key(&v.web, &window, step)?;
    }
    Ok(edit.filter(|&edit| !edit_page(&v.web, edit)))
}

fn key(web: &WebView, window: &NSWindow, step: KeyStep) -> Res<()> {
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
    remember(&event);
    match step {
        KeyStep::Flags { .. } => web.flagsChanged(&event),
        // An arrow moves the caret by lines in a contenteditable only through the window's text
        // input, which sees it as the first responder's.
        KeyStep::Down(_) if (0x7B..=0x7E).contains(&code) && web.focused() => {
            window.sendEvent(&event)
        }
        KeyStep::Down(_) => web.keyDown(&event),
        KeyStep::Up(_) => web.keyUp(&event),
    }
    Ok(())
}

/// What the Edit menu would do for a ⌘ key, on the page's own web view: AppKit sends it no
/// menu action for a key it didn't see. False for a copy, cut or paste, which go by the tab's
/// own clipboard instead.
fn edit_page(web: &WebView, edit: Edit) -> bool {
    match edit {
        Edit::SelectAll => {
            let _: () = unsafe { msg_send![web, selectAll: None::<&AnyObject>] };
        }
        Edit::Undo | Edit::Redo => {
            let undo: *mut AnyObject = unsafe { msg_send![web, undoManager] };
            if let Some(undo) = unsafe { undo.as_ref() } {
                let _: () = if edit == Edit::Undo {
                    unsafe { msg_send![undo, undo] }
                } else {
                    unsafe { msg_send![undo, redo] }
                };
            }
        }
        Edit::Copy | Edit::Cut | Edit::Paste => return false,
    }
    true
}

/// The rest of ⌘C, ⌘X or ⌘V, on the tab's own clipboard, never the user's: what the page had
/// selected kept (a copy, a cut), the selection deleted (a cut), or what's kept put in at the
/// caret (a paste), as typed.
pub fn agent_clipboard(id: &str, edit: Edit, selected: Option<String>) -> Res<()> {
    main_thread()?;
    let v = view(id).ok_or(GONE)?;
    if let Some(selected) = selected {
        ask(id, |a| a.clipboard = selected);
    }
    match edit {
        Edit::Cut => {
            let _: () = unsafe { msg_send![&*v.web, delete: None::<&AnyObject>] };
        }
        Edit::Paste => {
            let kept = NSString::from_str(&asked(id).clipboard);
            if kept.length() > 0 {
                let _: () =
                    unsafe { msg_send![&*v.web, insertText: &*kept, replacementRange: CARET] };
            }
        }
        _ => {}
    }
    Ok(())
}

/// NSNotFound: at the caret, in place of what's selected.
const CARET: NSRange = NSRange::new(isize::MAX as usize, 0);

/// `text` put in at the page's caret as an input method puts it (NSTextInputClient), a
/// character at a time: the page gets its input events, whatever the user's keyboard or input
/// source; a line break is an Enter.
pub fn agent_text(id: &str, text: &str) -> Res<()> {
    main_thread()?;
    let v = view(id).ok_or(GONE)?;
    for (n, line) in text.split('\n').enumerate() {
        if n > 0 {
            agent_keys(id, Keys::Press("Enter".into(), Half::Both))?;
        }
        for c in line.chars() {
            let c = NSString::from_str(c.encode_utf8(&mut [0; 4]));
            let _: () = unsafe { msg_send![&*v.web, insertText: &*c, replacementRange: CARET] };
        }
    }
    Ok(())
}

/// The next file chooser on the tab gets `files` (an agent's `upload`), the user's never.
pub fn agent_upload(id: &str, files: Vec<PathBuf>) -> Res<()> {
    main_thread()?;
    let v = view(id).ok_or(GONE)?;
    v.delegate.ivars().chosen.take();
    *v.delegate.ivars().upload.borrow_mut() = Some(files);
    Ok(())
}

/// How many files the chooser took, or why none, once it came; `stop`: no chooser after this
/// gets them.
pub fn agent_chosen(id: &str, stop: bool) -> Res<Option<Res<usize>>> {
    main_thread()?;
    let v = view(id).ok_or(GONE)?;
    if stop {
        v.delegate.ivars().upload.take();
        return Ok(v.delegate.ivars().chosen.take());
    }
    let chosen = v.delegate.ivars().chosen.borrow().clone();
    Ok(chosen)
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
        web.verify_sel::<(), *mut AnyObject>(sel!(undoManager))
            .unwrap();
        web.verify_sel::<(Option<&AnyObject>,), ()>(sel!(delete:))
            .unwrap();
        NSApplication::class()
            .verify_sel::<(&NSEvent,), ()>(sel!(sendEvent:))
            .unwrap();
    }
}
