//! The views `gitviber browser` drives (browser/agent.rs): each terminal pane's agent tab, made
//! here without its tab ever showing, kept drawing while its agents use it, and answering its
//! page's dialogs by itself.

use super::page_view::appearance;
use super::{
    apply, ask, asked, content_view, create, emit, main_thread, native, page_tools, panel,
    set_agent, url_text, view, Delegate, View, GONE, VIEWS,
};
use crate::browser::control::{
    self, origin, AgentScreen, Found, Listed, NewTab, Owner, Route, Routes, TabState, IDLE,
    MOST_TABS,
};
use crate::browser::Step;
use crate::browser::{epoch_ms, Look, PageState};
use crate::state::{AppState, Res};
use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, NSObject};
use objc2::{msg_send, sel, DefinedClass};
use objc2_foundation::{
    ns_string, NSDictionary, NSError, NSObjectNSDelayedPerforming, NSObjectProtocol, NSRect,
    NSSize, NSString, NSTimer, NSUUID,
};
use objc2_web_kit::{WKContentWorld, WKWebView, WKWebsiteDataStore};
use serde::Serialize;
use std::cell::{Cell, OnceCell, RefCell};
use std::collections::HashMap;
use std::ptr::NonNull;
use std::time::Instant;
use tauri::{AppHandle, Manager};

thread_local! {
    static ROUTES: RefCell<Routes> = RefCell::default();
    /// The timer that closes agents' tabs left unused, set for the soonest (`arm_sweep`), and
    /// when it's due.
    static SWEEP: RefCell<Option<(Instant, Retained<NSTimer>)>> = RefCell::default();
    static APP: OnceCell<AppHandle> = const { OnceCell::new() };
    /// Whether agents' tabs are muted (Settings → Browser), as they're made and live.
    static MUTED: Cell<bool> = const { Cell::new(true) };
}

/// A tab out of sight that never showed takes a page this size.
const SIZE: NSSize = NSSize::new(1280.0, 800.0);

#[derive(Serialize, Clone)]
struct Opened<'a> {
    id: &'a str,
    url: &'a str,
    root: &'a str,
    pty: u32,
    /// Opened for the user (`tab new --show`): theirs, and the workspace shows it.
    show: bool,
}

/// The tab named so, else the pane's default one, live and awake: the default made in the
/// background on the pane's first command (at `url`, if one) and again once the user closed it,
/// and either back from parking where it was. Its id, and how it was found.
pub fn agent_tab(
    app: &AppHandle,
    pty: u32,
    root: &str,
    name: Option<&str>,
    url: Option<&str>,
) -> Res<(String, Found)> {
    main_thread()?;
    let tab = match ROUTES.with_borrow_mut(|r| r.tab(pty, name, is_open))? {
        // Opened for the user: no command of the agents' goes to it.
        Some(route) if strict(&route.id) => {
            let name = ROUTES.with_borrow(|r| r.strict(&route.id).map(String::from));
            return Err(control::shown_tab(&name.unwrap_or_default()));
        }
        Some(route) => live(app, &route, "about:blank").map(|made| {
            let found = if made { Found::Restored } else { Found::Open };
            (route.id, found)
        }),
        None => {
            let new = NewTab {
                name: None,
                default: true,
                shared: false,
                idle: IDLE,
                show: false,
            };
            new_tab(app, pty, root, new, url).map(|(id, _)| (id, Found::Made))
        }
    };
    arm_sweep();
    tab
}

/// Another tab of the pane's, as `new` asks, at `url`: its id and name.
pub fn agent_new_tab(
    app: &AppHandle,
    pty: u32,
    root: &str,
    new: NewTab,
    url: Option<&str>,
) -> Res<(String, String)> {
    main_thread()?;
    let tab = new_tab(app, pty, root, new, url);
    arm_sweep();
    tab
}

