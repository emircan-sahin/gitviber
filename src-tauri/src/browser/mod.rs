//! The lite browser: each browser tab (src/features/browser) gets a WKWebView of its own, laid
//! over the code view where the tab's page leaves room for it. Not a Tauri webview: a page there
//! has no IPC script to reach the app's commands, its own website data, and only web URLs load.
//! macOS only for now; elsewhere every call fails and the page says so.
// Elsewhere only the stubs below are used, until Linux gets its own page.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

mod keys;
#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "macos")]
pub use macos::{
    close, close_all, close_root, create, focus, go, hide, list, navigate, place, snapshot,
};
#[cfg(not(target_os = "macos"))]
pub use other::{
    close, close_all, close_root, create, focus, go, hide, list, navigate, place, snapshot,
};

use serde::{Deserialize, Serialize};
use tauri::Url;

/// What the tab's address bar and buttons show; sent as `browser-state` while it changes.
#[derive(Serialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct State {
    pub id: String,
    pub url: String,
    pub title: String,
    pub loading: bool,
    /// 0 to 1, WebKit's estimate.
    pub progress: f64,
    pub can_back: bool,
    pub can_forward: bool,
    /// An https page on this machine whose self-signed certificate was let through.
    pub insecure: bool,
    /// The page that never loaded (no server on that port, say), shown by the app page instead.
    pub failed: Option<Failed>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Failed {
    pub url: String,
    pub message: String,
}

/// Where the view goes, in the app page's points from its top left.
#[derive(Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[derive(Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum Go {
    Back,
    Forward,
    Reload,
    /// Past the cache.
    HardReload,
    Stop,
}

#[derive(Debug, PartialEq)]
pub enum Policy {
    Allow,
    /// Handed to the app the system has for it (Mail for mailto:), the page staying put.
    External,
    Cancel,
}

/// What a page may load. Pages are web pages: no file:, no javascript: typed or linked, no app
/// scheme (tauri:, ipc:, asset:). A frame inside a page may also hold data:, blob: and srcdoc.
/// Mail and phone links leave for their app only when clicked, so a page can't open Mail itself.
pub fn policy(url: &str, main_frame: bool, clicked: bool) -> Policy {
    let Ok(url) = Url::parse(url) else {
        return Policy::Cancel;
    };
    match url.scheme() {
        "http" | "https" => Policy::Allow,
        "about" if !main_frame || url.as_str() == "about:blank" => Policy::Allow,
        "data" | "blob" if !main_frame => Policy::Allow,
        "mailto" | "tel" | "sms" if main_frame && clicked => Policy::External,
        _ => Policy::Cancel,
    }
}

/// What the address bar may load: a web page, or the blank one a new tab starts on.
pub fn loadable(url: &str) -> bool {
    policy(url, true, false) == Policy::Allow
}

