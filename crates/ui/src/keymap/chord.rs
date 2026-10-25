//! Chords as settings store them ("shift+cmd+e") and what GPUI and the screen make of them. Pure,
//! so it runs under plain tests. Off macOS "cmd" is Ctrl and "ctrl" the Super key, so one stored
//! binding means the same keys on every desktop.

use super::Command;

pub const IS_MAC: bool = cfg!(target_os = "macos");

const MODS: [&str; 4] = ["ctrl", "alt", "shift", "cmd"];

/// The canonical form of a stored chord, or None when it has unknown or repeated modifiers.
pub fn canonical(chord: &str) -> Option<String> {
    let lower = chord.to_lowercase();
    let mut parts: Vec<&str> = lower.split('+').collect();
    let key = parts.pop().filter(|k| !k.is_empty())?;
    if parts.iter().any(|p| !MODS.contains(p)) {
        return None;
    }
    let mut seen = parts.clone();
    seen.sort_unstable();
    seen.dedup();
    if seen.len() != parts.len() {
        return None;
    }
    let mut out: Vec<&str> = MODS.iter().copied().filter(|m| parts.contains(m)).collect();
    out.push(key);
    Some(out.join("+"))
}

fn split(chord: &str) -> (Vec<&str>, &str) {
    let mut parts: Vec<&str> = chord.split('+').collect();
    let key = parts.pop().unwrap_or_default();
    (parts, key)
}

/// What a US layout types with Shift on a digit or symbol key.
fn shifted(key: &str) -> Option<&'static str> {
    Some(match key {
        "`" => "~",
        "1" => "!",
        "2" => "@",
        "3" => "#",
        "4" => "$",
        "5" => "%",
        "6" => "^",
        "7" => "&",
        "8" => "*",
        "9" => "(",
        "0" => ")",
        "-" => "_",
        "=" => "+",
        "[" => "{",
        "]" => "}",
        "\\" => "|",
        ";" => ":",
        "'" => "\"",
        "," => "<",
        "." => ">",
        "/" => "?",
        _ => return None,
    })
}

/// The chord as GPUI's keymap reads it ("shift-cmd-e"), for this platform. On macOS GPUI reports
/// Shift with a digit or symbol as the character it types and no Shift (⇧⌘] comes as ⌘}), so
/// the binding names that character; letters and named keys keep Shift.
pub fn to_keystroke(chord: &str, mac: bool) -> String {
    let (mods, mut key) = split(chord);
    let mut mods: Vec<&str> = mods;
    if mac
        && mods.contains(&"shift")
        && let Some(typed) = shifted(key)
    {
        mods.retain(|m| *m != "shift");
        key = typed;
    }
    let mut out: Vec<&str> = mods
        .iter()
        .map(|m| match (*m, mac) {
            ("cmd", false) => "ctrl",
            ("ctrl", false) => "super",
            (m, _) => m,
        })
        .collect();
    // GPUI names the keys as we do (pageup, backspace, f6, `).
    out.push(key);
    out.join("-")
}

/// Whether macOS takes a chord before the app sees it (quit, hide, minimize, cycle windows,
/// screenshots). Off macOS "cmd" is Ctrl and nothing claims these.
pub fn is_reserved(chord: &str, mac: bool) -> bool {
    const MAC_RESERVED: [&str; 8] = [
        "cmd+q",
        "cmd+h",
        "alt+cmd+h",
        "cmd+m",
        "cmd+`",
        "shift+cmd+3",
        "shift+cmd+4",
        "shift+cmd+5",
    ];
    mac && MAC_RESERVED.contains(&chord)
}

/// Whether a chord still runs its command while focus is in a text field. Letters and ⌥ chords
/// type characters there, ⌃+letter edits the line on macOS (⌃A, ⌃K), and "cmd"-arrows move the
/// cursor. Other "cmd" chords, ⌃ with anything else (⌃Tab, ⌃1), the Super key and F-keys are free.
pub fn runs_while_typing(chord: &str, command: &Command, mac: bool) -> bool {
    if command.outside_text {
        return false;
    }
    let (mods, key) = split(chord);
    if mods.contains(&"cmd") {
        return !matches!(key, "left" | "right" | "up" | "down");
    }
    if mods.contains(&"ctrl") {
        return !mac || !is_letter(key);
    }
    is_fkey(key) && !mods.contains(&"alt")
}

