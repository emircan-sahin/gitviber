//! What a panel shows while there is nothing of the open repo to draw yet: grey rows where its
//! list will be, the same in every panel, so a switch never shows the last repo's.

use gpui_kit::component::skeleton::Skeleton;
use gpui_kit::{
    App, InteractiveElement as _, IntoElement, ParentElement as _, RenderOnce, Styled as _, Window,
    div, px, relative,
};

/// The rows' widths, varied so it reads as a list rather than a striped block.
const WIDTHS: [f32; 6] = [0.72, 0.48, 0.6, 0.38, 0.66, 0.5];

#[derive(IntoElement)]
pub struct PanelSkeleton {
    rows: usize,
}

impl PanelSkeleton {
    pub fn new() -> Self {
        Self { rows: WIDTHS.len() }
    }

    pub fn rows(mut self, rows: usize) -> Self {
        self.rows = rows;
        self
    }
}

impl Default for PanelSkeleton {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderOnce for PanelSkeleton {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        div()
            .id("panel-skeleton")
            .debug_selector(|| "panel-skeleton".into())
            .flex()
            .flex_col()
            .w_full()
            .py(px(6.))
            .children((0..self.rows).map(|i| {
                div()
                    .id(("skeleton-row", i))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .h(px(26.))
                    .px(px(10.))
                    .child(Skeleton::new().flex_none().size(px(12.)).rounded(px(3.)))
                    .child(
                        Skeleton::new()
                            .h(px(8.))
                            .w(relative(WIDTHS[i % WIDTHS.len()]))
                            .rounded(px(4.)),
                    )
            }))
    }
}
