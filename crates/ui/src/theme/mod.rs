//! The app's colors and fonts, from Settings and the OS appearance. `cx.app_theme()` has our
//! tokens (sidebar, panel, elevated, the git and diff colors…); gpui-component's own theme is
//! driven from the same palette, so its buttons, inputs and menus match. Both change live when
//! the settings or the OS appearance do.
//!
//! Syntax colors come with the code view: it reads `syntax_theme` (a Shiki-style id from
//! Settings) and installs its highlight theme; this module leaves gpui-component's alone.

#[cfg(target_os = "macos")]
mod display;
mod palettes;
pub mod syntax;

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use gpui_kit::component::{Theme, ThemeConfig, ThemeConfigColors, ThemeMode};
use gpui_kit::{
    Anchor, App, FontWeight, Global, Hsla, Pixels, Refineable as _, Rgba, SharedString,
    StyleRefinement, Styled as _, Subscription, Window, WindowAppearance,
    WindowBackgroundAppearance, font, px,
};
use serde::{Deserialize, Serialize};

use crate::keymap::IS_MAC;
use crate::settings::{Appearance, CodeFont, Settings, TerminalFont, UiFont, WINDOW_OPACITY};

/// A palette as the old app's CSS had it: #RRGGBB or #RRGGBBAA.
pub struct Palette {
    pub sidebar: &'static str,
    pub panel: &'static str,
    pub background: &'static str,
    pub elevated: &'static str,
    pub hover: &'static str,
    pub active: &'static str,
    pub border: &'static str,
    pub border_strong: &'static str,
    pub foreground: &'static str,
    pub muted_foreground: &'static str,
    pub subtle: &'static str,
    pub primary: &'static str,
    pub primary_foreground: &'static str,
    pub ring: &'static str,
    pub destructive: &'static str,
    pub added: &'static str,
    pub removed: &'static str,
    pub modified: &'static str,
    pub renamed: &'static str,
    pub conflict: &'static str,
    pub info: &'static str,
    pub added_fill: &'static str,
    pub removed_fill: &'static str,
    pub modified_fill: &'static str,
    pub renamed_fill: &'static str,
    pub conflict_fill: &'static str,
    pub on_status: &'static str,
    pub hatch: &'static str,
    pub add_bg: &'static str,
    pub add_gutter: &'static str,
    pub add_emph: &'static str,
    pub del_bg: &'static str,
    pub del_gutter: &'static str,
    pub del_emph: &'static str,
    pub find_mark: &'static str,
    pub find_match: &'static str,
    pub find_current: &'static str,
    /// A worktree's color is a hue at this OKLCH lightness and chroma.
    pub tint_l: f32,
    pub tint_c: f32,
    pub dark: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ThemeId {
    #[serde(rename = "dark")]
    Dark,
    #[serde(rename = "dim")]
    Dim,
    #[serde(rename = "nord")]
    Nord,
    #[serde(rename = "catppuccin-mocha")]
    CatppuccinMocha,
    #[serde(rename = "tokyo-night")]
    TokyoNight,
    #[serde(rename = "rose-pine")]
    RosePine,
    #[serde(rename = "solarized-dark")]
    SolarizedDark,
    #[serde(rename = "light")]
    Light,
    #[serde(rename = "catppuccin-latte")]
    CatppuccinLatte,
    #[serde(rename = "rose-pine-dawn")]
    RosePineDawn,
    #[serde(rename = "solarized-light")]
    SolarizedLight,
}

impl ThemeId {
    pub const ALL: &[ThemeId] = &[
        Self::Dark,
        Self::Dim,
        Self::Nord,
        Self::CatppuccinMocha,
        Self::TokyoNight,
        Self::RosePine,
        Self::SolarizedDark,
        Self::Light,
        Self::CatppuccinLatte,
        Self::RosePineDawn,
        Self::SolarizedLight,
    ];

    pub fn palette(self) -> &'static Palette {
        match self {
            Self::Dark => &palettes::DARK,
            Self::Dim => &palettes::DIM,
            Self::Nord => &palettes::NORD,
            Self::CatppuccinMocha => &palettes::CATPPUCCIN_MOCHA,
            Self::TokyoNight => &palettes::TOKYO_NIGHT,
            Self::RosePine => &palettes::ROSE_PINE,
            Self::SolarizedDark => &palettes::SOLARIZED_DARK,
            Self::Light => &palettes::LIGHT,
            Self::CatppuccinLatte => &palettes::CATPPUCCIN_LATTE,
            Self::RosePineDawn => &palettes::ROSE_PINE_DAWN,
            Self::SolarizedLight => &palettes::SOLARIZED_LIGHT,
        }
    }

    pub fn is_dark(self) -> bool {
        self.palette().dark
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Dark => "Dark",
            Self::Dim => "Dimmed",
            Self::Nord => "Nord",
            Self::CatppuccinMocha => "Catppuccin Mocha",
            Self::TokyoNight => "Tokyo Night",
            Self::RosePine => "Rosé Pine",
            Self::SolarizedDark => "Solarized Dark",
            Self::Light => "Light",
            Self::CatppuccinLatte => "Catppuccin Latte",
            Self::RosePineDawn => "Rosé Pine Dawn",
            Self::SolarizedLight => "Solarized Light",
        }
    }

    /// The id settings store it under.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Dark => "dark",
            Self::Dim => "dim",
            Self::Nord => "nord",
            Self::CatppuccinMocha => "catppuccin-mocha",
            Self::TokyoNight => "tokyo-night",
            Self::RosePine => "rose-pine",
            Self::SolarizedDark => "solarized-dark",
            Self::Light => "light",
            Self::CatppuccinLatte => "catppuccin-latte",
            Self::RosePineDawn => "rose-pine-dawn",
            Self::SolarizedLight => "solarized-light",
        }
    }

    pub fn parse(id: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|t| t.as_str() == id)
    }

    /// The syntax theme picking this palette picks too; either can be changed after.
    pub fn syntax(self) -> &'static str {
        match self {
            Self::Dark | Self::Dim => "dark-plus",
            Self::Nord => "nord",
            Self::CatppuccinMocha => "catppuccin-mocha",
            Self::TokyoNight => "tokyo-night",
            Self::RosePine => "rose-pine",
            Self::SolarizedDark => "solarized-dark",
            Self::Light => "github-light-default",
            Self::CatppuccinLatte => "catppuccin-latte",
            Self::RosePineDawn => "rose-pine-dawn",
            Self::SolarizedLight => "solarized-light",
        }
    }
}

