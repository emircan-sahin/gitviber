//! Shown while no GitHub login is found, GitHub refused the one found (expired or revoked), or it
//! can't see origin (a private repository). Nothing here looks for one by itself: only its buttons
//! do, and only the second may make the OS ask for a Keychain password. Also what shows when
//! origin isn't on GitHub.

use gitviber_ui::AppTheme;
use gitviber_ui::keymap::IS_MAC;
use gitviber_ui::memory::Live;
use gitviber_ui::workspace::Workspace;
use gpui_kit::assets::IconName;
use gpui_kit::component::Disableable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::{
    AnyElement, App, AppContext as _, ClickEvent, Context, Entity, IntoElement, ParentElement as _,
    Render, Styled as _, Task, WeakEntity, Window, div, prelude::FluentBuilder as _, px, rems,
};

use crate::cache::{Cache, Read};
use crate::logic::connect::{Gate, gate, hidden_note, no_github_text, signed_out_note};
use crate::model::{Account, is_expired, is_not_connected};
use crate::ui::common::{copy, error_text, theme};
use crate::ui::dialogs::AddRemoteDialog;

const LOGIN: &str = "gh auth login";
/// A new login for gh's account, in place of the one GitHub refused.
const REFRESH: &str = "gh auth refresh";

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
    /// GitHub refused the login found: expired or revoked.
    expired: bool,
    /// Whether gh is installed, once looked; a check looks again.
    gh: Option<bool>,
    busy: Option<Way>,
    missed: Option<Way>,
    /// A check found the login GitHub refused, handed back as it was.
    refused: Option<Way>,
    /// Hidden, a check found a login, and GitHub may still answer 404 with it.
    checked: bool,
    error: Option<String>,
    _look: Task<()>,
    _live: Live,
}

impl ConnectView {
    pub fn new(subject: &'static str, hidden: bool, cx: &mut Context<Self>) -> Self {
        let mut view = ConnectView {
            subject,
            hidden,
            expired: false,
            gh: None,
            busy: None,
            missed: None,
            refused: None,
            checked: false,
            error: None,
            _look: Task::ready(()),
            _live: Live::new("ConnectView"),
        };
        let installed = cx.background_spawn(async { gitviber_core::github::gh_installed() });
        view._look = cx.spawn(async move |this, cx| {
            let gh = installed.await;
            this.update(cx, |this, cx| {
                this.gh = Some(gh);
                cx.notify();
            })
            .ok();
        });
        view
    }

    /// `failure`: the account's or the list's error, which tells expired from signed out.
    fn set_state(&mut self, hidden: bool, failure: Option<&str>, cx: &mut Context<Self>) {
        // Kept while the account is read again (no error then), so the words don't flash.
        let expired = failure
            .filter(|e| is_not_connected(e))
            .map_or(self.expired, is_expired);
        if self.hidden != hidden || self.expired != expired {
            self.hidden = hidden;
            self.expired = expired;
            self.missed = None;
            self.refused = None;
            self.checked = false;
            cx.notify();
        }
    }

