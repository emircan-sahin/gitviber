//! The lite browser: each browser tab (src/features/browser) gets a WKWebView of its own, laid
//! over the code view where the tab's page leaves room for it. Not a Tauri webview: a page there
//! has no IPC script to reach the app's commands, its own website data, and only web URLs load.
//! macOS only for now; elsewhere every call fails and the page says so.
// Elsewhere only the stubs below are used, until Linux gets its own page.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

mod keys;
#[cfg(target_os = "macos")]
mod macos;

pub use keys::set_app_keys;
#[cfg(target_os = "macos")]
pub use macos::{close, close_all, close_root, create, focus, go, hide, navigate, place, snapshot};
#[cfg(not(target_os = "macos"))]
pub use other::{close, close_all, close_root, create, focus, go, hide, navigate, place, snapshot};

use serde::{Deserialize, Serialize};
use tauri::Url;

/// What the tab's address bar and buttons show; sent as `browser-state` while it changes.
#[derive(Serialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PageState {
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

/// A host as a URL and a certificate challenge both name it: IPv6 without brackets, lowercase.
pub fn host_key(host: &str) -> String {
    host.trim_start_matches('[')
        .trim_end_matches(']')
        .to_ascii_lowercase()
}

/// This machine, where a dev server's self-signed certificate is let through.
pub fn is_loopback(host: &str) -> bool {
    let host = host_key(host);
    host == "localhost"
        || host.ends_with(".localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

/// A worktree's folder as the views are kept by it: the page and git may spell one folder two
/// ways (a symlink, a trailing slash). Resolved while the folder is still there.
pub fn root_key(path: &str) -> String {
    std::fs::canonicalize(path).map_or_else(
        |_| path.trim_end_matches('/').to_string(),
        |p| p.to_string_lossy().into_owned(),
    )
}

/// An open view, with the tab and the worktree it belongs to.
struct Entry<V> {
    id: String,
    root: String,
    view: V,
}

/// The open views.
pub struct Registry<V> {
    views: Vec<Entry<V>>,
}

impl<V> Registry<V> {
    pub const fn new() -> Self {
        Self { views: Vec::new() }
    }

    pub fn get(&self, id: &str) -> Option<&V> {
        self.views.iter().find(|e| e.id == id).map(|e| &e.view)
    }

    pub fn insert(&mut self, id: &str, root: &str, view: V) {
        self.views.push(Entry {
            id: id.into(),
            root: root.into(),
            view,
        });
    }

    pub fn remove(&mut self, id: &str) -> Option<V> {
        let at = self.views.iter().position(|e| e.id == id)?;
        Some(self.views.remove(at).view)
    }

    /// Every view of a worktree, out of the registry.
    pub fn remove_root(&mut self, root: &str) -> Vec<V> {
        let (gone, kept) = std::mem::take(&mut self.views)
            .into_iter()
            .partition(|e| e.root == root);
        self.views = kept;
        gone.into_iter().map(|e| e.view).collect()
    }

    pub fn take_all(&mut self) -> Vec<V> {
        std::mem::take(&mut self.views)
            .into_iter()
            .map(|e| e.view)
            .collect()
    }
}

#[cfg(not(target_os = "macos"))]
mod other {
    use super::{Go, PageState, Rect};
    use crate::state::Res;

    const UNSUPPORTED: &str = "The browser tab needs macOS for now.";

    pub fn create(_: &tauri::WebviewWindow, _: &str, _: &str, _: &str) -> Res<PageState> {
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
    fn hosts_and_folders_are_named_one_way() {
        assert_eq!(host_key("[::1]"), "::1");
        assert_eq!(host_key("LocalHost"), "localhost");
        // Gone already: as written, less a trailing slash.
        assert_eq!(root_key("/no/such/worktree/"), "/no/such/worktree");
        let here = std::env::temp_dir();
        let resolved = std::fs::canonicalize(&here).unwrap();
        assert_eq!(
            root_key(&format!("{}/", here.display())),
            resolved.to_string_lossy()
        );
    }

    #[test]
    fn a_worktree_closes_with_its_views() {
        let mut r = Registry::new();
        r.insert("a", "/w/one", 1);
        r.insert("b", "/w/two", 2);
        r.insert("c", "/w/one", 3);
        assert_eq!(r.remove_root("/w/one"), [1, 3]);
        assert_eq!(r.get("a"), None);
        assert_eq!(r.get("b"), Some(&2));
        assert_eq!(r.remove("b"), Some(2));
        assert_eq!(r.remove("b"), None);
        r.insert("d", "/w/two", 4);
        assert_eq!(r.take_all(), [4]);
        assert_eq!(r.get("d"), None);
    }

    /// A cheap generator, so the fuzz below is the same on every run.
    fn lcg(seed: &mut u64) -> usize {
        *seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (*seed >> 33) as usize
    }

    #[test]
    fn no_spelling_of_a_scheme_gets_a_page_past_the_policy() {
        for no in [
            "JavaScript:alert(1)",
            "JAVASCRIPT:alert(1)",
            " javascript:alert(1)",
            "javascript://example.com/%0aalert(1)",
            "vbscript:msgbox(1)",
            "FILE:///etc/hosts",
            "file:/etc/hosts",
            "file://localhost/etc/hosts",
            "view-source:http://example.com/",
            "about:BLANK",
            "about:blank#x",
            "about:blank?x",
            "about:",
            "applewebdata://x/",
            "webkit-fake-url://x/",
            "x-apple.systempreferences:com.apple.preference",
            "vscode://file/tmp/x",
            "tauri://localhost/",
            "TAURI://localhost/",
            "ipc://localhost/",
            "asset://localhost/x",
            "ftp://example.com/",
            "ws://localhost:3000/",
            "wss://localhost:3000/",
            "",
            "http://",
            "https://",
            "http://[::1",
        ] {
            assert_eq!(policy(no, true, true), Policy::Cancel, "{no}");
            assert!(!loadable(no), "{no}");
        }
        for ok in [
            "HTTP://EXAMPLE.COM/",
            "https://[::1]:8443/",
            "http://user:pw@localhost:3000/",
            "http:example.com",
        ] {
            assert!(loadable(ok), "{ok}");
        }
        // A frame may hold what its page builds, never script or files.
        for no in [
            "javascript:1",
            "file:///tmp/x",
            "tauri://localhost/",
            "mailto:a@example.com",
        ] {
            assert_ne!(policy(no, false, true), Policy::Allow, "{no}");
        }
        assert_eq!(policy("about:srcdoc", false, false), Policy::Allow);
        // Mail and phone leave from the page itself only, clicked.
        for (url, main, clicked) in [
            ("mailto:a@example.com", false, true),
            ("MAILTO:a@example.com", true, false),
            ("tel:+15550100", false, false),
        ] {
            assert_eq!(policy(url, main, clicked), Policy::Cancel, "{url}");
        }
        assert_eq!(policy("MAILTO:a@example.com", true, true), Policy::External);
    }

    #[test]
    fn whatever_the_policy_allows_in_a_page_is_http_https_or_the_blank_one() {
        let parts = [
            "http",
            "https",
            "javascript",
            "file",
            "data",
            "blob",
            "about",
            "mailto",
            "tauri",
            ":",
            "//",
            "/",
            "blank",
            "srcdoc",
            "localhost",
            "[::1]",
            "@",
            "%0a",
            "#",
            "?",
            "ü",
            " ",
            "\t",
            "\0",
            "A",
            "1",
        ];
        let mut seed = 42;
        for _ in 0..20_000 {
            let mut url = String::new();
            for _ in 0..=lcg(&mut seed) % 7 {
                url.push_str(parts[lcg(&mut seed) % parts.len()]);
            }
            let main = lcg(&mut seed).is_multiple_of(2);
            let clicked = lcg(&mut seed).is_multiple_of(2);
            let got = policy(&url, main, clicked);
            let Ok(parsed) = Url::parse(&url) else {
                assert_eq!(got, Policy::Cancel, "{url:?}");
                continue;
            };
            match got {
                Policy::Allow if main => assert!(
                    matches!(parsed.scheme(), "http" | "https") || parsed.as_str() == "about:blank",
                    "{url:?}"
                ),
                Policy::Allow => assert!(
                    matches!(
                        parsed.scheme(),
                        "http" | "https" | "about" | "data" | "blob"
                    ),
                    "{url:?}"
                ),
                Policy::External => assert!(
                    main && clicked && matches!(parsed.scheme(), "mailto" | "tel" | "sms"),
                    "{url:?}"
                ),
                Policy::Cancel => {}
            }
        }
    }

    #[test]
    fn only_this_machine_counts_as_loopback() {
        for ok in [
            "LOCALHOST",
            "Foo.LocalHost",
            "a.b.localhost",
            "127.0.0.1",
            "127.255.255.254",
            "[::1]",
            "0:0:0:0:0:0:0:1",
        ] {
            assert!(is_loopback(ok), "{ok}");
        }
        for no in [
            "localhost.evil.example",
            "localhost.example.com",
            "127.0.0.1.nip.io",
            "evil-localhost",
            "xlocalhost",
            "localhostx",
            "localhost.",
            "localhost:3000",
            "127.0.0.1:3000",
            ".localhost.example",
            "0.0.0.0",
            "::",
            "[::]",
            "::ffff:127.0.0.1",
            "2130706433",
            "0x7f.0.0.1",
            "127.1",
            "192.168.1.1",
            "fe80::1",
            " localhost",
            "localhost ",
            "",
        ] {
            assert!(!is_loopback(no), "{no}");
        }
        // Nothing on the internet ends up loopback by what it's prefixed with.
        let mut seed = 9;
        let tail = [
            "evil.example",
            "nip.io",
            "com",
            "localhost.example",
            "127.0.0.1.example",
        ];
        for _ in 0..2000 {
            let head = ["localhost", "127.0.0.1", "[::1]", "::1"][lcg(&mut seed) % 4];
            let sep = [".", "-", "", "_"][lcg(&mut seed) % 4];
            let host = format!("{head}{sep}{}", tail[lcg(&mut seed) % tail.len()]);
            assert!(!is_loopback(&host), "{host}");
        }
    }

    #[test]
    fn the_registry_matches_a_plain_model_through_random_opens_and_closes() {
        let mut r = Registry::new();
        let mut model: Vec<(String, String, u32)> = Vec::new();
        let mut seed = 3;
        for n in 0..5000u32 {
            let id = format!("t{}", lcg(&mut seed) % 40);
            let root = format!("/w/{}", lcg(&mut seed) % 4);
            match lcg(&mut seed) % 5 {
                // create() opens an id once: one already open stays as it is.
                0 | 1 => {
                    if r.get(&id).is_none() {
                        r.insert(&id, &root, n);
                        model.push((id, root, n));
                    }
                }
                2 => {
                    let at = model.iter().position(|v| v.0 == id);
                    assert_eq!(r.remove(&id), at.map(|at| model.remove(at).2));
                }
                3 => {
                    let gone: Vec<u32> =
                        model.iter().filter(|v| v.1 == root).map(|v| v.2).collect();
                    model.retain(|v| v.1 != root);
                    assert_eq!(r.remove_root(&root), gone);
                }
                _ => {}
            }
            for v in &model {
                assert_eq!(r.get(&v.0), Some(&v.2));
            }
        }
        assert_eq!(r.take_all(), model.iter().map(|v| v.2).collect::<Vec<_>>());
        assert_eq!(r.remove_root("/w/0"), Vec::<u32>::new());
    }

    #[test]
    fn closing_a_root_leaves_other_roots_with_the_same_prefix() {
        let mut r = Registry::new();
        r.insert("a", "/w/one", 1);
        r.insert("b", "/w/one-two", 2);
        r.insert("c", "/w/one/", 3);
        assert_eq!(r.remove_root("/w/one"), [1]);
        assert_eq!(r.get("b"), Some(&2));
        assert_eq!(r.get("c"), Some(&3));
        assert_eq!(r.remove_root("/w/missing"), Vec::<i32>::new());
    }
}
