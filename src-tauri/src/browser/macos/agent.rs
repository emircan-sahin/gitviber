//! The views `gitviber browser` drives (browser/agent.rs): each terminal pane's agent tab, made
//! here without its tab ever showing, kept awake while a command runs on it, and answering its
//! page's dialogs by itself.

use super::{
    create, emit, main_thread, out_of_sight, page_tools, set_agent, stop_timer, url_text, view,
    Delegate, View, GONE, VIEWS,
};
use crate::browser::control::{AgentScreen, Route, Routes};
use crate::browser::picks::{self, Bounds};
use crate::browser::PageState;
use crate::state::Res;
use block2::RcBlock;
use objc2::runtime::{AnyObject, Bool};
use objc2::{msg_send, sel, DefinedClass};
use objc2_app_kit::NSImage;
use objc2_foundation::{
    ns_string, NSDictionary, NSError, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString, NSUUID,
};
use objc2_web_kit::{WKContentWorld, WKSnapshotConfiguration, WKWebView};
use serde::Serialize;
use std::cell::{Cell, RefCell};
use std::time::SystemTime;
use tauri::{AppHandle, Manager};

thread_local! {
    static ROUTES: RefCell<Routes> = RefCell::default();
}

/// A tab out of sight that never showed takes a page this size.
const SIZE: NSSize = NSSize::new(1280.0, 800.0);

#[derive(Serialize, Clone)]
struct Opened<'a> {
    id: &'a str,
    url: &'a str,
    root: &'a str,
    pty: u32,
}

/// The pane's tab, live: made in the background on its first command (at `url`, if one), made
/// again once the user closed it, and back from parking where it was. Its id, and whether it's
/// new, which the workspace hears as `browser-open` and adds without showing.
pub fn agent_tab(app: &AppHandle, pty: u32, root: &str, url: Option<&str>) -> Res<(String, bool)> {
    main_thread()?;
    let open = |id: &str| VIEWS.with_borrow(|r| r.get(id).is_some() || r.parked(id).is_some());
    let found = ROUTES.with_borrow_mut(|r| r.tab(pty, open));
    let made = found.is_none();
    let route = found.unwrap_or_else(|| Route {
        id: NSUUID::new().UUIDString().to_string().to_lowercase(),
        root: root.into(),
    });
    let url = url.unwrap_or("about:blank");
    if view(&route.id).is_none() {
        let window = app
            .get_webview_window("main")
            .ok_or("The window isn't ready.")?;
        create(&window, &route.id, &route.root, url, None)?;
        // Out of sight from the start: it counts toward the pages kept alive, and parks as they do.
        if let Some(v) = view(&route.id) {
            out_of_sight(&v, &route.id);
        }
    }
    view(&route.id)
        .ok_or(GONE)?
        .delegate
        .ivars()
        .agent
        .set(true);
    if made {
        ROUTES.with_borrow_mut(|r| r.set(pty, route.clone()));
        let opened = Opened {
            id: &route.id,
            url,
            root: &route.root,
            pty,
        };
        emit(app, "browser-open", opened);
    }
    Ok((route.id, made))
}

/// The app page reloaded: its tabs come back as the user's.
pub(super) fn forget_all() {
    ROUTES.with_borrow_mut(Routes::clear);
}

/// Awake while a command runs (`on`), counted, as two agents in a pane may run one each. Out of
/// its tab's sight WebKit would slow the page's timers and draw nothing: it shows meanwhile, off
/// the window's edge at the size it would be, and hides again after.
pub fn agent_awake(id: &str, on: bool) {
    let (Ok(_), Some(v)) = (main_thread(), view(id)) else {
        return;
    };
    let d = v.delegate.ivars();
    let was = d.awake.get();
    let shown = VIEWS.with_borrow(|r| r.shown(id));
    if on {
        d.awake.set(was + 1);
        if was == 0 {
            throttle(&v.web, false);
            if !shown {
                stop_timer(&v);
                aside(&v);
            }
        }
    } else if was > 0 {
        d.awake.set(was - 1);
        if was == 1 {
            throttle(&v.web, true);
            if !shown {
                out_of_sight(&v, id);
            }
        }
    }
}