/// How see-through the window is (macOS): the code at `code`, the bars and side panels a step
/// more opaque at `chrome` so the layout still reads, and `step`, the tint a panel's color adds
/// inside the code to reach the bars' level (tabs, file headers, cards).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glass {
    pub code: f32,
    pub chrome: f32,
    pub step: f32,
    /// The terminal shows the desktop through too.
    pub terminal: bool,
    pub blur: bool,
}

impl Glass {
    pub fn at(opacity: f32, terminal: bool, blur: bool) -> Self {
        let step = 0.25;
        Self {
            code: opacity,
            chrome: 1.0 - (1.0 - opacity) * (1.0 - step),
            step,
            terminal,
            blur,
        }
    }
}

/// Families resolved to what is installed, and the weights and sizes to draw them at.
#[derive(Clone, Debug, PartialEq)]
pub struct Fonts {
    pub ui: SharedString,
    /// Body text's weight in `ui`, as `AppTheme::weight` gives it; set it as is.
    pub ui_weight: FontWeight,
    /// What the Interface font weight setting adds to the normal, medium and semibold weights:
    /// 0 at Regular, 100 at Medium.
    pub ui_step: f32,
    pub code: SharedString,
    pub code_weight: FontWeight,
    pub code_size: Pixels,
    /// A multiple of the code font size.
    pub code_line_height: f32,
    pub terminal: SharedString,
    pub terminal_size: Pixels,
}

/// The resolved theme: our tokens as colors, the fonts, and the window's see-through levels.
#[derive(Clone, Debug, PartialEq)]
pub struct AppTheme {
    pub id: ThemeId,
    pub dark: bool,
    pub sidebar: Hsla,
    pub panel: Hsla,
    pub background: Hsla,
    pub elevated: Hsla,
    pub hover: Hsla,
    pub active: Hsla,
    pub border: Hsla,
    pub border_strong: Hsla,
    pub foreground: Hsla,
    pub muted_foreground: Hsla,
    /// ≥4.5:1 on every surface up to `elevated`, hovered and selected rows too: it's used for
    /// 11-12px text (timestamps, paths, line numbers).
    pub subtle: Hsla,
    pub primary: Hsla,
    pub primary_foreground: Hsla,
    pub ring: Hsla,
    pub destructive: Hsla,
    pub added: Hsla,
    pub removed: Hsla,
    pub modified: Hsla,
    pub renamed: Hsla,
    pub conflict: Hsla,
    /// News not looked at: blue, apart from the accent, which some themes make purple.
    pub info: Hsla,
    /// Solid status chips (badges, counts, the viewed check): bright fill, `on_status` ink.
    pub added_fill: Hsla,
    pub removed_fill: Hsla,
    pub modified_fill: Hsla,
    pub renamed_fill: Hsla,
    pub conflict_fill: Hsla,
    pub on_status: Hsla,
    pub hatch: Hsla,
    /// Diff rows: a faint tint, pre-mixed on `background` since a sticky gutter must be opaque;
    /// word changes are a translucent wash so syntax colors stay readable on top.
    pub add_bg: Hsla,
    pub add_gutter: Hsla,
    pub add_emph: Hsla,
    pub del_bg: Hsla,
    pub del_gutter: Hsla,
    pub del_emph: Hsla,
    /// Find's matches, the same in every theme: the scrollbar's mark, the text's wash, the current one.
    pub find_mark: Hsla,
    pub find_match: Hsla,
    pub find_current: Hsla,
    pub tint_l: f32,
    pub tint_c: f32,
    pub fonts: Fonts,
    /// The interface scale; rem is 16px times it.
    pub ui_scale: f32,
    /// The Settings syntax theme id for the appearance in use; the code view maps it to colors.
    pub syntax_theme: SharedString,
    /// Set while the window is see-through.
    pub glass: Option<Glass>,
}

impl Global for AppTheme {}

pub trait ActiveAppTheme {
    fn app_theme(&self) -> &AppTheme;
}

impl ActiveAppTheme for App {
    fn app_theme(&self) -> &AppTheme {
        self.global::<AppTheme>()
    }
}

impl AppTheme {
    /// A bar's or a side panel's color with the window see-through: `color` at the chrome level.
    pub fn chrome(&self, color: Hsla) -> Hsla {
        match self.glass {
            Some(g) => color.opacity(g.chrome),
            None => color,
        }
    }

    /// The center's color (code, diffs) with the window see-through.
    pub fn code(&self, color: Hsla) -> Hsla {
        match self.glass {
            Some(g) => color.opacity(g.code),
            None => color,
        }
    }

    /// A tab's, file header's or card's tint inside the center, to reach the bars' level.
    pub fn step(&self, color: Hsla) -> Hsla {
        match self.glass {
            Some(g) => color.opacity(g.step),
            None => color,
        }
    }

    /// The terminal's background: see-through only when its own setting says so.
    pub fn terminal_background(&self) -> Hsla {
        match self.glass {
            Some(g) if g.terminal => self.background.opacity(g.code),
            _ => self.background,
        }
    }

    pub fn window_background(&self) -> WindowBackgroundAppearance {
        match self.glass {
            Some(g) if g.blur => WindowBackgroundAppearance::Blurred,
            Some(_) => WindowBackgroundAppearance::Transparent,
            None => WindowBackgroundAppearance::Opaque,
        }
    }

    /// The text size most of the interface uses (12px at 100%), in rems.
    pub fn ui_text(&self) -> gpui_kit::Rems {
        gpui_kit::rems(0.75)
    }

    /// A weight for text in the UI font: 400 body, 500 medium, 600 semibold, 700 bold. Body,
    /// medium and semibold move up with the Interface font weight setting, so at its default a
    /// medium label draws at 600 and still stands out from 500 body text; bold stays. Every UI
    /// weight goes through here: a bare `FontWeight::MEDIUM` is body text at the default setting.
    pub fn weight(&self, weight: FontWeight) -> FontWeight {
        label_weight(weight, self.fonts.ui_step)
    }

    /// A label a step above body text: `weight(FontWeight::MEDIUM)`.
    pub fn medium(&self) -> FontWeight {
        self.weight(FontWeight::MEDIUM)
    }

