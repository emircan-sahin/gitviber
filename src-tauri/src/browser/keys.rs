//! Keys typed into a page: which the app takes (`browser-key`) and which stay the page's, and
//! how AppKit's key events read as DOM ones, so keybindings.ts handles them unchanged.

use serde::Serialize;
use std::collections::HashSet;
use std::sync::{LazyLock, Mutex};

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
    /// WebKit's: typing, editing, the page's own shortcuts (⌘B in an editor, ⌘↵ to send).
    Page,
    /// One of the app's commands, run again in the app page.
    App,
    /// Left to the menu bar: macOS's own keys (Quit, Hide, Minimize, cycle windows, full screen).
    System,
}

/// The chords the app's commands are bound to (keybindings.ts sends them as they change), as
/// commands.ts writes them: `shift+cmd+[`. Only these leave a page.
static APP_KEYS: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(Mutex::default);

pub fn set_app_keys(chords: Vec<String>) {
    *APP_KEYS.lock().unwrap_or_else(|e| e.into_inner()) = chords.into_iter().collect();
}

pub fn route_now(k: &Key) -> Route {
    route(k, &APP_KEYS.lock().unwrap_or_else(|e| e.into_inner()))
}

/// Who a key goes to: a chord bound to one of the app's commands goes to the app, anything else
/// stays the page's. Editing stays the page's even when bound (⌘Z is also Undo Git Action), and
/// on a layout that doesn't type ASCII a key counts by where it sits, so ⌘C on Cyrillic is still
/// copy. Bindings match as commands.ts's eventChords does: by what's typed, and punctuation by
/// its key too; a letter matched by its key alone would leave the app with a key it can't run.
pub fn route(k: &Key, app: &HashSet<String>) -> Route {
    let (cmd, ctrl, alt, shift) = (k.meta_key, k.ctrl_key, k.alt_key, k.shift_key);
    if !(cmd || ctrl) {
        return Route::Page;
    }
    let typed = token(&k.key);
    let at = position(k.code);
    let punctuation = punctuation(k.code);
    let not_ascii = !k.key.is_ascii();
    let is = |t: &str| {
        typed.as_deref() == Some(t)
            || not_ascii && (at.as_deref() == Some(t) || punctuation == Some(t))
    };
    let system = cmd
        && !shift
        && (!ctrl && !alt && (is("q") || is("h") || is("m") || is("`"))
            || !ctrl && alt && is("h")
            || ctrl && !alt && is("f"));
    if system {
        return Route::System;
    }
    let arrow = is("left") || is("right") || is("up") || is("down");
    let editing = cmd
        && !ctrl
        && (!alt
            && !shift
            && ["c", "v", "x", "a", "backspace", "delete", "enter"]
                .into_iter()
                .any(is)
            || !alt && is("z")
            || arrow);
    if editing {
        return Route::Page;
    }
    let mods: String = [
        (ctrl, "ctrl+"),
        (alt, "alt+"),
        (shift, "shift+"),
        (cmd, "cmd+"),
    ]
    .into_iter()
    .filter_map(|(held, m)| held.then_some(m))
    .collect();
    let bound = [typed.as_deref(), punctuation]
        .into_iter()
        .flatten()
        .any(|t| app.contains(&format!("{mods}{t}")));
    if bound {
        Route::App
    } else {
        Route::Page
    }
}

/// A key as commands.ts names it in a chord (its keyToken).
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

