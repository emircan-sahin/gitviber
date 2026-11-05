//! What a panel shows while there is nothing of the open repo to draw yet: grey rows where its
//! list will be, the same in every panel, so a switch never shows the last repo's.

use gpui_kit::{
    App, InteractiveElement as _, IntoElement, ParentElement as _, RenderOnce, Styled as _, Window,
    div, px, relative,
};

use crate::ActiveAppTheme as _;

/// The rows' widths, varied so it reads as a list rather than a striped block.
const WIDTHS: [f32; 6] = [0.72, 0.48, 0.6, 0.38, 0.66, 0.5];

/// Static: a pulse would draw the panel holding it every frame for as long as it waits.
#[derive(IntoElement, Default)]
pub struct PanelSkeleton;

impl PanelSkeleton {
    pub fn new() -> Self {
        Self
    }
}

impl RenderOnce for PanelSkeleton {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let grey = cx.app_theme().elevated;
        div()
            .id("panel-skeleton")
            .debug_selector(|| "panel-skeleton".into())
            .flex()
            .flex_col()
            .w_full()
            .py(px(6.))
            .children(WIDTHS.iter().map(|&w| {
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .h(px(26.))
                    .px(px(10.))
                    .child(div().flex_none().size(px(12.)).rounded(px(3.)).bg(grey))
                    .child(div().h(px(8.)).w(relative(w)).rounded(px(4.)).bg(grey))
            }))
    }
}
