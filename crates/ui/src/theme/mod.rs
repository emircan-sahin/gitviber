//! The app's colors and fonts, from Settings and the OS appearance. `cx.app_theme()` has our
//! tokens (sidebar, panel, elevated, the git and diff colors…); gpui-component's own theme is
//! driven from the same palette, so its buttons, inputs and menus match. Both change live when
//! the settings or the OS appearance do.
//!
//! Syntax colors come with the code view: it reads `syntax_theme` (a Shiki-style id from
//! Settings) and installs its highlight theme; this module leaves gpui-component's alone.

mod palettes;
pub mod syntax;

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

use gpui_kit::component::{Theme, ThemeConfig, ThemeConfigColors, ThemeMode};
use gpui_kit::{
    App, FontWeight, Global, Hsla, Pixels, Refineable as _, Rgba, SharedString, StyleRefinement,
    Styled as _, Subscription, Window, WindowAppearance, WindowBackgroundAppearance, px,
};
use serde::{Deserialize, Serialize};

use crate::keymap::IS_MAC;
use crate::settings::{CodeFont, Settings, TerminalFont, UiFont, WINDOW_OPACITY};

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
    pub ui_weight: FontWeight,
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
    /// ≥4.5:1 on every surface up to `elevated`: it's used for 11-12px text.
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

fn installed(name: &str, cx: &mut App) -> bool {
    if name.starts_with('.') {
        return true;
    }
    if let Some(hit) = cx.default_global::<InstalledFonts>().0.get(name) {
        return *hit;
    }
    crate::fonts::ensure(name, cx);
    let names = cx.text_system().all_font_names();
    let found = names.iter().any(|n| n == name);
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
    Fonts {
        ui,
        ui_weight: FontWeight(s.ui_font_weight as f32),
        code,
        code_weight: FontWeight(s.code_font_weight as f32),
        code_size: px(s.code_font_size),
        code_line_height: s.line_height,
        terminal,
        terminal_size: px(s.terminal_font_size),
    }
}

/// The OS is dark now. GITVIBER_APPEARANCE=dark|light stands in for it: an off-screen
/// snapshot's test platform always reports light.
fn system_dark(cx: &App) -> bool {
    match std::env::var("GITVIBER_APPEARANCE").as_deref() {
        Ok("dark") => return true,
        Ok("light") => return false,
        _ => {}
    }
    matches!(
        cx.window_appearance(),
        WindowAppearance::Dark | WindowAppearance::VibrantDark
    )
}

/// The theme the settings and OS appearance make, with `fonts` already resolved.
pub fn build(settings: &Settings, system_dark: bool, fonts: Fonts) -> AppTheme {
    let id = settings.theme(system_dark);
    let p = id.palette();
    let opacity = if IS_MAC && settings.window_opacity < 100.0 {
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
        syntax_theme: settings.code_theme(system_dark).to_string().into(),
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
    let fonts = resolve_fonts(&settings, cx);
    let next = build(&settings, system_dark(cx), fonts);
    if cx.try_global::<AppTheme>() == Some(&next) {
        return;
    }
    let config = Rc::new(component_config(&next));
    cx.set_global(next);
    Theme::update(cx, |theme| theme.apply_config(&config));
}

/// Keeps a window in step with the theme: the OS appearance it reports, its see-through
/// background, and a clear root so our surfaces paint the window. Call once per window and keep
/// the subscriptions as long as the window's view.
pub fn attach(window: &mut Window, cx: &mut App) -> Vec<Subscription> {
    // Deferred: while the window's view is being built its root isn't there yet.
    window.defer(cx, |window, cx| {
        gpui_kit::base::Root::update(window, cx, |root, _, _| {
            root.style()
                .refine(&StyleRefinement::default().bg(gpui_kit::transparent_black()));
        });
    });
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
        }),
    ]
}

pub(crate) fn init(cx: &mut App) {
    refresh(cx);
    syntax::init(cx);
    cx.observe_global::<Settings>(refresh).detach();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fonts() -> Fonts {
        Fonts {
            ui: ".SystemUIFont".into(),
            ui_weight: FontWeight::MEDIUM,
            code: "Menlo".into(),
            code_weight: FontWeight::MEDIUM,
            code_size: px(13.5),
            code_line_height: 1.6,
            terminal: "Menlo".into(),
            terminal_size: px(13.),
        }
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
        assert_eq!(build(&s, true, fonts()).id, ThemeId::Nord);
        assert_eq!(build(&s, false, fonts()).id, ThemeId::SolarizedLight);
        let dark = Settings {
            appearance: crate::settings::Appearance::Dark,
            ..s.clone()
        };
        assert_eq!(build(&dark, false, fonts()).id, ThemeId::Nord);
        assert_eq!(build(&dark, false, fonts()).syntax_theme, "dark-plus");
    }

    #[test]
    fn tokens_map_onto_gpui_component() {
        let t = build(&Settings::default(), true, fonts());
        assert_eq!(t.background, hex("#171718"));
        let config = component_config(&t);
        assert_eq!(config.mode, ThemeMode::Dark);
        assert_eq!(config.colors.background.as_deref(), Some("#171718ff"));
        assert_eq!(config.colors.sidebar.as_deref(), Some("#0e0e0fff"));
        assert_eq!(config.colors.accent.as_deref(), Some("#ffffff12"));
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
        let t = build(&s, true, fonts());
        if IS_MAC {
            let g = t.glass.unwrap();
            assert!(g.blur);
            assert!(g.chrome > g.code);
            assert_eq!(t.window_background(), WindowBackgroundAppearance::Blurred);
        } else {
            assert!(t.glass.is_none());
        }
        assert!(build(&Settings::default(), true, fonts()).glass.is_none());
    }
}
