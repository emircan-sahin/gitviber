//! The message for a reworded or squashed commit, and the name of a branch or tag made at a
//! commit. Each form is built when its dialog opens and dropped when it closes; it closes once
//! the action went through, keeping what was typed when it failed.

use std::rc::Rc;

use futures::channel::oneshot;
use gitviber_refs::names::ref_name_check;
use gitviber_ui::ActiveAppTheme;
use gitviber_ui::memory::Live;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::{Disableable as _, WindowExt as _};
use gpui_kit::{
    App, AppContext as _, Context, Entity, Focusable as _, IntoElement, ParentElement as _, Render,
    SharedString, Styled as _, Subscription, Task, Window, div, prelude::FluentBuilder as _, px,
    rems,
};

use crate::commit::Commit;

/// Runs the action the form names: resolves to whether it went through.
pub type Submit =
    Rc<dyn Fn(String, Option<String>, &mut Window, &mut App) -> oneshot::Receiver<bool>>;

/// What's being asked for: a new message for one commit, or the one for commits squashed into one.
#[derive(Clone)]
pub enum Messaging {
    Reword(Rc<Commit>),
    /// `commits` oldest first, their messages to start with; `apart`: others lie between them.
    Squash {
        commits: Vec<Rc<Commit>>,
        count: usize,
        onto: String,
        apart: bool,
    },
}

pub struct MessageForm {
    input: Entity<TextareaState>,
    submit: Submit,
    pending: bool,
    _wait: Option<Task<()>>,
    _sub: Subscription,
    _live: Live,
}

