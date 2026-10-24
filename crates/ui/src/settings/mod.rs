//! The user's settings: the old app's keys and defaults, kept as JSON in the app's config dir.
//! A value that is missing, of the wrong type or out of range falls back to its default, field by
//! field; keys this build doesn't know are kept as they were, for the build that wrote them.
//!
//! Read with `Settings::get(cx)`, change with `Settings::update(cx, |s| ..)`, follow with
//! `cx.observe_global::<Settings>(..)`.

mod schema;

use std::path::PathBuf;

use gpui_kit::{App, Global};
use serde_json::{Map, Value};

pub use schema::*;

use crate::store;

impl Global for Settings {}

impl Settings {
    pub fn get(cx: &App) -> &Settings {
        cx.global::<Settings>()
    }

    /// Applies `f`, keeps the values in range, stores the file and tells the observers.
    pub fn update(cx: &mut App, f: impl FnOnce(&mut Settings)) {
        let before = Settings::get(cx).clone();
        let mut next = before.clone();
        f(&mut next);
        next.normalize();
        if next == before {
            return;
        }
        cx.set_global(next);
        save(cx);
    }

    /// Back to the defaults, except what the user built up rather than chose: the repos they
    /// sign off in, their own Open in apps, the folder clones go to.
    pub fn reset(cx: &mut App) {
        Settings::update(cx, |s| *s = s.reset_value());
    }

    /// The defaults with what `reset` keeps from these settings.
    pub fn reset_value(&self) -> Settings {
        Settings {
            sign_off_repos: self.sign_off_repos.clone(),
            open_in_custom: self.open_in_custom.clone(),
            clone_parent: self.clone_parent.clone(),
            ..Settings::default()
        }
    }

    /// Moves the interface scale one step along UI_SCALES; 0 resets it.
    pub fn step_ui_scale(cx: &mut App, dir: i32) {
        Settings::update(cx, |s| s.ui_scale = step(UI_SCALES, s.ui_scale, dir, 1.0));
    }

    /// Moves the code font a half point; 0 resets it.
    pub fn step_code_font(cx: &mut App, dir: i32) {
        Settings::update(cx, |s| {
            s.code_font_size = if dir == 0 {
                DEFAULT_FONT_SIZE
            } else {
                s.code_font_size + dir as f32 * 0.5
            }
        });
    }

    /// Moves the terminal font a point; 0 resets it.
    pub fn step_terminal_font(cx: &mut App, dir: i32) {
        Settings::update(cx, |s| {
            s.terminal_font_size = if dir == 0 {
                DEFAULT_TERMINAL_FONT_SIZE
            } else {
                s.terminal_font_size + dir as f32
            }
        });
    }

    /// Parses a settings file's text; anything unreadable is a default.
    pub fn from_json(text: &str) -> Settings {
        Self::from_value(&serde_json::from_str(text).unwrap_or(Value::Null)).0
    }

    pub(crate) fn from_value(value: &Value) -> (Settings, Map<String, Value>) {
        let empty = Map::new();
        let map = value.as_object().unwrap_or(&empty);
        let mut settings = Settings::read(map);
        settings.normalize();
        let known: Vec<String> = Settings::keys().collect();
        let unknown = map
            .iter()
            .filter(|(k, _)| !known.contains(k))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        (settings, unknown)
    }

    /// The file's text: the known keys as this build has them, then what it didn't know.
    pub fn to_json(&self, unknown: &Map<String, Value>) -> String {
        let mut map = match serde_json::to_value(self) {
            Ok(Value::Object(map)) => map,
            _ => Map::new(),
        };
        for (k, v) in unknown {
            map.entry(k.clone()).or_insert_with(|| v.clone());
        }
        serde_json::to_string_pretty(&Value::Object(map)).unwrap_or_default()
    }
}

/// `value`'s neighbour `dir` steps along `steps`; 0 is `reset`.
fn step(steps: &[f32], value: f32, dir: i32, reset: f32) -> f32 {
    if dir == 0 {
        return reset;
    }
    let i = steps.iter().position(|s| *s == value).unwrap_or(0) as i32;
    steps[(i + dir).clamp(0, steps.len() as i32 - 1) as usize]
}

pub fn settings_path() -> PathBuf {
    store::config_dir().join("settings.json")
}

/// Writes the keys that changed into the file as it is now: a hand edit to another key, or
/// keys this build doesn't know, stay.
fn save(cx: &mut App) {
    if let Ok(Value::Object(map)) = serde_json::to_value(Settings::get(cx)) {
        store::save_object(cx, settings_path(), map);
    }
}

