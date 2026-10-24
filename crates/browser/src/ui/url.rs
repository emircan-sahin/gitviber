//! What a browser tab's address bar makes of what's typed.

use url::Url;

/// A tab before its first address.
pub const BLANK: &str = "about:blank";

/// A web page's address as the URL parser writes it; None for anything else (javascript:,
/// file:, mailto:), for port 0, and for one with a user name, which only serves to dress up its
/// host (`localhost:3000@evil.example` is evil.example).
fn web_url(text: &str) -> Option<String> {
    let url = Url::parse(text).ok()?;
    let web = matches!(url.scheme(), "http" | "https");
    (web && url.username().is_empty() && url.password().is_none() && url.port() != Some(0))
        .then(|| url.to_string())
}

fn digits(s: &str, most: usize) -> bool {
    (1..=most).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_digit())
}

/// `:5173`, `5173`, `5173/docs?x#y`: the port and what follows it.
fn port_of(text: &str) -> Option<(&str, &str)> {
    let text = text.strip_prefix(':').unwrap_or(text);
    let end = text.find(['/', '?', '#']).unwrap_or(text.len());
    let (port, rest) = text.split_at(end);
    digits(port, 5).then_some((port, rest))
}

/// This machine (127.1 too, as ping takes it) and the LAN, which dev servers serve without a
/// certificate, with a port or not.
fn plain_host(host: &str) -> bool {
    let (name, port) = match host.rfind(':') {
        Some(at) if !host.starts_with('[') || host[..at].ends_with(']') => {
            (&host[..at], Some(&host[at + 1..]))
        }
        _ => (host, None),
    };
    if port.is_some_and(|p| !digits(p, 5)) {
        return false;
    }
    let lower = name.to_ascii_lowercase();
    let word = |s: &str| {
        !s.is_empty()
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    };
    let numbers = |s: &str, counts: &[usize]| {
        let parts: Vec<&str> = s.split('.').collect();
        counts.contains(&parts.len()) && parts.iter().all(|p| digits(p, 3))
    };
    lower == "localhost"
        || lower.strip_suffix(".localhost").is_some_and(word)
        || numbers(name, &[4])
        || name
            .strip_prefix("127.")
            .is_some_and(|rest| numbers(rest, &[1, 2]))
        || name
            .strip_prefix('[')
            .and_then(|n| n.strip_suffix(']'))
            .is_some_and(|ip| {
                !ip.is_empty()
                    && ip
                        .bytes()
                        .all(|b| b.is_ascii_hexdigit() || b == b':' || b == b'.')
            })
}

/// A scheme of its own (javascript:, mailto:), unlike a name with a port (example.dev:8443).
fn own_scheme(text: &str) -> bool {
    let Some(colon) = text.find(':') else {
        return false;
    };
    let scheme = &text[..colon];
    let mut bytes = scheme.bytes();
    let starts = bytes.next().is_some_and(|b| b.is_ascii_alphabetic());
    let rest = bytes.all(|b| b.is_ascii_alphanumeric() || b"_+.-".contains(&b));
    let then_digit = text[colon + 1..]
        .bytes()
        .next()
        .is_some_and(|b| b.is_ascii_digit());
    starts && rest && !then_digit
}

/// The address to load for what's typed: a port is this machine's (`5173`, `:5173/docs`), this
/// machine and the LAN load over http, a name with a dot over https. There's no search: a bare
/// word or text with spaces is no address (None), and only web pages load.
pub fn normalize_url(input: &str) -> Option<String> {
    let text = input.trim();
    if text.is_empty() || text.chars().any(char::is_whitespace) {
        return None;
    }
    if text == BLANK {
        return Some(BLANK.into());
    }
    if let Some((port, rest)) = port_of(text) {
        let port: u32 = port.parse().ok()?;
        let rest = if rest.is_empty() { "/" } else { rest };
        return (1..=65535)
            .contains(&port)
            .then(|| web_url(&format!("http://localhost:{port}{rest}")))
            .flatten();
    }
    let lower = text.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return web_url(text);
    }
    let host = text.split(['/', '?', '#']).next().unwrap_or_default();
    if plain_host(host) {
        return web_url(&format!("http://{text}"));
    }
    if own_scheme(text) {
        return None;
    }
    host.contains('.')
        .then(|| web_url(&format!("https://{text}")))
        .flatten()
}

