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

/// The left panel is the primary side bar (⌘B, ⌘0 focuses it), the right one the secondary
/// (⌥⌘B). Tabs go by ⌃1–⌃8 and ⌘1 focuses the code. The terminal's pane keys stay: they only
/// listen in the terminal, so ⌥⌘←/→ there move between panes and elsewhere between tabs.
static VSCODE: &[Keys] = &[
    keys("workbench.showCommands", &["shift+cmd+p", "f1"]),
    keys("file.switchProject", &["ctrl+r"]).other(&["cmd+r"]),
    keys("file.reveal", &["alt+cmd+r"]).other(&[]),
    keys("view.changes", &["ctrl+shift+g"]).other(&["shift+cmd+g"]),
    keys("view.history", &[]),
    keys("view.pulls", &[]),
    keys("view.issues", &[]),
    keys("view.focusGitPanel", &["cmd+0"]),
    keys("view.focusCode", &["cmd+1", "cmd+e"]),
    keys("view.zoomReset", &[]),
    keys("tab.goto1", &["ctrl+1"]).other(&["alt+1"]),
    keys("tab.goto2", &["ctrl+2"]).other(&["alt+2"]),
    keys("tab.goto3", &["ctrl+3"]).other(&["alt+3"]),
    keys("tab.goto4", &["ctrl+4"]).other(&["alt+4"]),
    keys("tab.goto5", &["ctrl+5"]).other(&["alt+5"]),
    keys("tab.goto6", &["ctrl+6"]).other(&["alt+6"]),
    keys("tab.goto7", &["ctrl+7"]).other(&["alt+7"]),
    keys("tab.goto8", &["ctrl+8"]).other(&["alt+8"]),
    keys("tab.last", &["ctrl+0"]).other(&["alt+0"]),
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
    keys("terminal.toggle", &["ctrl+`", "cmd+j"]).other(&["cmd+`", "cmd+j"]),
    keys("terminal.new", &["ctrl+shift+`", "cmd+t"]).other(&["shift+cmd+`"]),
    keys("terminal.split", &["cmd+\\", "cmd+d"]).other(&["shift+cmd+5", "shift+cmd+d"]),
    keys("browser.open", &[]),
];

/// Once per install: true when this is a user from before presets, whose keys just moved to the
/// default one; the caller tells them where the classic keys are. A new install never hears it.
pub fn take_switch_notice(cx: &mut App) -> bool {
    if UiState::get(cx).keymap_preset_told {
        return false;
    }
    let state = UiState::get(cx);
    let existing = state.window.is_some() || !state.recent_projects.is_empty();
    UiState::update(cx, |s| s.keymap_preset_told = true);
    existing && Settings::get(cx).keymap_preset != Preset::Classic
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
    fn a_user_from_before_presets_is_told_once(cx: &mut gpui_kit::TestAppContext) {
        let used = UiState {
            recent_projects: vec!["/p".into()],
            ..UiState::default()
        };
        launch(cx, "preset-told-once", Some(used));
        assert_eq!(
            cx.update(|cx| Settings::get(cx).keymap_preset),
            Preset::VsCode
        );
        assert!(cx.update(take_switch_notice));
        assert!(!cx.update(take_switch_notice));
        assert!(cx.update(|cx| UiState::get(cx).keymap_preset_told));
    }

    #[gpui_kit::test]
    fn a_new_install_is_never_told(cx: &mut gpui_kit::TestAppContext) {
        launch(cx, "preset-new-install", None);
        assert!(!cx.update(take_switch_notice));
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
    }
}
