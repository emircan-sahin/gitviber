use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::keymap::{IS_MAC, Overrides, clean_overrides};
use crate::theme::ThemeId;

pub const DEFAULT_FONT_SIZE: f32 = 13.5;
pub const CODE_FONT_MIN: f32 = 10.0;
pub const CODE_FONT_MAX: f32 = 24.0;
pub const DEFAULT_TERMINAL_FONT_SIZE: f32 = 13.0;
pub const TERMINAL_FONT_MIN: f32 = 8.0;
pub const TERMINAL_FONT_MAX: f32 = 32.0;
/// Whole-app zoom steps. Separate from the code font size.
pub const UI_SCALES: &[f32] = &[0.8, 0.9, 1.0, 1.1, 1.2, 1.3, 1.4, 1.5];
pub const CODE_FONT_WEIGHTS: &[(u16, &str)] = &[
    (300, "Light"),
    (400, "Regular"),
    (500, "Medium"),
    (600, "Semibold"),
];
pub const UI_FONT_WEIGHTS: &[(u16, &str)] = &[(400, "Regular"), (500, "Medium")];
/// The window's opacity in percent, 100 solid; each theme stops higher where its text keeps 3:1
/// over any desktop (theme::opacity_floor).
pub const WINDOW_OPACITY: Range = Range {
    min: 50.0,
    max: 100.0,
    step: 5.0,
};
/// The radius the desktop behind a see-through window is blurred by; 0 leaves it sharp.
pub const BACKGROUND_BLUR: Range = Range {
    min: 0.0,
    max: 40.0,
    step: 1.0,
};
/// Minutes between background fetches; 0 is off.
pub const FETCH_INTERVALS: &[u32] = &[0, 5, 15, 30];
/// Seconds a command runs before its end is news.
pub const LONG_COMMAND_SECONDS: &[u32] = &[5, 10, 30, 60, 300];
/// Percent a split terminal's other panes fade; 0 is off.
pub const DIM_LEVELS: &[u32] = &[0, 10, 20, 35, 50];
/// Lines of history each terminal keeps.
pub const SCROLLBACK_LINES: &[u32] = &[1_000, 10_000, 50_000, 100_000];
pub const TERMINAL_LINE_HEIGHTS: &[f32] = &[1.0, 1.1, 1.2, 1.3];
/// Browser tabs out of sight whose pages stay alive; the rest park.
pub const BROWSER_LIVE_HIDDEN: &[u32] = &[0, 1, 2, 3, 4];
/// Minutes a browser tab's page stays alive out of sight; 0 is until the cap parks it.
pub const BROWSER_PARK_AFTER: &[u32] = &[0, 5, 10, 30, 60];
/// Agents' tabs drawing out of sight at once; past it the longest unused sleeps.
pub const BROWSER_AGENT_DRAWN: &[u32] = &[4, 8, 12, 16, 24];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Range {
    pub min: f32,
    pub max: f32,
    pub step: f32,
}

impl Range {
    /// `v` on the range's steps, inside it.
    pub fn snap(self, v: f32) -> f32 {
        if !v.is_finite() {
            return self.min;
        }
        let snapped = self.min + ((v - self.min) / self.step).round() * self.step;
        snapped.clamp(self.min, self.max)
    }
}