/// The pane's tabs, open or parked.
pub fn agent_tabs(pty: u32) -> Res<Vec<Listed>> {
    main_thread()?;
    let tabs = ROUTES.with_borrow_mut(|r| r.list(pty, is_open));
    Ok(tabs
        .into_iter()
        .map(|t| {
            let id = &t.route.id;
            let (state, url, title) = match view(id) {
                Some(v) => {
                    let page = v.delegate.state(&v.web);
                    let awake = VIEWS.with_borrow(|r| r.awake(id));
                    let state = if awake {
                        TabState::Awake
                    } else {
                        TabState::Sleeping
                    };
                    (state, page.url, page.title)
                }
                None => {
                    let parked = VIEWS.with_borrow(|r| r.parked(id).map(|p| p.url.clone()));
                    (TabState::Parked, parked.unwrap_or_default(), String::new())
                }
            };
            Listed {
                name: t.name,
                default: t.default,
                state,
                used_s: t.used.elapsed().as_secs(),
                idle_min: t.idle.as_secs() / 60,
                shared: t.shared,
                audio: view(id).is_some_and(|v| playing(&v.web)),
                origin: origin(&url),
                title,
                owner: t.owner,
            }
        })
        .collect())
}

/// The tab named so (the default one if None), not made when there's none.
pub fn agent_find_tab(pty: u32, name: Option<&str>) -> Res<String> {
    main_thread()?;
    ROUTES
        .with_borrow_mut(|r| r.tab(pty, name, is_open))?
        .map(|route| route.id)
        .ok_or_else(|| "No default tab open: name the one to close".into())
}

/// Closes the agent's tab `id`, its tab in the workspace too (`browser-closed`): its name.
pub fn agent_close_tab(app: &AppHandle, id: &str) -> Res<String> {
    main_thread()?;
    let name = ROUTES
        .with_borrow(|r| r.named(id).map(|t| t.name.clone()))
        .ok_or(GONE)?;
    super::close(id);
    emit(app, "browser-closed", super::Id { id });
    Ok(name)
}

#[derive(Serialize, Clone)]
struct Owned<'a> {
    id: &'a str,
    user: bool,
}

#[derive(Serialize, Clone)]
struct Released<'a> {
    ids: Vec<&'a str>,
}

/// The user's own press or key in an agent's tab: theirs until they give it back, its agents'
/// commands that act refused meanwhile (`users`), and the tab's Give back button showing
/// (`browser-owner`).
pub(super) fn user_took(id: &str) {
    if ROUTES.with_borrow_mut(|r| r.take(id)).is_some() {
        if let Some(v) = view(id) {
            // Keys its agents held would stay down in the page, and keep the tab awake.
            native::release_keys(&v);
            mute(&v.web, false);
        }
        if let Some(app) = APP.with(|a| a.get().cloned()) {
            emit(&app, "browser-owner", Owned { id, user: true });
        }
    }
}

/// Tab `id` back to its agents from the user (`tab release`, or the tab's button): its name, or
/// None when it was theirs already.
pub fn agent_give_back(app: &AppHandle, id: &str) -> Res<Option<String>> {
    main_thread()?;
    let name = ROUTES.with_borrow_mut(|r| r.give_back(id));
    if name.is_some() {
        if let Some(v) = view(id).filter(|_| muted(id)) {
            mute(&v.web, true);
        }
        emit(app, "browser-owner", Owned { id, user: false });
    }
    Ok(name)
}

/// Tab `id`'s name while it is the user's alone: opened for them, and not given back.
pub fn agent_shown(id: &str) -> Res<Option<String>> {
    main_thread()?;
    Ok(ROUTES.with_borrow(|r| r.strict(id).map(String::from)))
}

/// Whether tab `id` is the user's alone (`agent_shown`).
pub(super) fn strict(id: &str) -> bool {
    ROUTES.with_borrow(|r| r.strict(id).is_some())
}