/// A punctuation key by what a US keyboard types there: commands.ts's CODE_KEYS.
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

    /// Chords the app binds by default (commands.ts), some of them editing keys too.
    fn defaults() -> HashSet<String> {
        [
            "cmd+w",
            "cmd+p",
            "cmd+l",
            "cmd+r",
            "cmd+[",
            "cmd+]",
            "cmd+1",
            "shift+cmd+]",
            "ctrl+1",
            "ctrl+tab",
            "ctrl+`",
            "ctrl+cmd+c",
            "ctrl+shift+cmd+t",
            "alt+cmd+b",
            "cmd+z",
            "shift+cmd+z",
            "cmd+backspace",
            "cmd+enter",
            "cmd+right",
            "cmd+left",
        ]
        .into_iter()
        .map(String::from)
        .collect()
    }

    fn to(k: &Key) -> Route {
        route(k, &defaults())
    }

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
            ("Enter", "cmd"),
            ("j", ""),
            ("Escape", ""),
            ("a", "ctrl"),
            ("e", "ctrl"),
            // Unbound: the page's own (bold in an editor, a search field's shortcut).
            ("b", "cmd"),
            ("i", "cmd"),
            ("k", "cmd"),
        ] {
            assert_eq!(to(&key(k, mods)), Route::Page, "{mods}+{k}");
        }
        for (k, mods) in [
            ("w", "cmd"),
            ("p", "cmd"),
            ("l", "cmd"),
            ("r", "cmd"),
            ("[", "cmd"),
            ("1", "cmd"),
            ("}", "shift+cmd"),
            ("b", "alt+cmd"),
            ("1", "ctrl"),
            ("Tab", "ctrl"),
            ("`", "ctrl"),
            ("t", "ctrl+shift+cmd"),
        ] {
            assert_eq!(to(&key(k, mods)), Route::App, "{mods}+{k}");
        }
        for (k, mods) in [("q", "cmd"), ("h", "cmd"), ("h", "alt+cmd"), ("m", "cmd")] {
            assert_eq!(to(&key(k, mods)), Route::System, "{mods}+{k}");
        }
    }

    #[test]
    fn a_chord_goes_to_the_app_only_while_a_command_has_it() {
        let w = key("w", "cmd");
        assert_eq!(route(&w, &defaults()), Route::App);
        assert_eq!(route(&w, &HashSet::new()), Route::Page);
        // Rebound: the new chord leaves the page, the old one stays in it.
        let rebound: HashSet<String> = ["alt+cmd+w".to_string()].into();
        assert_eq!(route(&key("w", "alt+cmd"), &rebound), Route::App);
        assert_eq!(route(&w, &rebound), Route::Page);
        // The app matches a letter by what's typed (eventChords), so a Cyrillic ⌘W (ц) is the
        // page's: sent to the app it would find no command. Punctuation counts by its key.
        let cyrillic_w = Key {
            code: "KeyW",
            ..key("ц", "cmd")
        };
        assert_eq!(route(&cyrillic_w, &defaults()), Route::Page);
        let german_bracket = Key {
            code: "BracketLeft",
            ..key("ü", "cmd")
        };
        assert_eq!(route(&german_bracket, &defaults()), Route::App);
        // Dvorak's ⌘U sits where QWERTY has F: bound to cmd+f, it's still the page's U.
        let find: HashSet<String> = ["cmd+f".to_string()].into();
        let dvorak_u = Key {
            code: "KeyF",
            ..key("u", "cmd")
        };
        assert_eq!(route(&dvorak_u, &find), Route::Page);
        assert_eq!(
            route(
                &Key {
                    code: "KeyU",
                    ..key("f", "cmd")
                },
                &find
            ),
            Route::App
        );
        // And Dvorak's ⌘C (on QWERTY's I) copies, its key's I notwithstanding.
        assert_eq!(
            route(
                &Key {
                    code: "KeyI",
                    ..key("c", "cmd")
                },
                &defaults()
            ),
            Route::Page
        );
    }

    #[test]
    fn cmd_backspace_deletes_in_the_page() {
        assert_eq!(to(&key("Backspace", "cmd")), Route::Page);
    }

    #[test]
    fn cmd_shift_arrows_select_in_the_page() {
        for k in ["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"] {
            assert_eq!(to(&key(k, "shift+cmd")), Route::Page, "{k}");
        }
    }

    #[test]
    fn copy_paste_on_a_cyrillic_layout_stays_in_the_page() {
        for (k, code) in [
            ("с", "KeyC"),
            ("м", "KeyV"),
            ("ч", "KeyX"),
            ("ф", "KeyA"),
            ("я", "KeyZ"),
        ] {
            assert_eq!(
                to(&Key {
                    code,
                    ..key(k, "cmd")
                }),
                Route::Page,
                "{k}"
            );
        }
    }

    #[test]
    fn ctrl_cmd_f_full_screen_is_the_systems() {
        assert_eq!(to(&key("f", "ctrl+cmd")), Route::System);
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
    fn keys_without_cmd_or_ctrl_always_stay_in_the_page() {
        for k in [
            "a", "J", "s", "Escape", "Enter", "Tab", "ArrowUp", "F5", "é", "ı", "", " ",
        ] {
            for mods in ["", "shift", "alt", "alt+shift"] {
                assert_eq!(to(&key(k, mods)), Route::Page, "{mods}+{k}");
            }
        }
    }

    #[test]
    fn the_system_keeps_its_own_chords_and_nothing_else() {
        for (k, mods) in [("`", "cmd"), ("Q", "cmd"), ("H", "cmd"), ("M", "cmd")] {
            assert_eq!(to(&key(k, mods)), Route::System, "{mods}+{k}");
        }
        // ⇧ or ⌃ makes them another chord: Quit and Hide only as macOS binds them.
        for (k, mods) in [
            ("q", "ctrl+cmd"),
            ("Q", "shift+cmd"),
            ("H", "alt+shift+cmd"),
            ("m", "alt+cmd"),
        ] {
            assert_ne!(to(&key(k, mods)), Route::System, "{mods}+{k}");
        }
    }

    #[test]
    fn ctrl_with_a_letter_is_the_text_fields_and_with_anything_else_the_apps() {
        for k in ["a", "E", "k", "z"] {
            assert_eq!(to(&key(k, "ctrl")), Route::Page, "ctrl+{k}");
            assert_eq!(to(&key(k, "ctrl+shift")), Route::Page, "ctrl+shift+{k}");
        }
        for k in ["1", "Tab", "`"] {
            assert_eq!(to(&key(k, "ctrl")), Route::App, "ctrl+{k}");
        }
        // Not bound to anything: the page's.
        for k in ["[", "ArrowLeft", "Enter", " ", ""] {
            assert_eq!(to(&key(k, "ctrl")), Route::Page, "ctrl+{k}");
        }
        assert_eq!(to(&key("c", "ctrl+cmd")), Route::App);
        for k in ["a", "v", "z", "q"] {
            assert_eq!(to(&key(k, "ctrl+cmd")), Route::Page, "ctrl+cmd+{k}");
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
    fn any_key_and_modifiers_route_without_panicking() {
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
            let to = self::to(&ev);
            if !(ev.meta_key || ev.ctrl_key) {
                assert_eq!(to, Route::Page, "{ev:?}");
            }
            if to == Route::System {
                assert!(ev.meta_key && !ev.shift_key, "{ev:?}");
            }
        }
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

    fn at(k: &str, code: &'static str, mods: &str) -> Key {
        Key {
            code,
            ..key(k, mods)
        }
    }

    #[test]
    fn a_latin_layout_counts_by_what_it_types_wherever_the_key_sits() {
        // AZERTY: A and Q, Z and W trade places.
        assert_eq!(
            to(&at("a", "KeyQ", "cmd")),
            Route::Page,
            "AZERTY ⌘A selects all"
        );
        assert_eq!(
            to(&at("q", "KeyA", "cmd")),
            Route::System,
            "AZERTY ⌘Q quits"
        );
        assert_eq!(
            to(&at("w", "KeyZ", "cmd")),
            Route::App,
            "AZERTY ⌘W closes the tab"
        );
        assert_eq!(to(&at("z", "KeyW", "cmd")), Route::Page, "AZERTY ⌘Z undoes");
        // QWERTZ: Y and Z.
        assert_eq!(to(&at("z", "KeyY", "cmd")), Route::Page);
        assert_eq!(to(&at("y", "KeyZ", "cmd")), Route::Page, "unbound ⌘Y stays");
        // Dvorak: an unbound letter on a bound key's spot stays the page's.
        assert_eq!(to(&at("u", "KeyW", "cmd")), Route::Page);
        assert_eq!(to(&at("p", "KeyR", "cmd")), Route::App, "Dvorak ⌘P is ⌘P");
        assert_eq!(to(&at("j", "KeyC", "cmd")), Route::Page, "not copy");
    }

    #[test]
    fn a_layout_without_ascii_counts_by_where_the_key_sits_for_editing_and_the_system() {
        // Cyrillic: editing and macOS's keys by position; a binding never, as the app page
        // can't run a letter it doesn't see typed: ⌘W is left to the menu bar's own matching.
        assert_eq!(to(&at("й", "KeyQ", "cmd")), Route::System);
        assert_eq!(to(&at("р", "KeyH", "cmd")), Route::System);
        assert_eq!(to(&at("ц", "KeyW", "cmd")), Route::Page);
        assert_eq!(to(&at("з", "KeyP", "cmd")), Route::Page);
        assert_eq!(to(&at("я", "KeyZ", "shift+cmd")), Route::Page, "redo");
        // Greek, Hebrew.
        assert_eq!(to(&at("ψ", "KeyC", "cmd")), Route::Page);
        assert_eq!(to(&at("ב", "KeyC", "cmd")), Route::Page);
        // Punctuation counts by its key on any layout, as commands.ts's CODE_KEYS does.
        assert_eq!(to(&at("х", "BracketLeft", "cmd")), Route::App, "⌘[ is Back");
        assert_eq!(to(&at("ъ", "BracketRight", "shift+cmd")), Route::App);
        assert_eq!(to(&at("ö", "Semicolon", "cmd")), Route::Page, "unbound");
        // German ⌥⌘L types @; AppKit names it by its letter, as bound.
        let alt_b = at("b", "KeyB", "alt+cmd");
        assert_eq!(to(&alt_b), Route::App);
    }

    #[test]
    fn shifted_punctuation_is_named_as_commands_ts_names_its_key() {
        assert_eq!(to(&at("}", "BracketRight", "shift+cmd")), Route::App);
        assert_eq!(
            to(&at("{", "BracketLeft", "shift+cmd")),
            Route::Page,
            "⇧⌘[ unbound here"
        );
        let plus: HashSet<String> = ["shift+cmd+=".to_string()].into();
        assert_eq!(route(&at("+", "Equal", "shift+cmd"), &plus), Route::App);
        // Function keys and named keys.
        let named: HashSet<String> = ["cmd+f5", "ctrl+space", "ctrl+cmd+escape"]
            .map(String::from)
            .into();
        assert_eq!(route(&key("F5", "cmd"), &named), Route::App);
        assert_eq!(route(&key(" ", "ctrl"), &named), Route::App);
        assert_eq!(route(&key("Escape", "ctrl+cmd"), &named), Route::App);
        assert_eq!(
            route(&key("F", "cmd"), &named),
            Route::Page,
            "a letter isn't F-key"
        );
    }

    #[test]
    fn an_empty_or_odd_binding_set_never_takes_editing_or_panics() {
        let odd: HashSet<String> = ["", "cmd+", "+", "cmd+c", "cmd+v", "cmd+backspace", "cmd+z"]
            .map(String::from)
            .into();
        for (k, mods) in [
            ("c", "cmd"),
            ("v", "cmd"),
            ("Backspace", "cmd"),
            ("z", "cmd"),
        ] {
            assert_eq!(route(&key(k, mods), &odd), Route::Page, "{mods}+{k}");
            assert_eq!(
                route(&key(k, mods), &HashSet::new()),
                Route::Page,
                "{mods}+{k}"
            );
        }
        assert_eq!(route(&key("", "cmd"), &odd), Route::Page);
    }
}