/// Shown off the window's left edge, as the agent's device or the size it last had.
fn aside(v: &View) {
    let screen = v.delegate.ivars().agent_screen.borrow().clone();
    let size = match &screen {
        Some(s) => NSSize::new(s.w, s.h),
        None => {
            let frame = v.host.frame().size;
            let window = unsafe { v.host.superview() }.map(|p| p.bounds().size);
            if frame.width >= 1.0 && frame.height >= 1.0 {
                frame
            } else {
                window.filter(|s| s.width >= 1.0).unwrap_or(SIZE)
            }
        }
    };
    v.host
        .setFrame(NSRect::new(NSPoint::new(-size.width - 100.0, 0.0), size));
    if let Some(s) = screen {
        // Laid out at the device's own width; device mode sets its zoom again when it shows.
        unsafe { v.web.setPageZoom(1.0) };
        v.delegate.ivars().screen.set(None);
        if super::can_set_dpr(&v.web) {
            let dpr = s.dpr.unwrap_or(0.0);
            let _: () = unsafe { msg_send![&*v.web, _setOverrideDeviceScaleFactor: dpr] };
        }
    }
    v.host.setHidden(false);
}

/// WebKit's slowing of pages out of sight, and of a window covered by others, off while an
/// agent's command runs; private, so only where WebKit still has it.
fn throttle(web: &WKWebView, on: bool) {
    let on = Bool::new(on);
    let preferences = unsafe { web.configuration().preferences() };
    unsafe {
        if preferences.respondsToSelector(sel!(_setHiddenPageDOMTimerThrottlingEnabled:)) {
            let _: () = msg_send![&*preferences, _setHiddenPageDOMTimerThrottlingEnabled: on];
        }
        if preferences.respondsToSelector(sel!(_setPageVisibilityBasedProcessSuppressionEnabled:)) {
            let _: () =
                msg_send![&*preferences, _setPageVisibilityBasedProcessSuppressionEnabled: on];
        }
        if web.respondsToSelector(sel!(_setWindowOcclusionDetectionEnabled:)) {
            let _: () = msg_send![web, _setWindowOcclusionDetectionEnabled: on];
        }
    }
}

/// Runs `body` as an async function in GitViber's world (or the page's) with `input` as its
/// argument; `done` gets the string it returns.
pub fn agent_js(
    id: &str,
    page: bool,
    body: &str,
    input: &str,
    done: Box<dyn FnOnce(Res<String>)>,
) -> Res<()> {
    let mtm = main_thread()?;
    let v = view(id).ok_or(GONE)?;
    let world = if page {
        unsafe { WKContentWorld::pageWorld(mtm) }
    } else {
        page_tools::ours(mtm)
    };
    let input = NSString::from_str(input);
    let arguments =
        NSDictionary::<NSString, AnyObject>::from_slices(&[ns_string!("input")], &[&*input]);
    let done = Cell::new(Some(done));
    let block = RcBlock::new(move |value: *mut AnyObject, error: *mut NSError| {
        let Some(done) = done.take() else {
            return;
        };
        // SAFETY: WebKit's answer and error, objects or nil, alive for this call.
        if let Some(error) = unsafe { error.as_ref() } {
            return done(Err(js_error(error)));
        }
        let text = unsafe { value.as_ref() }
            .and_then(|v| v.downcast_ref::<NSString>())
            .map(|s| s.to_string());
        done(text.ok_or_else(|| "The page gave no answer.".into()));
    });
    unsafe {
        v.web
            .callAsyncJavaScript_arguments_inFrame_inContentWorld_completionHandler(
                &NSString::from_str(body),
                Some(&arguments),
                None,
                &world,
                Some(&block),
            )
    };
    Ok(())
}

/// What the page threw, as it said it; else what WebKit says went wrong.
fn js_error(error: &NSError) -> String {
    error
        .userInfo()
        .objectForKey(ns_string!("WKJavaScriptExceptionMessage"))
        .and_then(|m| m.downcast::<NSString>().ok())
        .map_or_else(
            || error.localizedDescription().to_string(),
            |m| m.to_string(),
        )
}

