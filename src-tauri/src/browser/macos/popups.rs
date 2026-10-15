//! Popups of agents' tabs (window.open, target=_blank): each a tab of its own in the pane, its
//! view made from the configuration WebKit hands over, which keeps `window.opener` and the
//! opener's cookies, so a login or payment window can post back and close itself. A user's tab
//! still loads the popup in place (mod.rs `new_window`): its tabs come from the app page.

use super::{agent, url_text, view, Delegate};
use crate::browser::{loadable, notes, popups};
use objc2::rc::Retained;
use objc2::{DefinedClass, MainThreadOnly, Message};
use objc2_web_kit::{WKNavigationAction, WKPreferences, WKWebView, WKWebViewConfiguration};
use std::cell::RefCell;

thread_local! {
    /// WebKit's configuration for the popup `create` is making now: it carries the opener.
    static GIVEN: RefCell<Option<Retained<WKWebViewConfiguration>>> = const { RefCell::new(None) };
}

/// The configuration a popup's view is made with, once; None for any other view.
pub(super) fn given() -> Option<Retained<WKWebViewConfiguration>> {
    GIVEN.take()
}

/// The popup's view, made a tab of the opener's pane and told in its next reply; None for a
/// user's tab, an address no page may open, or when it couldn't be made.
pub(super) fn open(
    opener: &Delegate,
    config: &WKWebViewConfiguration,
    action: &WKNavigationAction,
) -> Option<Retained<WKWebView>> {
    let d = opener.ivars();
    if !agent::routed(&d.id) {
        return None;
    }
    let url = url_text(unsafe { action.request().URL() });
    // window.open() with no address: blank, for its opener to write or send somewhere.
    let url = if url.is_empty() {
        "about:blank".into()
    } else {
        url
    };
    if !loadable(&url) {
        return None;
    }
    // The opener's switches otherwise, which a tab asleep slows (agent.rs `throttle`).
    unsafe { config.setPreferences(&WKPreferences::new(opener.mtm())) };
    GIVEN.set(Some(config.retain()));
    let made = agent::adopt(&d.app, &d.id, &url);
    // Left when `create` failed before taking it.
    GIVEN.take();
    let (id, name) = made.ok()?;
    if let Some((pty, opener_name)) = agent::pane_of(&d.id) {
        let text = popups::opened(&name, &url);
        notes::note(pty, Some((&d.id, &opener_name)), text);
    }
    view(&id).map(|v| Retained::into_super(v.web))
}

/// The page called window.close(): an agent's tab closes as `tab close` would, told in the
/// pane's next reply. A user's does nothing, as before.
pub(super) fn closed(delegate: &Delegate) {
    let d = delegate.ivars();
    let Some((pty, name)) = agent::pane_of(&d.id) else {
        return;
    };
    notes::note(pty, None, popups::closed(&name));
    crate::browser::agent::close_soon(&d.app, d.id.clone());
}