impl MessageForm {
    fn send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let message = self.input.read(cx).value().to_string();
        if message.trim().is_empty() || self.pending {
            return;
        }
        self.pending = true;
        let done = (self.submit)(message, None, window, cx);
        self._wait = Some(cx.spawn_in(window, async move |this, cx| {
            let ok = done.await.unwrap_or(false);
            let _ = this.update_in(cx, |this, window, cx| {
                this.pending = false;
                if ok {
                    gitviber_refs::askpass::close_form(window, cx);
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
}

impl Render for MessageForm {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().child(Textarea::new(&self.input).h(px(170.)))
    }
}

/// Opens the message dialog; `submit` gets the message.
pub fn open_message(m: Messaging, submit: Submit, window: &mut Window, cx: &mut App) {
    let (title, ok, text, about): (SharedString, &'static str, String, SharedString) = match &m {
        Messaging::Reword(c) => (
            "Reword commit".into(),
            "Reword",
            c.message(),
            format!(
                "A new message for {}; its changes stay as they are.",
                c.short
            )
            .into(),
        ),
        Messaging::Squash {
            commits,
            count,
            onto,
            apart,
        } => (
            format!("Squash {count} commits").into(),
            "Squash",
            commits
                .iter()
                .map(|c| c.message())
                .collect::<Vec<_>>()
                .join("\n\n"),
            if *apart {
                format!(
                    "These {count} commits become one where {} is, with this message.",
                    &onto[..onto.len().min(7)]
                )
                .into()
            } else {
                format!("These {count} commits become one, with this message.").into()
            },
        ),
    };
    let input = cx.new(|cx| TextareaState::new(window, cx).rows(8).default_value(text));
    let form = cx.new(|cx| {
        // ⌘↵ saves, as in the commit box; a plain ↵ is a new line.
        let sub = cx.subscribe_in(
            &input,
            window,
            |this: &mut MessageForm, _, e: &InputEvent, window, cx| {
                if let InputEvent::PressEnter {
                    secondary: true, ..
                } = e
                {
                    this.send(window, cx);
                }
            },
        );
        MessageForm {
            input: input.clone(),
            submit,
            pending: false,
            _wait: None,
            _sub: sub,
            _live: Live::new("MessageForm"),
        }
    });
    let first = std::cell::Cell::new(true);
    window.open_dialog(cx, move |dialog, window, cx| {
        if first.replace(false) {
            form.read(cx).input.focus_handle(cx).focus(window, cx);
        }
        let t = cx.app_theme();
        let pending = form.read(cx).pending;
        let f = form.clone();
        dialog
            .title(title.clone())
            .w(px(520.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(div().text_color(t.muted_foreground).child(about.clone()))
                    .child(form.clone()),
            )
            .footer(
                DialogFooter::new().child(
                    Button::new("message-ok")
                        .primary()
                        .label(ok)
                        .disabled(pending)
                        .on_click(move |_, window, cx| {
                            f.update(cx, |f, cx| f.send(window, cx));
                        }),
                ),
            )
    });
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NameKind {
    Branch,
    Tag,
}

pub struct NameForm {
    kind: NameKind,
    name: Entity<InputState>,
    message: Option<Entity<TextareaState>>,
    submit: Submit,
    pending: bool,
    _wait: Option<Task<()>>,
    _subs: Vec<Subscription>,
    _live: Live,
}

impl NameForm {
    fn checked(&self, cx: &App) -> gitviber_refs::names::NameCheck {
        ref_name_check(
            &self.name.read(cx).value(),
            &[],
            false,
            gitviber_refs::names::FOLD_CASE,
        )
    }

    fn send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.checked(cx).name;
        if name.is_empty() || self.pending {
            return;
        }
        let message = self
            .message
            .as_ref()
            .map(|m| m.read(cx).value().to_string())
            .filter(|m| !m.trim().is_empty());
        self.pending = true;
        let done = (self.submit)(name, message, window, cx);
        self._wait = Some(cx.spawn_in(window, async move |this, cx| {
            let ok = done.await.unwrap_or(false);
            let _ = this.update_in(cx, |this, window, cx| {
                this.pending = false;
                if ok {
                    gitviber_refs::askpass::close_form(window, cx);
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
}

impl Render for NameForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.app_theme();
        let hint = self.checked(cx).hint;
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(Input::new(&self.name))
            .when_some(hint, |d, h| {
                d.child(
                    div()
                        .text_size(rems(0.71875))
                        .text_color(t.muted_foreground)
                        .child(h),
                )
            })
            .when_some(self.message.clone(), |d, m| {
                d.child(Textarea::new(&m).h(px(72.)))
            })
            .when(self.kind == NameKind::Tag, |d| {
                d.child(
                    div().text_size(rems(0.6875)).text_color(t.subtle).child(
                        "A message makes an annotated tag, which Push with tags sends along.",
                    ),
                )
            })
    }
}

/// Opens the name dialog for a branch or tag at `commit`; `submit` gets the name (and a tag's message).
pub fn open_name(
    kind: NameKind,
    commit: &Commit,
    submit: Submit,
    window: &mut Window,
    cx: &mut App,
) {
    let name = cx.new(|cx| {
        InputState::new(window, cx).placeholder(if kind == NameKind::Branch {
            "Branch name"
        } else {
            "Tag name, e.g. v1.2.0"
        })
    });
    let message = (kind == NameKind::Tag).then(|| {
        cx.new(|cx| {
            TextareaState::new(window, cx)
                .rows(3)
                .placeholder("Message (optional)")
        })
    });
    let form = cx.new(|cx| {
        let mut subs = vec![cx.subscribe_in(
            &name,
            window,
            |this: &mut NameForm, _, e: &InputEvent, window, cx| match e {
                InputEvent::PressEnter { .. } => this.send(window, cx),
                InputEvent::Change => cx.notify(),
                _ => {}
            },
        )];
        if let Some(m) = &message {
            subs.push(cx.subscribe_in(
                m,
                window,
                |this: &mut NameForm, _, e: &InputEvent, window, cx| {
                    if let InputEvent::PressEnter {
                        secondary: true, ..
                    } = e
                    {
                        this.send(window, cx);
                    }
                },
            ));
        }
        NameForm {
            kind,
            name: name.clone(),
            message,
            submit,
            pending: false,
            _wait: None,
            _subs: subs,
            _live: Live::new("NameForm"),
        }
    });
    let title: SharedString = if kind == NameKind::Branch {
        "Create branch".into()
    } else {
        "Create tag".into()
    };
    let about: SharedString = format!(
        "At {} {}{}",
        commit.short,
        commit.subject,
        if kind == NameKind::Branch {
            ". You'll be switched to it; uncommitted changes come along."
        } else {
            ""
        }
    )
    .into();
    let first = std::cell::Cell::new(true);
    window.open_dialog(cx, move |dialog, window, cx| {
        if first.replace(false) {
            form.read(cx).name.focus_handle(cx).focus(window, cx);
        }
        let t = cx.app_theme();
        let f = form.clone();
        let ready = !form.read(cx).pending && !form.read(cx).checked(cx).name.is_empty();
        dialog
            .title(title.clone())
            .w(px(440.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(div().text_color(t.muted_foreground).child(about.clone()))
                    .child(form.clone()),
            )
            .footer(
                DialogFooter::new().child(
                    Button::new("name-ok")
                        .primary()
                        .label("Create")
                        .disabled(!ready)
                        .on_click(move |_, window, cx| {
                            f.update(cx, |f, cx| f.send(window, cx));
                        }),
                ),
            )
    });
}
