//! Takes the welcome screen's place while git can't run: nothing else works without it. Checked
//! once at launch, off the UI thread (a git only the login shell's PATH has shows up once that's
//! read), and again on Check again.

use gitviber_core::api;
use gitviber_core::git::GitInfo;
use gitviber_ui::ActiveAppTheme;
use gitviber_ui::assets::LOGO;
use gitviber_ui::keymap::IS_MAC;
use gitviber_ui::workspace::Toast;
use gpui_kit::assets::IconName;
use gpui_kit::component::Disableable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::{
    AnyElement, App, AppContext as _, Global, IntoElement, ParentElement as _, Styled as _, Task,
    div, img, prelude::FluentBuilder as _, px, rems,
};

use crate::help::toast;

#[derive(Default)]
pub struct GitCheck {
    /// What the last check found, while git can't run.
    missing: Option<GitInfo>,
    checking: bool,
    _task: Option<Task<()>>,
}

impl Global for GitCheck {}

/// Whether git is missing, as the last check found.
pub fn missing(cx: &App) -> Option<&GitInfo> {
    cx.try_global::<GitCheck>()?.missing.as_ref()
}

pub fn check(recheck: bool, cx: &mut App) {
    if !gitviber_ui::has_core(cx) {
        return;
    }
    let core = gitviber_ui::core(cx);
    let asked = cx.background_spawn(async move { api::repo::git_info(&core, recheck) });
    cx.default_global::<GitCheck>().checking = true;
    let task = cx.spawn(async move |cx| {
        let info = asked.await;
        cx.update(|cx| {
            let c = cx.default_global::<GitCheck>();
            c.checking = false;
            c.missing = info.ok().filter(|i| matches!(i.state, "missing" | "tools"));
            cx.refresh_windows();
        });
    });
    cx.default_global::<GitCheck>()._task = Some(task);
}

fn install_tools(cx: &mut App) {
    let core = gitviber_ui::core(cx);
    let started = cx.background_spawn(async move { api::repo::install_git(&core) });
    cx.spawn(async move |cx| {
        let shown = match started.await {
            Ok(()) => Toast::info("Then come back and click Check again.")
                .title("Follow the installer macOS opened"),
            Err(err) => Toast::error(err).title("Couldn't start the installer"),
        };
        cx.update(|cx| toast(shown, cx));
    })
    .detach();
}

pub fn screen(info: &GitInfo, cx: &App) -> AnyElement {
    let t = cx.app_theme().clone();
    let mac = info.state == "tools" || IS_MAC;
    let checking = cx.try_global::<GitCheck>().is_some_and(|c| c.checking);
    let lead = if info.state == "tools" {
        "git comes with Apple's Command Line Tools, which aren't installed yet."
    } else {
        "git isn't installed, or GitViber can't find it."
    };
    div()
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
                .gap_3()
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
                                        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                        .child("GitViber needs git"),
                                )
                                .child(
                                    div()
                                        .text_size(rems(0.75))
                                        .text_color(t.muted_foreground)
                                        .child(lead),
                                ),
                        ),
                )
                .when(mac, |d| {
                    d.child(
                        Button::new("install-tools")
                            .primary()
                            .icon(IconName::Download)
                            .label("Install Command Line Tools")
                            .on_click(|_, _, cx| install_tools(cx)),
                    )
                })
                .child(
                    div()
                        .text_size(rems(0.75))
                        .text_color(t.muted_foreground)
                        .child(if mac {
                            "Or with Homebrew: brew install git"
                        } else {
                            "Install git with your package manager or from git-scm.com."
                        }),
                )
                .child(
                    Button::new("check-git")
                        .icon(if checking {
                            IconName::LoaderCircle
                        } else {
                            IconName::RefreshCw
                        })
                        .label("Check again")
                        .disabled(checking)
                        .on_click(|_, _, cx| check(true, cx)),
                )
                .when_some(info.detail.clone(), |d, detail| {
                    d.child(
                        div()
                            .p_2()
                            .rounded(px(6.))
                            .border_1()
                            .border_color(t.border)
                            .bg(t.panel)
                            .font_family(t.fonts.code.clone())
                            .text_size(rems(0.6875))
                            .text_color(t.subtle)
                            .child(detail),
                    )
                }),
        )
        .into_any_element()
}
