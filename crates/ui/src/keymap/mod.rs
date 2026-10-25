//! The command registry and the keymap built from it. Defaults per platform, the user's
//! rebindings from settings (`keybindings`: command id → chords, an empty list unbinds), and the
//! key contexts that keep letters and ⌥ chords out of text fields and the terminal.

pub mod chord;
mod commands;
pub mod edit;

use std::cell::RefCell;
use std::collections::BTreeMap;

use gpui_kit::{
    Action, App, KeyBinding, KeyBindingContextPredicate, KeyBindingMetaIndex, Keystroke,
    KeystrokeEvent, Modifiers,
};

pub use chord::{IS_MAC, canonical, chord_keys, format_chord};
pub use commands::*;

use crate::settings::Settings;

/// What a feature crate asks of the app, which it can't call: dispatch one from a window.
pub mod requests {
    // AskGitIdentity: git refused a commit for want of a name or email; the app asks for them,
    // even after the user closed that ask once.
    gpui_kit::actions!(requests, [AskGitIdentity]);
}

/// Find Next / Previous in an open find box: the code view's, a diff's or a rendered document's.
/// Not commands (the old app left them to Monaco): no palette entry, and with no box open the
/// key goes on, so ⇧⌘G still focuses the Git panel.
pub mod find {
    gpui_kit::actions!(find, [Next, Prev]);
}

/// Where a find box can be open; its handlers step through the matches or let the key go on.
const FIND_PLACES: &str = "CodeView || MarkdownView || MarkdownFind || DiffView || DiffViewEditing || DiffFind || StackedDiff";

/// macOS's ⌘G and ⇧⌘G; F3 and ⇧F3 elsewhere, where Ctrl+G is Go to Line.
fn find_bindings(mac: bool) -> [KeyBinding; 2] {
    let (next, prev) = if mac {
        ("cmd-g", "shift-cmd-g")
    } else {
        ("f3", "shift-f3")
    };
    [
        KeyBinding::new(next, find::Next, Some(FIND_PLACES)),
        KeyBinding::new(prev, find::Prev, Some(FIND_PLACES)),
    ]
}

thread_local! {
    /// The keystroke being dispatched, set before its bindings run and cleared after.
    static PRESSED: RefCell<Option<Keystroke>> = const { RefCell::new(None) };
}

/// The keystroke running the action being handled; None when a click, the menu bar or the
/// palette ran it. For a handler that tells its command's chords apart (⌘↵ in a file being
/// edited is the editor's, F12 still goes to the definition).
pub fn pressed() -> Option<Keystroke> {
    PRESSED.with_borrow(|k| k.clone())
}

/// Latin script, as the old app's \p{Script=Latin}: the letters a European layout types.
fn is_latin_letter(c: char) -> bool {
    c.is_alphabetic()
        && matches!(c as u32,
            0x41..=0x5A | 0x61..=0x7A | 0xAA | 0xBA | 0xC0..=0x2AF | 0x1E00..=0x1EFF
            | 0x2C60..=0x2C7F | 0xA720..=0xA7FF | 0xAB30..=0xAB6F | 0xFB00..=0xFB06)
}

/// The letter to type instead when an in-editor command's chord, pressed in a code editor, types
/// a letter of the user's alphabet there (Polish ⌥Z is ż); a symbol (US ⌥Z is Ω) runs the command.
fn typed_letter(e: &KeystrokeEvent, cx: &App) -> Option<Keystroke> {
    let ks = &e.keystroke;
    // Typing without ⌥, ⌃ or ⌘ never reaches a command.
    let m = &ks.modifiers;
    if !(m.alt || m.control || m.platform) {
        return None;
    }
    let mut chars = ks.key_char.as_deref()?.chars();
    let letter = chars
        .next()
        .filter(|c| chars.next().is_none() && is_latin_letter(*c))?;
    let editing = e
        .context_stack
        .last()
        .and_then(|c| c.get("mode"))
        .is_some_and(|m| m == "editor");
    if !editing {
        return None;
    }
    let chord = edit::chord_of(ks, IS_MAC)?;
    let overrides = &Settings::get(cx).keybindings;
    all_commands()
        .any(|c| {
            chord::runs_in_editor(&chord, c) && bindings_for(c, overrides, IS_MAC).contains(&chord)
        })
        .then(|| Keystroke {
            modifiers: Modifiers::none(),
            key: letter.to_string(),
            key_char: Some(letter.to_string()),
        })
}