/// This machine, where a dev server's self-signed certificate is let through.
pub fn is_loopback(host: &str) -> bool {
    let host = host.trim_start_matches('[').trim_end_matches(']');
    let host = host.to_ascii_lowercase();
    host == "localhost"
        || host.ends_with(".localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

/// The open views by id, each with the worktree it belongs to.
pub struct Registry<V> {
    views: Vec<(String, String, V)>,
}

impl<V> Registry<V> {
    pub const fn new() -> Self {
        Self { views: Vec::new() }
    }

    pub fn get(&self, id: &str) -> Option<&V> {
        self.views.iter().find(|v| v.0 == id).map(|v| &v.2)
    }

    pub fn insert(&mut self, id: &str, root: &str, view: V) {
        self.views.push((id.into(), root.into(), view));
    }

    pub fn remove(&mut self, id: &str) -> Option<V> {
        let at = self.views.iter().position(|v| v.0 == id)?;
        Some(self.views.remove(at).2)
    }

    /// Every view of a worktree, out of the registry.
    pub fn remove_root(&mut self, root: &str) -> Vec<V> {
        let (gone, kept) = std::mem::take(&mut self.views)
            .into_iter()
            .partition(|v| v.1 == root);
        self.views = kept;
        gone.into_iter().map(|v| v.2).collect()
    }

    pub fn take_all(&mut self) -> Vec<V> {
        std::mem::take(&mut self.views)
            .into_iter()
            .map(|v| v.2)
            .collect()
    }

    pub fn in_root<'a>(&'a self, root: &'a str) -> impl Iterator<Item = (&'a str, &'a V)> {
        self.views
            .iter()
            .filter(move |v| v.1 == root)
            .map(|v| (v.0.as_str(), &v.2))
    }
}

#[cfg(not(target_os = "macos"))]
mod other {
    use super::{Go, Rect, State};
    use crate::state::Res;

    const UNSUPPORTED: &str = "The browser tab needs macOS for now.";

    pub fn create(_: &tauri::WebviewWindow, _: &str, _: &str, _: &str) -> Res<State> {
        Err(UNSUPPORTED.into())
    }
    pub fn place(_: &str, _: Rect) -> Res<()> {
        Err(UNSUPPORTED.into())
    }
    pub fn hide(_: &str) {}
    pub fn close(_: &str) {}
    pub fn close_root(_: &str) {}
    pub fn close_all() {}
    pub fn navigate(_: &str, _: &str) -> Res<()> {
        Err(UNSUPPORTED.into())
    }
    pub fn go(_: &str, _: Go) -> Res<()> {
        Err(UNSUPPORTED.into())
    }
    pub fn focus(_: &str, _: bool) {}
    pub fn list(_: &str) -> Vec<State> {
        Vec::new()
    }
    pub fn snapshot(_: &tauri::AppHandle, _: String) -> Res<Option<String>> {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_web_pages_load() {
        for ok in [
            "http://localhost:5173/",
            "https://example.com/a?b#c",
            "about:blank",
        ] {
            assert_eq!(policy(ok, true, false), Policy::Allow, "{ok}");
            assert!(loadable(ok), "{ok}");
        }
        for no in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "data:text/html,hi",
            "blob:http://localhost:5173/5f3c",
            "tauri://localhost/index.html",
            "ipc://localhost/x",
            "asset://localhost/x",
            "about:srcdoc",
            "not a url",
        ] {
            assert_eq!(policy(no, true, true), Policy::Cancel, "{no}");
            assert!(!loadable(no), "{no}");
        }
        // Frames may hold what a page builds itself.
        for ok in [
            "data:text/html,hi",
            "blob:http://localhost/1",
            "about:srcdoc",
        ] {
            assert_eq!(policy(ok, false, false), Policy::Allow, "{ok}");
        }
        assert_eq!(policy("javascript:1", false, false), Policy::Cancel);
    }

    #[test]
    fn mail_and_phone_links_leave_only_when_clicked() {
        assert_eq!(policy("mailto:a@example.com", true, true), Policy::External);
        assert_eq!(policy("tel:+15550100", true, false), Policy::Cancel);
        assert_eq!(policy("sms:+15550100", false, true), Policy::Cancel);
        assert!(!loadable("mailto:a@example.com"));
    }

    #[test]
    fn loopback_hosts() {
        for ok in [
            "localhost",
            "LocalHost",
            "app.localhost",
            "127.0.0.1",
            "127.1.2.3",
            "::1",
            "[::1]",
        ] {
            assert!(is_loopback(ok), "{ok}");
        }
        for no in [
            "example.com",
            "localhost.example.com",
            "10.0.0.1",
            "::2",
            "",
        ] {
            assert!(!is_loopback(no), "{no}");
        }
    }

    #[test]
    fn a_worktree_closes_with_its_views() {
        let mut r = Registry::new();
        r.insert("a", "/w/one", 1);
        r.insert("b", "/w/two", 2);
        r.insert("c", "/w/one", 3);
        assert_eq!(
            r.in_root("/w/one").map(|v| v.0).collect::<Vec<_>>(),
            ["a", "c"]
        );
        assert_eq!(r.remove_root("/w/one"), [1, 3]);
        assert_eq!(r.get("a"), None);
        assert_eq!(r.get("b"), Some(&2));
        assert_eq!(r.remove("b"), Some(2));
        assert_eq!(r.remove("b"), None);
        r.insert("d", "/w/two", 4);
        assert_eq!(r.take_all(), [4]);
        assert_eq!(r.get("d"), None);
    }
}
