//! The sets of default keys a user picks between in Settings. A preset is a table of the commands
//! whose keys differ from the classic ones each command lists; the user's rebindings go over it.

use gpui_kit::App;
use serde::{Deserialize, Serialize};

use super::Command;
use crate::settings::Settings;
use crate::state::UiState;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Preset {
    #[default]
    #[serde(rename = "vscode")]
    VsCode,
    #[serde(rename = "classic")]
    Classic,
}

impl Preset {
    pub const ALL: &[Preset] = &[Preset::VsCode, Preset::Classic];

    /// The name settings store it under.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::VsCode => "vscode",
            Self::Classic => "classic",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|p| p.as_str() == text)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::VsCode => "VS Code",
            Self::Classic => "GitViber (classic)",
        }
    }

    fn table(self) -> &'static [Keys] {
        match self {
            Self::VsCode => VSCODE,
            Self::Classic => &[],
        }
    }

    /// The chords `command` runs on by default in this preset, for the platform.
    pub fn keys(self, command: &Command, mac: bool) -> &'static [&'static str] {
        match self.table().iter().find(|k| k.id == command.id) {
            Some(k) => match k.other {
                Some(other) if !mac => other,
                _ => k.mac,
            },
            None => command.default_keys(mac),
        }
    }

    /// The command ids the preset changes.
    pub fn changed(self) -> impl Iterator<Item = &'static str> {
        self.table().iter().map(|k| k.id)
    }
}

/// One command's keys in a preset: macOS's, and elsewhere's when they differ ("cmd" is Ctrl there).
pub struct Keys {
    id: &'static str,
    mac: &'static [&'static str],
    other: Option<&'static [&'static str]>,
}

const fn keys(id: &'static str, mac: &'static [&'static str]) -> Keys {
    Keys {
        id,
        mac,
        other: None,
    }
}

impl Keys {
    const fn other(mut self, keys: &'static [&'static str]) -> Self {
        self.other = Some(keys);
        self
    }
}

/// Each panel has a key that shows or hides it and one that focuses it: the git panel ⌘B and
/// ⌘0, the explorer ⌘E (and VS Code's ⌥⌘B) and ⇧⌘E, the terminal ⌘J and ⌃`. Tabs go by ⌃1–⌃8
/// from anywhere; ⌘1–⌘9 pick the focused area's (the terminal's, the code area's) as in every
/// terminal and browser, and elsewhere ⌘1 focuses the code. The terminal's pane keys stay: they only listen in the terminal, so
/// ⌥⌘←/→ there move between panes and elsewhere between tabs.
static VSCODE: &[Keys] = &[
    keys("workbench.showCommands", &["shift+cmd+p", "f1"]),
    keys("file.switchProject", &["ctrl+r"]).other(&["cmd+r"]),
    keys("file.reveal", &["alt+cmd+r"]).other(&[]),
    keys("view.changes", &["ctrl+shift+g"]).other(&["shift+cmd+g"]),
    keys("view.history", &[]),
    keys("view.pulls", &[]),
    keys("view.issues", &[]),
    keys("view.focusGitPanel", &["cmd+0"]),
    keys("view.toggleExplorer", &["cmd+e", "alt+cmd+b"]),
    keys("view.focusCode", &["cmd+1"]),
    keys("view.zoomReset", &[]),
    keys("tab.goto1", &["ctrl+1", "cmd+1"]).other(&["alt+1", "cmd+1"]),
    keys("tab.goto2", &["ctrl+2", "cmd+2"]).other(&["alt+2", "cmd+2"]),
    keys("tab.goto3", &["ctrl+3", "cmd+3"]).other(&["alt+3", "cmd+3"]),
    keys("tab.goto4", &["ctrl+4", "cmd+4"]).other(&["alt+4", "cmd+4"]),
    keys("tab.goto5", &["ctrl+5", "cmd+5"]).other(&["alt+5", "cmd+5"]),
    keys("tab.goto6", &["ctrl+6", "cmd+6"]).other(&["alt+6", "cmd+6"]),
    keys("tab.goto7", &["ctrl+7", "cmd+7"]).other(&["alt+7", "cmd+7"]),
    keys("tab.goto8", &["ctrl+8", "cmd+8"]).other(&["alt+8", "cmd+8"]),
    keys("tab.last", &["ctrl+0", "ctrl+9", "cmd+9"]).other(&["alt+0", "alt+9", "cmd+9"]),
    keys("tab.next", &["alt+cmd+right", "shift+cmd+]", "ctrl+tab"])
        .other(&["cmd+pagedown", "cmd+tab"]),
    keys(
        "tab.prev",
        &["alt+cmd+left", "shift+cmd+[", "ctrl+shift+tab"],
    )
    .other(&["cmd+pageup", "shift+cmd+tab"]),
    keys("tab.reopenClosed", &["shift+cmd+t"]),
    keys("diff.nextChange", &["f7", "alt+f5", "alt+down"]),
    keys("diff.prevChange", &["shift+f7", "alt+shift+f5", "alt+up"]),
    keys("terminal.new", &["ctrl+shift+`", "cmd+t"]).other(&["shift+cmd+`"]),
    keys("terminal.split", &["cmd+\\", "cmd+d"]).other(&["shift+cmd+5", "shift+cmd+d"]),
    keys("browser.open", &[]),
];

