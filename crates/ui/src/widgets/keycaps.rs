//! Keycaps: the one look of a shortcut, wherever one shows (settings, the overlay, the palette,
//! menus, tooltips, pickers' hints). Menus and tooltips get it through gpui-kit's `Kbd`, which
//! `init` points here.

use gpui_kit::component::kbd::{self, Kbd};
use gpui_kit::{
    AnyElement, App, IntoElement, Keystroke, ParentElement, RenderOnce, SharedString, Styled,
    Window, div, prelude::FluentBuilder as _, px, rems,
};

use crate::keymap::chord::to_keystroke;
use crate::keymap::edit::chord_of;
use crate::keymap::{self, IS_MAC, chord_keys};
use crate::settings::Settings;
use crate::theme::ActiveAppTheme;

#[derive(IntoElement, Clone, Default)]
pub struct Keycaps {
    keys: Vec<SharedString>,
    compact: bool,
    lit: bool,
}

impl Keycaps {
    /// A chord as settings store it ("shift+cmd+p"): ⇧ ⌘ P.
    pub fn chord(chord: &str) -> Self {
        Self::of(chord, IS_MAC)
    }

    fn of(chord: &str, mac: bool) -> Self {
        Self::keys(chord_keys(chord, mac))
    }

    /// A command's first chord now, following the user's rebinding; no caps when it has none.
    pub fn command(id: &str, cx: &App) -> Self {
        command_chord(id, cx)
            .map(|c| Self::chord(&c))
            .unwrap_or_default()
    }

    /// Caps with these labels as they are ("↑", "↓").
    pub fn keys<S: Into<SharedString>>(keys: impl IntoIterator<Item = S>) -> Self {
        Self {
            keys: keys.into_iter().map(Into::into).collect(),
            ..Default::default()
        }
    }

    /// Smaller caps, for dense rows (palette, menus, tooltips, the overlay).
    pub fn compact(mut self) -> Self {
        self.compact = true;
        self
    }

    /// On a row lit in the primary color: the caps take its ink.
    pub fn lit(mut self, lit: bool) -> Self {
        self.lit = lit;
        self
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }
}

impl RenderOnce for Keycaps {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = cx.app_theme();
        let (min_w, pad, size, height) = if self.compact {
            (px(16.), px(3.), rems(0.625), rems(0.875))
        } else {
            (px(18.), px(4.), rems(0.6875), rems(1.))
        };
        let (border, bg, ink) = if self.lit {
            let ink = t.primary_foreground;
            (ink.opacity(0.45), ink.opacity(0.12), ink)
        } else {
            (t.border_strong, t.elevated, t.foreground)
        };
        div()
            .flex()
            .flex_none()
            .gap(px(2.))
            .children(self.keys.into_iter().map(|k| {
                div()
                    .min_w(min_w)
                    .px(pad)
                    .rounded(px(3.))
                    .border_1()
                    .border_color(border)
                    .bg(bg)
                    .text_color(ink)
                    .text_size(size)
                    .line_height(height)
                    .text_center()
                    .child(k)
            }))
    }
}

fn command_chord(id: &str, cx: &App) -> Option<String> {
    let command = keymap::command(id)?;
    keymap::bindings_for(command, &Settings::get(cx).keybindings, IS_MAC)
        .into_iter()
        .next()
}

/// A chord as gpui-kit's `Kbd`, for a tooltip's keys (`Tooltip::key_binding`,
/// `Button::tooltip_keys`): drawn as these keycaps.
pub fn chord_kbd(chord: &str) -> Option<Kbd> {
    Keystroke::parse(&to_keystroke(chord, IS_MAC))
        .ok()
        .map(Kbd::new)
}

/// A command's first chord as a `Kbd`; None when it has none.
pub fn command_kbd(id: &str, cx: &App) -> Option<Kbd> {
    chord_kbd(&command_chord(id, cx)?)
}

/// A find box's option key for `letter` (⌥⌘C on macOS, Alt+C elsewhere) as a `Kbd`.
pub fn find_option_kbd(letter: char) -> Option<Kbd> {
    let m = if IS_MAC { "alt+cmd+" } else { "alt+" };
    chord_kbd(&format!("{m}{letter}"))
}

/// Hints in a row, each its keys then what they do ("↵ open"), or text alone ("drag to
/// arrange"). A narrow row moves whole hints to the next line, never cuts one.
pub fn key_hints(hints: impl IntoIterator<Item = (Keycaps, SharedString)>) -> gpui_kit::Div {
    div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap_x_3()
        .gap_y_1()
        .children(hints.into_iter().map(|(keys, label)| {
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(px(4.))
                .whitespace_nowrap()
                .when(!keys.is_empty(), |d| d.child(keys.compact()))
                .child(label)
        }))
}

/// Menus and tooltips draw their shortcuts as these keycaps.
pub(crate) fn init(cx: &mut App) {
    kbd::set_renderer(cx, render_kbd);
}

fn render_kbd(k: &Keystroke, lit: bool, _: &App) -> AnyElement {
    Keycaps::chord(&chord_of(k, IS_MAC).unwrap_or_default())
        .compact()
        .lit(lit)
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_chord_is_one_cap_per_key() {
        let keys = |chord, mac| Keycaps::of(chord, mac).keys;
        assert_eq!(keys("shift+cmd+p", true), ["⇧", "⌘", "P"]);
        assert_eq!(keys("alt+cmd+-", true), ["⌥", "⌘", "−"]);
        assert_eq!(keys("enter", true), ["↵"]);
        assert_eq!(keys("f2", true), ["F2"]);
        assert_eq!(keys("shift+cmd+p", false), ["Ctrl", "Shift", "P"]);
    }

    #[test]
    fn a_tooltips_kbd_reads_back_as_its_chord() {
        // chord_kbd's keystroke, as the Kbd renderer gets it back.
        for (chord, mac) in [
            ("shift+cmd+]", true),
            ("alt+cmd+c", true),
            ("shift+cmd+p", false),
        ] {
            let ks = Keystroke::parse(&to_keystroke(chord, mac)).unwrap();
            assert_eq!(chord_of(&ks, mac).as_deref(), Some(chord));
        }
    }
}
