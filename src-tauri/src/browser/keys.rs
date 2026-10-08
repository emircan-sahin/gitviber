//! Keys typed into a page: which the app takes (`browser-key`) and which stay the page's, and
//! how AppKit's key events read as DOM ones, so keybindings.ts handles them unchanged.

use serde::Serialize;

/// A key event as a DOM KeyboardEvent names it: `key` what it types ignoring ⌥ and ⌃, `code`
/// the physical key, from which the page reads ⌥ chords.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Key {
    pub id: String,
    pub key: String,
    pub code: &'static str,
    pub meta_key: bool,
    pub ctrl_key: bool,
    pub alt_key: bool,
    pub shift_key: bool,
    pub repeat: bool,
}

#[derive(Debug, PartialEq)]
pub enum Route {
    /// WebKit's: typing, editing, the page's own shortcuts.
    Page,
    /// The app's commands, re-run in the app page.
    App,
    /// Left to the menu bar: macOS's own keys (Quit, Hide, Minimize, cycle windows).
    System,
}

/// Who a key goes to. Every ⌘ chord is the app's but editing, which the page needs; ⌃ with a
/// letter stays the page's too, as text fields read it (⌃A, ⌃E, ⌃K).
pub fn route(k: &Key) -> Route {
    if !(k.meta_key || k.ctrl_key) {
        return Route::Page;
    }
    let lower = k.key.to_lowercase();
    if k.ctrl_key && !k.meta_key {
        let letter = lower.len() == 1 && lower.as_bytes()[0].is_ascii_lowercase();
        return if letter { Route::Page } else { Route::App };
    }
    if k.ctrl_key {
        return Route::App;
    }
    let plain = !k.alt_key && !k.shift_key;
    // ⌘←/→ move to a line's ends in a page's text field.
    let editing = matches!(lower.as_str(), "c" | "v" | "x" | "a") && plain
        || lower == "z" && !k.alt_key
        || matches!(k.key.as_str(), "ArrowLeft" | "ArrowRight") && plain;
    if editing {
        return Route::Page;
    }
    let system = matches!(lower.as_str(), "q" | "h" | "m" | "`") && plain
        || lower == "h" && k.alt_key && !k.shift_key;
    if system {
        Route::System
    } else {
        Route::App
    }
}

/// AppKit's function-key characters (NSUpArrowFunctionKey…) by their DOM names.
pub fn name(typed: &str) -> String {
    let mut chars = typed.chars();
    let (Some(c), None) = (chars.next(), chars.next()) else {
        return typed.into();
    };
    let named = match c {
        '\r' | '\u{3}' => "Enter",
        '\u{1b}' => "Escape",
        // ⇧Tab types backtab.
        '\t' | '\u{19}' => "Tab",
        '\u{7f}' => "Backspace",
        '\u{f728}' => "Delete",
        '\u{f700}' => "ArrowUp",
        '\u{f701}' => "ArrowDown",
        '\u{f702}' => "ArrowLeft",
        '\u{f703}' => "ArrowRight",
        '\u{f729}' => "Home",
        '\u{f72b}' => "End",
        '\u{f72c}' => "PageUp",
        '\u{f72d}' => "PageDown",
        '\u{f704}'..='\u{f70f}' => return format!("F{}", c as u32 - 0xf704 + 1),
        _ => return typed.into(),
    };
    named.into()
}

/// The DOM `code` of a Mac virtual key code, for the keys a chord names; "" for the rest.
pub fn code(key_code: u16) -> &'static str {
    // In virtual key code order (kVK_ANSI_A = 0 … kVK_Escape = 53).
    #[rustfmt::skip]
    const CODES: [&str; 54] = [
        "KeyA", "KeyS", "KeyD", "KeyF", "KeyH", "KeyG", "KeyZ", "KeyX", "KeyC", "KeyV",
        "IntlBackslash", "KeyB", "KeyQ", "KeyW", "KeyE", "KeyR", "KeyY", "KeyT", "Digit1", "Digit2",
        "Digit3", "Digit4", "Digit6", "Digit5", "Equal", "Digit9", "Digit7", "Minus", "Digit8",
        "Digit0", "BracketRight", "KeyO", "KeyU", "BracketLeft", "KeyI", "KeyP", "Enter", "KeyL",
        "KeyJ", "Quote", "KeyK", "Semicolon", "Backslash", "Comma", "Slash", "KeyN", "KeyM",
        "Period", "Tab", "Space", "Backquote", "Backspace", "", "Escape",
    ];
    CODES.get(usize::from(key_code)).copied().unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(key: &str, mods: &str) -> Key {
        Key {
            id: "b1".into(),
            key: key.into(),
            code: "",
            meta_key: mods.contains("cmd"),
            ctrl_key: mods.contains("ctrl"),
            alt_key: mods.contains("alt"),
            shift_key: mods.contains("shift"),
            repeat: false,
        }
    }

    #[test]
    fn editing_stays_in_the_page_and_the_app_takes_its_chords() {
        for (k, mods) in [
            ("c", "cmd"),
            ("V", "cmd"),
            ("z", "cmd"),
            ("Z", "shift+cmd"),
            ("ArrowLeft", "cmd"),
            ("j", ""),
            ("Escape", ""),
            ("a", "ctrl"),
            ("e", "ctrl"),
        ] {
            assert_eq!(route(&key(k, mods)), Route::Page, "{mods}+{k}");
        }
        for (k, mods) in [
            ("w", "cmd"),
            ("p", "cmd"),
            ("l", "cmd"),
            ("r", "cmd"),
            ("[", "cmd"),
            ("1", "cmd"),
            ("C", "shift+cmd"),
            ("i", "alt+cmd"),
            ("a", "alt+cmd"),
            ("1", "ctrl"),
            ("Tab", "ctrl"),
            ("`", "ctrl"),
            ("t", "ctrl+shift+cmd"),
        ] {
            assert_eq!(route(&key(k, mods)), Route::App, "{mods}+{k}");
        }
        for (k, mods) in [("q", "cmd"), ("h", "cmd"), ("h", "alt+cmd"), ("m", "cmd")] {
            assert_eq!(route(&key(k, mods)), Route::System, "{mods}+{k}");
        }
    }

    #[test]
    fn keys_named_as_the_dom_names_them() {
        assert_eq!(name("w"), "w");
        assert_eq!(name("}"), "}");
        assert_eq!(name("\u{f702}"), "ArrowLeft");
        assert_eq!(name("\u{f704}"), "F1");
        assert_eq!(name("\u{f70f}"), "F12");
        assert_eq!(name("\u{19}"), "Tab");
        assert_eq!(name("\r"), "Enter");
        assert_eq!(code(0), "KeyA");
        assert_eq!(code(33), "BracketLeft");
        assert_eq!(code(50), "Backquote");
        assert_eq!(code(53), "Escape");
        assert_eq!(code(200), "");
    }
}
