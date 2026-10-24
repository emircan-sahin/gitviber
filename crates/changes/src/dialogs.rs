//! The questions the panel asks: a yes or no before something that throws work away, and a line
//! of text (a stash's message, a branch's name).

use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{Disableable as _, WindowExt as _, v_flex};
use gpui_kit::{
    App, AppContext as _, Entity, ParentElement as _, SharedString, Styled as _, Window,
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// Asks `question`; `ok` runs once on the destructive button.
pub fn confirm(
    window: &mut Window,
    cx: &mut App,
    title: impl Into<SharedString>,
    question: impl Into<SharedString>,
    ok_label: &'static str,
    ok: impl FnOnce(&mut Window, &mut App) + 'static,
) {
    let (title, question) = (title.into(), question.into());
    let ok = Rc::new(RefCell::new(Some(ok)));
    window.open_alert_dialog(cx, move |alert, _, _| {
        let ok = ok.clone();
        alert
            .ok_variant(gpui_kit::component::button::ButtonVariant::Danger)
            .title(title.clone())
            .description(question.clone())
            .ok_text(ok_label)
            .show_cancel(true)
            .on_ok(move |_, window, cx| {
                if let Some(ok) = ok.borrow_mut().take() {
                    ok(window, cx);
                }
                true
            })
    });
}

/// A checkbox a text dialog shows under its field.
pub struct Choice {
    pub label: SharedString,
    pub enabled: bool,
    pub checked: Rc<Cell<bool>>,
}

/// What a text dialog asks.
pub struct Ask {
    pub title: SharedString,
    pub description: SharedString,
    pub placeholder: &'static str,
    pub initial: String,
    pub ok_label: &'static str,
    /// A blank line is an answer (an optional stash message).
    pub allow_empty: bool,
    pub choices: Vec<Choice>,
}

/// Asks for a line of text; `ok` gets it, the choices left in their cells.
pub fn ask_text(
    window: &mut Window,
    cx: &mut App,
    ask: Ask,
    ok: impl Fn(String, &mut Window, &mut App) + 'static,
) {
    let Ask {
        title,
        description,
        placeholder,
        initial,
        ok_label,
        allow_empty,
        choices,
    } = ask;
    let input: Entity<InputState> = cx.new(|cx| {
        InputState::new(window, cx)
            .placeholder(placeholder)
            .default_value(initial)
    });
    let first = Cell::new(true);
    let ok = Rc::new(ok);
    let choices = Rc::new(choices);
    window.open_alert_dialog(cx, move |alert, window, cx| {
        if first.replace(false) {
            input.update(cx, |s, cx| s.focus(window, cx));
        }
        let (input_ok, ok) = (input.clone(), ok.clone());
        let boxes = choices.iter().enumerate().map(|(i, c)| {
            let checked = c.checked.clone();
            Checkbox::new(("dialog-choice", i))
                .label(c.label.clone())
                .checked(c.checked.get())
                .disabled(!c.enabled)
                .on_click(move |on, window, _| {
                    checked.set(*on);
                    window.refresh();
                })
        });
        alert
            .title(title.clone())
            .description(description.clone())
            .child(
                v_flex()
                    .w_full()
                    .gap_3()
                    .child(Input::new(&input))
                    .children(boxes),
            )
            .ok_text(ok_label)
            .show_cancel(true)
            .on_ok(move |_, window, cx| {
                let text = input_ok.read(cx).value().trim().to_string();
                if text.is_empty() && !allow_empty {
                    return false;
                }
                ok(text, window, cx);
                true
            })
    });
}
