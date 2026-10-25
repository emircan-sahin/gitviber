//! Syntax colors: the old app's 24 Shiki themes as gpui-component highlight themes (themes/*.json,
//! made by generate.mjs). The one Settings picks for the appearance in use is installed into
//! gpui-component's theme, so its code editor and markdown code blocks color with it; the diff
//! and anything else that colors code read `current`.
//!
//! The app's own surfaces stay the app's: the code background and the line numbers come from the
//! palette, as the old Monaco theme took them from the CSS variables.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use gpui_kit::component::Theme;
use gpui_kit::component::highlighter::HighlightTheme;
use gpui_kit::{App, Global};

use crate::theme::AppTheme;

macro_rules! themes {
    ($($id:literal),* $(,)?) => {
        /// Every theme's JSON, by its Settings id.
        const SOURCES: &[(&str, &str)] = &[$(($id, include_str!(concat!("themes/", $id, ".json")))),*];
    };
}

themes!(
    "github-dark-default",
    "dark-plus",
    "one-dark-pro",
    "vitesse-dark",
    "tokyo-night",
    "solarized-dark",
    "catppuccin-mocha",
    "vesper",
    "houston",
    "nord",
    "rose-pine",
    "kanagawa-wave",
    "everforest-dark",
    "poimandres",
    "github-light-default",
    "light-plus",
    "one-light",
    "vitesse-light",
    "catppuccin-latte",
    "min-light",
    "solarized-light",
    "rose-pine-dawn",
    "kanagawa-lotus",
    "everforest-light",
);

/// The ids there are themes for.
pub fn ids() -> impl Iterator<Item = &'static str> {
    SOURCES.iter().map(|(id, _)| *id)
}

/// The theme as shipped, parsed once; an unknown id gets dark-plus.
pub fn theme(id: &str) -> Arc<HighlightTheme> {
    static PARSED: OnceLock<Mutex<HashMap<&'static str, Arc<HighlightTheme>>>> = OnceLock::new();
    let (id, source) = SOURCES
        .iter()
        .find(|(i, _)| *i == id)
        .or_else(|| SOURCES.iter().find(|(i, _)| *i == "dark-plus"))
        .copied()
        .unwrap_or(SOURCES[0]);
    let mut parsed = PARSED
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    parsed
        .entry(id)
        .or_insert_with(|| {
            Arc::new(
                serde_json::from_str(source).unwrap_or_else(|e| panic!("syntax theme {id}: {e}")),
            )
        })
        .clone()
}

/// `theme` drawn on the app's palette: its background and line numbers.
pub fn on_palette(theme: &HighlightTheme, t: &AppTheme) -> HighlightTheme {
    let mut out = theme.clone();
    let style = &mut out.style;
    style.editor_background = Some(t.code(t.background));
    style.editor_gutter_background = Some(t.code(t.background));
    style.editor_line_number = Some(t.subtle);
    style.editor_active_line_number = Some(t.muted_foreground);
    style.editor_active_line = Some(t.foreground.opacity(0.04));
    out
}

/// The highlight theme in use, on the app's palette.
pub struct SyntaxTheme(pub Arc<HighlightTheme>);
impl Global for SyntaxTheme {}

/// The theme the code views color with now.
pub fn current(cx: &App) -> Arc<HighlightTheme> {
    match cx.try_global::<SyntaxTheme>() {
        Some(s) => s.0.clone(),
        None => theme("dark-plus"),
    }
}

fn install(cx: &mut App) {
    let t = cx.global::<AppTheme>();
    let next = on_palette(&theme(&t.syntax_theme), t);
    // A palette change re-applies gpui-component's config, which may put its own back.
    if cx.try_global::<SyntaxTheme>().is_some_and(|s| *s.0 == next)
        && *Theme::global(cx).highlight_theme == next
    {
        return;
    }
    let next = Arc::new(next);
    cx.set_global(SyntaxTheme(next.clone()));
    Theme::update(cx, |theme| theme.highlight_theme = next);
}

pub(crate) fn init(cx: &mut App) {
    install(cx);
    cx.observe_global::<AppTheme>(install).detach();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_theme_parses_with_its_colors() {
        for id in ids() {
            let t = theme(id);
            assert_eq!(t.name, id);
            let s = &t.style.syntax;
            for (name, style) in [
                ("comment", s.comment),
                ("keyword", s.keyword),
                ("string", s.string),
                ("comment.doc", s.comment_doc),
            ] {
                assert!(style.is_some(), "{id} has no {name}");
            }
            assert!(
                t.style
                    .syntax
                    .style("function")
                    .and_then(|s| s.color)
                    .is_some(),
                "{id}: function"
            );
        }
        assert_eq!(
            theme("github-light-default").appearance,
            gpui_kit::component::ThemeMode::Light
        );
        assert_eq!(theme("no-such-theme").name, "dark-plus");
    }

    #[test]
    fn settings_name_only_themes_there_are() {
        let known: Vec<&str> = ids().collect();
        for (id, _) in crate::settings::SYNTAX_THEMES
            .iter()
            .chain(crate::settings::LIGHT_SYNTAX_THEMES)
        {
            assert!(known.contains(id), "no theme for {id}");
        }
        assert_eq!(known.len(), 24);
    }
}