/// Whether a binding where focus is takes `keystroke`.
fn bound(keystroke: &Keystroke, e: &KeystrokeEvent, cx: &App) -> bool {
    let keymap = cx.key_bindings();
    let keymap = keymap.borrow();
    let (bindings, _) =
        keymap.bindings_for_input(std::slice::from_ref(keystroke), &e.context_stack);
    !bindings.is_empty()
}

/// A ⌥ key that types plain ASCII (German ⌥L is @, ⌥5 is [) is that character, not the chord on
/// its letter: a binding on ⌥L must not eat the @, in a text field or out of one. Only when
/// such a binding is there: otherwise the key goes on as it was, dead keys and all.
fn typed_ascii(e: &KeystrokeEvent, cx: &App) -> Option<Keystroke> {
    let ks = &e.keystroke;
    let m = &ks.modifiers;
    if !m.alt || m.control || m.platform || m.function {
        return None;
    }
    let typed = ks
        .key_char
        .as_deref()
        .filter(|c| c.len() == 1 && c.as_bytes()[0].is_ascii_graphic())?;
    if typed.eq_ignore_ascii_case(&ks.key) || !bound(ks, e, cx) {
        return None;
    }
    Some(Keystroke {
        modifiers: *m,
        key: typed.to_ascii_lowercase(),
        key_char: Some(typed.to_string()),
    })
}

/// A ⌘ or ⌃ chord on a punctuation key also counts as the key's US name, which the keyboard puts
/// in `key_char` (Turkish Q's ⌃" is ⌃`). Where both are bound the chord as typed wins: Turkish ⌘-
/// (the US = key) zooms out, though Zoom In is listed first.
fn typed_over_us_name(e: &KeystrokeEvent, cx: &App) -> Option<Keystroke> {
    let ks = &e.keystroke;
    let m = &ks.modifiers;
    if !(m.control || m.platform) || m.alt {
        return None;
    }
    let us = ks.key_char.as_deref().filter(|us| *us != ks.key)?;
    let typed = Keystroke {
        key_char: None,
        ..ks.clone()
    };
    // As the keymap matches a key_char: under ⌘ and ⌃ alone.
    let as_us = Keystroke {
        modifiers: Modifiers {
            control: m.control,
            platform: m.platform,
            ..Modifiers::default()
        },
        key: us.to_string(),
        key_char: None,
    };
    (bound(&typed, e, cx) && bound(&as_us, e, cx)).then_some(typed)
}

/// The key context every GitViber window root sets; global commands are bound under it.
pub const ROOT_CONTEXT: &str = "GitViber";
/// Contexts that type keys themselves. Feature crates put one of these on an element that takes
/// text (gpui-component's inputs already say "Input").
pub const INPUT_CONTEXT: &str = "Input";
pub const TERMINAL_CONTEXT: &str = "Terminal";

/// Marks our bindings in GPUI's keymap, so a rebinding replaces them and nothing else.
const META: KeyBindingMetaIndex = KeyBindingMetaIndex(0x6776);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Category {
    General,
    View,
    Tabs,
    Review,
    Diff,
    Editor,
    Terminal,
    Browser,
    Explorer,
    Git,
    GitHub,
    Help,
}

impl Category {
    pub fn label(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::View => "View",
            Self::Tabs => "Tabs",
            Self::Review => "Review",
            Self::Diff => "Diff",
            Self::Editor => "Editor",
            Self::Terminal => "Terminal",
            Self::Browser => "Browser",
            Self::Explorer => "Explorer",
            Self::Git => "Git",
            Self::GitHub => "GitHub",
            Self::Help => "Help",
        }
    }
}

/// One command: what the palette, the menu bar, the settings' shortcut list and the keymap read.
pub struct Command {
    /// The key a user's rebinding is stored under ("git.push"), as the old app had it.
    pub id: &'static str,
    pub title: &'static str,
    pub category: Category,
    /// The default chords on macOS, the first one shown.
    pub keys: &'static [&'static str],
    /// The defaults elsewhere, where they differ.
    pub keys_other: Option<&'static [&'static str]>,
    /// The key context it listens in, for a command only one place handles ("Terminal").
    pub local: Option<&'static str>,
    /// Never from a text field: they keep ⌘Z, ⌘⌫ and the like for themselves.
    pub outside_text: bool,
    /// Its keys go before the focused text field's own in its place: the field would take ⌘↵ as
    /// a new line, and the code editor ⇧⌘F as Replace.
    pub over_text: bool,
    /// Runs in a file being edited too, where its ⌥ key would type a symbol.
    pub in_editor: bool,
    /// Held down, it runs once: each run changes what the next one acts on. Handlers check it.
    pub no_repeat: bool,
    new_action: fn() -> Box<dyn Action>,
}