/// The one-time word for a user from before presets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwitchNotice {
    /// Their keys moved to the VS Code preset.
    Moved,
    /// They had rebound keys, so they stay on the classic preset (`Settings::from_value`).
    KeptClassic,
}

/// What to tell a user from before presets about their keys, at each launch until they close the
/// word (`notice_closed`): a toast that went with the window mustn't be the only telling. A new
/// install never hears it.
pub fn switch_notice(cx: &mut App) -> Option<SwitchNotice> {
    let state = UiState::get(cx);
    if state.keymap_preset_told {
        return None;
    }
    let s = Settings::get(cx);
    let existing =
        state.window.is_some() || !state.recent_projects.is_empty() || !s.keybindings.is_empty();
    let notice = match s.keymap_preset {
        _ if !existing => None,
        Preset::VsCode => Some(SwitchNotice::Moved),
        Preset::Classic if !s.keybindings.is_empty() => Some(SwitchNotice::KeptClassic),
        Preset::Classic => None,
    };
    if notice.is_none() {
        notice_closed(cx);
    }
    notice
}

/// The word was read: a button on it or its close button.
pub fn notice_closed(cx: &mut App) {
    UiState::update(cx, |s| s.keymap_preset_told = true);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn launch(cx: &mut gpui_kit::TestAppContext, name: &str, before: Option<UiState>) {
        crate::test_support::config_dir(name);
        cx.update(|cx| {
            crate::settings::init(cx);
            crate::state::init(cx);
            if let Some(state) = before {
                cx.set_global(state);
            }
        });
    }

    #[gpui_kit::test]
    fn a_user_from_before_presets_is_told_until_they_close_it(cx: &mut gpui_kit::TestAppContext) {
        let used = UiState {
            recent_projects: vec!["/p".into()],
            ..UiState::default()
        };
        launch(cx, "preset-told", Some(used));
        assert_eq!(
            cx.update(|cx| Settings::get(cx).keymap_preset),
            Preset::VsCode
        );
        // Asked again at each launch until the word is closed, then never.
        assert_eq!(cx.update(switch_notice), Some(SwitchNotice::Moved));
        assert_eq!(cx.update(switch_notice), Some(SwitchNotice::Moved));
        cx.update(notice_closed);
        assert_eq!(cx.update(switch_notice), None);
        assert!(cx.update(|cx| UiState::get(cx).keymap_preset_told));
    }

    #[gpui_kit::test]
    fn a_user_who_rebound_keys_stays_classic(cx: &mut gpui_kit::TestAppContext) {
        let dir = crate::test_support::config_dir("preset-rebound");
        let old = r#"{"keybindings": {"git.push": ["cmd+u"]}}"#;
        std::fs::write(dir.join("settings.json"), old).unwrap();
        cx.update(|cx| {
            crate::settings::init(cx);
            crate::state::init(cx);
        });
        assert_eq!(
            cx.update(|cx| Settings::get(cx).keymap_preset),
            Preset::Classic
        );
        assert_eq!(cx.update(switch_notice), Some(SwitchNotice::KeptClassic));
    }

    #[gpui_kit::test]
    fn a_new_install_is_never_told(cx: &mut gpui_kit::TestAppContext) {
        launch(cx, "preset-new-install", None);
        assert_eq!(cx.update(switch_notice), None);
        assert!(cx.update(|cx| UiState::get(cx).keymap_preset_told));
    }

    #[test]
    fn presets_read_and_write_by_name() {
        for p in Preset::ALL {
            assert_eq!(Preset::parse(p.as_str()), Some(*p));
            assert_eq!(serde_json::to_value(p).unwrap(), p.as_str());
        }
        let s = Settings::from_json(r#"{"keymapPreset": "classic"}"#);
        assert_eq!(s.keymap_preset, Preset::Classic);
        // A file from before presets, or a name this build doesn't know: the default.
        assert_eq!(Settings::from_json("{}").keymap_preset, Preset::VsCode);
        let s = Settings::from_json(r#"{"keymapPreset": "emacs"}"#);
        assert_eq!(s.keymap_preset, Preset::VsCode);
        // Keys rebound before presets were made over the classic ones; a picked preset stays.
        let rebound = r#""keybindings": {"git.push": ["cmd+u"]}"#;
        let s = Settings::from_json(&format!("{{{rebound}}}"));
        assert_eq!(s.keymap_preset, Preset::Classic);
        let s = Settings::from_json(&format!(r#"{{"keymapPreset": "vscode", {rebound}}}"#));
        assert_eq!(s.keymap_preset, Preset::VsCode);
    }
}
