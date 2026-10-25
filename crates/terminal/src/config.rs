//! The terminal's inputs from Settings and the theme, kept small so the app fills them from
//! gitviber-ui. Field names follow the old app's settings keys (src/lib/settings.ts).

use gpui::{App, Hsla, Rgba, SharedString};

/// What ⌥ does in the terminal on macOS (`optionAsMeta`): type characters, or send Meta from the
/// left ⌥ or either.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OptionAsMeta {
    #[default]
    Off,
    Left,
    Both,
}

/// `terminalCursor`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CursorStyle {
    #[default]
    Block,
    Bar,
    Underline,
}

pub const DEFAULT_FONT_SIZE: f32 = 13.;
pub const FONT_MIN: f32 = 8.;
pub const FONT_MAX: f32 = 32.;
/// `SCROLLBACK_LINES`: the history each terminal keeps.
pub const SCROLLBACK_LINES: [usize; 4] = [1_000, 10_000, 50_000, 100_000];
pub const LINE_HEIGHTS: [f32; 4] = [1., 1.1, 1.2, 1.3];

#[derive(Clone, Debug, PartialEq)]
pub struct TerminalConfig {
    /// Resolved to an installed family (`terminalFont`, "Editor" being the code font).
    pub font_family: SharedString,
    /// `terminalFontSize`, FONT_MIN to FONT_MAX.
    pub font_size: f32,
    /// `codeFontWeight`; bold is two steps above it.
    pub font_weight: f32,
    /// `terminalLineHeight`, one of LINE_HEIGHTS.
    pub line_height: f32,
    /// `terminalCursor`, `terminalCursorBlink`.
    pub cursor: CursorStyle,
    pub cursor_blink: bool,
    /// `terminalScrollback`, one of SCROLLBACK_LINES.
    pub scrollback: usize,
    /// `optionAsMeta`.
    pub option_as_meta: OptionAsMeta,
    /// `shellIntegration`: new terminals load it (zsh, bash 4.4+).
    pub shell_integration: bool,
    /// `terminalInactiveDim`: percent a split's other panes fade.
    pub inactive_dim: f32,
    /// `terminalScreenReader`: each pane tells VoiceOver what its screen shows.
    pub screen_reader: bool,
}

impl Default for TerminalConfig {
    fn default() -> Self {
        TerminalConfig {
            font_family: if cfg!(target_os = "macos") {
                "Menlo".into()
            } else {
                "DejaVu Sans Mono".into()
            },
            font_size: DEFAULT_FONT_SIZE,
            font_weight: 400.,
            line_height: 1.,
            cursor: CursorStyle::Block,
            cursor_blink: true,
            scrollback: 10_000,
            option_as_meta: OptionAsMeta::Off,
            shell_integration: true,
            inactive_dim: 20.,
            screen_reader: false,
        }
    }
}

impl TerminalConfig {
    /// A point bigger (1), smaller (-1), or back to the default (0), as ⌘= / ⌘− / ⌘0 and a pinch
    /// move it.
    pub fn stepped_font(&self, dir: i32) -> f32 {
        if dir == 0 {
            return DEFAULT_FONT_SIZE;
        }
        (self.font_size.round() + dir as f32).clamp(FONT_MIN, FONT_MAX)
    }

    /// The first of `names` installed, else the platform's monospace.
    pub fn mono_family(names: &[&str], cx: &App) -> SharedString {
        let installed = cx.text_system().all_font_names();
        names
            .iter()
            .chain(if cfg!(target_os = "macos") {
                &["SF Mono", "Menlo"][..]
            } else {
                &["JetBrains Mono", "DejaVu Sans Mono"][..]
            })
            .find(|n| installed.iter().any(|i| i == *n))
            .map(|n| SharedString::from(n.to_string()))
            .unwrap_or_else(|| TerminalConfig::default().font_family)
    }
}

/// The terminal's colors: the app's surfaces and accent, ANSI colors per theme.
#[derive(Clone, Debug, PartialEq)]
pub struct TerminalTheme {
    pub background: Hsla,
    pub foreground: Hsla,
    pub cursor: Hsla,
    /// The text under a block cursor.
    pub cursor_text: Hsla,
    pub selection: Hsla,
    /// Black, red, green, yellow, blue, magenta, cyan, white, then their bright ones.
    pub ansi: [Hsla; 16],
    /// The panel's chrome: tabs, headers, toolbar.
    pub panel: Hsla,
    pub elevated: Hsla,
    pub hover: Hsla,
    pub active: Hsla,
    pub border: Hsla,
    pub border_strong: Hsla,
    pub muted_foreground: Hsla,
    pub subtle: Hsla,
    pub primary: Hsla,
    pub primary_foreground: Hsla,
    pub destructive: Hsla,
    pub find_match: Hsla,
    pub find_current: Hsla,
    /// Text under a full-screen program's lines kept readable: xterm's minimumContrastRatio.
    pub minimum_contrast: f32,
}

/// `#rrggbb` or `#rrggbbaa`.
pub fn hex(s: &str) -> Hsla {
    let h = s.trim_start_matches('#');
    let v = u32::from_str_radix(h, 16).unwrap_or(0);
    let rgba = if h.len() == 8 {
        gpui::rgba(v)
    } else {
        gpui::rgb(v)
    };
    rgba.into()
}