impl Command {
    pub const fn new(
        id: &'static str,
        title: &'static str,
        category: Category,
        keys: &'static [&'static str],
        new_action: fn() -> Box<dyn Action>,
    ) -> Self {
        Self {
            id,
            title,
            category,
            keys,
            keys_other: None,
            local: None,
            outside_text: false,
            over_text: false,
            in_editor: false,
            no_repeat: false,
            new_action,
        }
    }

    pub const fn other(mut self, keys: &'static [&'static str]) -> Self {
        self.keys_other = Some(keys);
        self
    }

    pub const fn local(mut self, context: &'static str) -> Self {
        self.local = Some(context);
        self
    }

    pub const fn outside_text(mut self) -> Self {
        self.outside_text = true;
        self
    }

    pub const fn over_text(mut self) -> Self {
        self.over_text = true;
        self
    }

    pub const fn in_editor(mut self) -> Self {
        self.in_editor = true;
        self
    }

    pub const fn no_repeat(mut self) -> Self {
        self.no_repeat = true;
        self
    }

    pub fn action(&self) -> Box<dyn Action> {
        (self.new_action)()
    }

    /// Where a local command works, as a person names it ("the terminal").
    pub fn place(&self) -> Option<&'static str> {
        self.local.map(place_name)
    }

    /// The platform's defaults.
    pub fn default_keys(&self, mac: bool) -> &'static [&'static str] {
        match self.keys_other {
            Some(other) if !mac => other,
            _ => self.keys,
        }
    }
}

/// A local command's key context as the overlay and the shortcut list name it; an unknown one
/// as it is.
pub fn place_name(context: &'static str) -> &'static str {
    match context {
        "Terminal" => "the terminal",
        "Browser" => "the browser",
        "TabStrip" => "the tab strip",
        "CodeView || DiffView" => "a diff or the code view",
        "ImageView" => "the image view",
        "Explorer" => "the explorer",
        "ChangesList" => "the changes list",
        "CommitMessage" => "the commit message",
        "TagMessage" => "the tag message",
        "BranchPicker" => "the branch picker",
        "WorktreePicker" => "the worktree picker",
        "HistoryList" => "the history list",
        "ProjectList" => "the project list",
        "IssueComment" => "a comment box",
        other => other,
    }
}

/// Per-command overrides of the defaults, as settings keep them.
pub type Overrides = BTreeMap<String, Vec<String>>;

/// Every command a user can bind or find in the palette, key commands first.
pub fn all_commands() -> impl Iterator<Item = &'static Command> {
    COMMANDS.iter().chain(MENU_COMMANDS)
}

pub fn command(id: &str) -> Option<&'static Command> {
    all_commands().chain(SYSTEM_COMMANDS).find(|c| c.id == id)
}

/// The command an action instance runs, by its type.
pub fn command_for_action(action: &dyn Action) -> Option<&'static Command> {
    let name = action.name();
    all_commands()
        .chain(SYSTEM_COMMANDS)
        .find(|c| c.action().name() == name)
}

/// Keeps only overrides of known commands whose chords all parse; the rest fall back to defaults.
pub fn clean_overrides(raw: &Overrides) -> Overrides {
    raw.iter()
        .filter(|(id, _)| all_commands().any(|c| c.id == id.as_str()))
        .filter_map(|(id, keys)| {
            let parsed: Option<Vec<String>> = keys.iter().map(|k| canonical(k)).collect();
            parsed.map(|keys| (id.clone(), keys))
        })
        .collect()
}

/// The chords a command runs on now: the user's, else the platform's defaults.
pub fn bindings_for(command: &Command, overrides: &Overrides, mac: bool) -> Vec<String> {
    match overrides.get(command.id) {
        Some(keys) => keys.clone(),
        None => command
            .default_keys(mac)
            .iter()
            .map(|k| k.to_string())
            .collect(),
    }
}

/// A command's first chord, formatted for a tooltip or a hint; None when it has none.
pub fn shortcut_label(id: &str, cx: &App) -> Option<String> {
    let command = command(id)?;
    let keys = bindings_for(command, &Settings::get(cx).keybindings, IS_MAC);
    keys.first().map(|k| format_chord(k, IS_MAC))
}