    /// Two steps above body text: `weight(FontWeight::SEMIBOLD)`.
    pub fn semibold(&self) -> FontWeight {
        self.weight(FontWeight::SEMIBOLD)
    }
}

fn label_weight(weight: FontWeight, step: f32) -> FontWeight {
    if (400.0..=600.0).contains(&weight.0) {
        FontWeight(weight.0 + step)
    } else {
        weight
    }
}

/// #RRGGBB or #RRGGBBAA as a color; black for anything else.
pub fn hex(text: &str) -> Hsla {
    Rgba::try_from(text).map(Into::into).unwrap_or_default()
}

fn to_hex(color: Hsla) -> SharedString {
    let c = Rgba::from(color);
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}{:02x}", b(c.r), b(c.g), b(c.b), b(c.a)).into()
}

/// WCAG's contrast ratio of two sRGB colors.
fn contrast(a: [f32; 3], b: [f32; 3]) -> f32 {
    let lum = |c: [f32; 3]| {
        let ch = |v: f32| {
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * ch(c[0]) + 0.7152 * ch(c[1]) + 0.0722 * ch(c[2])
    };
    let (x, y) = (lum(a), lum(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

/// The lowest window opacity (percent, on WINDOW_OPACITY's steps) at which a theme's text on its
/// background keeps 3:1 over a black and a white desktop alike, the darkest and lightest a
/// blurred wallpaper averages to.
pub fn opacity_floor(theme: ThemeId) -> f32 {
    let rgb = |s: &str| {
        let c = Rgba::try_from(s).unwrap_or_default();
        [c.r, c.g, c.b]
    };
    let p = theme.palette();
    let (text, under) = (rgb(p.foreground), rgb(p.background));
    let readable = |pct: f32| {
        [0.0, 1.0].iter().all(|desk| {
            let mixed = under.map(|v| v * pct / 100.0 + desk * (1.0 - pct / 100.0));
            contrast(text, mixed) >= 3.0
        })
    };
    let mut pct = WINDOW_OPACITY.min;
    while pct < WINDOW_OPACITY.max && !readable(pct) {
        pct += WINDOW_OPACITY.step;
    }
    pct
}

/// The floor of the chosen light and dark themes, the higher one: one range for both, so
/// following the system's appearance doesn't move the slider.
pub fn appearance_floor(settings: &Settings) -> f32 {
    opacity_floor(settings.light_theme).max(opacity_floor(settings.dark_theme))
}

/// Which installed families each name resolved to, asked of the OS once per name.
#[derive(Default)]
struct InstalledFonts(HashMap<SharedString, bool>);

impl Global for InstalledFonts {}

/// Whether the family `name` is installed, asked of the OS once per name: for the font pickers.
pub fn installed(name: &str, cx: &mut App) -> bool {
    if name.starts_with('.') {
        return true;
    }
    if let Some(hit) = cx.default_global::<InstalledFonts>().0.get(name) {
        return *hit;
    }
    crate::fonts::ensure(name, cx);
    // Loads this one family instead of listing every font (40-50 ms before the first frame). A
    // family that fails to load resolves to gpui's fallback stack, whose font has another name.
    let text = cx.text_system();
    let found = text
        .get_font_for_id(text.resolve_font(&font(name.to_string())))
        .is_some_and(|f| f.family.as_ref() == name);
    cx.global_mut::<InstalledFonts>()
        .0
        .insert(SharedString::from(name.to_string()), found);
    found
}

fn first_installed(names: &[&str], cx: &mut App) -> SharedString {
    for name in names {
        if installed(name, cx) {
            return SharedString::from(name.to_string());
        }
    }
    SharedString::from(names.last().copied().unwrap_or(".SystemUIFont").to_string())
}

const MONO_FALLBACKS: &[&str] = if cfg!(target_os = "macos") {
    &["SF Mono", "Menlo"]
} else if cfg!(windows) {
    &["Cascadia Mono", "Consolas"]
} else {
    &[
        "JetBrains Mono",
        "DejaVu Sans Mono",
        "Noto Sans Mono",
        "Liberation Mono",
    ]
};

fn mono(first: &str, cx: &mut App) -> SharedString {
    let mut names = vec![first];
    names.extend(MONO_FALLBACKS.iter().copied().filter(|n| *n != first));
    first_installed(&names, cx)
}

fn code_family(font: CodeFont, custom: &str, cx: &mut App) -> SharedString {
    match font {
        CodeFont::Custom if !custom.is_empty() && installed(custom, cx) => {
            custom.to_string().into()
        }
        CodeFont::Custom => mono(CodeFont::platform_default().as_str(), cx),
        preset => mono(preset.as_str(), cx),
    }
}

fn resolve_fonts(s: &Settings, cx: &mut App) -> Fonts {
    let ui = match s.ui_font {
        UiFont::System => ".SystemUIFont".into(),
        UiFont::Custom if !s.custom_ui_font.trim().is_empty() => {
            first_installed(&[s.custom_ui_font.trim(), ".SystemUIFont"], cx)
        }
        UiFont::Custom => ".SystemUIFont".into(),
        preset => first_installed(&[preset.as_str(), ".SystemUIFont"], cx),
    };
    let code = code_family(s.code_font, s.custom_code_font.trim(), cx);
    let terminal = match s.terminal_font {
        TerminalFont::Editor => code.clone(),
        TerminalFont::Custom
            if !s.custom_terminal_font.trim().is_empty()
                && installed(s.custom_terminal_font.trim(), cx) =>
        {
            s.custom_terminal_font.trim().to_string().into()
        }
        TerminalFont::Custom => code.clone(),
        preset => mono(preset.as_str(), cx),
    };
    let ui_step = f32::from(s.ui_font_weight) - 400.0;
    Fonts {
        ui_weight: label_weight(FontWeight::NORMAL, ui_step),
        ui,
        ui_step,
        code,
        code_weight: FontWeight(s.code_font_weight as f32),
        code_size: px(s.code_font_size),
        code_line_height: s.line_height,
        terminal,
        terminal_size: px(s.terminal_font_size),
    }
}

/// What the theme follows of the OS besides the settings.
#[derive(Clone, Copy, Debug)]
pub struct Os {
    pub dark: bool,
    /// Reduce transparency is on: the window stays opaque whatever its opacity setting.
    pub reduce_transparency: bool,
    /// A window is in full screen, where the desktop is out of sight and a see-through window
    /// shows a gray backdrop: solid, as at 100%.
    pub full_screen: bool,
}

impl Os {
    fn now(cx: &App) -> Self {
        Self {
            dark: system_dark(cx),
            full_screen: cx
                .try_global::<FullScreen>()
                .is_some_and(|f| !f.0.is_empty()),
            #[cfg(target_os = "macos")]
            reduce_transparency: display::reduce_transparency(),
            #[cfg(not(target_os = "macos"))]
            reduce_transparency: false,
        }
    }
}

/// The OS is dark now. GITVIBER_APPEARANCE=dark|light stands in for it: an off-screen
/// snapshot's test platform always reports light. With the `snapshot` feature, `dark,light`
/// (or `light,dark`) switches a second in, to picture a live switch.
fn system_dark(cx: &App) -> bool {
    if let Ok(names) = std::env::var("GITVIBER_APPEARANCE") {
        #[cfg(feature = "snapshot")]
        let names = match names.split_once(',') {
            Some((_, after)) if snapshot_flip::done() => after.to_string(),
            Some((before, _)) => before.to_string(),
            None => names,
        };
        match names.as_str() {
            "dark" => return true,
            "light" => return false,
            _ => {}
        }
    }
    matches!(
        cx.window_appearance(),
        WindowAppearance::Dark | WindowAppearance::VibrantDark
    )
}

#[cfg(feature = "snapshot")]
mod snapshot_flip {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    use gpui_kit::App;

    static DONE: AtomicBool = AtomicBool::new(false);

    pub(super) fn done() -> bool {
        DONE.load(Ordering::Relaxed)
    }

    pub(super) fn schedule(cx: &mut App) {
        if !std::env::var("GITVIBER_APPEARANCE").is_ok_and(|v| v.contains(',')) {
            return;
        }
        let timer = cx.background_executor().timer(Duration::from_secs(1));
        cx.spawn(async move |cx| {
            timer.await;
            DONE.store(true, Ordering::Relaxed);
            cx.update(super::refresh);
        })
        .detach();
    }
}

/// The appearance the app's native parts (sheets, alerts, menus, web pages shown "as the app")
/// are held to: the theme's when Appearance is Dark or Light, the OS's under System.
fn native_appearance(settings: &Settings) -> Option<WindowAppearance> {
    match settings.appearance {
        Appearance::System => None,
        Appearance::Dark => Some(WindowAppearance::Dark),
        Appearance::Light => Some(WindowAppearance::Light),
    }
}

/// What `native_appearance` last gave macOS.
struct NativeAppearance(Option<WindowAppearance>);

impl Global for NativeAppearance {}

/// The theme the settings and the OS make, with `fonts` already resolved.
pub fn build(settings: &Settings, os: Os, fonts: Fonts) -> AppTheme {
    let id = settings.theme(os.dark);
    let p = id.palette();
    let opacity = if IS_MAC
        && settings.window_opacity < 100.0
        && !os.reduce_transparency
        && !os.full_screen
    {
        settings.window_opacity.max(appearance_floor(settings))
    } else {
        100.0
    };
    let glass = (opacity < 100.0).then(|| {
        Glass::at(
            opacity / 100.0,
            settings.terminal_glass,
            settings.background_blur > 0.0,
        )
    });
    AppTheme {
        id,
        dark: p.dark,
        sidebar: hex(p.sidebar),
        panel: hex(p.panel),
        background: hex(p.background),
        elevated: hex(p.elevated),
        hover: hex(p.hover),
        active: hex(p.active),
        border: hex(p.border),
        border_strong: hex(p.border_strong),
        foreground: hex(p.foreground),
        muted_foreground: hex(p.muted_foreground),
        subtle: hex(p.subtle),
        primary: hex(p.primary),
        primary_foreground: hex(p.primary_foreground),
        ring: hex(p.ring),
        destructive: hex(p.destructive),
        added: hex(p.added),
        removed: hex(p.removed),
        modified: hex(p.modified),
        renamed: hex(p.renamed),
        conflict: hex(p.conflict),
        info: hex(p.info),
        added_fill: hex(p.added_fill),
        removed_fill: hex(p.removed_fill),
        modified_fill: hex(p.modified_fill),
        renamed_fill: hex(p.renamed_fill),
        conflict_fill: hex(p.conflict_fill),
        on_status: hex(p.on_status),
        hatch: hex(p.hatch),
        add_bg: hex(p.add_bg),
        add_gutter: hex(p.add_gutter),
        add_emph: hex(p.add_emph),
        del_bg: hex(p.del_bg),
        del_gutter: hex(p.del_gutter),
        del_emph: hex(p.del_emph),
        find_mark: hex(p.find_mark),
        find_match: hex(p.find_match),
        find_current: hex(p.find_current),
        tint_l: p.tint_l,
        tint_c: p.tint_c,
        fonts,
        ui_scale: settings.ui_scale,
        syntax_theme: settings.code_theme(os.dark).to_string().into(),
        glass,
    }
}

/// Our palette as gpui-component's theme, so its controls wear the same colors.
pub fn component_config(t: &AppTheme) -> ThemeConfig {
    let mut c = ThemeConfigColors::default();
    macro_rules! set {
        ($($field:ident = $color:expr),* $(,)?) => { $(c.$field = Some(to_hex($color));)* };
    }
    let clear = gpui_kit::transparent_black();
    let mix = |over: Hsla, under: Hsla, amount: f32| under.blend(over.opacity(amount));
    let primary_hover = mix(t.primary, t.background, 0.85);
    set!(
        background = t.background,
        foreground = t.foreground,
        border = t.border,
        input = t.border_strong,
        muted = t.elevated,
        muted_foreground = t.muted_foreground,
        accent = t.hover,
        accent_foreground = t.foreground,
        popover = t.elevated,
        popover_foreground = t.foreground,
        primary = t.primary,
        primary_hover = primary_hover,
        primary_active = primary_hover,
        primary_foreground = t.primary_foreground,
        secondary = t.elevated,
        secondary_hover = t.elevated.blend(t.hover),
        secondary_active = t.elevated.blend(t.active),
        secondary_foreground = t.foreground,
        button = t.elevated,
        button_hover = t.elevated.blend(t.hover),
        button_active = t.elevated.blend(t.active),
        button_foreground = t.foreground,
        ring = t.ring,
        caret = t.primary,
        selection = t.primary.opacity(0.35),
        link = t.primary,
        link_hover = t.primary,
        link_active = t.primary,
        danger = t.destructive,
        danger_hover = mix(t.destructive, t.background, 0.85),
        danger_active = mix(t.destructive, t.background, 0.85),
        danger_foreground = t.primary_foreground,
        success = t.added_fill,
        success_foreground = t.on_status,
        warning = t.modified_fill,
        warning_foreground = t.on_status,
        info = t.info,
        info_foreground = t.primary_foreground,
        list = clear,
        list_even = clear,
        list_head = clear,
        list_hover = t.hover,
        list_active = t.active,
        list_active_border = t.primary,
        table = clear,
        table_even = clear,
        table_head = clear,
        table_hover = t.hover,
        table_active = t.active,
        table_row_border = t.border,
        sidebar = t.sidebar,
        sidebar_foreground = t.foreground,
        sidebar_border = t.border,
        sidebar_accent = t.active,
        sidebar_accent_foreground = t.foreground,
        sidebar_primary = t.primary,
        sidebar_primary_foreground = t.primary_foreground,
        tab_bar = clear,
        tab = clear,
        tab_active = t.background,
        tab_foreground = t.muted_foreground,
        tab_active_foreground = t.foreground,
        tab_bar_segmented = t.elevated,
        title_bar = t.sidebar,
        title_bar_border = t.border,
        status_bar = t.sidebar,
        status_bar_border = t.border,
        scrollbar = clear,
        scrollbar_thumb = t.border_strong,
        scrollbar_thumb_hover = t.subtle,
        switch = t.border_strong,
        switch_thumb = hex("#ffffff"),
        // Off, the white thumb on a light gray track nearly vanishes on a white surface.
        switch_border = t.subtle,
        // Placeholders apart from typed text: muted is too close to it in the light themes.
        input_placeholder = t.subtle,
        slider_bar = t.primary,
        slider_thumb = hex("#ffffff"),
        progress_bar = t.primary,
        skeleton = t.elevated,
        accordion = clear,
        group_box = t.panel,
        group_box_foreground = t.foreground,
        description_list_label = t.elevated,
        description_list_label_foreground = t.muted_foreground,
        drag_border = t.primary,
        drop_target = t.primary.opacity(0.2),
        overlay = hex("#00000066"),
        window_border = t.border,
    );
    ThemeConfig {
        name: t.id.label().into(),
        mode: if t.dark {
            ThemeMode::Dark
        } else {
            ThemeMode::Light
        },
        font_family: Some(t.fonts.ui.clone()),
        font_size: Some(16.0 * t.ui_scale),
        mono_font_family: Some(t.fonts.code.clone()),
        mono_font_size: Some(f32::from(t.fonts.code_size)),
        radius: Some(4),
        radius_lg: Some(6),
        shadow: Some(true),
        colors: c,
        ..Default::default()
    }
}

/// Recomputes the theme from the settings and the OS appearance; repaints when it changed.
pub fn refresh(cx: &mut App) {
    let settings = Settings::get(cx).clone();
    // First: while an appearance is held, the OS's reads as that one.
    let native = native_appearance(&settings);
    if cx.try_global::<NativeAppearance>().map(|n| n.0) != Some(native) {
        cx.set_window_appearance(native);
        cx.set_global(NativeAppearance(native));
    }
    let fonts = resolve_fonts(&settings, cx);
    let next = build(&settings, Os::now(cx), fonts);
    if cx.try_global::<AppTheme>() == Some(&next) {
        return;
    }
    let config = Rc::new(component_config(&next));
    cx.set_global(next);
    Theme::update(cx, |theme| {
        theme.apply_config(&config);
        // Bottom right, clear of the top bar's and Settings' controls.
        theme.notification.placement = Anchor::BottomRight;
        theme.notification.width = px(384.);
    });
}

/// The windows in full screen, by their NSWindow.
#[derive(Default)]
struct FullScreen(HashSet<usize>);

impl Global for FullScreen {}

/// `window` (an NSWindow) starts entering full screen (`on`) or leaving it, or closes: the theme
/// turns solid while one is in full screen, and see-through again after.
fn full_screen(window: usize, on: bool, cx: &mut App) {
    let set = &mut cx.default_global::<FullScreen>().0;
    let changed = if on {
        set.insert(window)
    } else {
        set.remove(&window)
    };
    if changed {
        refresh(cx);
    }
}

/// Keeps a window in step with the theme: the OS appearance it reports, its see-through
/// background, and a clear root so our surfaces paint the window. Call once per window and keep
/// the subscriptions as long as the window's view.
pub fn attach(window: &mut Window, cx: &mut App) -> Vec<Subscription> {
    // Deferred: while the window's view is being built its root isn't there yet.
    window.defer(cx, style_root);
    let applied = Rc::new(Cell::new(cx.app_theme().window_background()));
    window.set_background_appearance(applied.get());
    vec![
        window.observe_window_appearance(|_, cx| refresh(cx)),
        window.observe_global::<AppTheme>(cx, move |window, cx| {
            let want = cx.app_theme().window_background();
            if want != applied.get() {
                applied.set(want);
                window.set_background_appearance(want);
            }
            style_root(window, cx);
        }),
    ]
}

/// The window root's text: dialogs, toasts, menus and tooltips sit beside the window's view, not
/// in it, so without this they fell back to a 16px rem at the Regular weight.
fn style_root(window: &mut Window, cx: &mut App) {
    let t = cx.app_theme();
    let style = StyleRefinement::default()
        .bg(gpui_kit::transparent_black())
        .text_size(t.ui_text())
        .font_weight(t.fonts.ui_weight);
    gpui_kit::base::Root::update(window, cx, |root, _, cx| {
        root.style().refine(&style);
        cx.notify();
    });
}

pub(crate) fn init(cx: &mut App) {
    refresh(cx);
    syntax::init(cx);
    cx.observe_global::<Settings>(refresh).detach();
    #[cfg(target_os = "macos")]
    {
        watch_display_options(cx);
        watch_full_screen(cx);
    }
    #[cfg(feature = "snapshot")]
    snapshot_flip::schedule(cx);
}

/// Reduce transparency switched in System Settings: the window turns opaque or see-through again
/// at once, not at the next settings change.
#[cfg(target_os = "macos")]
fn watch_display_options(cx: &mut App) {
    use futures::StreamExt as _;

    let (tx, mut rx) = futures::channel::mpsc::unbounded::<()>();
    display::watch(move || {
        let _ = tx.unbounded_send(());
    });
    cx.spawn(async move |cx| {
        while rx.next().await.is_some() {
            cx.update(refresh);
        }
    })
    .detach();
}

/// As each full-screen transition starts, not once it's over, so the gray never shows; a window
/// launched into full screen enters it after it opens, so it's seen too.
#[cfg(target_os = "macos")]
fn watch_full_screen(cx: &mut App) {
    use futures::StreamExt as _;

    let (tx, mut rx) = futures::channel::mpsc::unbounded::<(usize, bool)>();
    display::watch_full_screen(move |window, on| {
        let _ = tx.unbounded_send((window, on));
    });
    cx.spawn(async move |cx| {
        while let Some((window, on)) = rx.next().await {
            cx.update(|cx| full_screen(window, on, cx));
        }
    })
    .detach();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn os(dark: bool) -> Os {
        Os {
            dark,
            reduce_transparency: false,
            full_screen: false,
        }
    }

    fn fonts() -> Fonts {
        Fonts {
            ui: ".SystemUIFont".into(),
            ui_weight: FontWeight::MEDIUM,
            ui_step: 100.0,
            code: "Menlo".into(),
            code_weight: FontWeight::MEDIUM,
            code_size: px(13.5),
            code_line_height: 1.6,
            terminal: "Menlo".into(),
            terminal_size: px(13.),
        }
    }

    #[test]
    fn label_weights_step_with_the_setting() {
        let mut t = build(&Settings::default(), os(true), fonts());
        let w = |t: &AppTheme, weight: FontWeight| t.weight(weight).0;
        // Interface font weight Medium: body 500, labels 600, semibold 700.
        assert_eq!(w(&t, FontWeight::NORMAL), 500.0);
        assert_eq!(w(&t, FontWeight::MEDIUM), 600.0);
        assert_eq!(w(&t, FontWeight::SEMIBOLD), 700.0);
        assert_eq!(w(&t, FontWeight::BOLD), 700.0);
        // Regular: no step.
        t.fonts.ui_step = 0.0;
        assert_eq!(w(&t, FontWeight::NORMAL), 400.0);
        assert_eq!(t.medium().0, 500.0);
        assert_eq!(t.semibold().0, 600.0);
    }

    #[test]
    fn every_palette_parses() {
        for id in ThemeId::ALL {
            let p = id.palette();
            for value in [
                p.sidebar,
                p.background,
                p.foreground,
                p.hover,
                p.add_emph,
                p.on_status,
            ] {
                assert!(Rgba::try_from(value).is_ok(), "{} {value}", id.as_str());
            }
            assert_eq!(ThemeId::parse(id.as_str()), Some(*id));
        }
        assert_eq!(ThemeId::ALL.iter().filter(|t| t.is_dark()).count(), 7);
    }

    #[test]
    fn appearance_picks_the_theme() {
        let s = Settings {
            dark_theme: ThemeId::Nord,
            light_theme: ThemeId::SolarizedLight,
            ..Settings::default()
        };
        assert_eq!(build(&s, os(true), fonts()).id, ThemeId::Nord);
        assert_eq!(build(&s, os(false), fonts()).id, ThemeId::SolarizedLight);
        let dark = Settings {
            appearance: crate::settings::Appearance::Dark,
            ..s.clone()
        };
        assert_eq!(build(&dark, os(false), fonts()).id, ThemeId::Nord);
        assert_eq!(build(&dark, os(false), fonts()).syntax_theme, "dark-plus");
    }

    #[test]
    fn tokens_map_onto_gpui_component() {
        let t = build(&Settings::default(), os(true), fonts());
        assert_eq!(t.background, hex("#171718"));
        let config = component_config(&t);
        assert_eq!(config.mode, ThemeMode::Dark);
        assert_eq!(config.colors.background.as_deref(), Some("#171718ff"));
        assert_eq!(config.colors.sidebar.as_deref(), Some("#0e0e0fff"));
        assert_eq!(config.colors.accent.as_deref(), Some("#ffffff12"));
        assert_eq!(config.colors.input_placeholder, Some(to_hex(t.subtle)));
        assert_eq!(config.font_size, Some(16.0));
    }

    #[test]
    fn opacity_floors_match_the_old_app() {
        // Light's dark text on white stays readable lowest.
        assert_eq!(opacity_floor(ThemeId::Light), 50.0);
        for id in ThemeId::ALL {
            let f = opacity_floor(*id);
            assert!((50.0..=100.0).contains(&f), "{}", id.as_str());
        }
    }

    #[test]
    fn glass_only_below_full_opacity_on_macos() {
        let s = Settings {
            window_opacity: 85.0,
            background_blur: 20.0,
            ..Settings::default()
        };
        let t = build(&s, os(true), fonts());
        if IS_MAC {
            let g = t.glass.unwrap();
            assert!(g.blur);
            assert!(g.chrome > g.code);
            assert_eq!(t.window_background(), WindowBackgroundAppearance::Blurred);
        } else {
            assert!(t.glass.is_none());
        }
        assert!(
            build(&Settings::default(), os(true), fonts())
                .glass
                .is_none()
        );
    }

    #[test]
    fn glass_levels_step_the_bars_a_quarter_more_opaque() {
        let g = Glass::at(1.0, false, false);
        assert_eq!((g.code, g.chrome, g.step), (1.0, 1.0, 0.25));
        let g = Glass::at(0.5, false, false);
        assert_eq!((g.code, g.chrome, g.step), (0.5, 0.625, 0.25));
        assert!((Glass::at(0.7, false, false).chrome - 0.775).abs() < 1e-6);
        // The code's tint and one step over it come to the bars' level: tabs and headers match them.
        for i in 0..=10 {
            let o = 0.5 + i as f32 * 0.05;
            let g = Glass::at(o, false, false);
            assert!(
                (1.0 - (1.0 - g.code) * (1.0 - g.step) - g.chrome).abs() < 1e-3,
                "{o}"
            );
            assert!(g.chrome >= g.code, "{o}");
        }
    }

    #[test]
    fn light_and_dark_share_the_higher_floor_and_the_setting_keeps_its_value() {
        let s = Settings {
            window_opacity: 50.0,
            ..Settings::default()
        };
        // Dark stays readable down to 55%, Light to 50%: both stop at 55%.
        assert_eq!(opacity_floor(s.light_theme), 50.0);
        assert_eq!(opacity_floor(s.dark_theme), 55.0);
        assert_eq!(appearance_floor(&s), 55.0);
        for dark in [true, false] {
            let t = build(&s, os(dark), fonts());
            assert_eq!(t.glass.map(|g| g.code), IS_MAC.then_some(0.55), "{dark}");
        }
        assert_eq!(s.window_opacity, 50.0);
    }

    #[test]
    fn the_terminal_shows_through_only_with_its_switch_and_a_see_through_window() {
        let with = |terminal_glass, window_opacity| Settings {
            window_opacity,
            terminal_glass,
            ..Settings::default()
        };
        let t = build(&with(false, 80.0), os(true), fonts());
        assert_eq!(t.terminal_background(), t.background);
        let t = build(&with(true, 80.0), os(true), fonts());
        let shown = if IS_MAC {
            t.background.opacity(0.8)
        } else {
            t.background
        };
        assert_eq!(t.terminal_background(), shown);
        let t = build(&with(true, 100.0), os(true), fonts());
        assert_eq!(t.terminal_background(), t.background);
    }

    #[test]
    fn full_screen_keeps_the_window_and_the_terminal_solid() {
        let s = Settings {
            window_opacity: 70.0,
            background_blur: 20.0,
            terminal_glass: true,
            ..Settings::default()
        };
        let full = Os {
            full_screen: true,
            ..os(true)
        };
        let t = build(&s, full, fonts());
        assert!(t.glass.is_none());
        assert_eq!(t.window_background(), WindowBackgroundAppearance::Opaque);
        assert_eq!(t.terminal_background(), t.background);
        assert_eq!(t.chrome(t.sidebar), t.sidebar);
        assert_eq!(build(&s, os(true), fonts()).glass.is_some(), IS_MAC);
    }

    /// Entering full screen turns the theme solid as the transition starts, the last window
    /// leaving it (or closing) turns it see-through again, at the opacity set meanwhile.
    #[gpui_kit::test]
    fn full_screen_is_solid_and_see_through_again_on_the_way_out(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        crate::test_support::config_dir("theme-full-screen");
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::settings::init(cx);
            init(cx);
        });
        // Its observer starts with the next update.
        cx.update(|cx| {
            Settings::update(cx, |s| {
                s.window_opacity = 70.0;
                s.terminal_glass = true;
            });
        });
        let glass = |cx: &mut gpui_kit::TestAppContext| cx.update(|cx| cx.app_theme().glass);
        assert_eq!(glass(cx).map(|g| g.code), IS_MAC.then_some(0.7));
        cx.update(|cx| full_screen(1, true, cx));
        let t = cx.update(|cx| cx.app_theme().clone());
        assert!(t.glass.is_none());
        assert_eq!(t.terminal_background(), t.background);
        assert_eq!(t.window_background(), WindowBackgroundAppearance::Opaque);
        // Another window in full screen too: solid until the last one leaves.
        cx.update(|cx| {
            full_screen(2, true, cx);
            full_screen(1, false, cx);
            Settings::update(cx, |s| s.window_opacity = 60.0);
        });
        assert!(glass(cx).is_none());
        cx.update(|cx| full_screen(2, false, cx));
        let back = glass(cx);
        assert_eq!(
            back.map(|g| (g.code, g.terminal)),
            IS_MAC.then_some((0.6, true))
        );
        // Its close after it left changes nothing.
        cx.update(|cx| full_screen(2, false, cx));
        assert_eq!(glass(cx), back);
    }

    #[gpui_kit::test]
    fn reset_makes_the_window_solid_again(cx: &mut gpui_kit::TestAppContext) {
        crate::test_support::config_dir("theme-reset");
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::settings::init(cx);
            init(cx);
        });
        // Its observer starts with the next update.
        cx.update(|cx| {
            Settings::update(cx, |s| {
                s.window_opacity = 70.0;
                s.background_blur = 30.0;
            });
        });
        assert_eq!(cx.update(|cx| cx.app_theme().glass.is_some()), IS_MAC);
        cx.update(Settings::reset);
        let (t, s) = cx.update(|cx| (cx.app_theme().clone(), Settings::get(cx).clone()));
        assert_eq!((s.window_opacity, s.background_blur), (100.0, 0.0));
        assert!(t.glass.is_none());
        assert_eq!(t.window_background(), WindowBackgroundAppearance::Opaque);
    }

    /// Stored below 100%, a window opens see-through with its root clear, so our surfaces paint
    /// it, and stays so behind other windows.
    #[gpui_kit::test]
    fn a_window_opens_see_through_and_stays_so_in_the_background(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        struct Plain;
        impl gpui_kit::Render for Plain {
            fn render(
                &mut self,
                _: &mut Window,
                _: &mut gpui_kit::Context<Self>,
            ) -> impl gpui_kit::IntoElement {
                gpui_kit::div()
            }
        }
        let dir = crate::test_support::config_dir("theme-launch");
        std::fs::write(
            dir.join("settings.json"),
            r#"{"windowOpacity": 70, "backgroundBlur": 20}"#,
        )
        .unwrap();
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::settings::init(cx);
            init(cx);
        });
        let want = if IS_MAC {
            WindowBackgroundAppearance::Blurred
        } else {
            WindowBackgroundAppearance::Opaque
        };
        assert_eq!(cx.update(|cx| cx.app_theme().window_background()), want);
        let (_, vcx) = cx.add_window_view(|window, cx| {
            use gpui_kit::AppContext as _;
            let view = cx.new(|_| Plain);
            gpui_kit::base::Root::new(view, window, cx)
        });
        let _subs = vcx.update(attach);
        vcx.run_until_parked();
        let bg = vcx.update(|window, cx| {
            gpui_kit::base::Root::update(window, cx, |root, _, _| root.style().background.clone())
        });
        assert_eq!(bg, Some(gpui_kit::transparent_black().into()));
        vcx.deactivate_window();
        vcx.run_until_parked();
        assert_eq!(vcx.update(|_, cx| cx.app_theme().window_background()), want);
    }

    #[test]
    fn reduce_transparency_keeps_the_window_opaque() {
        let s = Settings {
            window_opacity: 85.0,
            background_blur: 20.0,
            ..Settings::default()
        };
        let reduced = Os {
            reduce_transparency: true,
            ..os(true)
        };
        let t = build(&s, reduced, fonts());
        assert!(t.glass.is_none());
        assert_eq!(t.window_background(), WindowBackgroundAppearance::Opaque);
        assert_eq!(t.chrome(t.sidebar), t.sidebar);
        // Turned off again, the same settings are see-through on macOS.
        assert_eq!(build(&s, os(true), fonts()).glass.is_some(), IS_MAC);
    }

    #[test]
    fn native_parts_follow_a_chosen_appearance() {
        let with = |appearance| Settings {
            appearance,
            ..Settings::default()
        };
        assert_eq!(native_appearance(&with(Appearance::System)), None);
        assert_eq!(
            native_appearance(&with(Appearance::Dark)),
            Some(WindowAppearance::Dark)
        );
        assert_eq!(
            native_appearance(&with(Appearance::Light)),
            Some(WindowAppearance::Light)
        );
    }

    #[gpui_kit::test]
    fn refresh_holds_the_native_appearance(cx: &mut gpui_kit::TestAppContext) {
        crate::test_support::config_dir("theme-native");
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::settings::init(cx);
            Settings::update(cx, |s| s.appearance = Appearance::Dark);
            refresh(cx);
            assert_eq!(
                cx.global::<NativeAppearance>().0,
                Some(WindowAppearance::Dark)
            );
            Settings::update(cx, |s| s.appearance = Appearance::System);
            refresh(cx);
            assert_eq!(cx.global::<NativeAppearance>().0, None);
        });
    }

    /// Flipping dark and light live (Auto, macOS switching) re-colors what keeps its own copy of
    /// the theme: gpui-component's controls, and the highlight theme code views and markdown
    /// code blocks color with.
    #[gpui_kit::test]
    fn a_live_switch_reaches_every_copy_of_the_theme(cx: &mut gpui_kit::TestAppContext) {
        crate::test_support::config_dir("theme-live");
        // SAFETY: nextest runs each test in a process of its own.
        unsafe { std::env::set_var("GITVIBER_APPEARANCE", "dark") };
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::settings::init(cx);
            init(cx);
        });
        let read = |cx: &mut gpui_kit::TestAppContext| {
            cx.update(|cx| {
                let component = Theme::global(cx);
                (
                    cx.app_theme().clone(),
                    component.mode,
                    component.background,
                    component.highlight_theme.clone(),
                    syntax::current(cx),
                )
            })
        };
        let (dark, mode, background, highlight, current) = read(cx);
        assert_eq!(dark.id, ThemeId::Dark);
        assert_eq!(mode, ThemeMode::Dark);
        assert_eq!(to_hex(background), to_hex(dark.background));
        assert_eq!(*highlight, *current);
        unsafe { std::env::set_var("GITVIBER_APPEARANCE", "light") };
        cx.update(refresh);
        cx.run_until_parked();
        let (light, mode, background, highlight, current) = read(cx);
        assert_eq!(light.id, ThemeId::Light);
        assert_eq!(mode, ThemeMode::Light);
        assert_eq!(to_hex(background), to_hex(light.background));
        assert_eq!(*highlight, *current);
        assert_eq!(
            *current,
            syntax::on_palette(&syntax::theme("github-light-default"), &light)
        );
    }

    /// `color` over `under`, as the screen shows it.
    fn over(color: Hsla, under: [f32; 3]) -> [f32; 3] {
        let c = Rgba::from(color);
        let mix = |top: f32, below: f32| top * c.a + below * (1.0 - c.a);
        [mix(c.r, under[0]), mix(c.g, under[1]), mix(c.b, under[2])]
    }

    fn rgb(color: Hsla) -> [f32; 3] {
        let c = Rgba::from(color);
        [c.r, c.g, c.b]
    }

    #[test]
    fn small_text_reads_in_every_palette() {
        for id in ThemeId::ALL {
            let s = Settings {
                dark_theme: *id,
                light_theme: *id,
                ..Settings::default()
            };
            let t = build(&s, os(id.is_dark()), fonts());
            assert_eq!(t.id, *id);
            let name = id.as_str();
            let surfaces = [t.sidebar, t.panel, t.background, t.elevated];
            // Every row a list can show: plain, hovered, selected.
            let rows: Vec<(String, [f32; 3])> = ["sidebar", "panel", "background", "elevated"]
                .iter()
                .zip(surfaces)
                .flat_map(|(surface, color)| {
                    let base = rgb(color);
                    [
                        (surface.to_string(), base),
                        (format!("{surface}, hovered"), over(t.hover, base)),
                        (format!("{surface}, selected"), over(t.active, base)),
                    ]
                })
                .collect();
            let worst = |text: Hsla| {
                rows.iter()
                    .map(|(row, bg)| (contrast(over(text, *bg), *bg), row.clone()))
                    .min_by(|a, b| a.0.total_cmp(&b.0))
                    .unwrap()
            };
            let (subtle, row) = worst(t.subtle);
            assert!(subtle >= 4.5, "{name} subtle {subtle:.2}:1 on {row}");
            let (muted, row) = worst(t.muted_foreground);
            assert!(muted >= 4.5, "{name} muted {muted:.2}:1 on {row}");
            assert!(
                muted > subtle,
                "{name}: muted text reads weaker than subtle"
            );
            for (status, color) in [
                ("added", t.added),
                ("removed", t.removed),
                ("modified", t.modified),
                ("renamed", t.renamed),
                ("conflict", t.conflict),
                ("info", t.info),
            ] {
                for s in surfaces {
                    let c = contrast(over(color, rgb(s)), rgb(s));
                    assert!(c >= 4.5, "{name} {status} text {c:.2}:1");
                }
            }
            for (status, fill) in [
                ("added", t.added_fill),
                ("removed", t.removed_fill),
                ("modified", t.modified_fill),
                ("renamed", t.renamed_fill),
                ("conflict", t.conflict_fill),
            ] {
                let c = contrast(over(t.on_status, rgb(fill)), rgb(fill));
                assert!(c >= 4.5, "{name} {status} pill {c:.2}:1");
            }
        }
    }
}
