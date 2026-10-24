//! Toasts with a button (Undo, Pull, Remove lock) and errors explained: gpui-component's
//! notifications in the window an action started from. The workspace's own `Toast` has no
//! buttons; this is for the ones that need one.

use std::rc::Rc;

use gitviber_ui::keymap::view;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{Sizable as _, WindowExt as _};
use gpui_kit::{AnyWindowHandle, App, SharedString, Window};

use crate::errors::{GitFix, SIGNING_HELP, explain_git_error};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Info,
    Success,
    Error,
}

pub type Run = Rc<dyn Fn(&mut Window, &mut App)>;

/// A toast's button.
#[derive(Clone)]
pub struct Action {
    pub label: SharedString,
    pub run: Run,
}

impl Action {
    pub fn new(
        label: impl Into<SharedString>,
        run: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        Action {
            label: label.into(),
            run: Rc::new(run),
        }
    }
}

pub struct Note {
    pub kind: Kind,
    pub title: SharedString,
    pub detail: Option<SharedString>,
    pub action: Option<Action>,
}

impl Note {
    pub fn new(kind: Kind, title: impl Into<SharedString>) -> Self {
        Note {
            kind,
            title: title.into(),
            detail: None,
            action: None,
        }
    }
    pub fn info(title: impl Into<SharedString>) -> Self {
        Self::new(Kind::Info, title)
    }
    pub fn success(title: impl Into<SharedString>) -> Self {
        Self::new(Kind::Success, title)
    }
    pub fn error(title: impl Into<SharedString>) -> Self {
        Self::new(Kind::Error, title)
    }
    pub fn detail(mut self, detail: impl Into<SharedString>) -> Self {
        self.detail = Some(detail.into());
        self
    }
    pub fn maybe_detail(mut self, detail: Option<impl Into<SharedString>>) -> Self {
        self.detail = detail.map(Into::into);
        self
    }
    pub fn action(mut self, action: Option<Action>) -> Self {
        self.action = action;
        self
    }

    pub fn show(self, window: &mut Window, cx: &mut App) {
        let message = self.detail.clone().unwrap_or_default();
        let note = match self.kind {
            Kind::Info => Notification::info(message),
            Kind::Success => Notification::success(message),
            Kind::Error => Notification::error(message),
        }
        .title(self.title);
        let note = match self.action {
            None => note,
            // An action needs time to reach for: kept until dismissed, as errors are.
            Some(a) => note
                .action(move |_, _, cx| {
                    let run = a.run.clone();
                    Button::new("toast-action")
                        .small()
                        .primary()
                        .label(a.label.clone())
                        .on_click(cx.listener(move |this, _, window, cx| {
                            run(window, cx);
                            this.dismiss(window, cx);
                        }))
                })
                .autohide(self.kind != Kind::Error),
        };
        // Errors stay until dismissed: they carry hook, GPG or push output that takes a while to read.
        let note = if self.kind == Kind::Error {
            note.autohide(false)
        } else {
            note
        };
        window.push_notification(note, cx);
    }

    /// In the window `handle` names, if it's still open.
    pub fn show_in(self, handle: AnyWindowHandle, cx: &mut App) {
        let _ = handle.update(cx, |_, window, cx| self.show(window, cx));
    }
}

/// The buttons a caller adds for a way out only it can take (a pull after a refused push).
pub type Fixes = Vec<(GitFix, Action)>;

/// The ways out that fit wherever the failure happens; `target` is what git's words named.
fn always(fix: GitFix, target: Option<&str>) -> Option<Action> {
    match fix {
        GitFix::Conflicted => Some(Action::new("Open Changes", |window, cx| {
            window.dispatch_action(Box::new(view::Changes), cx)
        })),
        GitFix::Signing => Some(Action::new("Signing guide", |_, cx| {
            cx.open_url(SIGNING_HELP)
        })),
        GitFix::IndexLock => {
            let lock = target?.to_string();
            Some(Action::new("Remove lock", move |window, cx| {
                let (core, lock) = (gitviber_ui::core(cx), lock.clone());
                let handle = window.window_handle();
                let task = cx.background_executor().spawn(async move {
                    gitviber_core::api::changes::remove_index_lock(&core, lock)
                });
                cx.spawn(async move |cx| {
                    let note = match task.await {
                        Ok(()) => Note::success("Index lock removed").detail("Try again now."),
                        Err(e) => Note::error("Could not remove the lock").detail(e),
                    };
                    cx.update(|cx| note.show_in(handle, cx));
                })
                .detach();
            }))
        }
        GitFix::Secret => {
            let url = target?.to_string();
            Some(Action::new("Open on GitHub", move |_, cx| {
                cx.open_url(&url)
            }))
        }
        _ => None,
    }
}

/// The error toast for a failed git action: git's own words, or for a failure `errors` knows,
/// what it means and the way out, with git's words after.
pub fn git_failed(title: impl Into<SharedString>, message: &str, fixes: &Fixes) -> Note {
    let Some(help) = explain_git_error(message) else {
        return Note::error(title).detail(message.to_string());
    };
    let action = help.fix.and_then(|fix| {
        fixes
            .iter()
            .find(|(f, _)| *f == fix)
            .map(|(_, a)| a.clone())
            .or_else(|| always(fix, help.target.as_deref()))
    });
    Note::error(title)
        .detail(format!(
            "{}\n{}\n\n{}",
            help.title,
            help.explanation,
            message.trim()
        ))
        .action(action)
}