/// Tab `id`'s name while the user has it.
pub fn agent_users(id: &str) -> Res<Option<String>> {
    main_thread()?;
    Ok(users(id))
}

pub(super) fn users(id: &str) -> Option<String> {
    ROUTES.with_borrow(|r| r.users(id).map(String::from))
}

/// A step of an agent's command on tab `id`, refused once the user has taken it.
pub(super) fn yours(id: &str) -> Res<()> {
    match users(id) {
        Some(name) => Err(control::interrupted(&name)),
        None => Ok(()),
    }
}

/// The tab closed, by the user or with its worktree: no agent drives it again, even reopened.
pub(super) fn forget_tab(id: &str) {
    ROUTES.with_borrow_mut(|r| r.close(id));
    super::downloads::forget(id);
    super::user_downloads::forget(id);
    super::popups::forget(Some(id));
}

/// Whether the tab is still there for its agents: live, or parked to load again.
fn is_open(id: &str) -> bool {
    VIEWS.with_borrow(|r| r.get(id).is_some() || r.parked(id).is_some())
}

/// A tab made for the pane, which the workspace hears as `browser-open` and adds without
/// showing: its id and name. Unless `shared`, its cookies and storage are its own, in memory
/// and kept while it parks, so two tabs can be two accounts.
fn new_tab(
    app: &AppHandle,
    pty: u32,
    root: &str,
    new: NewTab,
    url: Option<&str>,
) -> Res<(String, String)> {
    let mtm = main_thread()?;
    APP.with(|a| {
        a.get_or_init(|| app.clone());
    });
    // A command that passed its pane's check as the pane closed makes no tab for it.
    if app.state::<AppState>().ptys.pane(pty).is_none() {
        return Err("This terminal isn't one of GitViber's open panes.".into());
    }
    let route = Route {
        id: NSUUID::new().UUIDString().to_string().to_lowercase(),
        root: root.into(),
    };
    let (shared, show) = (new.shared, new.show);
    if show {
        ROUTES.with_borrow_mut(|r| r.may_show(pty, is_open, Instant::now()))?;
    }
    // An agent's tab before its view is made, so its first page loads as one (the user's, for
    // one opened to show).
    let name = ROUTES.with_borrow_mut(|r| r.add(pty, new, route.clone(), is_open))?;
    if !shared {
        let store = unsafe { WKWebsiteDataStore::nonPersistentDataStore(mtm) };
        ask(&route.id, |a| a.store = Some(store));
    }
    let url = url.unwrap_or("about:blank");
    if let Err(e) = live(app, &route, url) {
        forget_tab(&route.id);
        super::forget(&route.id);
        return Err(e);
    }
    let opened = Opened {
        id: &route.id,
        url,
        root: &route.root,
        pty,
        show,
    };
    emit(app, "browser-open", opened);
    if show {
        emit(
            app,
            "browser-owner",
            Owned {
                id: &route.id,
                user: true,
            },
        );
    }
    Ok((route.id, name))
}

/// The tab's view made, at `url`, when it has none (new, or parked), and awake: whether it was
/// made.
fn live(app: &AppHandle, route: &Route, url: &str) -> Res<bool> {
    let made = view(&route.id).is_none();
    if made {
        let window = app
            .get_webview_window("main")
            .ok_or("The window isn't ready.")?;
        let was = asked(&route.id);
        let look = Look {
            touch: was.screen.as_ref().is_some_and(|s| s.touch),
            ua: was.screen.and_then(|s| s.ua),
            zoom: None,
            dark: was.dark,
        };
        create(&window, &route.id, &route.root, url, &look, true)?;
    }
    let v = view(&route.id).ok_or(GONE)?;
    let woke = if made {
        Some(Step::Offscreen)
    } else {
        VIEWS.with_borrow_mut(|r| r.wake(&route.id))
    };
    if let Some(step) = woke {
        throttle(&v.web, false);
        apply(&v, step);
    }
    if !running(&v) {
        restart_idle(&v);
    }
    VIEWS.with_borrow_mut(|r| r.touch(&route.id));
    sleep_past_cap(&route.id);
    Ok(made)
}

