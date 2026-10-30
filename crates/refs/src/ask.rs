//! Asking before an action: a yes/no with the warnings to read first, and the rewrite's
//! "also move these branches" question. Both answer through a oneshot; closing is no.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use futures::channel::oneshot;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::{
    App, ParentElement as _, SharedString, Styled as _, Window, div, prelude::FluentBuilder as _,
    px, rems,
};

use gitviber_ui::ActiveAppTheme;

use crate::names::move_along;

#[derive(Clone)]
pub struct Ask {
    pub title: SharedString,
    pub message: SharedString,
    pub ok: SharedString,
    /// The action loses or rewrites something: the button says so.
    pub danger: bool,
}

impl Ask {
    pub fn new(
        title: impl Into<SharedString>,
        message: impl Into<SharedString>,
        ok: impl Into<SharedString>,
    ) -> Self {
        Ask {
            title: title.into(),
            message: message.into(),
            ok: ok.into(),
            danger: false,
        }
    }

    pub fn danger(mut self) -> Self {
        self.danger = true;
        self
    }
}

type Reply<T> = Rc<RefCell<Option<oneshot::Sender<T>>>>;

fn reply<T>(slot: &Reply<T>, value: T) {
    if let Some(tx) = slot.borrow_mut().take() {
        let _ = tx.send(value);
    }
}

/// Asks; true when confirmed.
pub fn ask(ask: Ask, window: &mut Window, cx: &mut App) -> oneshot::Receiver<bool> {
    let (tx, rx) = oneshot::channel();
    let slot: Reply<bool> = Rc::new(RefCell::new(Some(tx)));
    window.open_alert_dialog(cx, move |alert, _, _| {
        let (yes, no, closed) = (slot.clone(), slot.clone(), slot.clone());
        alert
            .title(ask.title.clone())
            .description(ask.message.clone())
            .ok_text(ask.ok.clone())
            .ok_variant(if ask.danger {
                ButtonVariant::Danger
            } else {
                ButtonVariant::Primary
            })
            .show_cancel(true)
            .on_ok(move |_, _, _| {
                reply(&yes, true);
                true
            })
            .on_cancel(move |_, _, _| {
                reply(&no, false);
                true
            })
            .on_close(move |_, _, _| reply(&closed, false))
    });
    rx
}

/// `ask` with a second way forward beside its button: Some(true) for the button, Some(false)
/// for `other`, None when cancelled or closed.
pub fn ask_or(
    ask: Ask,
    other: impl Into<SharedString>,
    window: &mut Window,
    cx: &mut App,
) -> oneshot::Receiver<Option<bool>> {
    let (tx, rx) = oneshot::channel();
    let slot: Reply<Option<bool>> = Rc::new(RefCell::new(Some(tx)));
    let other = other.into();
    window.open_dialog(cx, move |dialog, _, cx| {
        let t = cx.app_theme();
        let (yes, alt, cancel, closed) = (slot.clone(), slot.clone(), slot.clone(), slot.clone());
        let ok = Button::new("ask-ok").label(ask.ok.clone());
        let ok = if ask.danger {
            ok.danger()
        } else {
            ok.primary()
        };
        dialog
            .title(ask.title.clone())
            .w(px(448.))
            .child(
                div()
                    .text_color(t.muted_foreground)
                    .child(ask.message.clone()),
            )
            .footer(
                DialogFooter::new()
                    .child(Button::new("ask-cancel").label("Cancel").on_click(
                        move |_, window, cx| {
                            reply(&cancel, None);
                            window.close_dialog(cx);
                        },
                    ))
                    .child(Button::new("ask-other").label(other.clone()).on_click(
                        move |_, window, cx| {
                            reply(&alt, Some(false));
                            window.close_dialog(cx);
                        },
                    ))
                    .child(ok.on_click(move |_, window, cx| {
                        reply(&yes, Some(true));
                        window.close_dialog(cx);
                    })),
            )
            .on_close(move |_, _, _| reply(&closed, None))
    });
    rx
}

/// What a rewrite or rebase asks before it runs: `branches` are the local ones on the commits it
/// replays. Some(move them along) when it goes ahead; None when cancelled.
pub fn ask_stacked(
    title: impl Into<SharedString>,
    message: impl Into<SharedString>,
    ok: impl Into<SharedString>,
    branches: Vec<String>,
    checked: bool,
    window: &mut Window,
    cx: &mut App,
) -> oneshot::Receiver<Option<bool>> {
    let (tx, rx) = oneshot::channel();
    let slot: Reply<Option<bool>> = Rc::new(RefCell::new(Some(tx)));
    let (title, message, ok) = (title.into(), message.into(), ok.into());
    let names: SharedString = format!("Also move {}", move_along(&branches)).into();
    let moving = Rc::new(Cell::new(checked));
    window.open_dialog(cx, move |dialog, _, cx| {
        let t = cx.app_theme();
        let (go, cancel, closed, toggle) = (slot.clone(), slot.clone(), slot.clone(), moving.clone());
        let on = moving.get();
        let read = moving.clone();
        dialog
            .title(title.clone())
            .w(px(448.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .when_some((!message.is_empty()).then(|| message.clone()), |d, m| d.child(div().text_color(t.muted_foreground).child(m)))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(Checkbox::new("stacked-move").label(names.clone()).checked(on).on_click(move |checked, window, _| {
                                toggle.set(*checked);
                                window.refresh();
                            }))
                            .child(div().pl_6().text_size(rems(0.71875)).text_color(t.muted_foreground).child(
                                "They point at commits this replays; moved, they stay on the new ones. Branches checked out in another worktree stay.",
                            )),
                    ),
            )
            .footer(
                DialogFooter::new()
                    .child(Button::new("stacked-cancel").label("Cancel").on_click(move |_, window, cx| {
                        reply(&cancel, None);
                        window.close_dialog(cx);
                    }))
                    .child(Button::new("stacked-ok").primary().label(ok.clone()).on_click(move |_, window, cx| {
                        reply(&go, Some(read.get()));
                        window.close_dialog(cx);
                    })),
            )
            .on_close(move |_, _, _| reply(&closed, None))
    });
    rx
}