macro_rules! string_enum {
    ($(#[$meta:meta])* $name:ident { $($variant:ident = $text:literal),* $(,)? }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
        pub enum $name {
            $(#[serde(rename = $text)] $variant,)*
        }

        impl $name {
            pub const ALL: &[$name] = &[$($name::$variant),*];

            /// The name settings store it under, which is also its label.
            pub fn as_str(self) -> &'static str {
                match self { $($name::$variant => $text,)* }
            }

            pub fn parse(text: &str) -> Option<Self> {
                match text { $($text => Some($name::$variant),)* _ => None }
            }
        }
    };
}

string_enum!(
    /// System follows the OS between `light_theme` and `dark_theme`.
    Appearance { System = "system", Light = "light", Dark = "dark" }
);
string_enum!(CodeFont {
    SfMono = "SF Mono",
    GeistMono = "Geist Mono",
    JetBrainsMono = "JetBrains Mono",
    Menlo = "Menlo",
    Monaco = "Monaco",
    CourierNew = "Courier New",
    Custom = "Custom",
});
string_enum!(
    /// "Editor" follows the code font; the rest are its presets, or Custom by name.
    TerminalFont {
        Editor = "Editor",
        SfMono = "SF Mono",
        GeistMono = "Geist Mono",
        JetBrainsMono = "JetBrains Mono",
        Menlo = "Menlo",
        Monaco = "Monaco",
        CourierNew = "Courier New",
        Custom = "Custom",
    }
);
string_enum!(UiFont {
    System = "System",
    Geist = "Geist",
    HelveticaNeue = "Helvetica Neue",
    AvenirNext = "Avenir Next",
    Custom = "Custom",
});
string_enum!(Whitespace { Amount = "amount", All = "all" });
string_enum!(
    /// What ⌥ does in the terminal on macOS: type characters, or send Meta from the left ⌥ or either.
    OptionKey { Off = "off", Left = "left", Both = "both" }
);
string_enum!(
    /// A restored terminal an agent was running in: its resume command typed at the prompt, run, or neither.
    ResumeMode { Type = "type", Run = "run", Off = "off" }
);
string_enum!(TerminalCursor { Block = "block", Bar = "bar", Underline = "underline" });
string_enum!(
    /// How a changed image shows its two versions.
    ImageCompare { Side = "side", Swipe = "swipe", Onion = "onion" }
);

impl CodeFont {
    /// Only macOS has these; elsewhere they'd render as the system monospace under a wrong name.
    pub fn mac_only(self) -> bool {
        matches!(self, Self::SfMono | Self::Menlo | Self::Monaco)
    }

    /// The presets this platform can show.
    pub fn choices() -> impl Iterator<Item = CodeFont> {
        Self::ALL
            .iter()
            .copied()
            .filter(|f| IS_MAC || !f.mac_only())
    }

    /// SF Mono on macOS; elsewhere a font that looks the same on every desktop.
    pub fn platform_default() -> Self {
        if IS_MAC {
            Self::SfMono
        } else {
            Self::JetBrainsMono
        }
    }
}

impl UiFont {
    pub fn mac_only(self) -> bool {
        matches!(self, Self::HelveticaNeue | Self::AvenirNext)
    }

    pub fn choices() -> impl Iterator<Item = UiFont> {
        Self::ALL
            .iter()
            .copied()
            .filter(|f| IS_MAC || !f.mac_only())
    }
}

impl TerminalFont {
    pub fn mac_only(self) -> bool {
        matches!(self, Self::SfMono | Self::Menlo | Self::Monaco)
    }
}

/// The syntax themes for a dark app, by id, with their labels. Colors come with the code view.
pub const SYNTAX_THEMES: &[(&str, &str)] = &[
    ("github-dark-default", "GitHub Dark"),
    ("dark-plus", "VS Code Dark+"),
    ("one-dark-pro", "One Dark Pro"),
    ("vitesse-dark", "Vitesse Dark"),
    ("tokyo-night", "Tokyo Night"),
    ("solarized-dark", "Solarized Dark"),
    ("catppuccin-mocha", "Catppuccin Mocha"),
    ("vesper", "Vesper"),
    ("houston", "Houston"),
    // Muted, low-saturation palettes.
    ("nord", "Nord"),
    ("rose-pine", "Rosé Pine"),
    ("kanagawa-wave", "Kanagawa Wave"),
    ("everforest-dark", "Everforest Dark"),
    ("poimandres", "Poimandres"),
];

pub const LIGHT_SYNTAX_THEMES: &[(&str, &str)] = &[
    ("github-light-default", "GitHub Light"),
    ("light-plus", "VS Code Light+"),
    ("one-light", "One Light"),
    ("vitesse-light", "Vitesse Light"),
    ("catppuccin-latte", "Catppuccin Latte"),
    ("min-light", "Min Light"),
    ("solarized-light", "Solarized Light"),
    ("rose-pine-dawn", "Rosé Pine Dawn"),
    ("kanagawa-lotus", "Kanagawa Lotus"),
    ("everforest-light", "Everforest Light"),
];

/// The agent CLIs Commit Messages knows, each with the reasoning efforts it takes.
pub const SUGGEST_PRESETS: &[(&str, &[&str])] = &[
    ("claude", &["low", "medium", "high", "xhigh", "max"]),
    ("codex", &["minimal", "low", "medium", "high", "xhigh"]),
    (
        "opencode",
        &["minimal", "low", "medium", "high", "xhigh", "max"],
    ),
    (
        "pi",
        &["off", "minimal", "low", "medium", "high", "xhigh", "max"],
    ),
    ("llm", &[]),
];

/// A user's own "Open in" entry: a command with {path}, {file} and {line}, run without a shell.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustomApp {
    pub id: String,
    pub name: String,
    pub command: String,
}