/// Past the cap (Settings → Browser), the tabs drawn out of sight longest unused sleep: each
/// panel is a window's backing store, and its page runs at full rate. One a command runs on,
/// one holding a key, or `keep` (the one a command is about to), is passed over for the next.
pub(super) fn sleep_past_cap(keep: &str) {
    let past = VIEWS.with_borrow(|r| r.past_cap(keep, |v| busy(v) || holding(v)));
    for v in past.iter().filter_map(|id| view(id)) {
        stop_idle(&v);
        sleep(&v);
    }
}

/// Whether tab `id`'s downloads are the user's (Downloads, under their eye): any tab but an
/// agent's, and one of an agent's the user took. A tab opened for the user (`--show`) is not
/// among them: its page is an agent's choice, so it may not put a file there (downloads.rs).
pub(super) fn saves_for_user(id: &str) -> bool {
    !routed(id) || (users(id).is_some() && !strict(id))
}

/// Whether tab `id` is a pane's agent's.
pub(super) fn routed(id: &str) -> bool {
    ROUTES.with_borrow(|r| r.named(id).is_some())
}

/// The app page reloaded: its tabs come back as the user's. Their views all go with it
/// (close_all), so none stays a user's tab with network.js on it.
pub(super) fn forget_all() {
    ROUTES.with_borrow_mut(Routes::clear);
    super::downloads::forget_all();
    super::user_downloads::forget_all();
    super::popups::forget(None);
    super::ASKED.with_borrow_mut(HashMap::clear);
}

/// The pane closed: its tabs are the user's from now on, asleep as theirs are, without the
/// network log the agents read.
pub fn agent_pane_closed(pty: u32) {
    if main_thread().is_err() {
        return;
    }
    crate::browser::notes::forget(pty);
    let released = ROUTES.with_borrow_mut(|r| r.release(pty));
    for route in &released {
        if let Some(v) = view(&route.id) {
            native::release_keys(&v);
            page_tools::guard_clipboard(&v.web, false);
            page_tools::release_network(&v);
            mute(&v.web, false);
            stop_idle(&v);
            sleep(&v);
        }
    }
    // The workspace stops hiding them, as theirs now.
    if let (false, Some(app)) = (released.is_empty(), APP.with(|a| a.get().cloned())) {
        let ids: Vec<&str> = released.iter().map(|r| r.id.as_str()).collect();
        emit(&app, "browser-released", Released { ids });
    }
}

/// `command` running on the tab (`on`) or done: meanwhile its page's dialogs answer themselves
/// and it doesn't idle. Each command starts and ends once however often it asks, so one that
/// timed out can't leave the tab answering.
pub fn agent_running(id: &str, command: u64, on: bool) {
    let (Ok(_), Some(v)) = (main_thread(), view(id)) else {
        return;
    };
    let mut running = v.delegate.ivars().running.borrow_mut();
    let was_idle = running.is_empty();
    if on {
        running.insert(command);
    } else {
        running.remove(&command);
    }
    let idle = running.is_empty();
    drop(running);
    // A command guards the page's clipboard again, though the user acted on it since
    // (scripts/clipboard.js); it stays guarded after. The panel's key may change with it.
    if was_idle && !idle && routed(id) && users(id).is_none() {
        page_tools::guard_clipboard(&v.web, true);
    }
    panel::settle(&v);
    if idle {
        v.web.ivars().menus.set(0);
        restart_idle(&v);
    } else {
        stop_idle(&v);
    }
}

/// Back to a hidden tab's ways: throttled, and out of sight parking in time.
fn sleep(v: &View) {
    if let Some(step) = VIEWS.with_borrow_mut(|r| r.sleep(&v.delegate.ivars().id)) {
        throttle(&v.web, true);
        apply(v, step);
    }
}

