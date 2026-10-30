//! A project's tile: its folder's first letter on the project's color (projects.rs).

use gpui_kit::{
    App, Context, Div, FontWeight, IntoElement, ParentElement as _, Render, Styled as _, Window,
    div, px, rems, rgb,
};

use crate::UiState;
use crate::projects::{Swatch, project_name};
use crate::theme::ActiveAppTheme;

/// `size` in pixels: 16 in a compact row, 18 in the top bar, 24 in a roomy list.
pub fn project_tile(path: &str, size: f32, cx: &App) -> Div {
    let swatch = cx
        .try_global::<UiState>()
        .map(|s| s.project_org.swatch_of(path))
        .unwrap_or_else(|| crate::projects::ProjectOrg::default().swatch_of(path));
    swatch_tile(swatch, project_name(path), size, cx)
}

/// `name`'s letter on `swatch`; also the color menu's sample.
pub fn swatch_tile(swatch: &Swatch, name: &str, size: f32, cx: &App) -> Div {
    let dark = cx.app_theme().dark;
    let letter: String = name.chars().next().unwrap_or('?').to_uppercase().collect();
    div()
        .size(px(size))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .rounded(px(4.))
        .bg(rgb(swatch.fill(dark)))
        .text_color(rgb(swatch.letter(dark)))
        .text_size(rems(if size >= 18. { 0.6875 } else { 0.59375 }))
        .font_weight(FontWeight::BOLD)
        .child(letter)
}

/// A project on the move, from the top bar picker or the projects list: either takes it.
#[derive(Clone)]
pub struct DraggedProject {
    pub path: String,
}

impl Render for DraggedProject {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tile = project_tile(&self.path, 16., cx).into_any_element();
        super::drag_chip(Some(tile), project_name(&self.path).to_string(), cx)
    }
}