/// The page as the address bar has it, and whether its console is kept (Settings → Browser).
pub fn agent_page(id: &str) -> Res<(PageState, bool)> {
    main_thread()?;
    let v = view(id).ok_or(GONE)?;
    Ok((v.delegate.state(&v.web), page_tools::capturing()))
}

/// A PNG of the page as it shows, or of `rect` in it (CSS px, from the viewport's top left).
pub fn agent_picture(
    id: &str,
    rect: Option<Bounds>,
    done: Box<dyn FnOnce(Option<Vec<u8>>)>,
) -> Res<()> {
    let mtm = main_thread()?;
    let v = view(id).ok_or(GONE)?;
    let config = unsafe { WKSnapshotConfiguration::new(mtm) };
    if let Some(rect) = rect {
        let size = v.web.bounds().size;
        let zoom = unsafe { v.web.pageZoom() };
        let r = picks::picture_rect(rect, zoom, (size.width, size.height))
            .ok_or("The element doesn't show on the page.")?;
        unsafe { config.setRect(NSRect::new(NSPoint::new(r.x, r.y), NSSize::new(r.w, r.h))) };
    }
    let done = Cell::new(Some(done));
    let block = RcBlock::new(move |image: *mut NSImage, _error: *mut NSError| {
        if let Some(done) = done.take() {
            // SAFETY: WebKit's picture, or nil, alive for this call.
            done(unsafe { image.as_ref() }.and_then(page_tools::png));
        }
    });
    unsafe {
        v.web
            .takeSnapshotWithConfiguration_completionHandler(Some(&config), &block)
    };
    Ok(())
}

#[derive(Serialize, Clone)]
struct DeviceChoice {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    w: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    h: Option<u32>,
}

#[derive(Serialize, Clone)]
struct Device {
    id: String,
    device: Option<DeviceChoice>,
}

/// The page as a device (None: as itself): its user agent now, its size while out of sight, and
/// the tab's device mode once it shows (`browser-device`).
pub fn agent_device(app: &AppHandle, id: &str, screen: Option<AgentScreen>) -> Res<()> {
    main_thread()?;
    let v = view(id).ok_or(GONE)?;
    let choice = screen.as_ref().map(|s| DeviceChoice {
        name: s.name.clone(),
        w: s.size.map(|s| s.0),
        h: s.size.map(|s| s.1),
    });
    set_agent(id, screen.as_ref().and_then(|s| s.ua.as_deref()))?;
    let off = screen.is_none();
    *v.delegate.ivars().agent_screen.borrow_mut() = screen;
    if off && super::can_set_dpr(&v.web) {
        let _: () = unsafe { msg_send![&*v.web, _setOverrideDeviceScaleFactor: 0.0f64] };
    }
    if v.delegate.ivars().awake.get() > 0 && !VIEWS.with_borrow(|r| r.shown(id)) {
        if off {
            // Back to the window's size, not the device's it had.
            v.host.setFrame(NSRect::ZERO);
        }
        aside(&v);
    }
    emit(
        app,
        "browser-device",
        Device {
            id: id.into(),
            device: choice,
        },
    );
    Ok(())
}

/// An agent's tab answers its page's dialogs itself (OK, Cancel, no text) and logs each; false
/// for any other tab, whose dialogs the user answers.
pub(super) fn answered(
    delegate: &Delegate,
    web: &WKWebView,
    what: &str,
    message: &NSString,
) -> bool {
    let d = delegate.ivars();
    if !d.agent.get() {
        return false;
    }
    let answer = match what {
        "alert" => "OK",
        "confirm" => "Cancel",
        _ => "no text",
    };
    let ts = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0.0, |d| d.as_millis() as f64);
    let line = format!(
        "{what}({:?}) answered {answer} by itself",
        message.to_string()
    );
    d.console
        .borrow_mut()
        .note(&line, &url_text(unsafe { web.URL() }), ts);
    delegate.console_changed();
    true
}
