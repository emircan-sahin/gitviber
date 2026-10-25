//! Keys typed into a page: the chords that are macOS's own or the page's editing whoever binds
//! them (the tab's `page_key` gives the rest to the app's keymap), and how AppKit's key events
//! read as DOM ones.

/// A key event as a DOM KeyboardEvent names it: `key` what it types ignoring ⌥ and ⌃, `code`
/// the physical key.
#[derive(Clone, Debug, PartialEq)]
pub struct Key {
    pub key: String,
    pub code: &'static str,
    pub meta_key: bool,
    pub ctrl_key: bool,
    pub alt_key: bool,
    pub shift_key: bool,
}

/// What a ⌘ or ⌃ chord is to a page with the keys.
#[derive(Debug, PartialEq)]
pub enum Chord {
    /// macOS's own, left to the menu bar: Quit, Hide, Minimize, cycle windows, full screen.
    System,
    /// Editing: the page's even where a command of the app's has the key (⌘Z is Undo Git
    /// Action), as in a text field.
    Editing,
    /// The app's where one of its commands is bound to it, else the page's.
    Other,
}

/// The chord of these modifiers and the key `is` names ("z", "left", "`").
pub fn chord(cmd: bool, ctrl: bool, alt: bool, shift: bool, is: impl Fn(&str) -> bool) -> Chord {
    let system = cmd
        && !shift
        && (!ctrl && !alt && (is("q") || is("h") || is("m") || is("`"))
            || !ctrl && alt && is("h")
            || ctrl && !alt && is("f"));
    if system {
        return Chord::System;
    }
    let arrow = is("left") || is("right") || is("up") || is("down");
    let editing = cmd
        && !ctrl
        && (!alt
            && !shift
            && ["c", "v", "x", "a", "backspace", "delete", "enter"]
                .into_iter()
                .any(&is)
            || !alt && is("z")
            || arrow);
    if editing {
        Chord::Editing
    } else {
        Chord::Other
    }
}

/// Whether `key` is the one a chord names: by what it types, and on a layout that doesn't type
/// ASCII by where it sits, so ⌘C on Cyrillic is still Copy.
fn names(k: &Key) -> impl Fn(&str) -> bool + '_ {
    let typed = token(&k.key);
    let at = position(k.code);
    let punctuation = punctuation(k.code);
    let not_ascii = !k.key.is_ascii();
    move |t: &str| {
        typed.as_deref() == Some(t)
            || not_ascii && (at.as_deref() == Some(t) || punctuation == Some(t))
    }
}

/// Whether a key AppKit offers the page is macOS's own, for the menu bar.
pub fn is_system(k: &Key) -> bool {
    (k.meta_key || k.ctrl_key)
        && chord(k.meta_key, k.ctrl_key, k.alt_key, k.shift_key, names(k)) == Chord::System
}

/// What the Edit menu sends the page for an editing chord.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Edit {
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    SelectAll,
}

/// The Edit menu's command a chord the page left alone stands for, so it's still the page's own
/// Undo or Copy, never a command of the app's with the same key.
pub fn edit(k: &Key) -> Option<Edit> {
    if !k.meta_key || k.ctrl_key || k.alt_key {
        return None;
    }
    let is = names(k);
    Some(match () {
        _ if is("z") => {
            if k.shift_key {
                Edit::Redo
            } else {
                Edit::Undo
            }
        }
        _ if k.shift_key => return None,
        _ if is("x") => Edit::Cut,
        _ if is("c") => Edit::Copy,
        _ if is("v") => Edit::Paste,
        _ if is("a") => Edit::SelectAll,
        _ => return None,
    })
}

/// A key as a chord names it.
fn token(key: &str) -> Option<String> {
    let named = match key {
        "ArrowUp" => "up",
        "ArrowDown" => "down",
        "ArrowLeft" => "left",
        "ArrowRight" => "right",
        "Enter" => "enter",
        "Escape" => "escape",
        "Tab" => "tab",
        " " => "space",
        "Backspace" => "backspace",
        "Delete" => "delete",
        "Home" => "home",
        "End" => "end",
        "PageUp" => "pageup",
        "PageDown" => "pagedown",
        "+" => "=",
        "{" => "[",
        "}" => "]",
        _ => {
            let mut chars = key.chars();
            let one = chars.next().filter(|_| chars.next().is_none());
            let f_key = key.len() > 1
                && key.starts_with('F')
                && key[1..].bytes().all(|b| b.is_ascii_digit());
            return (one.is_some() || f_key).then(|| key.to_lowercase());
        }
    };
    Some(named.into())
}