    fn look(&mut self, way: Way, cx: &mut Context<Self>) {
        self.busy = Some(way);
        self.missed = None;
        self.refused = None;
        self.checked = false;
        self.error = None;
        let task = crate::account::connect(way == Way::Store, cx);
        self._look = cx.spawn(async move |this, cx| {
            let result = task.await;
            let gh = cx
                .background_spawn(async { gitviber_core::github::gh_installed() })
                .await;
            this.update(cx, |this, cx| {
                this.busy = None;
                this.gh = Some(gh);
                match result {
                    // The same login, most likely: if GitHub still says 404, the screen says so.
                    Ok(()) => this.checked = this.hidden,
                    Err(e) if is_expired(&e) => this.refused = Some(way),
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

/// `line` in a new terminal tab of the open repository. Without a terminal in this window it's
/// copied instead.
fn run_in_terminal(line: &str, window: &mut Window, cx: &mut App) {
    let cwd = crate::core(cx)
        .repo()
        .ok()
        .or_else(std::env::home_dir)
        .unwrap_or_default();
    if !gitviber_terminal::run_in_new_tab(cwd, line, window, cx) {
        copy(
            line,
            "Command copied: this window has no terminal",
            window,
            cx,
        );
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
                .font_weight(t.medium())
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
        // A login was found (hidden, expired) or gh is there: no install to suggest.
        let install = !self.hidden && !self.expired && self.gh != Some(true);
        let command = if self.expired {
            REFRESH.to_string()
        } else if IS_MAC && install {
            format!("brew install gh && {LOGIN}")
        } else {
            LOGIN.to_string()
        };
        let origin = Cache::origin(cx);
        let (title, note) = if self.expired {
            (
                "GitHub rejected gh's login".to_string(),
                "It's expired or revoked. Refresh it with the GitHub CLI, then check again."
                    .to_string(),
            )
        } else if self.hidden {
            (
                format!("Sign in to GitHub to see {}", self.subject),
                hidden_note(origin.as_deref(), self.subject),
            )
        } else {
            (
                format!("Sign in to GitHub to see {}", self.subject),
                signed_out_note(origin.as_deref(), self.subject),
            )
        };
        let missed_gh = if self.gh == Some(false) {
            "gh isn't installed yet. Install it, sign in with it, then check again."
        } else {
            "Still no login from gh."
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
        let (shown, run) = (command.clone(), command.clone());
        div()
            .px(px(20.))
            .pt(px(48.))
            .flex()
            .flex_col()
            .items_center()
            .child(Icon::new(if self.hidden || self.expired { IconName::Lock } else { IconName::GitPullRequest }).large().text_color(t.subtle))
            .when_some(self.error.clone(), |d, e| d.child(div().mt(px(8.)).w_full().text_center().text_size(rems(0.72)).child(error_text("connect-error", &e, t.removed, cx))))
            .child(div().mt(px(12.)).text_center().text_size(rems(0.8125)).font_weight(t.medium()).child(title))
            .child(div().mt(px(4.)).text_center().text_size(rems(0.75)).text_color(t.muted_foreground).child(note))
            .child(
                div()
                    .mt(px(20.))
                    .w_full()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(
                        step("1", if self.expired { "Refresh gh's login" } else { "With the GitHub CLI (recommended)" }, &t)
                            .when(!IS_MAC && install, |d| d.child(small("Install gh (cli.github.com), then:")))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(4.))
                                    .child(div().flex_1().min_w_0().truncate().rounded(px(3.)).bg(t.background).px(px(8.)).py(px(4.)).font_family(t.fonts.code.clone()).text_size(rems(0.72)).child(shown))
                                    .child(Button::new("copy-login").ghost().xsmall().icon(IconName::Copy).tooltip("Copy command").on_click(move |_, window, cx: &mut App| copy(&command, "Command copied", window, cx))),
                            )
                            .child(
                                div().mt(px(8.)).flex().flex_wrap().gap(px(6.)).child(
                                    Button::new("run-login")
                                        .small()
                                        .icon(IconName::SquareTerminal)
                                        .label("Run in Terminal")
                                        .on_click(move |_, window, cx: &mut App| run_in_terminal(&run, window, cx)),
                                ).child(
                                    Button::new("look-gh")
                                        .small()
                                        .icon(IconName::RefreshCw)
                                        .label("I've signed in, check again")
                                        .loading(self.busy == Some(Way::Gh))
                                        .disabled(busy)
                                        .on_click(cx.listener(|this, _, _, cx| this.look(Way::Gh, cx))),
                                ),
                            )
                            .when(self.missed == Some(Way::Gh), |d| d.child(small(missed_gh)))
                            .when(self.refused == Some(Way::Gh), |d| d.child(small("gh still has the login GitHub rejected.")))
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
                            .when(self.missed == Some(Way::Store), |d| d.child(small("Git has no login stored for github.com.")))
                            .when(self.refused == Some(Way::Store), |d| d.child(small("GitHub rejected the login git stored (expired or revoked)."))),
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
    let failure = acct.error.as_deref().or(list_error);
    match gate(
        acct.data.is_some(),
        acct.error.as_deref(),
        list_error,
        shown,
    ) {
        Some(Gate::SignIn { hidden }) => {
            let view = connect
                .get_or_insert_with(|| cx.new(|cx| ConnectView::new(subject, hidden, cx)))
                .clone();
            view.update(cx, |v, cx| v.set_state(hidden, failure, cx));
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
                .font_weight(t.medium())
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
