//! Shown while no GitHub login is found, or the one found can't see origin (a private
//! repository). Nothing here looks for one by itself: only its buttons do, and only the second
//! may make the OS ask for a Keychain password. Also what shows when origin isn't on GitHub.

use gitviber_ui::AppTheme;
use gitviber_ui::keymap::IS_MAC;
use gitviber_ui::memory::Live;
use gitviber_ui::workspace::Workspace;
use gpui_kit::assets::IconName;
use gpui_kit::component::Disableable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::{
    AnyElement, App, AppContext as _, ClickEvent, Context, Entity, FontWeight, IntoElement,
    ParentElement as _, Render, Styled as _, Task, WeakEntity, Window, div,
    prelude::FluentBuilder as _, px, rems,
};

use crate::cache::{Cache, Read};
use crate::logic::connect::{Gate, gate, hidden_note, no_github_text, signed_out_note};
use crate::model::{Account, is_not_connected};
use crate::ui::common::{copy, error_text, theme};
use crate::ui::dialogs::AddRemoteDialog;

const LOGIN: &str = "gh auth login";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Way {
    Gh,
    Store,
}

pub struct ConnectView {
    /// "pull requests" or "issues".
    subject: &'static str,
    /// A login was found, and GitHub answered 404 for origin with it.
    hidden: bool,
    busy: Option<Way>,
    missed: Option<Way>,
    /// Hidden, a check found a login, and GitHub may still answer 404 with it.
    checked: bool,
    error: Option<String>,
    _look: Task<()>,
    _live: Live,
}

impl ConnectView {
    pub fn new(subject: &'static str, hidden: bool) -> Self {
        ConnectView {
            subject,
            hidden,
            busy: None,
            missed: None,
            checked: false,
            error: None,
            _look: Task::ready(()),
            _live: Live::new("ConnectView"),
        }
    }

    fn set_hidden(&mut self, hidden: bool, cx: &mut Context<Self>) {
        if self.hidden != hidden {
            self.hidden = hidden;
            self.missed = None;
            self.checked = false;
            cx.notify();
        }
    }

    fn look(&mut self, way: Way, cx: &mut Context<Self>) {
        self.busy = Some(way);
        self.missed = None;
        self.checked = false;
        self.error = None;
        let task = crate::account::connect(way == Way::Store, cx);
        self._look = cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |this, cx| {
                this.busy = None;
                match result {
                    // The same login, most likely: if GitHub still says 404, the screen says so.
                    Ok(()) => this.checked = this.hidden,
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
        // A login was found: what's needed is another account's, not an install.
        let command = if IS_MAC && !self.hidden {
            format!("brew install gh && {LOGIN}")
        } else {
            LOGIN.to_string()
        };
        let origin = Cache::origin(cx);
        let note = if self.hidden {
            hidden_note(origin.as_deref(), self.subject)
        } else {
            signed_out_note(origin.as_deref(), self.subject)
        };
        // After a check, until the account is read again with what it found.
        let reading = crate::account::current_account(cx).loading;
        let busy = self.busy.is_some() || reading;
        let still = self.checked && !busy;
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
            .child(Icon::new(if self.hidden { IconName::Lock } else { IconName::GitPullRequest }).large().text_color(t.subtle))
            .when_some(self.error.clone(), |d, e| d.child(div().mt(px(8.)).w_full().text_center().text_size(rems(0.72)).child(error_text("connect-error", &e, t.removed, cx))))
            .child(div().mt(px(12.)).text_center().text_size(rems(0.8125)).font_weight(FontWeight::MEDIUM).child(format!("Sign in to GitHub to see {}", self.subject)))
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
                            .when(self.missed == Some(Way::Gh), |d| d.child(small("Still no login from gh.")))
                            .when(still, |d| d.child(small("GitHub still doesn't show it to this login."))),
                    )
                    // gh's login is asked first, so the stored one can't stand in for it here.
                    .when(!self.hidden, |d| d.child(
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
                    )),
            )
    }
}

/// What a list panel draws in place of its lists, if anything. `connect` is its sign-in screen,
/// kept while it's up so a check's result shows on it, dropped once the lists show.
pub fn gate_view<P: 'static>(
    connect: &mut Option<Entity<ConnectView>>,
    subject: &'static str,
    acct: &Read<Account>,
    list_error: Option<&str>,
    ws: WeakEntity<Workspace>,
    t: &AppTheme,
    cx: &mut Context<P>,
) -> Option<AnyElement> {
    let shown = connect.as_ref().map(|c| c.read(cx).hidden);
    match gate(
        acct.data.is_some(),
        acct.error.as_deref(),
        list_error,
        shown,
    ) {
        Some(Gate::SignIn { hidden }) => {
            let view = connect
                .get_or_insert_with(|| cx.new(|_| ConnectView::new(subject, hidden)))
                .clone();
            view.update(cx, |v, cx| v.set_hidden(hidden, cx));
            Some(view.into_any_element())
        }
        other => {
            *connect = None;
            other.map(|g| {
                let add = move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                    AddRemoteDialog::open(ws.clone(), window, cx)
                };
                no_github(g, subject, add, t)
            })
        }
    }
}

/// In place of a list panel when origin isn't on GitHub: why, and with no origin, adding one
/// (`add`). The filters and New would have nothing to act on, so they don't show.
fn no_github(
    gate: Gate,
    subject: &'static str,
    add: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    t: &AppTheme,
) -> AnyElement {
    let (title, note) = no_github_text(gate, subject);
    div()
        .px(px(20.))
        .pt(px(48.))
        .flex()
        .flex_col()
        .items_center()
        .child(Icon::new(IconName::CloudOff).large().text_color(t.subtle))
        .child(
            div()
                .mt(px(12.))
                .text_center()
                .text_size(rems(0.8125))
                .font_weight(FontWeight::MEDIUM)
                .child(title),
        )
        .child(
            div()
                .mt(px(4.))
                .text_center()
                .text_size(rems(0.75))
                .text_color(t.muted_foreground)
                .child(note),
        )
        .when(gate == Gate::NoOrigin, |d| {
            d.child(
                div().mt(px(16.)).child(
                    Button::new("add-origin")
                        .primary()
                        .small()
                        .icon(IconName::Plus)
                        .label("Add remote…")
                        .on_click(add),
                ),
            )
        })
        .into_any_element()
}