/// The context predicate a global chord is bound under: out of text fields and the terminal
/// unless it is a chord they leave alone.
pub fn context_for(chord: &str, command: &Command, mac: bool) -> String {
    let typing = command.local.is_some() || chord::runs_while_typing(chord, command, mac);
    let place = command.local.unwrap_or(ROOT_CONTEXT);
    let mut context = place.to_string();
    if command.local.is_none() {
        if !typing {
            context.push_str(" && !");
            context.push_str(INPUT_CONTEXT);
        }
        if !chord::runs_in_terminal(chord, command, mac) {
            context.push_str(" && !");
            context.push_str(TERMINAL_CONTEXT);
        }
        // A code editor's own key context (gpui-component's "Input mode=editor").
        if !typing && chord::runs_in_editor(chord, command) {
            context = format!("({context}) || ({place} > {INPUT_CONTEXT} && mode == editor)");
        }
    }
    // Matched at the field's depth, the chord outranks the field's own binding of it (GPUI lets
    // the deepest context win); ours are added after gpui-component's, so a tie goes to us too.
    if command.over_text && typing {
        context = format!("({context}) || (({place}) > {INPUT_CONTEXT})");
    }
    context
}

/// Our part of GPUI's keymap, in the order to add it: a later binding wins a shared keystroke,
/// so the commands go in reverse (the first listed wins), and within one command the first chord
/// goes first (the menu bar shows a command's earliest binding).
pub fn key_bindings(overrides: &Overrides, mac: bool) -> Vec<KeyBinding> {
    let mut out = Vec::new();
    let mut add = |command: &Command, chord: &str| {
        if !command.id.starts_with("app.")
            && !command.id.starts_with("window.")
            && chord::is_reserved(chord, mac)
        {
            return;
        }
        let keystroke = chord::to_keystroke(chord, mac);
        let context = context_for(chord, command, mac);
        let Ok(predicate) = KeyBindingContextPredicate::parse(&context) else {
            return;
        };
        if let Ok(binding) = KeyBinding::load(
            &keystroke,
            command.action(),
            Some(predicate.into()),
            false,
            None,
            &gpui_kit::DummyKeyboardMapper,
        ) {
            out.push(binding.with_meta(META));
        }
    };
    for command in all_commands().collect::<Vec<_>>().into_iter().rev() {
        for chord in bindings_for(command, overrides, mac) {
            add(command, &chord);
        }
    }
    for command in SYSTEM_COMMANDS {
        for chord in command.default_keys(mac) {
            add(command, chord);
        }
    }
    out
}

/// Puts the commands' bindings into GPUI's keymap, replacing the ones from before; other
/// crates' bindings stay.
pub fn install(cx: &mut App) {
    let overrides = Settings::get(cx).keybindings.clone();
    let ours = key_bindings(&overrides, IS_MAC);
    let keymap = cx.key_bindings();
    let theirs: Vec<KeyBinding> = keymap
        .borrow()
        .bindings()
        .filter(|b| b.meta() != Some(META))
        .cloned()
        .collect();
    cx.clear_key_bindings();
    cx.bind_keys(theirs);
    cx.bind_keys(ours);
}