/// What a stored tab may load again: a web page, or the blank one it started on.
pub fn is_page_url(url: &str) -> bool {
    url == BLANK || web_url(url).as_deref() == Some(url)
}

/// A port's page on this machine.
pub fn port_url(port: u16) -> String {
    format!("http://localhost:{port}/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_port_this_machine_and_the_lan_load_over_http_a_name_with_a_dot_over_https() {
        for (typed, url) in [
            ("3000", "http://localhost:3000/"),
            (":5173", "http://localhost:5173/"),
            ("  5173 ", "http://localhost:5173/"),
            ("localhost", "http://localhost/"),
            (
                "localhost:5173/docs?x=1#top",
                "http://localhost:5173/docs?x=1#top",
            ),
            ("app.localhost:3000", "http://app.localhost:3000/"),
            ("127.0.0.1:8080", "http://127.0.0.1:8080/"),
            ("192.168.1.20:3000", "http://192.168.1.20:3000/"),
            ("[::1]:4000", "http://[::1]:4000/"),
            ("example.dev", "https://example.dev/"),
            ("example.dev:8443/a", "https://example.dev:8443/a"),
            ("docs.example.com/guide", "https://docs.example.com/guide"),
            ("http://example.com", "http://example.com/"),
            ("HTTPS://Example.com/A", "https://example.com/A"),
            (BLANK, BLANK),
            ("LOCALHOST", "http://localhost/"),
            ("App.LocalHost:3000", "http://app.localhost:3000/"),
            ("[::1]", "http://[::1]/"),
            ("[::1]:3000/a?b#c", "http://[::1]:3000/a?b#c"),
            ("0.0.0.0:3000", "http://0.0.0.0:3000/"),
            ("10.0.0.1", "http://10.0.0.1/"),
            ("00080", "http://localhost/"),
            ("65535", "http://localhost:65535/"),
            ("\tlocalhost:5173\n", "http://localhost:5173/"),
            ("bücher.example", "https://xn--bcher-kva.example/"),
            ("Example.COM/Path", "https://example.com/Path"),
            ("example.com?q=1", "https://example.com/?q=1"),
            ("example.com#top", "https://example.com/#top"),
            ("HTTP://LOCALHOST:3000", "http://localhost:3000/"),
            (
                "http://example.com\\@evil.example",
                "http://example.com/@evil.example",
            ),
        ] {
            assert_eq!(normalize_url(typed).as_deref(), Some(url), "{typed:?}");
        }
    }

    #[test]
    fn no_search_and_nothing_but_web_pages_however_spelled() {
        for typed in [
            "",
            "   ",
            "react hooks",
            "foo",
            "0",
            "70000",
            "javascript:alert(1)",
            "file:///etc/passwd",
            "data:text/html,hi",
            "mailto:a@example.com",
            "tauri://localhost/",
            "ftp://example.com/",
            "about:srcdoc",
            "999.1.1.1",
            "http://",
            "JavaScript:alert(1)",
            "javascript://example.com/%0aalert(1)",
            "vbscript:msgbox(1)",
            "view-source:http://example.com",
            "blob:https://example.com/1",
            "data:,x",
            "file:/etc/hosts",
            "x-apple.systempreferences:com.apple.preference",
            "vscode://file/tmp/x",
            "ABOUT:BLANK",
            "about:blank#x",
            "http:",
            "https://",
            "http:///",
            "http:example.com",
            "//example.com",
            "/path",
            "?q",
            "#x",
            "::1",
            "example.com:abc",
            "example.com:8443:1",
            "localhost:",
            "localhost:99999",
            "1.2.3.4.5",
            "[fe80::1%en0]:3000",
            "127.0.0.1:8080/a b",
            "localhost:3000@evil.example",
        ] {
            assert_eq!(normalize_url(typed), None, "{typed:?}");
        }
    }

    #[test]
    fn a_stored_tabs_address_loads_again_only_when_its_a_web_page() {
        assert!(is_page_url("http://localhost:5173/"));
        assert!(is_page_url(BLANK));
        assert!(!is_page_url("javascript:alert(1)"));
        assert!(!is_page_url("file:///tmp/x.html"));
        assert!(!is_page_url("http://localhost:5173"));
    }
}