/// Whether an `in_editor` command's chord runs in a code editor too, where its key would type a
/// symbol: one with ⌥, ⌃ or ⌘. A letter of the user's alphabet still types (keymap::typed_letter).
pub fn runs_in_editor(chord: &str, command: &Command) -> bool {
    command.in_editor
        && split(chord)
            .0
            .iter()
            .any(|m| matches!(*m, "alt" | "ctrl" | "cmd"))
}

/// Whether the terminal hands a chord to its command instead of the shell: the physical Ctrl with
/// anything but a letter (⌃Tab, ⌃1). Ctrl+letter is always the shell's; Ctrl+Shift+letter is the
/// app's, as in Linux terminals.
pub fn taken_from_terminal(chord: &str, command: &Command, mac: bool) -> bool {
    let (mods, key) = split(chord);
    let shell_key = is_letter(key) && !mods.contains(&"shift");
    mods.contains(&if mac { "ctrl" } else { "cmd" })
        && !shell_key
        && runs_while_typing(chord, command, mac)
}

/// Whether a chord runs its command while the terminal has focus: only what the terminal doesn't
/// read itself, so a key never goes to both. F-keys stay the shell's (htop, mc use them).
pub fn runs_in_terminal(chord: &str, command: &Command, mac: bool) -> bool {
    let (mods, _) = split(chord);
    if mods.contains(&if mac { "cmd" } else { "ctrl" }) {
        return runs_while_typing(chord, command, mac);
    }
    taken_from_terminal(chord, command, mac)
}

fn is_letter(key: &str) -> bool {
    key.len() == 1 && key.as_bytes()[0].is_ascii_lowercase()
}

fn is_fkey(key: &str) -> bool {
    key.len() >= 2 && key.starts_with('f') && key[1..].bytes().all(|b| b.is_ascii_digit())
}

fn glyph(part: &str) -> Option<&'static str> {
    Some(match part {
        "ctrl" => "⌃",
        "alt" => "⌥",
        "shift" => "⇧",
        "cmd" => "⌘",
        "up" => "↑",
        "down" => "↓",
        "left" => "←",
        "right" => "→",
        "enter" => "↵",
        "escape" => "⎋",
        "tab" => "⇥",
        "backspace" => "⌫",
        "delete" => "⌦",
        "space" => "Space",
        "pageup" => "PgUp",
        "pagedown" => "PgDn",
        "home" => "Home",
        "end" => "End",
        "-" => "−",
        _ => return None,
    })
}

/// A chord's keys one by one, as keycaps show them: ⇧ ⌘ E on macOS, Ctrl Shift E elsewhere.
pub fn chord_keys(chord: &str, mac: bool) -> Vec<String> {
    let (mods, key) = split(chord);
    let key_name = |k: &str| {
        glyph(k)
            .map(str::to_string)
            .unwrap_or_else(|| k.to_uppercase())
    };
    if mac {
        let mut out: Vec<String> = mods.iter().map(|m| key_name(m)).collect();
        out.push(key_name(key));
        return out;
    }
    // Windows and Linux lead with Ctrl, not with ⌃⌥⇧⌘'s order.
    let name = |m: &str| match m {
        "cmd" => "Ctrl",
        "ctrl" if cfg!(windows) => "Win",
        "ctrl" => "Super",
        "alt" => "Alt",
        _ => "Shift",
    };
    let mut out: Vec<String> = ["cmd", "ctrl", "alt", "shift"]
        .iter()
        .filter(|m| mods.contains(m))
        .map(|m| name(m).to_string())
        .collect();
    out.push(key_name(key));
    out
}

