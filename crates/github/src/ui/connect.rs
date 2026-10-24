//! Shown while no GitHub login is found. Nothing here looks for one by itself: only its two
//! buttons do, and only the second may make the OS ask for a Keychain password.

use gitviber_ui::AppTheme;
use gitviber_ui::keymap::IS_MAC;
use gitviber_ui::memory::Live;
use gpui_kit::assets::IconName;
use gpui_kit::component::Disableable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::{
    App, Context, FontWeight, IntoElement, ParentElement as _, Render, Styled as _, Task, Window,
    div, prelude::FluentBuilder as _, px, rems,
};

use crate::cache::Cache;
use crate::logic::connect::signed_out_note;
use crate::model::is_not_connected;
use crate::ui::common::{copy, theme};

const LOGIN: &str = "gh auth login";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Way {
    Gh,
    Store,
}

pub struct ConnectView {
    /// "pull requests" or "issues".
    subject: &'static str,
    busy: Option<Way>,
    missed: Option<Way>,
    error: Option<String>,
    _look: Task<()>,
    _live: Live,
}

impl ConnectView {
    pub fn new(subject: &'static str) -> Self {
        ConnectView {
            subject,
            busy: None,
            missed: None,
            error: None,
            _look: Task::ready(()),
            _live: Live::new("ConnectView"),
        }
    }

    fn look(&mut self, way: Way, cx: &mut Context<Self>) {
        self.busy = Some(way);
        self.missed = None;
        self.error = None;
        let task = crate::account::connect(way == Way::Store, cx);
        self._look = cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |this, cx| {
                this.busy = None;
                match result {
                    Ok(()) => {}
                    // Still none: the screen stays, and says so.
                    Err(e) if is_not_connected(&e) => this.missed = Some(way),
                    Err(e) => this.error = Some(format!("Could not look for a login: {e}")),
                }
                cx.notify();
            })
            .ok();
        });
        cx.notify();
    }
}

fn step(n: &'static str, title: &'static str, t: &AppTheme) -> gpui_kit::Div {
    div()
        .rounded(px(6.))
        .border_1()
        .border_color(t.border)
        .bg(t.panel)
        .p(px(10.))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .mb(px(6.))
                .text_size(rems(0.75))
                .font_weight(FontWeight::MEDIUM)
                .child(
                    div()
                        .size(px(16.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(3.))
                        .bg(t.elevated)
                        .text_size(rems(0.625))
                        .child(n),
                )
                .child(title),
        )
}

impl Render for ConnectView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let command = if IS_MAC {
            format!("brew install gh && {LOGIN}")
        } else {
            LOGIN.to_string()
        };
        let note = signed_out_note(Cache::origin(cx).as_deref(), self.subject);
        let busy = self.busy.is_some();
        let small = |s: &str| {
            div()
                .mt(px(6.))
                .text_size(rems(0.72))
                .text_color(t.muted_foreground)
                .child(s.to_string())
        };
        let shown = command.clone();
        div()
            .px(px(20.))
            .pt(px(48.))
            .flex()
            .flex_col()
            .items_center()
            .child(Icon::new(IconName::GitPullRequest).large().text_color(t.subtle))
            .when_some(self.error.clone(), |d, e| d.child(div().mt(px(8.)).text_size(rems(0.72)).text_color(t.removed).child(e)))
            .child(div().mt(px(12.)).text_size(rems(0.8125)).font_weight(FontWeight::MEDIUM).child(format!("Sign in to GitHub to see {}", self.subject)))
            .child(div().mt(px(4.)).text_center().text_size(rems(0.75)).text_color(t.muted_foreground).child(note))
            .child(
                div()
                    .mt(px(20.))
                    .w_full()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(
                        step("1", "With the GitHub CLI (recommended)", &t)
                            .when(!IS_MAC, |d| d.child(small("Install gh (cli.github.com), then:")))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(4.))
                                    .child(div().flex_1().min_w_0().truncate().rounded(px(3.)).bg(t.background).px(px(8.)).py(px(4.)).font_family(t.fonts.code.clone()).text_size(rems(0.72)).child(shown))
                                    .child(Button::new("copy-login").ghost().xsmall().icon(IconName::Copy).tooltip("Copy command").on_click(move |_, window, cx: &mut App| copy(&command, "Command copied", window, cx))),
                            )
                            .child(
                                div().mt(px(8.)).child(
                                    Button::new("look-gh")
                                        .small()
                                        .icon(IconName::RefreshCw)
                                        .label("I've signed in, check again")
                                        .loading(self.busy == Some(Way::Gh))
                                        .disabled(busy)
                                        .on_click(cx.listener(|this, _, _, cx| this.look(Way::Gh, cx))),
                                ),
                            )
                            .when(self.missed == Some(Way::Gh), |d| d.child(small("Still no login from gh."))),
                    )
                    .child(
                        step("2", "Or the login git stored for HTTPS", &t)
                            .child(small(if IS_MAC {
                                "Reads what git saved for github.com (Keychain, GitHub Desktop, Git Credential Manager). macOS may ask for your Keychain password."
                            } else {
                                "Reads what git saved for github.com (a credential helper, Git Credential Manager)."
                            }))
                            .child(
                                div().mt(px(8.)).child(
                                    Button::new("look-store")
                                        .small()
                                        .label("Use it")
                                        .loading(self.busy == Some(Way::Store))
                                        .disabled(busy)
                                        .on_click(cx.listener(|this, _, _, cx| this.look(Way::Store, cx))),
                                ),
                            )
                            .when(self.missed == Some(Way::Store), |d| d.child(small("Git has no login stored for github.com."))),
                    ),
            )
    }
}
