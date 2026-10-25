//! What a popup of an agent's tab (macos/popups.rs) is called and how it's told, apart from
//! AppKit so it can be tested.

use std::time::{Duration, Instant};

/// A page opens this many popups in a burst (a user's tab: one per click or key, which a page
/// can repeat); the next ones wait for the window to pass.
const BURST: usize = 5;
const BURST_WINDOW: Duration = Duration::from_secs(10);

/// Whether a page may open one more popup at `now`, given those it opened lately; counts it.
pub fn allow(recent: &mut Vec<Instant>, now: Instant) -> bool {
    recent.retain(|&at| now.duration_since(at) < BURST_WINDOW);
    let ok = recent.len() < BURST;
    if ok {
        recent.push(now);
    }
    ok
}

/// The longest address a note gives: a login's carries a long query.
const URL_MAX: usize = 200;

/// The popup's tab name: its opener's with `-popup1`, `-popup2`, ..., the first no open tab of
/// the pane has; `popup1`, ... when that would pass a name's 32 characters.
pub fn name(opener: &str, taken: impl Fn(&str) -> bool) -> String {
    (1..)
        .map(|n| {
            let named = format!("{opener}-popup{n}");
            if named.len() <= 32 {
                named
            } else {
                format!("popup{n}")
            }
        })
        .find(|name| !taken(name))
        .unwrap_or_default()
}

pub fn opened(name: &str, url: &str) -> String {
    let shown = super::cut(url, URL_MAX);
    let more = if shown.len() < url.len() { "…" } else { "" };
    format!("popup opened as tab {name} ({shown}{more})")
}

/// No tab could be made (`why`, as Routes says it): the part before its advice.
pub fn blocked(why: &str) -> String {
    let why = why.split(": ").next().unwrap_or(why).trim_end_matches('.');
    let mut chars = why.chars();
    let why = chars.next().map_or_else(String::new, |first| {
        first.to_lowercase().chain(chars).collect()
    });
    format!("popup blocked: {why}")
}

pub fn closed(name: &str) -> String {
    format!("tab {name} closed: its page called window.close()")
}

#[cfg(test)]
mod tests {
    use super::super::control::tab_name;
    use super::*;

    #[test]
    fn a_popup_is_named_for_its_opener_with_the_first_free_number() {
        let none = |_: &str| false;
        assert_eq!(name("t1", none), "t1-popup1");
        assert_eq!(name("login", |n| n == "login-popup1"), "login-popup2");
        let long = "a".repeat(30);
        assert_eq!(name(&long, none), "popup1");
        assert_eq!(name(&long, |n| n == "popup1"), "popup2");
        // Always a name an agent can give back with --tab.
        for opener in ["t1", "rv268", &"x".repeat(26), &"y".repeat(32)] {
            assert!(tab_name(&name(opener, none)).is_ok(), "{opener}");
        }
    }

    #[test]
    fn a_burst_of_popups_is_cut_off_until_the_window_passes() {
        let start = Instant::now();
        let mut recent = Vec::new();
        for n in 0..BURST as u64 {
            assert!(allow(&mut recent, start + Duration::from_millis(n)));
        }
        assert!(!allow(&mut recent, start + Duration::from_secs(5)));
        // A refused one isn't counted: the window frees when the first ones age out.
        assert!(allow(&mut recent, start + BURST_WINDOW));
    }

    #[test]
    fn a_popups_note_cuts_a_long_address() {
        assert_eq!(
            opened("t1-popup1", "https://login.test/authorize"),
            "popup opened as tab t1-popup1 (https://login.test/authorize)"
        );
        let long = format!("https://login.test/?state={}", "s".repeat(400));
        let said = opened("t1-popup1", &long);
        assert!(said.ends_with("…)") && said.chars().count() < 260);
        assert_eq!(
            blocked("This pane has 12 tabs: close one first, `tab close <name>`"),
            "popup blocked: this pane has 12 tabs"
        );
        assert_eq!(
            blocked("This browser tab is closed."),
            "popup blocked: this browser tab is closed"
        );
        assert_eq!(
            closed("t1-popup1"),
            "tab t1-popup1 closed: its page called window.close()"
        );
    }
}