/// The letter or digit a US keyboard has where this key was pressed.
fn position(code: &str) -> Option<String> {
    let at = code
        .strip_prefix("Key")
        .or_else(|| code.strip_prefix("Digit"));
    at.filter(|c| c.len() == 1).map(str::to_ascii_lowercase)
}

/// A punctuation key by what a US keyboard types there.
fn punctuation(code: &str) -> Option<&'static str> {
    Some(match code {
        "Equal" => "=",
        "Minus" => "-",
        "Comma" => ",",
        "Period" => ".",
        "Slash" => "/",
        "Backslash" => "\\",
        "Semicolon" => ";",
        "Quote" => "'",
        "BracketLeft" => "[",
        "BracketRight" => "]",
        "Backquote" => "`",
        "IntlBackslash" => "§",
        _ => return None,
    })
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
            key: key.into(),
            code: "",
            meta_key: mods.contains("cmd"),
            ctrl_key: mods.contains("ctrl"),
            alt_key: mods.contains("alt"),
            shift_key: mods.contains("shift"),
        }
    }

    fn at(k: &str, code: &'static str, mods: &str) -> Key {
        Key {
            code,
            ..key(k, mods)
        }
    }

    fn kind(k: &Key) -> Chord {
        chord(k.meta_key, k.ctrl_key, k.alt_key, k.shift_key, names(k))
    }

    #[test]
    fn editing_chords_are_the_pages_whoever_binds_them() {
        for (k, mods) in [
            ("c", "cmd"),
            ("V", "cmd"),
            ("x", "cmd"),
            ("a", "cmd"),
            ("z", "cmd"),
            ("Z", "shift+cmd"),
            ("Backspace", "cmd"),
            ("Delete", "cmd"),
            ("Enter", "cmd"),
        ] {
            assert_eq!(kind(&key(k, mods)), Chord::Editing, "{mods}+{k}");
        }
        for k in ["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"] {
            for mods in ["cmd", "shift+cmd", "alt+cmd", "alt+shift+cmd"] {
                assert_eq!(kind(&key(k, mods)), Chord::Editing, "{mods}+{k}");
            }
        }
        // Another modifier makes them another chord, the app's if it binds one.
        for (k, mods) in [
            ("c", "ctrl+cmd"),
            ("z", "alt+cmd"),
            ("v", "shift+cmd"),
            ("w", "cmd"),
            ("[", "cmd"),
            ("1", "ctrl"),
            ("a", "ctrl"),
        ] {
            assert_eq!(kind(&key(k, mods)), Chord::Other, "{mods}+{k}");
        }
    }

    #[test]
    fn the_system_keeps_its_own_chords_and_nothing_else() {
        for (k, mods) in [
            ("`", "cmd"),
            ("Q", "cmd"),
            ("H", "cmd"),
            ("M", "cmd"),
            ("h", "alt+cmd"),
            ("f", "ctrl+cmd"),
        ] {
            assert!(is_system(&key(k, mods)), "{mods}+{k}");
        }
        // ⇧ or ⌃ makes them another chord: Quit and Hide only as macOS binds them.
        for (k, mods) in [
            ("q", "ctrl+cmd"),
            ("Q", "shift+cmd"),
            ("H", "alt+shift+cmd"),
            ("m", "alt+cmd"),
            ("q", ""),
            ("h", "shift"),
        ] {
            assert!(!is_system(&key(k, mods)), "{mods}+{k}");
        }
    }

    #[test]
    fn a_layout_without_ascii_counts_by_where_the_key_sits() {
        assert!(is_system(&at("й", "KeyQ", "cmd")));
        assert!(is_system(&at("р", "KeyH", "cmd")));
        for (k, code, edit) in [
            ("с", "KeyC", Edit::Copy),
            ("м", "KeyV", Edit::Paste),
            ("ч", "KeyX", Edit::Cut),
            ("ф", "KeyA", Edit::SelectAll),
            ("я", "KeyZ", Edit::Undo),
            ("ψ", "KeyC", Edit::Copy),
            ("ב", "KeyC", Edit::Copy),
        ] {
            assert_eq!(super::edit(&at(k, code, "cmd")), Some(edit), "{k}");
        }
        assert_eq!(super::edit(&at("я", "KeyZ", "shift+cmd")), Some(Edit::Redo));
        // A Latin layout counts by what it types wherever the key sits: AZERTY's A is on Q.
        assert_eq!(super::edit(&at("a", "KeyQ", "cmd")), Some(Edit::SelectAll));
        assert!(is_system(&at("q", "KeyA", "cmd")));
        assert_eq!(
            super::edit(&at("j", "KeyC", "cmd")),
            None,
            "Dvorak's J, not copy"
        );
    }

    #[test]
    fn only_the_edit_menus_chords_stand_for_its_commands() {
        assert_eq!(edit(&key("z", "cmd")), Some(Edit::Undo));
        assert_eq!(edit(&key("Z", "shift+cmd")), Some(Edit::Redo));
        for (k, mods) in [
            ("c", "shift+cmd"),
            ("c", "alt+cmd"),
            ("c", "ctrl+cmd"),
            ("c", "ctrl"),
            ("c", ""),
            ("Backspace", "cmd"),
            ("b", "cmd"),
        ] {
            assert_eq!(edit(&key(k, mods)), None, "{mods}+{k}");
        }
    }

    /// A cheap generator, so the fuzz below is the same on every run.
    fn lcg(seed: &mut u64) -> usize {
        *seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (*seed >> 33) as usize
    }

    #[test]
    fn any_key_and_modifiers_read_without_panicking() {
        let keys = [
            "a",
            "Z",
            "ü",
            "ş",
            "ı",
            "с",
            "ß",
            "😀",
            "",
            "ArrowLeft",
            "Backspace",
            "Escape",
            "`",
            "1",
            "\u{f700}",
            "\u{0}",
            "İ",
            "ǅ",
        ];
        let mut seed = 5;
        for _ in 0..20_000 {
            let k = keys[lcg(&mut seed) % keys.len()];
            let m = lcg(&mut seed);
            let ev = Key {
                meta_key: m & 1 != 0,
                ctrl_key: m & 2 != 0,
                alt_key: m & 4 != 0,
                shift_key: m & 8 != 0,
                ..key(k, "")
            };
            if is_system(&ev) {
                assert!(ev.meta_key && !ev.shift_key, "{ev:?}");
            }
            if edit(&ev).is_some() {
                assert!(ev.meta_key && !ev.ctrl_key && !ev.alt_key, "{ev:?}");
            }
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

    #[test]
    fn every_function_key_character_has_its_dom_name() {
        for n in 0..12u32 {
            let c = char::from_u32(0xf704 + n).unwrap();
            assert_eq!(name(&c.to_string()), format!("F{}", n + 1));
        }
        for (typed, named) in [
            ("\u{f700}", "ArrowUp"),
            ("\u{f701}", "ArrowDown"),
            ("\u{f703}", "ArrowRight"),
            ("\u{f728}", "Delete"),
            ("\u{f729}", "Home"),
            ("\u{f72b}", "End"),
            ("\u{f72c}", "PageUp"),
            ("\u{f72d}", "PageDown"),
            ("\u{7f}", "Backspace"),
            ("\u{3}", "Enter"),
            ("\u{1b}", "Escape"),
            ("\t", "Tab"),
        ] {
            assert_eq!(name(typed), named, "{typed:?}");
        }
        // Two characters (a dead key's), or none, pass through as typed.
        for typed in ["", "ab", "\u{f700}\u{f701}", "´e"] {
            assert_eq!(name(typed), typed, "{typed:?}");
        }
    }

    #[test]
    fn every_virtual_key_code_up_to_escape_has_a_dom_code() {
        for c in 0..=53u16 {
            assert_eq!(code(c).is_empty(), c == 52, "{c}");
        }
        let all: Vec<_> = (0..=53u16).map(code).filter(|c| !c.is_empty()).collect();
        let distinct: std::collections::HashSet<_> = all.iter().collect();
        assert_eq!(all.len(), distinct.len());
        for (c, dom) in [
            (13, "KeyW"),
            (35, "KeyP"),
            (37, "KeyL"),
            (38, "KeyJ"),
            (12, "KeyQ"),
            (51, "Backspace"),
            (36, "Enter"),
            (48, "Tab"),
        ] {
            assert_eq!(code(c), dom, "{c}");
        }
        for c in [54, 123, 126, u16::MAX] {
            assert_eq!(code(c), "", "{c}");
        }
    }
}
