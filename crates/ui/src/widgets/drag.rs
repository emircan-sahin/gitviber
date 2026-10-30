//! Dragging to reorder: what follows the pointer while a tab, a terminal or a project is
//! dragged, and the line on the edge of the tab it would land beside.

use gpui_kit::{
    AnyElement, App, BoxShadow, Div, Hsla, ParentElement as _, SharedString, StyleRefinement,
    Styled as _, div, point, prelude::FluentBuilder as _, px,
};

use crate::theme::ActiveAppTheme;

/// `label` on a raised chip, after `leading` (an icon or a project tile) when given.
pub fn drag_chip(leading: Option<AnyElement>, label: impl Into<SharedString>, cx: &App) -> Div {
    let t = cx.app_theme();
    div()
        .flex()
        .items_center()
        .gap_1p5()
        .px_3()
        .py_1()
        .rounded(px(4.))
        .bg(t.elevated)
        .border_1()
        .border_color(t.border_strong)
        .shadow_md()
        .text_color(t.foreground)
        .text_size(t.ui_text())
        .when_some(leading, |d, l| d.child(l))
        .child(label.into())
}

/// Where a tab dropped on the one at `to` lands: on its left from the right, on its right from
/// the left; that edge takes the drop line. None over itself.
pub fn drop_edge(from: usize, to: usize) -> Option<bool> {
    (from != to).then_some(from < to)
}

/// The drop line on a tab's left (`right` false) or right edge: an inset shadow, as the tab's own
/// border already has its color.
pub fn drop_line(style: StyleRefinement, right: bool, color: Hsla) -> StyleRefinement {
    let x = if right { px(-2.) } else { px(2.) };
    style.shadow(vec![BoxShadow {
        color,
        offset: point(x, px(0.)),
        blur_radius: px(0.),
        spread_radius: px(0.),
        inset: true,
    }])
}