/// How a value is read from the file: None when it can't be, and the field keeps its default.
pub trait FromSetting: Sized {
    fn from_setting(value: &Value) -> Option<Self>;
}

macro_rules! via_serde {
    ($($ty:ty),*) => {
        $(impl FromSetting for $ty {
            fn from_setting(value: &Value) -> Option<Self> {
                <$ty>::deserialize(value).ok()
            }
        })*
    };
}

via_serde!(
    bool,
    f32,
    u16,
    u32,
    String,
    Appearance,
    CodeFont,
    TerminalFont,
    UiFont,
    Whitespace,
    OptionKey,
    ResumeMode,
    TerminalCursor,
    ImageCompare,
    ThemeId
);

impl FromSetting for Option<String> {
    fn from_setting(value: &Value) -> Option<Self> {
        Some(value.as_str().map(str::to_string))
    }
}

/// The strings of a list, the rest dropped.
impl FromSetting for Vec<String> {
    fn from_setting(value: &Value) -> Option<Self> {
        let list = value.as_array()?;
        Some(
            list.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect(),
        )
    }
}

impl FromSetting for BTreeMap<String, String> {
    fn from_setting(value: &Value) -> Option<Self> {
        let map = value.as_object()?;
        Some(
            map.iter()
                .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string())))
                .collect(),
        )
    }
}

impl FromSetting for Overrides {
    fn from_setting(value: &Value) -> Option<Self> {
        let map = value.as_object()?;
        let raw = map
            .iter()
            .filter_map(|(k, v)| {
                let keys: Option<Vec<String>> = v
                    .as_array()?
                    .iter()
                    .map(|k| k.as_str().map(str::to_string))
                    .collect();
                Some((k.clone(), keys?))
            })
            .collect();
        Some(clean_overrides(&raw))
    }
}

impl FromSetting for Vec<CustomApp> {
    fn from_setting(value: &Value) -> Option<Self> {
        let list = value.as_array()?;
        Some(
            list.iter()
                .filter_map(|v| CustomApp::deserialize(v).ok())
                .collect(),
        )
    }
}

/// snake_case → the camelCase the file uses.
fn camel(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut upper = false;
    for c in name.chars() {
        if c == '_' {
            upper = true;
        } else if upper {
            out.extend(c.to_uppercase());
            upper = false;
        } else {
            out.push(c);
        }
    }
    out
}