/// From now, the time the agent's tab may go without a command before it sleeps (gvIdleDue);
/// none for one that never idles.
fn restart_idle(v: &View) {
    stop_idle(v);
    let idle = ROUTES.with_borrow(|r| r.named(&v.delegate.ivars().id).map(|t| t.idle));
    if let Some(idle) = idle.filter(|idle| !idle.is_zero()) {
        unsafe {
            v.delegate.performSelector_withObject_afterDelay(
                sel!(gvIdleDue),
                None,
                idle.as_secs_f64(),
            )
        };
    }
}

fn stop_idle(v: &View) {
    unsafe {
        NSObject::cancelPreviousPerformRequestsWithTarget_selector_object(
            &v.delegate,
            sel!(gvIdleDue),
            None,
        )
    };
}

/// The one timer for every agent tab's close deadline (control.rs `Routes::due`), apart from
/// the views, so a tab that parked closes too. Set again only for a sooner one: one set for
/// later than due finds nothing to close, and sets itself again.
fn arm_sweep() {
    let Some(due) = ROUTES.with_borrow(Routes::due) else {
        return;
    };
    SWEEP.with_borrow_mut(|armed| {
        if armed.as_ref().is_some_and(|a| a.0 <= due) {
            return;
        }
        if let Some((_, timer)) = armed.take() {
            timer.invalidate();
        }
        let after = due.saturating_duration_since(Instant::now());
        let sweep = RcBlock::new(|_: NonNull<NSTimer>| sweep());
        let timer = unsafe {
            NSTimer::scheduledTimerWithTimeInterval_repeats_block(
                after.as_secs_f64(),
                false,
                &sweep,
            )
        };
        *armed = Some((due, timer));
    });
}

/// Closes the agents' tabs unused past their time, as `tab close` would, never through a command
/// running on one; one the user is looking at gets more time instead.
fn sweep() {
    SWEEP.with_borrow_mut(|armed| armed.take());
    let now = Instant::now();
    let app = APP.with(|a| a.get().cloned());
    for id in ROUTES.with_borrow(|r| r.expired(now)) {
        let busy = view(&id).is_some_and(|v| busy(&v));
        if busy || VIEWS.with_borrow(|r| r.shown(&id)) {
            ROUTES.with_borrow_mut(|r| r.postpone(&id, now));
        } else if let Some(app) = &app {
            let _ = agent_close_tab(app, &id);
        }
    }
    arm_sweep();
}

fn running(v: &View) -> bool {
    !v.delegate.ivars().running.borrow().is_empty()
}

/// Its idle timer is up: asleep, unless a command runs on it or it holds a key. Its next command
/// to end sets the timer again.
pub(super) fn idle(id: &str) {
    if let Some(v) = view(id).filter(|v| !busy(v) && !holding(v)) {
        sleep(&v);
    }
}

/// An agent holds a key down on it (`key down`): asleep, its page would lose its focus and its
/// frames with the key still down, and the key couldn't go down again until it was let go.
fn holding(v: &View) -> bool {
    v.web.ivars().held.borrow().any()
}

/// A command runs on it, or its page reloads after its process ended: then it's neither slept
/// nor moved out of its panel (crashes.rs).
fn busy(v: &View) -> bool {
    running(v) || super::crashes::recovering(v)
}

/// How the page's dialogs are answered while an agent's command runs on it.
pub fn agent_dialogs(id: &str, accept: bool) -> Res<()> {
    main_thread()?;
    ask(id, |a| a.accept_dialogs = accept);
    Ok(())
}

/// The page light, dark or as the app (None), and the tab's own toggle the same
/// (`browser-appearance`).
pub fn agent_appearance(app: &AppHandle, id: &str, dark: Option<bool>) -> Res<()> {
    appearance(id, dark)?;
    emit(app, "browser-appearance", Shown { id, dark });
    Ok(())
}

