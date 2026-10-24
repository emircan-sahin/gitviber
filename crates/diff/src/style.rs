//! How diffs look and what they show, set by the app from its theme and settings. Kept as
//! globals so every open diff follows a change at once; a view's own toggles write them back,
//! and the app observes them to save the settings.

use gpui_kit::component::highlighter::HighlightTheme;
use gpui_kit::{App, Global, Hsla, Pixels, Rgba, SharedString, px};
use std::sync::Arc;

/// The diff's colors: the old app's tokens of the same names.
#[derive(Clone, Debug, PartialEq)]
pub struct DiffColors {
    pub background: Hsla,
    pub panel: Hsla,
    pub elevated: Hsla,
    pub hover: Hsla,
    pub border: Hsla,
    pub foreground: Hsla,
    pub muted: Hsla,
    pub subtle: Hsla,
    pub primary: Hsla,
    pub primary_foreground: Hsla,
    pub added: Hsla,
    pub removed: Hsla,
    pub add_bg: Hsla,
    pub add_gutter: Hsla,
    pub add_emph: Hsla,
    pub del_bg: Hsla,
    pub del_gutter: Hsla,
    pub del_emph: Hsla,
    /// Lines picked for staging or a note.
    pub selection: Hsla,
    pub destructive: Hsla,
}

fn hex(s: &str) -> Hsla {
    let n = u32::from_str_radix(s.trim_start_matches('#'), 16).unwrap_or(0);
    let rgba = if s.len() > 7 { n } else { (n << 8) | 0xff };
    Rgba {
        r: (rgba >> 24) as f32 / 255.,
        g: ((rgba >> 16) & 0xff) as f32 / 255.,
        b: ((rgba >> 8) & 0xff) as f32 / 255.,
        a: (rgba & 0xff) as f32 / 255.,
    }
    .into()
}

impl DiffColors {
    /// The old app's Dark theme.
    pub fn dark() -> Self {
        DiffColors {
            background: hex("#171718"),
            panel: hex("#131314"),
            elevated: hex("#1e1e20"),
            hover: hex("#ffffff0f"),
            border: hex("#242426"),
            foreground: hex("#ececee"),
            muted: hex("#a1a1a8"),
            subtle: hex("#86868d"),
            primary: hex("#4a9ff5"),
            primary_foreground: hex("#ffffff"),
            added: hex("#57d17a"),
            removed: hex("#f47067"),
            add_bg: hex("#1d2821"),
            add_gutter: hex("#203126"),
            add_emph: hex("#57d17a33"),
            del_bg: hex("#2b1f1f"),
            del_gutter: hex("#362323"),
            del_emph: hex("#f4706738"),
            selection: hex("#4a9ff533"),
            destructive: hex("#f47067"),
        }
    }

    /// The old app's Light theme.
    pub fn light() -> Self {
        DiffColors {
            background: hex("#ffffff"),
            panel: hex("#f8f8f9"),
            elevated: hex("#ffffff"),
            hover: hex("#0000000a"),
            border: hex("#e3e3e6"),
            foreground: hex("#1d1d1f"),
            muted: hex("#57575d"),
            subtle: hex("#6e6e75"),
            primary: hex("#0969da"),
            primary_foreground: hex("#ffffff"),
            added: hex("#1a7f37"),
            removed: hex("#cf222e"),
            add_bg: hex("#e9f7ec"),
            add_gutter: hex("#d3f0da"),
            add_emph: hex("#1a7f3733"),
            del_bg: hex("#fdeeee"),
            del_gutter: hex("#fad5d5"),
            del_emph: hex("#cf222e2b"),
            selection: hex("#0969da26"),
            destructive: hex("#cf222e"),
        }
    }
}

/// Find's colors, the same in every theme: its mark on the overview, a match's wash, the current
/// one's. The app's theme has them; a diff shown without it (tests, the demo) uses the defaults.
pub(crate) struct FindColors {
    pub mark: Hsla,
    pub found: Hsla,
    pub current: Hsla,
}

