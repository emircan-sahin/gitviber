//! Popups of agents' tabs (window.open, target=_blank): each a tab of its own in the pane, its
//! view made from the configuration WebKit hands over, which keeps `window.opener` and the
//! opener's cookies, so a login or payment window can post back and close itself. A user's tab
//! gets the same, as a tab of the workspace next to its opener (`open_for_user`): the app page
//! adds it on `browser-popup`.

use super::{agent, asked, create, emit, url_text, view, Delegate, Id, Look, GONE, VIEWS};
use crate::browser::{loadable, notes, popups};
use objc2::rc::Retained;
use objc2::{DefinedClass, MainThreadOnly, Message};
use objc2_foundation::NSUUID;
use objc2_web_kit::{WKNavigationAction, WKPreferences, WKWebView, WKWebViewConfiguration};
use serde::Serialize;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::time::Instant;
use tauri::Manager;

thread_local! {
    /// WebKit's configuration for the popup `create` is making now: it carries the opener.
    static GIVEN: RefCell<Option<Retained<WKWebViewConfiguration>>> = const { RefCell::new(None) };
    /// The agents' tabs that are popups, which may close themselves.
    static POPUPS: RefCell<HashSet<String>> = RefCell::default();
    /// When each user's tab opened its latest popups (popups.rs `allow`).
    static RECENT: RefCell<HashMap<String, Vec<Instant>>> = RefCell::default();
}

/// A user's tab made a popup (`id`), or was refused one (`browser-popup-blocked`, no id).
#[derive(Serialize, Clone)]
struct Popped<'a> {
    id: Option<&'a str>,
    opener: &'a str,
    url: &'a str,
}

/// The configuration a popup's view is made with, once; None for any other view.
pub(super) fn given() -> Option<Retained<WKWebViewConfiguration>> {
    GIVEN.take()
}

/// What becomes of a page's new window.
pub(super) enum Popup {
    /// A tab of the opener's pane, with this view.
    Tab(Retained<WKWebView>),
    /// None made, as a blocked popup: the opener stays where it is, its agent told why.
    Blocked,
    /// Loaded in the opener's own view: an address no page opens.
    InPlace,
}

/// The popup of `opener`'s page, made a tab of its pane and told in its next reply.
pub(super) fn open(
    opener: &Delegate,
    config: &WKWebViewConfiguration,
    action: &WKNavigationAction,
) -> Popup {
    let d = opener.ivars();
    let url = url_text(unsafe { action.request().URL() });
    // window.open() with no address: blank, for its opener to write or send somewhere.
    let url = if url.is_empty() {
        "about:blank".into()
    } else {
        url
    };
    if !loadable(&url) {
        return Popup::InPlace;
    }
    let Some((pty, opener_name)) = agent::pane_of(&d.id) else {
        return open_for_user(opener, config, &url);
    };
    give(opener, config);
    let made = agent::adopt(&d.app, &d.id, &url);
    // Left when `create` failed before taking it.
    GIVEN.take();
    let opener_tab = Some((d.id.as_str(), opener_name.as_str()));
    // Nothing of a page opened for the user is told to its agents.
    let told = !agent::strict(&d.id);
    let view = made.and_then(|(id, name)| {
        POPUPS.with_borrow_mut(|p| p.insert(id.clone()));
        if told {
            notes::note(pty, opener_tab, popups::opened(&name, &url));
        }
        view(&id).ok_or_else(|| GONE.to_string())
    });
    match view {
        Ok(v) => Popup::Tab(Retained::into_super(v.web)),
        Err(why) => {
            if told {
                notes::note(pty, opener_tab, popups::blocked(&why));
            }
            Popup::Blocked
        }
    }
}

/// WebKit's configuration for the popup `create` makes next. The opener's switches otherwise,
/// which a tab asleep slows (agent.rs `throttle`).
fn give(opener: &Delegate, config: &WKWebViewConfiguration) {
    unsafe { config.setPreferences(&WKPreferences::new(opener.mtm())) };
    GIVEN.set(Some(config.retain()));
}

/// A popup of a user's tab: a view of the opener's worktree, made at once as WebKit needs, and
/// a tab the app page puts next to the opener's. Refused past a burst of them.
fn open_for_user(opener: &Delegate, config: &WKWebViewConfiguration, url: &str) -> Popup {
    let d = opener.ivars();
    let (Some(root), Some(window)) = (
        VIEWS.with_borrow(|r| r.root_of(&d.id).map(String::from)),
        d.app.get_webview_window("main"),
    ) else {
        return Popup::Blocked;
    };
    let allowed = RECENT
        .with_borrow_mut(|r| popups::allow(r.entry(d.id.clone()).or_default(), Instant::now()));
    if !allowed {
        emit(
            &d.app,
            "browser-popup-blocked",
            Popped {
                id: None,
                opener: &d.id,
                url,
            },
        );
        return Popup::Blocked;
    }
    let id = NSUUID::new().UUIDString().to_string().to_lowercase();
    let look = Look {
        dark: asked(&d.id).dark,
        ..Look::default()
    };
    give(opener, config);
    let made = create(&window, &id, &root, url, &look, false);
    GIVEN.take();
    match made.and_then(|_| view(&id).ok_or_else(|| GONE.to_string())) {
        Ok(v) => {
            POPUPS.with_borrow_mut(|p| p.insert(id.clone()));
            emit(
                &d.app,
                "browser-popup",
                Popped {
                    id: Some(&id),
                    opener: &d.id,
                    url,
                },
            );
            Popup::Tab(Retained::into_super(v.web))
        }
        Err(_) => Popup::Blocked,
    }
}

/// A page called window.close(): a popup's tab closes as `tab close` would, told in the pane's
/// next reply. WebKit asks it of any page with one entry in its history too, which stays.
pub(super) fn closed(delegate: &Delegate) {
    let d = delegate.ivars();
    if !POPUPS.with_borrow(|p| p.contains(&d.id)) {
        return;
    }
    let Some((pty, name)) = agent::pane_of(&d.id) else {
        // A user's popup: the app page closes its tab as it would for the user.
        emit(&d.app, "browser-closed", Id { id: &d.id });
        return;
    };
    if !agent::strict(&d.id) {
        notes::note(pty, None, popups::closed(&name));
    }
    crate::browser::agent::close_soon(&d.app, d.id.clone());
}

/// The tab closed; None: every tab.
pub(super) fn forget(id: Option<&str>) {
    POPUPS.with_borrow_mut(|p| match id {
        Some(id) => drop(p.remove(id)),
        None => p.clear(),
    });
    RECENT.with_borrow_mut(|r| match id {
        Some(id) => drop(r.remove(id)),
        None => r.clear(),
    });
}