#[derive(Serialize, Clone)]
struct Shown<'a> {
    id: &'a str,
    dark: Option<bool>,
}

/// Drawn in its panel (panel.rs), as the agent's device or the size it last had.
pub(super) fn offscreen(v: &View) {
    let screen = asked(&v.delegate.ivars().id).screen;
    let size = match &screen {
        Some(s) => NSSize::new(s.w, s.h),
        None => {
            let frame = v.host.frame().size;
            let window = content_view(&v.delegate.ivars().app).map(|c| c.bounds().size);
            if frame.width >= 1.0 && frame.height >= 1.0 {
                frame
            } else {
                window.filter(|s| s.width >= 1.0).unwrap_or(SIZE)
            }
        }
    };
    panel::hold(v, size);
    if let Some(s) = screen {
        // Laid out at the device's own width; device mode sets its zoom again when it shows.
        unsafe { v.web.setPageZoom(1.0) };
        v.delegate.ivars().screen.set(None);
        if super::can_set_dpr(&v.web) {
            let dpr = s.dpr.unwrap_or(0.0);
            let _: () = unsafe { msg_send![&*v.web, _setOverrideDeviceScaleFactor: dpr] };
        }
    }
}

/// WebKit's slowing of pages out of sight, and of a window covered by others, off while an
/// agent's tab is in use; private, so only where WebKit still has it.
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
    super::crashes::check(&v)?;
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

// Both are WebKit's private API, so each is guarded by respondsToSelector.

/// Whether tab `id`'s page is playing sound, muted or not.
pub fn agent_playing(id: &str) -> Res<bool> {
    main_thread()?;
    let v = view(id).ok_or(GONE)?;
    Ok(playing(&v.web))
}

/// Still so while the page is muted: it plays on.
fn playing(web: &WKWebView) -> bool {
    web.respondsToSelector(sel!(_isPlayingAudio)) && {
        let on: Bool = unsafe { msg_send![web, _isPlayingAudio] };
        on.as_bool()
    }
}

/// _WKMediaMutedState's audio bit.
const AUDIO_MUTED: usize = 1;

/// The page muted at WebKit, not by its scripts: its media plays on and its audio contexts run,
/// only nothing reaches the speakers.
fn mute(web: &WKWebView, on: bool) {
    if web.respondsToSelector(sel!(_setPageMuted:)) {
        let state = if on { AUDIO_MUTED } else { 0 };
        let _: () = unsafe { msg_send![web, _setPageMuted: state] };
    }
}

/// Whether tab `id` is muted: an agent's, while its agents have it (Settings → Browser).
fn muted(id: &str) -> bool {
    MUTED.get() && routed(id) && users(id).is_none()
}

/// A view made for tab `id`, muted if it should be (a tab the user took stays theirs to hear,
/// parked and back).
pub(super) fn mute_new(web: &WKWebView, id: &str) {
    if muted(id) {
        mute(web, true);
    }
}

/// Settings → Browser: agents' tabs muted or not, now and as they're made.
pub(super) fn set_muted(on: bool) {
    MUTED.set(on);
    let views: Vec<View> = VIEWS.with_borrow(|r| r.live().into_iter().cloned().collect());
    for v in views.iter().filter(|v| routed(&v.delegate.ivars().id)) {
        mute(&v.web, muted(&v.delegate.ivars().id));
    }
}