pub(crate) fn find_colors(cx: &App) -> FindColors {
    match cx.try_global::<gitviber_ui::AppTheme>() {
        Some(t) => FindColors {
            mark: t.find_mark,
            found: t.find_match,
            current: t.find_current,
        },
        None => FindColors {
            mark: hex("#d18616"),
            found: hex("#ea5c0054"),
            current: hex("#ea5c00a8"),
        },
    }
}

/// Colors and the code font.
#[derive(Clone)]
pub struct DiffStyle {
    pub colors: DiffColors,
    pub syntax: Arc<HighlightTheme>,
    pub font_family: SharedString,
    pub font_size: Pixels,
    /// Line height as a multiple of the font size.
    pub line_height: f32,
    /// The UI font size, for headers and buttons.
    pub ui_font_size: Pixels,
}

impl DiffStyle {
    pub fn dark() -> Self {
        DiffStyle {
            colors: DiffColors::dark(),
            syntax: HighlightTheme::default_dark(),
            font_family: if cfg!(target_os = "macos") {
                "Menlo".into()
            } else {
                "monospace".into()
            },
            font_size: px(12.5),
            line_height: 1.5,
            ui_font_size: px(12.),
        }
    }

    pub fn line_px(&self) -> Pixels {
        (self.font_size * self.line_height).round()
    }

    pub fn get(cx: &App) -> &DiffStyle {
        cx.try_global::<DiffStyle>()
            .unwrap_or_else(|| default_style())
    }

    /// Replaces the style; open diffs redraw.
    pub fn set(style: DiffStyle, cx: &mut App) {
        cx.set_global(style);
        cx.refresh_windows();
    }
}

fn default_style() -> &'static DiffStyle {
    static STYLE: std::sync::OnceLock<DiffStyle> = std::sync::OnceLock::new();
    STYLE.get_or_init(DiffStyle::dark)
}

impl Global for DiffStyle {}

/// Unified or split.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Layout {
    #[default]
    Unified,
    Split,
}

/// What changes a diff ignores, as git's `-b` (`Amount`: re-indentation, trailing spaces) and
/// `-w` (`All`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Whitespace {
    #[default]
    Exact,
    Amount,
    All,
}

impl Whitespace {
    pub fn core_name(self) -> Option<String> {
        match self {
            Whitespace::Exact => None,
            Whitespace::Amount => Some("amount".into()),
            Whitespace::All => Some("all".into()),
        }
    }
}

/// How a changed image's two versions show.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ImageMode {
    #[default]
    Side,
    /// A divider between them.
    Swipe,
    /// After faded in over before.
    Onion,
}

/// What every diff shows: the settings' sideBySide, hideUnchanged, ignoreWhitespace and
/// imageCompare.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiffOptions {
    pub layout: Layout,
    /// Unchanged lines folded away around the changes (⌥C shows the whole file).
    pub collapse: bool,
    pub whitespace: Whitespace,
    pub image: ImageMode,
}

impl Default for DiffOptions {
    fn default() -> Self {
        DiffOptions {
            layout: Layout::Unified,
            collapse: true,
            whitespace: Whitespace::Exact,
            image: ImageMode::Side,
        }
    }
}

impl Global for DiffOptions {}

impl DiffOptions {
    pub fn get(cx: &App) -> DiffOptions {
        cx.try_global::<DiffOptions>().copied().unwrap_or_default()
    }

    /// Every open diff follows; `cx.observe_global::<DiffOptions>` saves them.
    pub fn set(options: DiffOptions, cx: &mut App) {
        cx.set_global(options);
        cx.refresh_windows();
    }

    pub fn update(cx: &mut App, f: impl FnOnce(&mut DiffOptions)) {
        let mut o = DiffOptions::get(cx);
        f(&mut o);
        DiffOptions::set(o, cx);
    }
}
