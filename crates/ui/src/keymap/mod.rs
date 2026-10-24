//! The command registry and the keymap built from it. Defaults per platform, the user's
//! rebindings from settings (`keybindings`: command id → chords, an empty list unbinds), and the
//! key contexts that keep letters and ⌥ chords out of text fields and the terminal.

pub mod chord;
mod commands;
pub mod edit;

use std::collections::BTreeMap;

use gpui_kit::{Action, App, KeyBinding, KeyBindingContextPredicate, KeyBindingMetaIndex};

pub use chord::{IS_MAC, canonical, chord_keys, format_chord};
pub use commands::*;

use crate::settings::Settings;

/// What a feature crate asks of the app, which it can't call: dispatch one from a window.
pub mod requests {
    // AskGitIdentity: git refused a commit for want of a name or email; the app asks for them,
    // even after the user closed that ask once.
    gpui_kit::actions!(requests, [AskGitIdentity]);
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

    /// The platform's defaults.
    pub fn default_keys(&self, mac: bool) -> &'static [&'static str] {
        match self.keys_other {
            Some(other) if !mac => other,
            _ => self.keys,
        }
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
    if let Some(local) = command.local {
        return local.to_string();
    }
    let mut context = ROOT_CONTEXT.to_string();
    if !chord::runs_while_typing(chord, command, mac) {
        context.push_str(" && !");
        context.push_str(INPUT_CONTEXT);
    }
    if !chord::runs_in_terminal(chord, command, mac) {
        context.push_str(" && !");
        context.push_str(TERMINAL_CONTEXT);
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

    /// The quoted strings of the list after `field: [` on `line`, brackets in them included.
    fn ts_list(line: &str, field: &str) -> Option<Vec<String>> {
        let start = line.find(&format!("{field}: ["))? + field.len() + 3;
        let mut out = Vec::new();
        let mut chars = line[start..].chars();
        loop {
            match chars.next()? {
                ']' => return Some(out),
                '"' => out.push(chars.by_ref().take_while(|c| *c != '"').collect()),
                _ => {}
            }
        }
    }

    fn ts_field<'a>(line: &'a str, field: &str) -> Option<&'a str> {
        let start = line.find(&format!("{field}: \""))? + field.len() + 3;
        Some(&line[start..start + line[start..].find('"')?])
    }

    #[test]
    fn every_default_is_the_old_apps() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../src/lib/commands/commands.ts"
        );
        // The old app goes once the port is done; until then each default must match it.
        let Ok(ts) = std::fs::read_to_string(path) else {
            return;
        };
        let body = &ts[ts.find("export const COMMANDS = [").unwrap()
            ..ts.find("] as const satisfies").unwrap()];
        // The page's reload: there's no page to reload in the native app.
        const DROPPED: &[&str] = &["window.reload"];
        let mut seen = 0;
        for line in body
            .lines()
            .map(str::trim)
            .filter(|l| l.starts_with("{ id: \""))
        {
            let id = ts_field(line, "id").unwrap();
            if DROPPED.contains(&id) {
                assert!(command(id).is_none(), "{id} is dropped");
                continue;
            }
            let c = command(id).unwrap_or_else(|| panic!("{id} missing"));
            let keys = ts_list(line, "keys").unwrap();
            assert_eq!(
                c.keys,
                keys.iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>()
                    .as_slice(),
                "{id} keys"
            );
            let other = ts_list(line, "keysOther");
            let ours = c
                .keys_other
                .map(|k| k.iter().map(|k| k.to_string()).collect::<Vec<_>>());
            assert_eq!(ours, other, "{id} keysOther");
            assert_eq!(
                c.local.is_some(),
                ts_field(line, "local").is_some(),
                "{id} local"
            );
            assert_eq!(
                c.outside_text,
                line.contains("outsideText: true"),
                "{id} outsideText"
            );
            assert_eq!(
                c.in_editor,
                line.contains("inEditor: true"),
                "{id} inEditor"
            );
            assert_eq!(
                c.no_repeat,
                line.contains("noRepeat: true"),
                "{id} noRepeat"
            );
            seen += 1;
        }
        assert_eq!(seen, COMMANDS.len());
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
    }

    #[test]
    fn actions_map_back_to_their_command() {
        let action = git::Pull.boxed_clone();
        assert_eq!(command_for_action(action.as_ref()).unwrap().id, "git.pull");
        assert_eq!(git::Pull.name(), "git::Pull");
    }
}
