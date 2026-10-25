use gpui::{
    AnyElement, App, IntoElement, ParentElement, RenderOnce, StyleRefinement, Styled, Window,
    relative, rems,
};
use gpui_base::DialogTitle as BaseDialogTitle;

use crate::StyledExt as _;

/// Title element for a dialog header.
#[derive(IntoElement)]
pub struct DialogTitle {
    base: BaseDialogTitle,
    style: StyleRefinement,
    children: Vec<AnyElement>,
}

impl DialogTitle {
    pub fn new() -> Self {
        Self {
            base: BaseDialogTitle::new(),
            style: StyleRefinement::default(),
            children: vec![],
        }
    }
}

impl ParentElement for DialogTitle {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl Styled for DialogTitle {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for DialogTitle {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        // GitViber: 13.5px, a step over the 12px body (upstream: 16px).
        self.base
            .text_size(rems(0.84375))
            .font_semibold()
            .line_height(relative(1.25))
            .refine_style(&self.style)
            .children(self.children)
    }
}
