//! Popups of agents' tabs (window.open, target=_blank): each a tab of its own in the pane, its
//! view made from the configuration WebKit hands over, which keeps `window.opener` and the
//! opener's cookies, so a login or payment window can post back and close itself. A user's tab
//! still loads the popup in place (mod.rs `new_window`): its tabs come from the app page.

use super::{agent, url_text, view, Delegate, GONE};
use crate::browser::{loadable, notes, popups};
use objc2::rc::Retained;
use objc2::{DefinedClass, MainThreadOnly, Message};
use objc2_web_kit::{WKNavigationAction, WKPreferences, WKWebView, WKWebViewConfiguration};
use std::cell::RefCell;
use std::collections::HashSet;

thread_local! {
    /// WebKit's configuration for the popup `create` is making now: it carries the opener.
    static GIVEN: RefCell<Option<Retained<WKWebViewConfiguration>>> = const { RefCell::new(None) };
    /// The agents' tabs that are popups, which may close themselves.
    static POPUPS: RefCell<HashSet<String>> = RefCell::default();
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
    /// Loaded in the opener's own view, as before: a user's tab, or an address no page opens.
    InPlace,
}

/// The popup of `opener`'s page, made a tab of its pane and told in its next reply.
pub(super) fn open(
    opener: &Delegate,
    config: &WKWebViewConfiguration,
    action: &WKNavigationAction,
) -> Popup {
    let d = opener.ivars();
    let Some((pty, opener_name)) = agent::pane_of(&d.id) else {
        return Popup::InPlace;
    };
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
    // The opener's switches otherwise, which a tab asleep slows (agent.rs `throttle`).
    unsafe { config.setPreferences(&WKPreferences::new(opener.mtm())) };
    GIVEN.set(Some(config.retain()));
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

/// A page called window.close(): a popup's tab closes as `tab close` would, told in the pane's
/// next reply. WebKit asks it of any page with one entry in its history too, which stays.
pub(super) fn closed(delegate: &Delegate) {
    let d = delegate.ivars();
    if !POPUPS.with_borrow(|p| p.contains(&d.id)) {
        return;
    }
    let Some((pty, name)) = agent::pane_of(&d.id) else {
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
}
