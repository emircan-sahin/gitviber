//! A short message in the window's corner: a title, what happened, git's own words folded under
//! Details, and buttons (Undo, Open Settings).

use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{Sizable as _, WindowExt as _};
use gpui_kit::{
    App, InteractiveElement as _, IntoElement, ParentElement as _, SharedString,
    StatefulInteractiveElement as _, Styled as _, Window, div, prelude::FluentBuilder as _, px,
    rems,
};

use crate::ActiveAppTheme;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Clone, Debug)]
pub struct Toast {
    pub kind: ToastKind,
    pub title: Option<SharedString>,
    pub message: SharedString,
    /// The first button; `more` has the others.
    pub action: Option<ToastAction>,
    pub more: Vec<ToastAction>,
    /// Long output (a hook's, git's own words), folded under Details.
    pub detail: Option<SharedString>,
}

/// What a toast's button does.
pub type ToastRun = Rc<dyn Fn(&mut Window, &mut App)>;

/// A toast's button: its label, and what a click does (the toast then goes).
#[derive(Clone)]
pub struct ToastAction {
    pub label: SharedString,
    pub run: ToastRun,
}

impl std::fmt::Debug for ToastAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToastAction")
            .field("label", &self.label)
            .finish()
    }
}

impl Toast {
    pub fn new(kind: ToastKind, message: impl Into<SharedString>) -> Self {
        Self {
            kind,
            title: None,
            message: message.into(),
            action: None,
            more: Vec::new(),
            detail: None,
        }
    }
    pub fn info(message: impl Into<SharedString>) -> Self {
        Self::new(ToastKind::Info, message)
    }
    pub fn success(message: impl Into<SharedString>) -> Self {
        Self::new(ToastKind::Success, message)
    }
    pub fn warning(message: impl Into<SharedString>) -> Self {
        Self::new(ToastKind::Warning, message)
    }
    pub fn error(message: impl Into<SharedString>) -> Self {
        Self::new(ToastKind::Error, message)
    }
    pub fn title(mut self, title: impl Into<SharedString>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Text shown folded under Details: a hook's output, git's own words under an explanation.
    pub fn detail(mut self, detail: impl Into<SharedString>) -> Self {
        self.detail = Some(detail.into()).filter(|d| !d.is_empty());
        self
    }

    /// A button on it, after the ones before; a toast with one stays until it's clicked or closed.
    pub fn action(
        mut self,
        label: impl Into<SharedString>,
        run: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        let action = ToastAction {
            label: label.into(),
            run: Rc::new(run),
        };
        match self.action {
            None => self.action = Some(action),
            Some(_) => self.more.push(action),
        }
        self
    }

    /// Shows it in `window` now. Errors stay until dismissed: they carry hook, GPG or push output
    /// that takes a while to read.
    pub fn show(self, window: &mut Window, cx: &mut App) {
        let note = match self.kind {
            ToastKind::Info => Notification::info(self.message),
            ToastKind::Success => Notification::success(self.message),
            ToastKind::Warning => Notification::warning(self.message),
            ToastKind::Error => Notification::error(self.message).autohide(false),
        };
        let note = match self.title {
            Some(title) => note.title(title),
            None => note,
        };
        let note = match self.action {
            Some(a) => note.action(move |_, _, cx| {
                let run = a.run.clone();
                Button::new("toast-action")
                    .label(a.label.clone())
                    .on_click(cx.listener(move |this, _, window, cx| {
                        run(window, cx);
                        this.dismiss(window, cx);
                    }))
            }),
            None => note,
        };
        let (more, detail) = (self.more, self.detail);
        let note = if more.is_empty() && detail.is_none() {
            note
        } else {
            note.content(move |_, window, cx| {
                let t = cx.app_theme().clone();
                let unfolded = window.use_keyed_state(
                    ("toast-details", cx.entity_id().as_u64()),
                    cx,
                    |_, _| false,
                );
                let open = *unfolded.read(cx);
                let buttons = more.iter().enumerate().map(|(i, a)| {
                    let run = a.run.clone();
                    Button::new(("toast-more", i))
                        .small()
                        .ghost()
                        .label(a.label.clone())
                        .on_click(cx.listener(move |this, _, window, cx| {
                            run(window, cx);
                            this.dismiss(window, cx);
                        }))
                });
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .when(!more.is_empty(), |d| {
                        d.child(div().flex().flex_wrap().gap_1().children(buttons))
                    })
                    .when_some(detail.clone(), |d, detail| {
                        d.child(
                            div()
                                .id("toast-details-toggle")
                                .text_size(rems(0.6875))
                                .text_color(t.subtle)
                                .cursor_pointer()
                                .hover(|s| s.text_color(t.foreground))
                                .child(if open { "▾ Details" } else { "▸ Details" })
                                .on_click(move |_, _, cx| {
                                    unfolded.update(cx, |o, cx| {
                                        *o = !*o;
                                        cx.notify();
                                    })
                                }),
                        )
                        .when(open, |d| {
                            d.child(
                                div()
                                    .id("toast-details")
                                    .max_h(px(160.))
                                    .overflow_y_scroll()
                                    .font_family(t.fonts.code.clone())
                                    .text_size(rems(0.71875))
                                    .text_color(t.muted_foreground)
                                    .child(detail),
                            )
                        })
                    })
                    .into_any_element()
            })
        };
        window.push_notification(note, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buttons_add_up_and_empty_details_are_none() {
        let t = Toast::error("Push failed")
            .action("Pull", |_, _| {})
            .action("Open Settings", |_, _| {})
            .detail("");
        assert_eq!(t.action.map(|a| a.label), Some("Pull".into()));
        assert_eq!(t.more.len(), 1);
        assert!(t.detail.is_none());
    }
}
