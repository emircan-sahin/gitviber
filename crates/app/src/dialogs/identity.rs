//! On a fresh machine the first commit fails with "Author identity unknown". This asks for the
//! missing parts when a repo opens, as git resolves them there (includeIf applies), and sets
//! them globally. Asked once per launch: a user who closes it may be setting it up their own way;
//! `ask_again` (a commit refused for want of one) shows it again.

use gitviber_core::api;
use gitviber_ui::ActiveAppTheme;
use gitviber_ui::memory::Live;
use gitviber_ui::workspace::Toast;
use gpui_kit::component::Disableable as _;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::{
    App, AppContext as _, Context, Entity, Global, IntoElement, ParentElement as _, Render,
    Styled as _, Subscription, Task, Window, div, prelude::FluentBuilder as _, px,
};

use crate::help::toast;

#[derive(Default)]
struct Asks {
    /// Asks so far; one closed with "Not now" is remembered by its number.
    count: u32,
    dismissed: Option<u32>,
    open: bool,
    _check: Option<Task<()>>,
}

impl Global for Asks {}

/// A repo opened in `window`: asks if git has no name or email for it.
pub fn check(window: &mut Window, cx: &mut App) {
    let asks = cx.default_global::<Asks>();
    if asks.open || asks.dismissed == Some(asks.count) || !gitviber_ui::has_core(cx) {
        return;
    }
    let core = gitviber_ui::core(cx);
    let asked = cx.background_spawn(async move { api::repo::git_identity(&core) });
    let handle = window.window_handle();
    let task = cx.spawn(async move |cx| {
        let Ok(found) = asked.await else { return };
        if found.current.name.is_some() && found.current.email.is_some() {
            return;
        }
        let _ = handle.update(cx, |_, window, cx| {
            let a = cx.default_global::<Asks>();
            if a.open || a.dismissed == Some(a.count) {
                return;
            }
            let suggested = found.suggested.unwrap_or_default();
            open(
                found.current.name.is_none(),
                found.current.email.is_none(),
                suggested.name.unwrap_or_default(),
                suggested.email.unwrap_or_default(),
                window,
                cx,
            );
        });
    });
    cx.default_global::<Asks>()._check = Some(task);
}

/// A commit git refused for want of a name or email: shown again, even after "Not now".
pub fn ask_again(window: &mut Window, cx: &mut App) {
    cx.default_global::<Asks>().count += 1;
    check(window, cx);
}

pub struct IdentityForm {
    name: Option<Entity<InputState>>,
    email: Option<Entity<InputState>>,
    busy: bool,
    _live: Live,
    _subs: Vec<Subscription>,
    _save: Option<Task<()>>,
}

impl IdentityForm {
    fn ready(&self, cx: &App) -> bool {
        let filled = |i: &Option<Entity<InputState>>| {
            i.as_ref()
                .is_none_or(|i| !i.read(cx).value().trim().is_empty())
        };
        !self.busy && filled(&self.name) && filled(&self.email)
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.ready(cx) {
            return;
        }
        let value = |i: &Option<Entity<InputState>>| {
            i.as_ref().map(|i| i.read(cx).value().trim().to_string())
        };
        let (name, email) = (value(&self.name), value(&self.email));
        self.busy = true;
        let core = gitviber_ui::core(cx);
        let saved =
            cx.background_spawn(async move { api::repo::set_git_identity(&core, name, email) });
        let handle = window.window_handle();
        self._save = Some(cx.spawn(async move |this, cx| {
            let done = saved.await;
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                cx.notify();
            });
            cx.update(|cx| match done {
                Ok(()) => {
                    toast(
                        Toast::success("Set in your global git config (~/.gitconfig).")
                            .title("git identity saved"),
                        cx,
                    );
                    let _ = handle.update(cx, |_, window, cx| window.close_dialog(cx));
                }
                Err(err) => toast(
                    Toast::error(err).title("Couldn't set your git identity"),
                    cx,
                ),
            });
        }));
        cx.notify();
    }
}

impl Render for IdentityForm {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_2()
            .when_some(self.name.clone(), |d, i| d.child(Input::new(&i)))
            .when_some(self.email.clone(), |d, i| d.child(Input::new(&i)))
    }
}

fn open(
    need_name: bool,
    need_email: bool,
    name: String,
    email: String,
    window: &mut Window,
    cx: &mut App,
) {
    cx.default_global::<Asks>().open = true;
    let input = |value: String, placeholder: &'static str, window: &mut Window, cx: &mut App| {
        cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(placeholder)
                .default_value(value)
        })
    };
    let name = need_name.then(|| input(name, "Your name", window, cx));
    let email = need_email.then(|| input(email, "Your email", window, cx));
    let first = name.clone().or(email.clone());
    let form = cx.new(|cx| {
        let subs = name
            .iter()
            .chain(email.iter())
            .map(|i| {
                cx.subscribe_in(
                    i,
                    window,
                    |this: &mut IdentityForm, _, e: &InputEvent, window, cx| match e {
                        InputEvent::PressEnter { .. } => this.save(window, cx),
                        _ => cx.notify(),
                    },
                )
            })
            .collect();
        IdentityForm {
            name,
            email,
            busy: false,
            _live: Live::new("IdentityDialog"),
            _subs: subs,
            _save: None,
        }
    });
    let what = if need_name && need_email {
        "aren't"
    } else {
        "isn't fully"
    };
    let description = format!(
        "git records a name and email with every commit, and yours {what} set yet. They go in \
         your global git config, so every repository uses them."
    );
    let focused = std::cell::Cell::new(false);
    window.open_dialog(cx, move |dialog, window, cx| {
        if !focused.replace(true)
            && let Some(first) = &first
        {
            first.update(cx, |i, cx| i.focus(window, cx));
        }
        let t = cx.app_theme();
        let ready = form.read(cx).ready(cx);
        let save = form.downgrade();
        dialog
            .title("Who's committing?")
            .w(px(448.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(
                        div()
                            .text_color(t.muted_foreground)
                            .child(description.clone()),
                    )
                    .child(form.clone()),
            )
            .footer(
                DialogFooter::new()
                    .child(
                        Button::new("identity-later")
                            .label("Not now")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("identity-save")
                            .primary()
                            .label("Save")
                            .disabled(!ready)
                            .on_click(move |_, window, cx| {
                                let _ = save.update(cx, |f, cx| f.save(window, cx));
                            }),
                    ),
            )
            .on_close(|_, _, cx| {
                let a = cx.default_global::<Asks>();
                a.open = false;
                a.dismissed = Some(a.count);
            })
    });
}
