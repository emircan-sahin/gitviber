//! Buttons that stay on or off. gpui-component's `toggled` only reaches assistive tech and its
//! `selected` paints a ghost button barely apart from its hover, so every on/off button goes
//! through `toggle`: one accent tint for "on", in every theme.

use gpui_kit::component::button::Button;
use gpui_kit::component::{ActiveTheme as _, Selectable as _, Sizable as _};
use gpui_kit::{App, Div, ParentElement as _, Styled as _, div, prelude::FluentBuilder as _, px};

pub trait Toggle {
    /// Announced as a toggle and drawn in the accent tint while `on`; off, it keeps its look.
    fn toggle(self, on: bool, cx: &App) -> Self;
}

impl Toggle for Button {
    fn toggle(self, on: bool, cx: &App) -> Self {
        let b = self.toggled(on).selected(on);
        if !on {
            return b;
        }
        // gpui-component's theme (our palette in the app): the diff demo and tests have no AppTheme.
        let accent = cx.theme().primary;
        b.bg(accent.opacity(0.16))
            .text_color(accent)
            .border_color(accent.opacity(0.5))
    }
}

/// Buttons that pick one of a few modes (Unified | Split), framed as one control at 12px in a
/// 28px row, as Settings' are. Build each segment with `toggle`; the ones off are muted so the
/// chosen one reads first.
pub fn segmented(segments: impl IntoIterator<Item = Button>, cx: &App) -> Div {
    let theme = cx.theme();
    let (dim, line) = (theme.muted_foreground, theme.input);
    div()
        .flex()
        .flex_none()
        .items_center()
        .h(px(28.))
        .rounded(px(4.))
        .overflow_hidden()
        .border_1()
        .border_color(line)
        .children(segments.into_iter().enumerate().map(|(i, b)| {
            let on = b.is_selected();
            b.small()
                .h_full()
                .rounded(px(0.))
                .when(!on, |b| b.text_color(dim))
                // After `toggle`'s accent border: the dividers stay the frame's.
                .when(i > 0, |b| b.border_l_1().border_color(line))
        }))
}

#[cfg(test)]
mod tests {
    use gpui_kit::assets::IconName;

    use super::*;

    /// Most of the top bar's and the panels' buttons are an icon with a tooltip: VoiceOver had
    /// only "button" to say for them.
    #[test]
    fn a_button_is_named_for_screen_readers() {
        let name = |b: Button| b.accessibility_name();
        let icon = || Button::new("b").icon(IconName::PanelBottom);
        assert_eq!(
            name(icon().tooltip("Toggle Terminal")).as_deref(),
            Some("Toggle Terminal")
        );
        assert_eq!(name(icon()), None);
        let labeled = Button::new("b").label("Pull").tooltip("Pull from origin");
        assert_eq!(name(labeled).as_deref(), Some("Pull"));
        let explicit = icon()
            .tooltip("Terminal")
            .accessibility_label("Show Terminal");
        assert_eq!(name(explicit).as_deref(), Some("Show Terminal"));
        // Children speak for themselves.
        let content = Button::new("b")
            .child("VS Code Dark+")
            .tooltip("Syntax theme");
        assert_eq!(name(content), None);
    }
}