pub(crate) fn init(cx: &mut App) {
    install(cx);
    // Not ours to replace on a rebinding (no META): install keeps them.
    cx.bind_keys(find_bindings(IS_MAC));
    // For the app's life, holding nothing.
    cx.intercept_keystrokes(|e, window, cx| {
        PRESSED.set(Some(e.keystroke.clone()));
        let instead = typed_letter(e, cx)
            .or_else(|| typed_ascii(e, cx))
            .or_else(|| typed_over_us_name(e, cx));
        if let Some(keystroke) = instead {
            cx.stop_propagation();
            window.defer(cx, move |window, cx| {
                window.dispatch_keystroke(keystroke, cx);
            });
        }
    })
    .detach();
    cx.observe_keystrokes(|_, _, _| PRESSED.set(None)).detach();
    let mut last = Settings::get(cx).keybindings.clone();
    cx.observe_global::<Settings>(move |cx| {
        let now = &Settings::get(cx).keybindings;
        if *now != last {
            last = now.clone();
            install(cx);
        }
    })
    .detach();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_actions_distinct() {
        let mut ids: Vec<&str> = all_commands()
            .chain(SYSTEM_COMMANDS)
            .map(|c| c.id)
            .collect();
        let n = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), n);
        let mut names: Vec<&str> = all_commands().map(|c| c.action().name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), all_commands().count());
    }

    #[test]
    fn every_default_chord_is_canonical_and_binds() {
        for command in all_commands().chain(SYSTEM_COMMANDS) {
            for mac in [true, false] {
                for key in command.default_keys(mac) {
                    assert_eq!(canonical(key).as_deref(), Some(*key), "{}", command.id);
                    let ks = chord::to_keystroke(key, mac);
                    assert!(
                        gpui_kit::Keystroke::parse(&ks).is_ok(),
                        "{} {ks}",
                        command.id
                    );
                }
            }
        }
        let bindings = key_bindings(&Overrides::new(), true);
        assert!(bindings.len() > 100, "{}", bindings.len());
    }

    #[test]
    fn local_contexts_parse_so_their_keys_bind() {
        for command in all_commands().filter(|c| c.local.is_some()) {
            let context = command.local.unwrap();
            assert!(
                KeyBindingContextPredicate::parse(context).is_ok(),
                "{} {context}",
                command.id
            );
            // Shown to people: never the raw context.
            assert_ne!(command.place(), Some(context), "{}", command.id);
        }
    }

    /// The old app's commands as v0.1.17 shipped them (`git show v0.1.17:src/lib/commands/commands.ts`,
    /// its COMMANDS list): each one's default chords and the flags that place them.
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct OldCommand {
        id: String,
        keys: Vec<String>,
        keys_other: Option<Vec<String>>,
        #[serde(default)]
        local: bool,
        #[serde(default)]
        outside_text: bool,
        #[serde(default)]
        in_editor: bool,
        #[serde(default)]
        no_repeat: bool,
    }

    #[test]
    fn every_default_is_the_old_apps() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/src/keymap/old_defaults.json");
        let text = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("{path}: {e}; the old app's defaults are the guard"));
        let old: Vec<OldCommand> = serde_json::from_str(&text).unwrap();
        assert_eq!(old.len(), 153, "the fixture is v0.1.17's whole list");
        // The page's reload: there's no page to reload in the native app.
        const DROPPED: &[&str] = &["window.reload"];
        // The old app's terminal tabs were a Tab stop of the page; here a pane's Tab is the shell's.
        const NEW: &[&str] = &["terminal.focusTabs"];
        // The old app's Linux "ctrl+alt" was Super+Alt, not the Ctrl+Alt its docs named.
        const MOVED_OFF_SUPER: &[(&str, &str)] = &[
            ("terminal.prevPane", "alt+cmd+left"),
            ("terminal.nextPane", "alt+cmd+right"),
        ];
        // The window's zoom keys added to the image view's (there they zoomed the window).
        const ADDED: &[(&str, &[&str])] = &[
            ("media.zoomIn", &["cmd+=", "shift+cmd+="]),
            ("media.zoomOut", &["cmd+-"]),
            ("media.fit", &["cmd+0"]),
        ];
        let mut seen = 0;
        for o in &old {
            let id = o.id.as_str();
            if DROPPED.contains(&id) {
                assert!(command(id).is_none(), "{id} is dropped");
                continue;
            }
            let c = command(id).unwrap_or_else(|| panic!("{id} missing"));
            let mut keys = o.keys.clone();
            if let Some((_, added)) = ADDED.iter().find(|(a, _)| *a == id) {
                keys.extend(added.iter().map(|k| k.to_string()));
            }
            assert_eq!(c.keys, keys.as_slice(), "{id} keys");
            let other = match MOVED_OFF_SUPER.iter().find(|(m, _)| *m == id) {
                Some((_, key)) => Some(vec![key.to_string()]),
                None => o.keys_other.clone(),
            };
            let ours = c
                .keys_other
                .map(|k| k.iter().map(|k| k.to_string()).collect::<Vec<_>>());
            assert_eq!(ours, other, "{id} keysOther");
            assert_eq!(c.local.is_some(), o.local, "{id} local");
            assert_eq!(c.outside_text, o.outside_text, "{id} outsideText");
            assert_eq!(c.in_editor, o.in_editor, "{id} inEditor");
            assert_eq!(c.no_repeat, o.no_repeat, "{id} noRepeat");
            seen += 1;
        }
        for id in NEW {
            assert!(command(id).is_some(), "{id} missing");
        }
        assert_eq!(seen + NEW.len(), COMMANDS.len());
    }

    /// GPUI lets the later of two bindings win a keystroke, so the commands go in reverse: the one
    /// listed first runs a chord two share, and a local command never runs out of its place.
    #[test]
    fn the_first_listed_command_wins_a_shared_chord() {
        let winner = |chord: &str, overrides: &Overrides, context: &str| {
            let keymap = gpui_kit::Keymap::new(key_bindings(overrides, true));
            let stack = [gpui_kit::KeyContext::parse(context).unwrap()];
            let typed = gpui_kit::Keystroke::parse(&chord::to_keystroke(chord, true)).unwrap();
            let (bindings, _) = keymap.bindings_for_input(&[typed], &stack);
            bindings
                .first()
                .and_then(|b| command_for_action(b.action()))
                .map(|c| c.id)
        };
        let mut overrides = Overrides::new();
        overrides.insert("view.toggleGitPanel".into(), vec!["cmd+1".into()]);
        assert_eq!(
            winner("cmd+1", &overrides, ROOT_CONTEXT),
            Some("view.toggleGitPanel")
        );
        assert_eq!(
            edit::winner("cmd+1", &overrides, true).map(|c| c.id),
            Some("view.toggleGitPanel")
        );
        let mut overrides = Overrides::new();
        overrides.insert("tab.goto1".into(), vec!["cmd+b".into()]);
        assert_eq!(
            winner("cmd+b", &overrides, ROOT_CONTEXT),
            Some("view.toggleGitPanel")
        );
        assert_eq!(
            winner("cmd+1", &Overrides::new(), ROOT_CONTEXT),
            Some("tab.goto1")
        );
        // ⌘↵ is the commit box's, never the window's.
        assert_eq!(winner("cmd+enter", &Overrides::new(), ROOT_CONTEXT), None);
        assert!(edit::winner("cmd+enter", &Overrides::new(), true).is_none());
    }

    #[test]
    fn overrides_replace_and_unbind() {
        let push = command("git.push").unwrap();
        let mut overrides = Overrides::new();
        assert_eq!(bindings_for(push, &overrides, true), vec!["ctrl+cmd+p"]);
        assert!(bindings_for(push, &overrides, false).is_empty());
        overrides.insert("git.push".into(), vec!["shift+cmd+u".into()]);
        assert_eq!(bindings_for(push, &overrides, true), vec!["shift+cmd+u"]);
        overrides.insert("git.push".into(), vec![]);
        assert!(bindings_for(push, &overrides, true).is_empty());
    }

    #[test]
    fn cleaning_drops_unknown_ids_and_bad_chords() {
        let mut raw = Overrides::new();
        raw.insert("git.push".into(), vec!["CMD+Shift+U".into()]);
        raw.insert("no.such".into(), vec!["cmd+u".into()]);
        raw.insert("git.pull".into(), vec!["hyper+u".into()]);
        raw.insert("git.fetch".into(), vec![]);
        let clean = clean_overrides(&raw);
        assert_eq!(
            clean.get("git.push").unwrap(),
            &vec!["shift+cmd+u".to_string()]
        );
        assert!(!clean.contains_key("no.such"));
        assert!(!clean.contains_key("git.pull"));
        assert_eq!(clean.get("git.fetch").unwrap(), &Vec::<String>::new());
    }

    #[test]
    fn contexts_keep_letters_out_of_text_and_terminal() {
        let next = command("review.nextFile").unwrap();
        assert_eq!(
            context_for("j", next, true),
            "GitViber && !Input && !Terminal"
        );
        let palette = command("workbench.showCommands").unwrap();
        assert_eq!(context_for("shift+cmd+p", palette, true), "GitViber");
        let split = command("terminal.split").unwrap();
        assert_eq!(context_for("cmd+d", split, true), "Terminal");
        let undo = command("git.undo").unwrap();
        assert_eq!(
            context_for("cmd+z", undo, true),
            "GitViber && !Input && !Terminal"
        );
        // ⌥Z types a symbol in a code editor, so Toggle Word Wrap takes it there too.
        let wrap = command("editor.toggleWrap").unwrap();
        let context = context_for("alt+z", wrap, true);
        assert_eq!(
            context,
            "(GitViber && !Input && !Terminal) || (GitViber > Input && mode == editor)"
        );
        assert!(KeyBindingContextPredicate::parse(&context).is_ok());
        // Not a plain text field's ⌥Z, nor an in-editor command's letter alone.
        assert!(!chord::runs_in_editor("z", wrap));
        assert!(!chord::runs_in_editor("alt+z", undo));
    }

    #[test]
    fn a_latin_letter_types_where_a_symbol_runs_the_command() {
        for c in ['z', 'ż', 'Ż', 'å', 'ß', 'ő', 'ẞ'] {
            assert!(is_latin_letter(c), "{c}");
        }
        for c in ['Ω', '≈', '÷', '×', 'я', '1', ' '] {
            assert!(!is_latin_letter(c), "{c}");
        }
    }

    #[test]
    fn find_keys_bind_where_a_find_box_can_be() {
        assert!(KeyBindingContextPredicate::parse(FIND_PLACES).is_ok());
        for mac in [true, false] {
            let [next, prev] = find_bindings(mac);
            assert!(next.action().partial_eq(&find::Next));
            assert!(prev.action().partial_eq(&find::Prev));
        }
    }

    #[test]
    fn over_text_chords_match_at_the_fields_depth_too() {
        let commit = command("git.commit").unwrap();
        assert_eq!(
            context_for("cmd+enter", commit, true),
            "(CommitMessage) || ((CommitMessage) > Input)"
        );
        let in_files = command("search.findInFiles").unwrap();
        assert_eq!(
            context_for("shift+cmd+f", in_files, true),
            "(GitViber) || ((GitViber) > Input)"
        );
        // A chord kept out of text fields stays out of them.
        assert_eq!(
            context_for("j", in_files, true),
            "GitViber && !Input && !Terminal"
        );
        for c in all_commands().filter(|c| c.over_text) {
            for k in c.default_keys(true) {
                let context = context_for(k, c, true);
                assert!(
                    KeyBindingContextPredicate::parse(&context).is_ok(),
                    "{} {context}",
                    c.id
                );
            }
        }
    }

    #[test]
    fn linux_keeps_ctrl_alt_and_leaves_macos_system_keys_alone() {
        let prev = command("terminal.prevPane").unwrap();
        assert_eq!(
            chord::to_keystroke(prev.default_keys(false)[0], false),
            "alt-ctrl-left"
        );
        for command in all_commands().chain(SYSTEM_COMMANDS) {
            for key in command.default_keys(false) {
                let ks = chord::to_keystroke(key, false);
                assert!(
                    !(ks.contains("super") && ks.contains("alt")),
                    "{} {ks}",
                    command.id
                );
            }
        }
        let system: Vec<&str> = SYSTEM_COMMANDS
            .iter()
            .filter(|c| !c.default_keys(false).is_empty())
            .map(|c| c.id)
            .collect();
        assert_eq!(system, ["app.quit"]);
    }

    /// A window whose focus sits in `context`, under the root, keeping what our commands ran.
    struct Keys {
        focus: gpui_kit::FocusHandle,
        context: &'static str,
        ran: std::rc::Rc<RefCell<Vec<&'static str>>>,
    }

    fn log<A: Action>(
        ran: &std::rc::Rc<RefCell<Vec<&'static str>>>,
        name: &'static str,
    ) -> impl Fn(&A, &mut gpui_kit::Window, &mut App) + 'static {
        let ran = ran.clone();
        move |_, _, _| ran.borrow_mut().push(name)
    }

    impl gpui_kit::Render for Keys {
        fn render(
            &mut self,
            _: &mut gpui_kit::Window,
            _: &mut gpui_kit::Context<Self>,
        ) -> impl gpui_kit::IntoElement {
            use gpui_kit::{InteractiveElement as _, ParentElement as _, Styled as _};
            gpui_kit::div()
                .key_context(ROOT_CONTEXT)
                .size_full()
                .on_action(log::<view::ZoomIn>(&self.ran, "zoom in"))
                .on_action(log::<view::ZoomOut>(&self.ran, "zoom out"))
                .on_action(log::<terminal::Toggle>(&self.ran, "terminal"))
                .on_action(log::<editor::ToggleWrap>(&self.ran, "wrap"))
                .child(
                    gpui_kit::div()
                        .key_context(self.context)
                        .track_focus(&self.focus)
                        .size_full(),
                )
        }
    }

    /// What `keystroke`, as the keyboard reports it, runs with focus in `context`, and the
    /// keystrokes the window took, a key handed on as another one included.
    fn press(
        cx: &mut gpui_kit::TestAppContext,
        context: &'static str,
        keystroke: Keystroke,
    ) -> (Vec<&'static str>, Vec<String>) {
        let ran = std::rc::Rc::new(RefCell::new(Vec::new()));
        let keys = ran.clone();
        let (view, vcx) = cx.add_window_view(|_, cx| Keys {
            focus: cx.focus_handle(),
            context,
            ran: keys,
        });
        vcx.update(|window, cx| {
            let focus = view.read(cx).focus.clone();
            window.focus(&focus, cx);
        });
        vcx.run_until_parked();
        let seen = std::rc::Rc::new(RefCell::new(Vec::new()));
        let log = seen.clone();
        let _watch = vcx.update(|_, cx| {
            cx.observe_keystrokes(move |e, _, _| {
                let k = &e.keystroke;
                let m = &k.modifiers;
                let mut shown: String =
                    [(m.control, "ctrl-"), (m.alt, "alt-"), (m.platform, "cmd-")]
                        .iter()
                        .filter(|(on, _)| *on)
                        .map(|(_, name)| *name)
                        .collect();
                shown.push_str(&format!(
                    "{}->{}",
                    k.key,
                    k.key_char.as_deref().unwrap_or("")
                ));
                log.borrow_mut().push(shown);
            })
        });
        let window = vcx.update(|window, _| window.window_handle());
        vcx.dispatch_keystroke(window, keystroke);
        vcx.run_until_parked();
        let ran = ran.borrow().clone();
        (ran, seen.borrow().clone())
    }

    fn keystroke(key: &str, key_char: Option<&str>, f: impl FnOnce(&mut Modifiers)) -> Keystroke {
        let mut modifiers = Modifiers::default();
        f(&mut modifiers);
        Keystroke {
            modifiers,
            key: key.into(),
            key_char: key_char.map(Into::into),
        }
    }

    fn layouts_setup(cx: &mut gpui_kit::TestAppContext, name: &str) {
        crate::test_support::config_dir(name);
        cx.update(|cx| {
            crate::settings::init(cx);
            init(cx);
        });
        // Its observer starts with the next update.
        cx.update(|cx| {
            Settings::update(cx, |s| {
                s.keybindings
                    .insert("editor.toggleWrap".into(), vec!["alt+l".into()]);
            })
        });
    }

    /// On Turkish Q the key left of 1 types ", so ⌃` arrives as ⌃" with the key's US name: it still
    /// toggles the terminal. The ⌘ key the layout calls - is the US = key: Zoom Out, as typed,
    /// though Zoom In comes first and takes ⌘=.
    #[gpui_kit::test]
    fn a_punctuation_key_counts_as_its_us_name_and_as_typed_first(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        layouts_setup(cx, "keymap-us-names");
        let ctrl = |m: &mut Modifiers| m.control = true;
        let cmd = |m: &mut Modifiers| m.platform = true;
        let (ran, _) = press(cx, "Pane", keystroke("\"", Some("`"), ctrl));
        assert_eq!(ran, ["terminal"]);
        // In the terminal too: ⌃` is the app's there.
        let (ran, _) = press(cx, TERMINAL_CONTEXT, keystroke("\"", Some("`"), ctrl));
        assert_eq!(ran, ["terminal"]);
        let (ran, _) = press(cx, "Pane", keystroke("-", Some("="), cmd));
        assert_eq!(ran, ["zoom out"]);
        // The US * key on Turkish Q is the US - key: Zoom Out, by its US name.
        let (ran, _) = press(cx, "Pane", keystroke("*", Some("-"), cmd));
        assert_eq!(ran, ["zoom out"]);
        let (ran, _) = press(cx, "Pane", keystroke("=", None, cmd));
        assert_eq!(ran, ["zoom in"]);
    }

    /// German ⌥L types @: with Toggle Word Wrap bound to ⌥L, the @ is typed, in a code editor and
    /// out of a field alike, while a US ⌥L (¬, a symbol) still wraps.
    #[gpui_kit::test]
    fn a_layouts_ascii_on_option_is_typed_not_a_binding(cx: &mut gpui_kit::TestAppContext) {
        layouts_setup(cx, "keymap-option-ascii");
        let alt = |m: &mut Modifiers| m.alt = true;
        for context in ["Input mode=editor", "Pane"] {
            let (ran, seen) = press(cx, context, keystroke("l", Some("@"), alt));
            assert!(ran.is_empty(), "{context}: {ran:?}");
            assert_eq!(
                seen.last().map(String::as_str),
                Some("alt-@->@"),
                "{context}"
            );
        }
        let (ran, _) = press(cx, "Input mode=editor", keystroke("l", Some("¬"), alt));
        assert_eq!(ran, ["wrap"]);
        let (ran, _) = press(cx, "Pane", keystroke("l", Some("¬"), alt));
        assert_eq!(ran, ["wrap"]);
        // With nothing bound to the letter's chord the key goes on as it was.
        let (_, seen) = press(cx, "Pane", keystroke("q", Some("@"), alt));
        assert_eq!(seen, ["alt-q->@"]);
    }

    #[test]
    fn actions_map_back_to_their_command() {
        let action = git::Pull.boxed_clone();
        assert_eq!(command_for_action(action.as_ref()).unwrap().id, "git.pull");
        assert_eq!(git::Pull.name(), "git::Pull");
    }
}
