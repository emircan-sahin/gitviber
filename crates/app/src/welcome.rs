//! What the center shows with nothing in it: with no project, a way to open one; with no tab,
//! the keys to get going.

use gitviber_ui::actions::{file, workbench};
use gitviber_ui::assets::LOGO;
use gitviber_ui::widgets::KeyHint;
use gitviber_ui::{ActiveAppTheme, UiState};
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::{
    AnyElement, App, InteractiveElement as _, IntoElement, ParentElement as _, SharedString,
    StatefulInteractiveElement as _, Styled as _, div, img, px, rems,
};

use crate::menus::OpenRecent;

const RECENT_SHOWN: usize = 8;

fn hint_row(label: &'static str, id: &'static str, cx: &App) -> AnyElement {
    let t = cx.app_theme();
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap_6()
        .w(px(260.))
        .child(div().text_color(t.muted_foreground).child(label))
        .child(KeyHint::command(id, cx))
        .into_any_element()
}

/// The center with no tab open.
pub fn empty_center(cx: &App) -> AnyElement {
    div()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_3()
        .child(img(LOGO).size(px(48.)))
        .child(hint_row("Command Palette", "workbench.showCommands", cx))
        .child(hint_row("Open File", "workbench.quickOpen", cx))
        .child(hint_row("Toggle Terminal", "terminal.toggle", cx))
        .child(hint_row("Settings", "workbench.openSettings", cx))
        .into_any_element()
}

/// The window with no project open.
pub fn no_project(cx: &App) -> AnyElement {
    let t = cx.app_theme().clone();
    let recent: Vec<String> = UiState::get(cx)
        .recent_projects
        .iter()
        .take(RECENT_SHOWN)
        .cloned()
        .collect();
    let rows = recent.into_iter().enumerate().map(|(i, path)| {
        let name = path.rsplit('/').next().unwrap_or(&path).to_string();
        let open = OpenRecent {
            path: SharedString::from(path.clone()),
        };
        div()
            .id(("recent", i))
            .flex()
            .items_baseline()
            .gap_2()
            .w_full()
            .px_2()
            .py_1()
            .rounded(px(4.))
            .cursor_pointer()
            .hover(|s| s.bg(t.hover))
            .child(div().flex_none().text_color(t.foreground).child(name))
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_color(t.subtle)
                    .text_size(rems(0.6875))
                    .child(path),
            )
            .on_click(move |_, window, cx| window.dispatch_action(Box::new(open.clone()), cx))
    });
    div()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .bg(t.code(t.background))
        .child(
            div()
                .w(px(420.))
                .flex()
                .flex_col()
                .items_center()
                .gap_3()
                .child(img(LOGO).size(px(64.)))
                .child(
                    div()
                        .text_size(rems(1.25))
                        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                        .child("GitViber"),
                )
                .child(
                    div()
                        .text_color(t.muted_foreground)
                        .child("Open a repository to see what your agents wrote."),
                )
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .mt_2()
                        .child(
                            Button::new("open-folder")
                                .primary()
                                .small()
                                .label("Open Folder…")
                                .on_click(|_, window, cx| {
                                    window.dispatch_action(Box::new(file::OpenRepo), cx)
                                }),
                        )
                        .child(
                            Button::new("palette")
                                .small()
                                .label("Command Palette")
                                .on_click(|_, window, cx| {
                                    window.dispatch_action(Box::new(workbench::ShowCommands), cx)
                                }),
                        ),
                )
                .child(
                    div()
                        .mt_4()
                        .w_full()
                        .flex()
                        .flex_col()
                        .gap_px()
                        .children(rows),
                ),
        )
        .into_any_element()
}
