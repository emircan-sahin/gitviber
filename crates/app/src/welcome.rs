//! What the center shows with nothing in it: with no project, a way to open one; with no tab,
//! the keys to get going.

use gitviber_ui::actions::file;
use gitviber_ui::assets::LOGO;
use gitviber_ui::widgets::Keycaps;
use gitviber_ui::{ActiveAppTheme, UiState};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::{
    AnyElement, App, InteractiveElement as _, IntoElement, ParentElement as _, Styled as _, Window,
    div, img, prelude::FluentBuilder as _, px, rems,
};

fn hint_row(label: &'static str, id: &'static str, cx: &App) -> AnyElement {
    let t = cx.app_theme();
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap_6()
        .w(px(260.))
        .child(div().text_color(t.muted_foreground).child(label))
        .child(Keycaps::command(id, cx))
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

/// The window with no project open: open or clone one, or pick a saved one.
pub fn no_project(window: &mut Window, cx: &mut App) -> AnyElement {
    let t = cx.app_theme().clone();
    let recent = (!UiState::get(cx).recent_projects.is_empty())
        .then(|| crate::projects::list("welcome-projects", None, None, false, None, window, cx));
    div()
        .debug_selector(|| "welcome".into())
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .bg(t.code(t.background))
        .child(
            div()
                .w(px(400.))
                .flex()
                .flex_col()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(img(LOGO).size(px(44.)))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .text_size(rems(1.25))
                                        .font_weight(t.semibold())
                                        .child("GitViber"),
                                )
                                .child(
                                    div()
                                        .text_size(rems(0.75))
                                        .text_color(t.muted_foreground)
                                        .child("Review what your agent wrote. Plain git, no workspace."),
                                ),
                        ),
                )
                .child(
                    Button::new("welcome-open")
                        .primary()
                        .mt_6()
                        .w_full()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .w_full()
                                .child(Icon::new(IconName::FolderOpen).small())
                                .child("Open repository")
                                .child(
                                    div()
                                        .ml_auto()
                                        .child(Keycaps::command("file.openRepo", cx).compact().lit(true)),
                                ),
                        )
                        .on_click(|_, window, cx| window.dispatch_action(Box::new(file::OpenRepo), cx)),
                )
                .child(
                    Button::new("welcome-clone")
                        .mt_2()
                        .w_full()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .w_full()
                                .child(Icon::new(IconName::FolderDown).small())
                                .child("Clone repository"),
                        )
                        .on_click(|_, window, cx| window.dispatch_action(Box::new(file::CloneRepo), cx)),
                )
                .when_some(recent, |d, recent| {
                    d.child(
                        div()
                            .mt_6()
                            .pt_3()
                            .border_t_1()
                            .border_color(t.border)
                            .child(
                                div()
                                    .mb_1p5()
                                    .text_size(rems(0.65625))
                                    .font_weight(t.semibold())
                                    .text_color(t.subtle)
                                    .child("RECENT"),
                            )
                            .child(recent),
                    )
                }),
        )
        .into_any_element()
}