/// ⇧⌘E on macOS, Ctrl+Shift+E elsewhere.
pub fn format_chord(chord: &str, mac: bool) -> String {
    chord_keys(chord, mac).join(if mac { "" } else { "+" })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keymap::{Category, Command};

    fn command(outside_text: bool) -> Command {
        let c = Command::new("t.t", "T", Category::General, &[], || unreachable!());
        if outside_text { c.outside_text() } else { c }
    }

    #[test]
    fn canonical_orders_modifiers_and_rejects_junk() {
        assert_eq!(canonical("CMD+Shift+P").as_deref(), Some("shift+cmd+p"));
        assert_eq!(
            canonical("cmd+alt+ctrl+t").as_deref(),
            Some("ctrl+alt+cmd+t")
        );
        assert_eq!(canonical("cmd+-").as_deref(), Some("cmd+-"));
        assert_eq!(canonical("hyper+p"), None);
        assert_eq!(canonical("cmd+cmd+p"), None);
        assert_eq!(canonical(""), None);
        assert_eq!(canonical("f6").as_deref(), Some("f6"));
    }

    #[test]
    fn keystrokes_follow_the_platform() {
        assert_eq!(to_keystroke("shift+cmd+p", true), "shift-cmd-p");
        assert_eq!(to_keystroke("shift+cmd+p", false), "shift-ctrl-p");
        assert_eq!(to_keystroke("ctrl+1", false), "super-1");
        assert_eq!(to_keystroke("alt+cmd+=", true), "alt-cmd-=");
        assert_eq!(to_keystroke("f7", true), "f7");
    }

    #[test]
    fn shifted_symbols_bind_as_macos_reports_them() {
        assert_eq!(to_keystroke("shift+cmd+[", true), "cmd-{");
        assert_eq!(to_keystroke("shift+cmd+]", true), "cmd-}");
        assert_eq!(to_keystroke("shift+cmd+=", true), "cmd-+");
        assert_eq!(to_keystroke("shift+/", true), "?");
        assert_eq!(to_keystroke("shift+cmd+1", true), "cmd-!");
        // Letters and named keys keep Shift; elsewhere nothing changes.
        assert_eq!(to_keystroke("shift+cmd+p", true), "shift-cmd-p");
        assert_eq!(to_keystroke("ctrl+shift+tab", true), "ctrl-shift-tab");
        assert_eq!(to_keystroke("shift+cmd+[", false), "shift-ctrl-[");
        // What GPUI's macOS backend makes of ⇧⌘[ is ⌘{: the binding has to take that.
        let typed = gpui_kit::Keystroke::parse("cmd-{").unwrap();
        let bind = |ks: &str| {
            gpui_kit::KeyBinding::load(
                ks,
                Box::new(crate::actions::tab::Prev),
                None,
                false,
                None,
                &gpui_kit::DummyKeyboardMapper,
            )
            .unwrap()
        };
        assert_eq!(
            bind(&to_keystroke("shift+cmd+[", true)).match_keystrokes(std::slice::from_ref(&typed)),
            Some(false)
        );
        assert_eq!(bind("shift-cmd-[").match_keystrokes(&[typed]), None);
    }

    #[test]
    fn keystrokes_parse_in_gpui() {
        for chord in [
            "cmd+-",
            "shift+cmd+]",
            "ctrl+`",
            "alt+cmd+f",
            "shift+f12",
            "cmd+,",
        ] {
            for mac in [true, false] {
                let ks = to_keystroke(chord, mac);
                assert!(gpui_kit::Keystroke::parse(&ks).is_ok(), "{ks}");
            }
        }
    }

    #[test]
    fn typing_rules_match_the_old_app() {
        let c = command(false);
        assert!(runs_while_typing("shift+cmd+p", &c, true));
        assert!(!runs_while_typing("cmd+left", &c, true));
        assert!(!runs_while_typing("j", &c, true));
        assert!(!runs_while_typing("alt+s", &c, true));
        assert!(!runs_while_typing("ctrl+a", &c, true));
        assert!(runs_while_typing("ctrl+1", &c, true));
        assert!(runs_while_typing("f7", &c, true));
        assert!(!runs_while_typing("cmd+z", &command(true), true));
    }

    #[test]
    fn terminal_keeps_its_keys() {
        let c = command(false);
        assert!(runs_in_terminal("cmd+j", &c, true));
        assert!(runs_in_terminal("ctrl+`", &c, true));
        assert!(!runs_in_terminal("ctrl+r", &c, true));
        assert!(!runs_in_terminal("f6", &c, true));
        assert!(!runs_in_terminal("alt+s", &c, true));
        // Elsewhere "cmd" is Ctrl: Ctrl+J is the shell's unless it comes with Shift.
        assert!(!runs_in_terminal("cmd+r", &c, false));
        assert!(runs_in_terminal("shift+cmd+t", &c, false));
    }

    #[test]
    fn formats_for_each_platform() {
        assert_eq!(format_chord("shift+cmd+e", true), "⇧⌘E");
        assert_eq!(format_chord("alt+cmd+-", true), "⌥⌘−");
        assert!(format_chord("shift+cmd+e", false).starts_with("Ctrl+"));
    }
}