/// The page as the address bar has it, and whether its console is kept (Settings → Browser).
pub fn agent_page(id: &str) -> Res<(PageState, bool)> {
    main_thread()?;
    let v = view(id).ok_or(GONE)?;
    super::crashes::check(&v)?;
    Ok((v.delegate.state(&v.web), page_tools::capturing()))
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
    let touch = screen.as_ref().is_some_and(|s| s.touch);
    set_agent(id, screen.as_ref().and_then(|s| s.ua.as_deref()), touch)?;
    let off = screen.is_none();
    ask(id, |a| a.screen = screen);
    if off && super::can_set_dpr(&v.web) {
        let _: () = unsafe { msg_send![&*v.web, _setOverrideDeviceScaleFactor: 0.0f64] };
    }
    if VIEWS.with_borrow(|r| r.offscreen(id)) {
        if off {
            // Back to the window's size, not the device's it had.
            v.host.setFrame(NSRect::ZERO);
        }
        offscreen(&v);
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

/// While an agent's command runs on the tab, its page's dialogs answer themselves (`dialogs`:
/// dismissed unless accepted), each logged: whether it was accepted. None at any other time,
/// when the user answers them.
pub(super) fn answered(
    delegate: &Delegate,
    web: &WKWebView,
    what: &str,
    message: &NSString,
) -> Option<bool> {
    let d = delegate.ivars();
    if d.running.borrow().is_empty() {
        return None;
    }
    let accept = asked(&d.id).accept_dialogs || what == "alert";
    let answer = match (what, accept) {
        ("alert", _) => "OK",
        ("prompt", true) => "with its default text",
        (_, true) => "OK",
        (_, false) => "Cancel",
    };
    let ts = epoch_ms();
    let line = format!(
        "{what}({:?}) answered {answer} by itself",
        message.to_string()
    );
    d.console
        .borrow_mut()
        .note(&line, &url_text(unsafe { web.URL() }), ts);
    delegate.console_changed();
    Some(accept)
}

/// Agent tab `id`'s pane and name, for what's told of it (notes.rs).
pub(super) fn pane_of(id: &str) -> Option<(u32, String)> {
    ROUTES.with_borrow(|r| r.pane_of(id).map(|(pty, t)| (pty, t.name.clone())))
}

/// A popup of agent tab `opener`'s page (popups.rs), a tab of the same pane from now on, named
/// for it (`browser-open` as any other), and awake in its cookies, device and look: its id and
/// name. Its view is the one `create` makes from WebKit's configuration, which loads `url`.
pub(super) fn adopt(app: &AppHandle, opener: &str, url: &str) -> Res<(String, String)> {
    let (pty, parent) = ROUTES
        .with_borrow(|r| r.pane_of(opener).map(|(pty, t)| (pty, t.clone())))
        .ok_or(GONE)?;
    let open = ROUTES.with_borrow_mut(|r| r.list(pty, is_open));
    // Said as the popup's note puts it (popups.rs `blocked`): no advice to close one.
    if open.len() >= MOST_TABS {
        return Err(format!("This pane has {MOST_TABS} tabs"));
    }
    let name = crate::browser::popups::name(&parent.name, |n| open.iter().any(|t| t.name == n));
    let route = Route {
        id: NSUUID::new().UUIDString().to_string().to_lowercase(),
        root: parent.route.root.clone(),
    };
    let new = NewTab {
        name: Some(&name),
        default: false,
        shared: parent.shared,
        idle: parent.idle,
        // A page opened for the user opens its popups the same: theirs alone.
        show: parent.shown && parent.owner == Owner::User,
    };
    ROUTES.with_borrow_mut(|r| {
        r.add(pty, new, route.clone(), is_open)?;
        // Their tab's popup is theirs, but its agents may read it as any tab the user took.
        if parent.owner == Owner::User {
            r.take(&route.id);
        }
        Ok::<_, String>(())
    })?;
    let was = asked(opener);
    ask(&route.id, |a| *a = was);
    if let Err(e) = live(app, &route, url) {
        forget_tab(&route.id);
        super::forget(&route.id);
        return Err(e);
    }
    let opened = Opened {
        id: &route.id,
        url,
        root: &route.root,
        pty,
        show: false,
    };
    emit(app, "browser-open", opened);
    if parent.owner == Owner::User {
        emit(
            app,
            "browser-owner",
            Owned {
                id: &route.id,
                user: true,
            },
        );
    }
    arm_sweep();
    Ok((route.id, name))
}
