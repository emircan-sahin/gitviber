//! Buttons that stay on or off. gpui-component's `toggled` only reaches assistive tech and its
//! `selected` paints a ghost button barely apart from its hover, so every on/off button goes
//! through `toggle`: one accent tint for "on", in every theme.

use gpui_kit::component::button::Button;
use gpui_kit::component::{ActiveTheme as _, Selectable as _};
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

/// Buttons that pick one of a few modes (Unified | Split), framed as one control. Build each
/// segment with `toggle`; the ones off are dimmed so the chosen one reads first.
pub fn segmented(segments: impl IntoIterator<Item = Button>, cx: &App) -> Div {
    let theme = cx.theme();
    let dim = theme.muted_foreground;
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(1.))
        .p(px(1.))
        .rounded(px(5.))
        .border_1()
        .border_color(theme.input)
        .children(segments.into_iter().map(|b| {
            let on = b.is_selected();
            b.rounded(px(3.)).when(!on, |b| b.text_color(dim))
        }))
}