/// A file that can't be read is moved aside (`settings.json.bad-<time>`) rather than replaced
/// by the defaults at the next save.
pub(crate) fn init(cx: &mut App) {
    let value = store::read_object(&settings_path())
        .map(Value::Object)
        .unwrap_or(Value::Null);
    cx.set_global(Settings::from_value(&value).0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ThemeId;

    #[test]
    fn empty_and_broken_files_give_the_defaults() {
        assert_eq!(Settings::from_json(""), Settings::default());
        assert_eq!(Settings::from_json("{not json"), Settings::default());
        assert_eq!(Settings::from_json("[1,2]"), Settings::default());
    }

    #[test]
    fn keys_are_the_old_apps() {
        let json: Value = serde_json::from_str(&Settings::default().to_json(&Map::new())).unwrap();
        let map = json.as_object().unwrap();
        for key in [
            "codeFont",
            "customCodeFont",
            "codeFontSize",
            "codeFontWeight",
            "lineHeight",
            "appearance",
            "darkTheme",
            "lightTheme",
            "windowOpacity",
            "backgroundBlur",
            "terminalGlass",
            "uiFont",
            "uiFontWeight",
            "syntaxTheme",
            "lightSyntaxTheme",
            "uiScale",
            "terminalFontSize",
            "browserParkAfterMin",
            "keybindings",
            "signOffRepos",
            "openInCustom",
            "sidebarProjects",
        ] {
            assert!(map.contains_key(key), "{key}");
        }
        assert_eq!(map.len(), Settings::keys().count());
        assert_eq!(map["codeFontSize"], 13.5);
        assert_eq!(map["darkTheme"], "dark");
        assert_eq!(map["suggestCommand"], "claude -p");
        assert_eq!(map["cloneParent"], Value::Null);
    }

    #[test]
    fn a_bad_field_falls_back_alone() {
        let s = Settings::from_json(
            r#"{"uiScale": "big", "codeFontSize": 15, "darkTheme": "light", "lightTheme": "nord",
                "appearance": "dark", "windowOpacity": 3, "backgroundBlur": 17.4, "terminalFont": "Comic",
                "hiddenVaults": ["a", 2, "b"], "keybindings": {"git.push": ["CMD+U"], "nope": ["cmd+u"]}}"#,
        );
        assert_eq!(s.ui_scale, 1.0);
        assert_eq!(s.code_font_size, 15.0);
        assert_eq!(s.dark_theme, ThemeId::Dark);
        assert_eq!(s.light_theme, ThemeId::Light);
        assert_eq!(s.appearance, Appearance::Dark);
        assert_eq!(s.window_opacity, 50.0);
        assert_eq!(s.background_blur, 17.0);
        assert_eq!(s.terminal_font, TerminalFont::Editor);
        assert_eq!(s.hidden_vaults, vec!["a".to_string(), "b".to_string()]);
        assert_eq!(s.keybindings.len(), 1);
        assert_eq!(s.keybindings["git.push"], vec!["cmd+u".to_string()]);
    }

    #[test]
    fn values_are_kept_in_range() {
        let mut s = Settings {
            code_font_size: 99.3,
            terminal_font_size: 1.0,
            custom_code_font: " \"Fira\"; } ".into(),
            ui_font_weight: 700,
            terminal_scrollback: 7,
            ..Settings::default()
        };
        s.normalize();
        assert_eq!(s.code_font_size, CODE_FONT_MAX);
        assert_eq!(s.terminal_font_size, TERMINAL_FONT_MIN);
        // Kept as typed but for the quoting characters: a space mid-name gets typed too.
        assert_eq!(s.custom_code_font, " Fira  ");
        let mut typing = Settings {
            custom_code_font: "Fira ".into(),
            ..Settings::default()
        };
        typing.normalize();
        assert_eq!(typing.custom_code_font, "Fira ");
        assert_eq!(s.ui_font_weight, 500);
        assert_eq!(s.terminal_scrollback, 10_000);
    }

    #[test]
    fn unknown_keys_survive_a_save() {
        let (s, unknown) =
            Settings::from_value(&serde_json::json!({"futureThing": [1], "uiScale": 1.2}));
        assert_eq!(s.ui_scale, 1.2);
        let text = s.to_json(&unknown);
        let back: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(back["futureThing"], serde_json::json!([1]));
        assert_eq!(Settings::from_json(&text), s);
    }

    #[test]
    fn suggest_models_and_efforts_keep_known_presets_only() {
        let s = Settings::from_json(
            r#"{"suggestModels": {"claude": "opus", "nope": "x", "codex": 3},
                "suggestEfforts": {"claude": "max", "codex": "max", "pi": ""}}"#,
        );
        assert_eq!(s.suggest_models.len(), 1);
        assert_eq!(s.suggest_models["claude"], "opus");
        assert_eq!(
            s.suggest_efforts.get("claude").map(String::as_str),
            Some("max")
        );
        assert!(!s.suggest_efforts.contains_key("codex"));
        assert_eq!(s.suggest_efforts.get("pi").map(String::as_str), Some(""));
    }

    #[test]
    fn steps_walk_the_scale() {
        assert_eq!(step(UI_SCALES, 1.0, 1, 1.0), 1.1);
        assert_eq!(step(UI_SCALES, 1.5, 1, 1.0), 1.5);
        assert_eq!(step(UI_SCALES, 0.8, -1, 1.0), 0.8);
        assert_eq!(step(UI_SCALES, 1.3, 0, 1.0), 1.0);
    }

    #[test]
    fn reset_keeps_what_the_user_built_up() {
        let s = Settings {
            ui_scale: 1.3,
            sign_off_repos: vec!["/r".into()],
            clone_parent: Some("/c".into()),
            ..Settings::default()
        };
        let reset = s.reset_value();
        assert_eq!(reset.ui_scale, 1.0);
        assert_eq!(reset.sign_off_repos, vec!["/r".to_string()]);
        assert_eq!(reset.clone_parent.as_deref(), Some("/c"));
    }
}
