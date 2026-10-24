//! The diff crate knows nothing of the app's theme or settings: its look follows the theme
//! and the syntax theme, and what it shows follows Settings both ways (a diff's own toggles
//! are saved).

use gitviber_diff::{DiffColors, DiffOptions, DiffStyle, ImageMode, Layout, Whitespace};
use gitviber_ui::settings::{ImageCompare, Whitespace as SettingsWhitespace};
use gitviber_ui::theme::syntax::{self, SyntaxTheme};
use gitviber_ui::{ActiveAppTheme as _, AppTheme, Settings};
use gpui_kit::{App, px};

/// Keeps the diffs' look and options in step from now on. Once, from `init`.
pub fn install(cx: &mut App) {
    gitviber_diff::set_core(crate::core(cx), cx);
    apply_style(cx);
    apply_settings(cx);
    cx.observe_global::<AppTheme>(apply_style).detach();
    cx.observe_global::<SyntaxTheme>(apply_style).detach();
    cx.observe_global::<Settings>(apply_settings).detach();
    cx.observe_global::<DiffOptions>(save_options).detach();
}

fn apply_style(cx: &mut App) {
    if !cx.has_global::<AppTheme>() {
        return;
    }
    let t = cx.app_theme();
    let colors = DiffColors {
        background: t.code(t.background),
        panel: t.step(t.panel),
        elevated: t.elevated,
        hover: t.hover,
        border: t.border,
        foreground: t.foreground,
        muted: t.muted_foreground,
        subtle: t.subtle,
        primary: t.primary,
        primary_foreground: t.primary_foreground,
        added: t.added,
        removed: t.removed,
        add_bg: t.add_bg,
        add_gutter: t.add_gutter,
        add_emph: t.add_emph,
        del_bg: t.del_bg,
        del_gutter: t.del_gutter,
        del_emph: t.del_emph,
        selection: t.primary.opacity(0.22),
        destructive: t.destructive,
    };
    let style = DiffStyle {
        colors,
        syntax: syntax::current(cx),
        font_family: t.fonts.code.clone(),
        font_size: t.fonts.code_size,
        line_height: t.fonts.code_line_height,
        ui_font_size: px(12. * t.ui_scale),
    };
    let same = cx.try_global::<DiffStyle>().is_some_and(|s| {
        s.colors == style.colors
            && std::sync::Arc::ptr_eq(&s.syntax, &style.syntax)
            && s.font_family == style.font_family
            && s.font_size == style.font_size
            && s.line_height == style.line_height
    });
    if !same {
        DiffStyle::set(style, cx);
    }
}

/// What the settings say the diffs show.
fn options_of(s: &Settings) -> DiffOptions {
    DiffOptions {
        layout: if s.side_by_side {
            Layout::Split
        } else {
            Layout::Unified
        },
        collapse: s.hide_unchanged,
        whitespace: match s.diff_whitespace() {
            None => Whitespace::Exact,
            Some(SettingsWhitespace::All) => Whitespace::All,
            Some(SettingsWhitespace::Amount) => Whitespace::Amount,
        },
        image: match s.image_compare {
            ImageCompare::Side => ImageMode::Side,
            ImageCompare::Swipe => ImageMode::Swipe,
            ImageCompare::Onion => ImageMode::Onion,
        },
    }
}

fn apply_settings(cx: &mut App) {
    let next = options_of(Settings::get(cx));
    if DiffOptions::get(cx) != next {
        DiffOptions::set(next, cx);
    }
}

/// A diff's header toggled something: the setting follows, so it holds everywhere and after a restart.
fn save_options(cx: &mut App) {
    let o = DiffOptions::get(cx);
    if options_of(Settings::get(cx)) == o {
        return;
    }
    Settings::update(cx, |s| {
        s.side_by_side = o.layout == Layout::Split;
        s.hide_unchanged = o.collapse;
        match o.whitespace {
            Whitespace::Exact => s.ignore_whitespace = false,
            Whitespace::Amount => {
                s.ignore_whitespace = true;
                s.whitespace_mode = SettingsWhitespace::Amount;
            }
            Whitespace::All => {
                s.ignore_whitespace = true;
                s.whitespace_mode = SettingsWhitespace::All;
            }
        }
        s.image_compare = match o.image {
            ImageMode::Side => ImageCompare::Side,
            ImageMode::Swipe => ImageCompare::Swipe,
            ImageMode::Onion => ImageCompare::Onion,
        };
    });
}
