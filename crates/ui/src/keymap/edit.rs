//! The shortcut editor's rules, pure: a pressed key as the chord it's stored as, a rebinding kept
//! only while it differs from the default, and which command a shared chord runs.

use gpui_kit::Keystroke;

use super::chord::{canonical, format_chord};
use super::{Command, Overrides, all_commands, bindings_for, place_name};

/// The digit or symbol a US layout types a shifted character with: macOS reports ⇧⌘] as ⌘}.
fn unshifted(key: &str) -> Option<&'static str> {
    Some(match key {
        "~" => "`",
        "!" => "1",
        "@" => "2",
        "#" => "3",
        "$" => "4",
        "%" => "5",
        "^" => "6",
        "&" => "7",
        "*" => "8",
        "(" => "9",
        ")" => "0",
        "_" => "-",
        // "+" can't be a token (it's the separator); every layout's plus key means zoom in anyway.
        "+" => "=",
        "{" => "[",
        "}" => "]",
        "|" => "\\",
        ":" => ";",
        "\"" => "'",
        "<" => ",",
        ">" => ".",
        "?" => "/",
        _ => return None,
    })
}

/// The chord a pressed key is stored as, or None for a lone modifier. Off macOS Ctrl is "cmd"
/// and the Super key "ctrl", as everywhere in the keymap.
pub fn chord_of(ks: &Keystroke, mac: bool) -> Option<String> {
    let mut key = ks.key.to_lowercase();
    if key.is_empty()
        || matches!(
            key.as_str(),
            "shift" | "control" | "alt" | "platform" | "function" | "capslock" | "fn"
        )
    {
        return None;
    }
    let m = &ks.modifiers;
    let (cmd, ctrl) = if mac {
        (m.platform, m.control)
    } else {
        (m.control, m.platform)
    };
    let mut shift = m.shift;
    if let Some(base) = unshifted(&key) {
        shift = true;
        key = base.to_string();
    }
    let mut parts: Vec<&str> = Vec::new();
    for (on, name) in [
        (ctrl, "ctrl"),
        (m.alt, "alt"),
        (shift, "shift"),
        (cmd, "cmd"),
    ] {
        if on {
            parts.push(name);
        }
    }
    parts.push(&key);
    canonical(&parts.join("+"))
}

/// `overrides` with command `id` bound to `keys`; None, or the platform's defaults, drops the
/// override: storing what equals the default would pin it and hide a later default's change.
pub fn with_binding(
    overrides: &Overrides,
    id: &str,
    keys: Option<Vec<String>>,
    mac: bool,
) -> Overrides {
    let mut next = overrides.clone();
    let defaults: Option<Vec<String>> = all_commands()
        .find(|c| c.id == id)
        .map(|c| c.default_keys(mac).iter().map(|k| k.to_string()).collect());
    match keys {
        Some(keys) if Some(&keys) != defaults.as_ref() => {
            next.insert(id.to_string(), keys);
        }
        _ => {
            next.remove(id);
        }
    }
    next
}

/// `keys` with the one at `index` replaced by `chord` (or `chord` added past the end), each
/// chord once.
pub fn recorded(keys: &[String], index: usize, chord: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let replaced = keys
        .iter()
        .take(index)
        .cloned()
        .chain(std::iter::once(chord.to_string()))
        .chain(keys.iter().skip(index + 1).cloned());
    for k in replaced {
        if !out.contains(&k) {
            out.push(k);
        }
    }
    out
}

/// The global command a chord runs: the first one listed that has it, as the keymap orders them.
pub fn winner(chord: &str, overrides: &Overrides, mac: bool) -> Option<&'static Command> {
    all_commands()
        .filter(|c| c.local.is_none())
        .find(|c| bindings_for(c, overrides, mac).iter().any(|k| k == chord))
}

/// Why `chord` on `c` is ambiguous, naming which command runs; None when it isn't.
pub fn clash_note(c: &Command, chord: &str, overrides: &Overrides, mac: bool) -> Option<String> {
    // By default some local commands take a global key over in their own place (⌘W in the
    // terminal): not a clash until the user rebinds one of the two.
    let meant = |o: &Command| {
        o.local.is_some() != c.local.is_some()
            && !overrides.contains_key(o.id)
            && !overrides.contains_key(c.id)
    };
    let others: Vec<&Command> = all_commands()
        .filter(|o| o.id != c.id && !meant(o))
        .filter(|o| bindings_for(o, overrides, mac).iter().any(|k| k == chord))
        .collect();
    if others.is_empty() {
        return None;
    }
    let k = format_chord(chord, mac);
    let titles = |list: &[&Command]| list.iter().map(|o| o.title).collect::<Vec<_>>().join(", ");
    let global: Vec<&Command> = others
        .iter()
        .copied()
        .filter(|o| o.local.is_none())
        .collect();
    // Two local commands never meet: each listens in its own place.
    if let Some(local) = c.local {
        return (!global.is_empty()).then(|| {
            format!(
                "{k} also runs {} outside {}",
                titles(&global),
                place_name(local)
            )
        });
    }
    if global.is_empty() {
        let named: Vec<String> = others
            .iter()
            .map(|o| match o.place() {
                Some(place) => format!("{} in {place}", o.title),
                None => o.title.to_string(),
            })
            .collect();
        return Some(format!("{k} also runs {}", named.join(", ")));
    }
    match winner(chord, overrides, mac) {
        Some(w) if w.id == c.id => Some(format!(
            "{k} is also bound to {}; this command takes precedence",
            titles(&global)
        )),
        Some(w) => Some(format!(
            "{k} is also bound to {}, which takes precedence",
            w.title
        )),
        None => None,
    }
}

