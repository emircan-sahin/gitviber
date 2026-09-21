//! Keeps the app window on the app. Nothing in it navigates on purpose, but a link's native
//! "Open Link" or a middle click would load a remote page with the app's IPC in reach.

use tauri::{plugin::TauriPlugin, Runtime, Url};

pub fn guard<R: Runtime>(dev_url: Option<Url>) -> TauriPlugin<R> {
    tauri::plugin::Builder::new("navigation-guard")
        .on_navigation(move |_, url| allowed(url, dev_url.as_ref()))
        .build()
}

/// The bundled app (tauri://localhost; http://tauri.localhost on Windows), the blob: URLs
/// media previews show in frames, about:blank, and the dev server in debug builds.
fn allowed(url: &Url, dev: Option<&Url>) -> bool {
    match url.scheme() {
        "tauri" => url.host_str() == Some("localhost"),
        "blob" | "about" => true,
        "http" | "https" => {
            url.host_str() == Some("tauri.localhost")
                || dev.is_some_and(|d| url.origin() == d.origin())
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_app_itself() {
        let u = |s: &str| Url::parse(s).unwrap();
        let dev = u("http://localhost:1420");
        for ok in [
            "tauri://localhost/index.html#x",
            "http://tauri.localhost/",
            "blob:tauri://localhost/5f3c",
            "about:blank",
            "http://localhost:1420/src/main.tsx",
        ] {
            assert!(allowed(&u(ok), Some(&dev)), "{ok}");
        }
        for no in [
            "https://github.com/a/b",
            "http://localhost:1421/",
            "tauri://evil.example/",
            "file:///etc/passwd",
            "data:text/html,hi",
        ] {
            assert!(!allowed(&u(no), Some(&dev)), "{no}");
        }
        assert!(!allowed(&u("http://localhost:1420/"), None));
    }
}