macro_rules! settings {
    ($($(#[$doc:meta])* $field:ident: $ty:ty = $default:expr,)*) => {
        #[derive(Clone, Debug, PartialEq, Serialize)]
        #[serde(rename_all = "camelCase")]
        pub struct Settings {
            $($(#[$doc])* pub $field: $ty,)*
        }

        impl Default for Settings {
            fn default() -> Self {
                Self { $($field: $default,)* }
            }
        }

        impl Settings {
            /// The keys the file has, camelCase.
            pub fn keys() -> impl Iterator<Item = String> {
                [$(stringify!($field)),*].into_iter().map(camel)
            }

            pub(super) fn read(map: &Map<String, Value>) -> Self {
                let mut s = Self::default();
                $(
                    if let Some(v) = map.get(&camel(stringify!($field))).and_then(<$ty>::from_setting) {
                        s.$field = v;
                    }
                )*
                s
            }
        }
    };
}

settings! {
    code_font: CodeFont = CodeFont::platform_default(),
    custom_code_font: String = String::new(),
    code_font_size: f32 = DEFAULT_FONT_SIZE,
    /// One of CODE_FONT_WEIGHTS; bold text goes two steps up from it.
    code_font_weight: u16 = 500,
    line_height: f32 = 1.6,
    appearance: Appearance = Appearance::System,
    dark_theme: ThemeId = ThemeId::Dark,
    light_theme: ThemeId = ThemeId::Light,
    /// WINDOW_OPACITY's percent and BACKGROUND_BLUR's radius; applied on macOS only.
    window_opacity: f32 = 100.0,
    background_blur: f32 = 0.0,
    /// The terminal shows the desktop through at the window's opacity too; solid otherwise.
    terminal_glass: bool = false,
    ui_font: UiFont = UiFont::System,
    custom_ui_font: String = String::new(),
    /// One of UI_FONT_WEIGHTS; medium and semibold labels stay a step above it.
    ui_font_weight: u16 = 500,
    syntax_theme: String = "dark-plus".into(),
    light_syntax_theme: String = "github-light-default".into(),
    side_by_side: bool = false,
    hide_unchanged: bool = true,
    /// Diffs hide lines whose only change is whitespace, of the kind `whitespace_mode` says.
    ignore_whitespace: bool = false,
    whitespace_mode: Whitespace = Whitespace::Amount,
    word_wrap: bool = false,
    ligatures: bool = false,
    /// Whole-app zoom, one of UI_SCALES.
    ui_scale: f32 = 1.0,
    option_as_meta: OptionKey = OptionKey::Off,
    /// New terminals load the shell integration (zsh, bash 4.4+): command marks, ⌘↑ / ⌘↓ between prompts.
    shell_integration: bool = true,
    resume_agents: ResumeMode = ResumeMode::Type,
    /// ⌘Q and closing the window ask first while an agent is working, a command runs, or a push or pull is on its way.
    ask_before_quit: bool = true,
    /// One of DIM_LEVELS.
    terminal_inactive_dim: u32 = 20,
    terminal_font: TerminalFont = TerminalFont::Editor,
    custom_terminal_font: String = String::new(),
    /// Its own, as in terminal apps: the terminal wants a bigger size than the dense diffs.
    terminal_font_size: f32 = DEFAULT_TERMINAL_FONT_SIZE,
    /// One of TERMINAL_LINE_HEIGHTS. 1, not 1.2: at 1.2 a pane had a sixth fewer rows, and Claude
    /// Code drops its usage lines, then its header, below ~16 rows.
    terminal_line_height: f32 = 1.0,
    terminal_cursor: TerminalCursor = TerminalCursor::Block,
    terminal_cursor_blink: bool = true,
    /// One of SCROLLBACK_LINES.
    terminal_scrollback: u32 = 10_000,
    terminal_screen_reader: bool = false,
    /// Markdown files open rendered rather than as source (diffs always start on the diff).
    markdown_preview: bool = true,
    /// The explorer shows Obsidian's vaults, when Obsidian lists any.
    obsidian: bool = true,
    /// Vaults (by path) the explorer leaves out.
    hidden_vaults: Vec<String> = Vec::new(),
    browser_live_hidden: u32 = 2,
    browser_park_after_min: u32 = 10,
    browser_agent_drawn: u32 = 12,
    /// A worktree's browser pages load again when an agent there finishes.
    browser_reload_on_agent_done: bool = false,
    browser_console: bool = true,
    /// Agents in the terminals may drive a browser tab of their pane's with `gitviber browser`.
    browser_agent_control: bool = false,
    browser_show_agent_tabs: bool = true,
    browser_mute_agent_tabs: bool = true,
    /// On, as agent control is the opt-in: an agent that doesn't know the command never uses it.
    browser_agent_skill: bool = true,
    /// The last Code / Preview choice on an SVG. Set from the viewer, not the settings.
    svg_preview: bool = false,
    /// The last mode picked on a changed image. Set from the viewer.
    image_compare: ImageCompare = ImageCompare::Side,
    /// The file view shows who last changed each line. Set from the viewer.
    blame: bool = false,
    /// Holding ⌘ by itself shows the shortcuts that apply.
    shortcut_overlay: bool = true,
    /// Per-command overrides of the default key bindings; an empty list unbinds.
    keybindings: Overrides = Overrides::new(),
    /// Repos (main worktree paths) whose commits are signed off. Set from the commit box.
    sign_off_repos: Vec<String> = Vec::new(),
    /// Off until asked for: a fetch can prompt for an SSH key (1Password, a hardware key) every few minutes.
    background_fetch: u32 = 0,
    /// Off until asked for: turning it on is what asks the OS for permission.
    notify: bool = false,
    notify_agent_done: bool = true,
    notify_agent_waiting: bool = true,
    notify_terminal: bool = true,
    /// Off: an update shouldn't start notifying.
    notify_long_command: bool = false,
    /// 5 s is enough for a notification alone; here the tab's dot comes with it.
    long_command_seconds: u32 = 10,
    notify_git: bool = true,
    auto_update: bool = true,
    /// Where the last clone went; the next one offers the same folder.
    clone_parent: Option<String> = None,
    /// New worktrees go in `<worktree_root>/<project>/` rather than `<project>.worktrees` beside it.
    worktree_root: Option<String> = None,
    /// A ✦ button in the commit box runs `suggest_command` for a message. Off: nothing is ever run.
    suggest_enabled: bool = true,
    suggest_command: String = "claude -p".into(),
    /// Model ids typed per preset; a preset missing here runs its default.
    suggest_models: BTreeMap<String, String> = BTreeMap::new(),
    /// Reasoning effort per preset, "" for the CLI's own.
    suggest_efforts: BTreeMap<String, String> = BTreeMap::new(),
    /// Guided reviews' own agent CLI; None runs `suggest_command`.
    review_command: Option<String> = None,
    review_models: BTreeMap<String, String> = BTreeMap::new(),
    review_efforts: BTreeMap<String, String> = BTreeMap::new(),
    /// The language guided reviews are written in; commit messages and pull requests stay English.
    review_language: String = "English".into(),
    /// The app "Open in" runs on a click: a built-in id or a CustomApp's; "" until one is picked.
    open_in_app: String = String::new(),
    open_in_custom: Vec<CustomApp> = Vec::new(),
    open_in_hide_builtins: bool = false,
    /// The projects are listed at the top of the left panel as well as in the top bar's switcher.
    sidebar_projects: bool = false,
}

/// A typed font name without the characters that could break out of a quoted family name.
pub fn clean_font_name(name: &str) -> String {
    name.chars()
        .filter(|c| !matches!(c, '"' | '\'' | '\\' | ';' | '{' | '}'))
        .collect::<String>()
        .trim()
        .to_string()
}

fn one_of<T: Copy + PartialEq>(value: &mut T, allowed: &[T], fallback: T) {
    if !allowed.contains(value) {
        *value = fallback;
    }
}

fn keep_presets(map: &mut BTreeMap<String, String>, efforts: bool) {
    map.retain(|preset, value| {
        SUGGEST_PRESETS.iter().any(|(name, levels)| {
            name == preset && (!efforts || value.is_empty() || levels.contains(&value.as_str()))
        })
    });
}

impl Settings {
    /// Keeps every value in range, as the old app's load did.
    pub fn normalize(&mut self) {
        let d = Settings::default();
        if !IS_MAC && self.code_font.mac_only() {
            self.code_font = d.code_font;
        }
        if !IS_MAC && self.ui_font.mac_only() {
            self.ui_font = d.ui_font;
        }
        if !IS_MAC && self.terminal_font.mac_only() {
            self.terminal_font = d.terminal_font;
        }
        let weights: Vec<u16> = CODE_FONT_WEIGHTS.iter().map(|w| w.0).collect();
        one_of(&mut self.code_font_weight, &weights, d.code_font_weight);
        let weights: Vec<u16> = UI_FONT_WEIGHTS.iter().map(|w| w.0).collect();
        one_of(&mut self.ui_font_weight, &weights, d.ui_font_weight);
        self.code_font_size = clamp_half(
            self.code_font_size,
            CODE_FONT_MIN,
            CODE_FONT_MAX,
            d.code_font_size,
        );
        self.terminal_font_size = if self.terminal_font_size.is_finite() {
            self.terminal_font_size
                .round()
                .clamp(TERMINAL_FONT_MIN, TERMINAL_FONT_MAX)
        } else {
            d.terminal_font_size
        };
        if !self.line_height.is_finite() || !(1.0..=3.0).contains(&self.line_height) {
            self.line_height = d.line_height;
        }
        self.custom_code_font = clean_font_name(&self.custom_code_font);
        self.custom_ui_font = clean_font_name(&self.custom_ui_font);
        self.custom_terminal_font = clean_font_name(&self.custom_terminal_font);
        if !SYNTAX_THEMES.iter().any(|(id, _)| *id == self.syntax_theme) {
            self.syntax_theme = d.syntax_theme.clone();
        }
        if !LIGHT_SYNTAX_THEMES
            .iter()
            .any(|(id, _)| *id == self.light_syntax_theme)
        {
            self.light_syntax_theme = d.light_syntax_theme.clone();
        }
        if !self.dark_theme.is_dark() {
            self.dark_theme = d.dark_theme;
        }
        if self.light_theme.is_dark() {
            self.light_theme = d.light_theme;
        }
        self.window_opacity = WINDOW_OPACITY.snap(self.window_opacity);
        self.background_blur = BACKGROUND_BLUR.snap(self.background_blur);
        one_of(&mut self.ui_scale, UI_SCALES, d.ui_scale);
        one_of(
            &mut self.terminal_inactive_dim,
            DIM_LEVELS,
            d.terminal_inactive_dim,
        );
        one_of(
            &mut self.terminal_line_height,
            TERMINAL_LINE_HEIGHTS,
            d.terminal_line_height,
        );
        one_of(
            &mut self.terminal_scrollback,
            SCROLLBACK_LINES,
            d.terminal_scrollback,
        );
        one_of(
            &mut self.browser_live_hidden,
            BROWSER_LIVE_HIDDEN,
            d.browser_live_hidden,
        );
        one_of(
            &mut self.browser_park_after_min,
            BROWSER_PARK_AFTER,
            d.browser_park_after_min,
        );
        one_of(
            &mut self.browser_agent_drawn,
            BROWSER_AGENT_DRAWN,
            d.browser_agent_drawn,
        );
        one_of(
            &mut self.background_fetch,
            FETCH_INTERVALS,
            d.background_fetch,
        );
        one_of(
            &mut self.long_command_seconds,
            LONG_COMMAND_SECONDS,
            d.long_command_seconds,
        );
        self.keybindings = clean_overrides(&self.keybindings);
        keep_presets(&mut self.suggest_models, false);
        keep_presets(&mut self.suggest_efforts, true);
        keep_presets(&mut self.review_models, false);
        keep_presets(&mut self.review_efforts, true);
    }

    /// Whether the app is dark now: `system_dark` is the OS's appearance.
    pub fn is_dark(&self, system_dark: bool) -> bool {
        match self.appearance {
            Appearance::System => system_dark,
            Appearance::Dark => true,
            Appearance::Light => false,
        }
    }

    /// The palette in use for that appearance.
    pub fn theme(&self, system_dark: bool) -> ThemeId {
        if self.is_dark(system_dark) {
            self.dark_theme
        } else {
            self.light_theme
        }
    }

    /// The syntax theme id in use for that appearance.
    pub fn code_theme(&self, system_dark: bool) -> &str {
        if self.is_dark(system_dark) {
            &self.syntax_theme
        } else {
            &self.light_syntax_theme
        }
    }

    /// The whitespace diffs ignore now, or None.
    pub fn diff_whitespace(&self) -> Option<Whitespace> {
        self.ignore_whitespace.then_some(self.whitespace_mode)
    }
}

fn clamp_half(v: f32, min: f32, max: f32, fallback: f32) -> f32 {
    if !v.is_finite() {
        return fallback;
    }
    ((v * 2.0).round() / 2.0).clamp(min, max)
}