/// Whether the shortcut list's search finds `c`: its title, category, id or keys, either way
/// they're written.
pub fn matches_query(c: &Command, query: &str, overrides: &Overrides, mac: bool) -> bool {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return true;
    }
    let keys = bindings_for(c, overrides, mac);
    [c.title, c.category.label(), c.id]
        .into_iter()
        .map(str::to_string)
        .chain(keys.iter().cloned())
        .chain(keys.iter().map(|k| format_chord(k, mac)))
        .any(|t| t.to_lowercase().contains(&q))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keymap::command;
    use gpui_kit::Modifiers;

    fn ks(key: &str, f: impl FnOnce(&mut Modifiers)) -> Keystroke {
        let mut modifiers = Modifiers::default();
        f(&mut modifiers);
        Keystroke {
            modifiers,
            key: key.into(),
            key_char: None,
        }
    }

    #[test]
    fn keys_record_as_stored_chords() {
        assert_eq!(
            chord_of(&ks("p", |m| (m.platform, m.shift) = (true, true)), true).as_deref(),
            Some("shift+cmd+p")
        );
        // ⇧⌘] arrives as ⌘}.
        assert_eq!(
            chord_of(&ks("}", |m| m.platform = true), true).as_deref(),
            Some("shift+cmd+]")
        );
        assert_eq!(
            chord_of(&ks("+", |m| m.platform = true), true).as_deref(),
            Some("shift+cmd+=")
        );
        assert_eq!(
            chord_of(&ks("s", |m| m.alt = true), true).as_deref(),
            Some("alt+s")
        );
        assert_eq!(chord_of(&ks("f7", |_| {}), true).as_deref(), Some("f7"));
        // Off macOS Ctrl is "cmd" and Super "ctrl".
        assert_eq!(
            chord_of(&ks("j", |m| m.control = true), false).as_deref(),
            Some("cmd+j")
        );
        assert_eq!(
            chord_of(&ks("1", |m| m.platform = true), false).as_deref(),
            Some("ctrl+1")
        );
        assert_eq!(chord_of(&ks("shift", |m| m.shift = true), true), None);
    }

    #[test]
    fn a_default_binding_is_not_stored() {
        let push = command("git.push").unwrap();
        let mut o = with_binding(&Overrides::new(), push.id, Some(vec!["cmd+u".into()]), true);
        assert_eq!(o["git.push"], vec!["cmd+u".to_string()]);
        o = with_binding(&o, push.id, Some(vec!["ctrl+cmd+p".into()]), true);
        assert!(!o.contains_key("git.push"));
        o = with_binding(&o, push.id, Some(vec![]), true);
        assert!(o["git.push"].is_empty());
        o = with_binding(&o, push.id, None, true);
        assert!(o.is_empty());
    }

    #[test]
    fn recording_replaces_one_key_and_collapses_duplicates() {
        let keys = vec!["cmd+a".to_string(), "cmd+b".into()];
        assert_eq!(recorded(&keys, 0, "cmd+c"), ["cmd+c", "cmd+b"]);
        assert_eq!(recorded(&keys, 2, "cmd+c"), ["cmd+a", "cmd+b", "cmd+c"]);
        assert_eq!(recorded(&keys, 1, "cmd+a"), ["cmd+a"]);
    }

    #[test]
    fn clashes_name_the_command_that_runs() {
        let fetch = command("git.fetch").unwrap();
        let pull = command("git.pull").unwrap();
        let mut o = Overrides::new();
        o.insert("git.fetch".into(), vec!["cmd+u".into()]);
        o.insert("git.pull".into(), vec!["cmd+u".into()]);
        let first = all_commands()
            .position(|c| c.id == "git.fetch")
            .unwrap()
            .min(all_commands().position(|c| c.id == "git.pull").unwrap());
        let (winning, losing) = if all_commands().nth(first).unwrap().id == "git.fetch" {
            (fetch, pull)
        } else {
            (pull, fetch)
        };
        assert_eq!(winner("cmd+u", &o, true).unwrap().id, winning.id);
        assert!(
            clash_note(winning, "cmd+u", &o, true)
                .unwrap()
                .ends_with("this command takes precedence")
        );
        assert!(
            clash_note(losing, "cmd+u", &o, true)
                .unwrap()
                .contains(&format!("bound to {}, which", winning.title))
        );
        // Unshared, nothing to say.
        assert_eq!(clash_note(fetch, "cmd+u", &Overrides::new(), true), None);
    }

    #[test]
    fn a_local_default_over_a_global_key_is_no_clash() {
        // ⌘D splits in the terminal by default; no global command has it unless rebound.
        let split = command("terminal.split").unwrap();
        let o = Overrides::new();
        for key in bindings_for(split, &o, true) {
            assert_eq!(clash_note(split, &key, &o, true), None, "{key}");
        }
    }

    #[test]
    fn search_reads_titles_ids_and_keys() {
        let palette = command("workbench.showCommands").unwrap();
        let o = Overrides::new();
        assert!(matches_query(palette, "palette", &o, true));
        assert!(matches_query(palette, "workbench", &o, true));
        assert!(matches_query(palette, "⇧⌘P", &o, true));
        assert!(matches_query(palette, "shift+cmd+p", &o, true));
        assert!(!matches_query(palette, "nothing like it", &o, true));
    }
}
