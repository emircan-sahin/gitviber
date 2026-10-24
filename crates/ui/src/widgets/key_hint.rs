//! A chord as keycaps, following the user's rebinding when it names a command.

use gpui_kit::{
    App, IntoElement, ParentElement, RenderOnce, SharedString, Styled, Window, div, px, rems,
};

use crate::keymap::{self, IS_MAC, chord_keys};
use crate::settings::Settings;
use crate::theme::ActiveAppTheme;

#[derive(IntoElement)]
pub struct KeyHint {
    chord: Option<String>,
}

impl KeyHint {
    /// A chord as settings store it ("shift+cmd+p").
    pub fn chord(chord: impl Into<String>) -> Self {
        Self {
            chord: Some(chord.into()),
        }
    }

    /// A command's first chord now; nothing when it has none.
    pub fn command(id: &str, cx: &App) -> Self {
        let chord = keymap::command(id).and_then(|c| {
            keymap::bindings_for(c, &Settings::get(cx).keybindings, IS_MAC)
                .into_iter()
                .next()
        });
        Self { chord }
    }
}

impl RenderOnce for KeyHint {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = cx.app_theme();
        let keys = self
            .chord
            .map(|c| chord_keys(&c, IS_MAC))
            .unwrap_or_default();
        div()
            .flex()
            .flex_none()
            .gap(px(2.))
            .children(keys.into_iter().map(|k| {
                div()
                    .min_w(px(16.))
                    .px(px(3.))
                    .rounded(px(3.))
                    .border_1()
                    .border_color(t.border_strong)
                    .bg(t.elevated)
                    .text_color(t.muted_foreground)
                    .text_size(rems(0.656))
                    .line_height(rems(0.875))
                    .text_center()
                    .child(SharedString::from(k))
            }))
    }
}
