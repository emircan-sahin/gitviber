//! A project outside any git repository: its files, terminals and agents work as in any other,
//! and git's places (the git panel, the branch chip, git commands) offer `git init` instead.
//! Once it ran, the project opens again as the repository it is now, in place.

use gitviber_ui::{ActiveAppTheme, Project};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::{
    AnyElement, App, Entity, IntoElement, ParentElement as _, Styled as _, Window, div, px, rems,
};

use crate::toast::{Action, Note};

/// Runs `git init` in the project's folder; git's refusal shows in `window`.
pub fn initialize(project: &Entity<Project>, window: &mut Window, cx: &mut App) {
    let made = project.update(cx, |p, cx| p.init_repository(cx));
    let handle = window.window_handle();
    cx.spawn(async move |cx| {
        if let Err(err) = made.await {
            cx.update(|cx| {
                Note::error("Couldn't initialize the repository")
                    .detail(err)
                    .show_in(handle, cx)
            });
        }
    })
    .detach();
}

fn init_button(id: &'static str, project: &Entity<Project>) -> Button {
    let project = project.clone();
    Button::new(id)
        .label("Initialize Repository")
        .on_click(move |_, window, cx| initialize(&project, window, cx))
}

/// What a git view shows for a plain folder: why it's empty, and the way to fill it.
pub fn empty_state(project: &Entity<Project>, cx: &App) -> AnyElement {
    let t = cx.app_theme();
    div()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_3()
        .p_4()
        .text_center()
        .child(Icon::new(IconName::GitBranch).large().text_color(t.subtle))
        .child(
            div()
                .font_weight(t.semibold())
                .text_size(rems(0.8125))
                .child("Not a git repository"),
        )
        .child(
            div()
                .max_w(px(240.))
                .text_size(rems(0.75))
                .text_color(t.muted_foreground)
                .child(
                    "Files, terminals and agents work here as they are. Initialize a repository \
                     to see changes, history, branches and pull requests.",
                ),
        )
        .child(init_button("folder-init", project).small().primary())
        .into_any_element()
}

/// The branch chip's place in the top bar for a plain folder.
pub fn init_chip(project: &Entity<Project>, cx: &App) -> AnyElement {
    let t = cx.app_theme();
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap_1()
        .child(div().text_color(t.subtle).child("/"))
        .child(
            init_button("folder-init-chip", project)
                .ghost()
                .small()
                .icon(IconName::GitBranch)
                .tooltip("Not a git repository: run git init here"),
        )
        .into_any_element()
}

/// A git command in a plain folder: says why it didn't run, and offers `git init`.
pub fn not_a_repo(project: &Entity<Project>, window: &mut Window, cx: &mut App) {
    let project = project.downgrade();
    Note::info("Not a git repository")
        .detail("This folder isn't in a git repository yet.")
        .action(Some(Action::new(
            "Initialize Repository",
            move |window, cx| {
                if let Some(project) = project.upgrade() {
                    initialize(&project, window, cx);
                }
            },
        )))
        .show(window, cx);
}