const ANSI_DARK: [&str; 16] = [
    "#3a3a3e", "#f47067", "#57d17a", "#e2b84c", "#4a9ff5", "#b392f0", "#56c8d8", "#d4d4d8",
    "#6c6c73", "#ff8a80", "#7ee29a", "#f0cf74", "#74b6f7", "#c9b0f5", "#7fdbe6", "#ffffff",
];
const ANSI_LIGHT: [&str; 16] = [
    "#1d1d1f", "#cf222e", "#1a7f37", "#9a6700", "#0969da", "#8250df", "#1b7c83", "#6e7781",
    "#5b5b62", "#a40e26", "#116329", "#7d4e00", "#0550ae", "#6639ba", "#136061", "#8c959f",
];
const SOLARIZED: (&str, &str) = (
    "#073642 #dc322f #859900 #b58900 #268bd2 #d33682 #2aa198 #eee8d5",
    "#002b36 #cb4b16 #586e75 #657b83 #839496 #6c71c4 #93a1a1 #fdf6e3",
);

/// Each named theme's own terminal colors, else the dark or light set.
fn theme_ansi(theme: &str) -> Option<(&'static str, &'static str)> {
    Some(match theme {
        "nord" => (
            "#3b4252 #bf616a #a3be8c #ebcb8b #81a1c1 #b48ead #88c0d0 #e5e9f0",
            "#4c566a #bf616a #a3be8c #ebcb8b #81a1c1 #b48ead #8fbcbb #eceff4",
        ),
        "catppuccin-mocha" => (
            "#45475a #f38ba8 #a6e3a1 #f9e2af #89b4fa #f5c2e7 #94e2d5 #a6adc8",
            "#585b70 #f37799 #89d88b #ebd391 #74a8fc #f2aede #6bd7ca #bac2de",
        ),
        "tokyo-night" => (
            "#363b54 #f7768e #73daca #e0af68 #7aa2f7 #bb9af7 #7dcfff #787c99",
            "#363b54 #f7768e #73daca #e0af68 #7aa2f7 #bb9af7 #7dcfff #acb0d0",
        ),
        "rose-pine" => (
            "#26233a #eb6f92 #31748f #f6c177 #9ccfd8 #c4a7e7 #ebbcba #e0def4",
            "#908caa #eb6f92 #31748f #f6c177 #9ccfd8 #c4a7e7 #ebbcba #e0def4",
        ),
        "solarized-dark" | "solarized-light" => SOLARIZED,
        "catppuccin-latte" => (
            "#5c5f77 #d20f39 #40a02b #df8e1d #1e66f5 #ea76cb #179299 #acb0be",
            "#6c6f85 #de293e #49af3d #eea02d #456eff #fe85d8 #2d9fa8 #bcc0cc",
        ),
        "rose-pine-dawn" => (
            "#f2e9e1 #b4637a #286983 #ea9d34 #56949f #907aa9 #d7827e #575279",
            "#797593 #b4637a #286983 #ea9d34 #56949f #907aa9 #d7827e #575279",
        ),
        _ => return None,
    })
}

/// The 16 ANSI colors for a theme id (gitviber-ui's ThemeId::as_str) and appearance.
pub fn ansi_colors(theme: &str, dark: bool) -> [Hsla; 16] {
    let mut out = [Hsla::default(); 16];
    match theme_ansi(theme) {
        Some((normal, bright)) => {
            for (i, c) in normal
                .split(' ')
                .chain(bright.split(' '))
                .enumerate()
                .take(16)
            {
                out[i] = hex(c);
            }
        }
        None => {
            let set = if dark { ANSI_DARK } else { ANSI_LIGHT };
            for (i, c) in set.iter().enumerate() {
                out[i] = hex(c);
            }
        }
    }
    out
}

impl TerminalTheme {
    /// The app's Dark theme, as the old index.css had it.
    pub fn dark() -> Self {
        TerminalTheme {
            background: hex("#171718"),
            foreground: hex("#ececee"),
            cursor: hex("#4a9ff5"),
            cursor_text: hex("#171718"),
            selection: hex("#4a9ff555"),
            ansi: ansi_colors("dark", true),
            panel: hex("#131314"),
            elevated: hex("#1e1e20"),
            hover: hex("#ffffff12"),
            active: hex("#ffffff17"),
            border: hex("#242426"),
            border_strong: hex("#303033"),
            muted_foreground: hex("#a2a2a8"),
            subtle: hex("#86868d"),
            primary: hex("#4a9ff5"),
            primary_foreground: hex("#0e0e0f"),
            destructive: hex("#f47067"),
            find_match: hex("#ea5c0054"),
            find_current: hex("#ea5c00a8"),
            minimum_contrast: 4.5,
        }
    }

    /// The app's Light theme.
    pub fn light() -> Self {
        TerminalTheme {
            background: hex("#ffffff"),
            foreground: hex("#1d1d1f"),
            cursor: hex("#0969da"),
            cursor_text: hex("#ffffff"),
            selection: hex("#0969da55"),
            ansi: ansi_colors("light", false),
            panel: hex("#f8f8f9"),
            elevated: hex("#ffffff"),
            hover: hex("#0000000b"),
            active: hex("#00000012"),
            border: hex("#e3e3e6"),
            border_strong: hex("#d0d0d5"),
            muted_foreground: hex("#5b5b62"),
            subtle: hex("#6e6e75"),
            primary: hex("#0969da"),
            primary_foreground: hex("#ffffff"),
            destructive: hex("#cf222e"),
            find_match: hex("#ea5c0054"),
            find_current: hex("#ea5c00a8"),
            minimum_contrast: 4.5,
        }
    }
}

/// A color as alacritty's answers to a program's color query take it.
pub(crate) fn to_rgb(c: Hsla) -> alacritty_terminal::vte::ansi::Rgb {
    let Rgba { r, g, b, .. } = c.to_rgb();
    let byte = |v: f32| (v.clamp(0., 1.) * 255.).round() as u8;
    alacritty_terminal::vte::ansi::Rgb {
        r: byte(r),
        g: byte(g),
        b: byte(b),
    }
}
