//! A short message in the window's corner: a title, what happened, git's own words folded under
//! Details, and buttons (Undo, Open Settings).
//!
//! When one goes by itself: Info, Success and Warning after a few seconds, the countdown paused
//! while the pointer is on it or the window is in the background. An error stays until closed,
//! and so does a toast whose button is the only way forward (Sign in, Locate…, Retry). One whose
//! buttons are all optional goes like any success: Undo (`Toast::undo`) is a second chance, Show
//! Logs (`Toast::extra`) a way to learn more.

use std::rc::Rc;

use gpui_kit::base::text::{TextView, TextViewStyle};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{Sizable as _, WindowExt as _};
use gpui_kit::{
    App, ClipboardItem, ElementId, Hsla, InteractiveElement as _, IntoElement, ParentElement as _,
    SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
    prelude::FluentBuilder as _, px, rems,
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
    /// Run once it goes, by a button or its close button; not when the window goes with it.
    pub closed: Option<ToastClosed>,
    /// How many of its buttons are optional (Undo, an extra): with only those, it goes by itself.
    pub optional: usize,
}

/// `Toast::on_close`'s hook.
#[derive(Clone)]
pub struct ToastClosed(pub ToastRun);

impl std::fmt::Debug for ToastClosed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ToastClosed")
    }
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
            closed: None,
            optional: 0,
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

    /// What to do once it's closed, by a button or by hand.
    pub fn on_close(mut self, run: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.closed = Some(ToastClosed(Rc::new(run)));
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

    /// The one Undo of a success toast ("Discarded a.rs"), as its first button wherever it's
    /// added. Optional: it doesn't keep the toast up. `run` says how the undo went: a success
    /// toast ("Restored a.rs"), or an error toast with git's own words.
    pub fn undo(mut self, run: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        let undo = ToastAction {
            label: "Undo".into(),
            run: Rc::new(run),
        };
        if let Some(first) = self.action.replace(undo) {
            self.more.insert(0, first);
        }
        self.optional += 1;
        self
    }

    /// A button that's no way forward, only a way to learn more (Show Logs): it doesn't keep the
    /// toast up.
    pub fn extra(
        mut self,
        label: impl Into<SharedString>,
        run: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        self = self.action(label, run);
        self.optional += 1;
        self
    }

    /// Whether it goes by itself: not an error (hook, GPG or push output takes a while to read),
    /// nor one with a button that's the way forward.
    pub fn autohides(&self) -> bool {
        let buttons = usize::from(self.action.is_some()) + self.more.len();
        self.kind != ToastKind::Error && buttons == self.optional
    }

    /// Shows it in `window` now.
    pub fn show(self, window: &mut Window, cx: &mut App) {
        let autohide = self.autohides();
        let copy = self.detail.as_ref().map(|detail| {
            let head = self.title.iter().chain([&self.message]);
            let text = head.chain([detail]).map(|s| s.as_ref()).collect::<Vec<_>>();
            SharedString::from(text.join("\n"))
        });
        let note = match self.kind {
            ToastKind::Info => Notification::info(self.message),
            ToastKind::Success => Notification::success(self.message),
            ToastKind::Warning => Notification::warning(self.message),
            ToastKind::Error => Notification::error(self.message),
        };
        let note = match self.title {
            Some(title) => note.title(title),
            None => note,
        };
        let note = match self.closed {
            Some(ToastClosed(run)) => note.on_close(move |window, cx| run(window, cx)),
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
        // After `action`, which keeps any toast with a button up.
        let note = note.autohide(autohide);
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
                let copied = window.use_keyed_state(
                    ("toast-copied", cx.entity_id().as_u64()),
                    cx,
                    |_, _| false,
                );
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
                        let toggle = div()
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
                            });
                        let done = *copied.read(cx);
                        let text = copy.clone().unwrap_or_default();
                        let copy = div()
                            .id("toast-copy")
                            .text_size(rems(0.6875))
                            .text_color(t.subtle)
                            .cursor_pointer()
                            .hover(|s| s.text_color(t.foreground))
                            .child(if done { "Copied" } else { "Copy" })
                            .on_click(move |_, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(text.to_string()));
                                copied.update(cx, |c, cx| {
                                    *c = true;
                                    cx.notify();
                                })
                            });
                        d.child(div().flex().gap_3().child(toggle).child(copy))
                            .when(open, |d| {
                                d.child(
                                    div()
                                        .id("toast-details")
                                        .max_h(px(160.))
                                        .overflow_y_scroll()
                                        .font_family(t.fonts.code.clone())
                                        .text_size(rems(0.71875))
                                        .child(selectable_text(
                                            "toast-details-text",
                                            &detail,
                                            t.muted_foreground,
                                            cx,
                                        )),
                                )
                            })
                    })
                    .into_any_element()
            })
        };
        window.push_notification(note, cx);
    }
}

/// Plain text the user can select and copy (⌘C), its line breaks and spacing as written: git's
/// output, an error. It takes its font from where it's put.
pub fn selectable_text(id: impl Into<ElementId>, text: &str, color: Hsla, cx: &App) -> TextView {
    let style =
        TextViewStyle::from_theme(&gpui_kit::base::Theme::global(cx)).with_foreground(color);
    TextView::html(id, pre_html(text))
        .style(style)
        .selectable(true)
}

/// `text` as HTML that shows it as it is. The parser drops one line feed right after `<pre>`, so
/// one is put there.
fn pre_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 16);
    out.push_str("<pre>\n");
    escape_html(text, &mut out);
    out.push_str("</pre>");
    out
}

/// `text` onto `out` with what HTML would read as markup escaped, attribute quotes too.
pub fn escape_html(text: &str, out: &mut String) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
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

    #[test]
    fn undo_goes_by_itself_and_other_ways_forward_stay() {
        assert!(Toast::success("Discarded a.rs").autohides());
        assert!(Toast::warning("a moved").autohides());
        assert!(Toast::success("Discarded a.rs").undo(|_, _| {}).autohides());
        let undo = Toast::success("Discarded a.rs").undo(|_, _| {});
        assert_eq!(undo.action.map(|a| a.label), Some("Undo".into()));
        assert!(
            !Toast::warning("Signed out")
                .action("Sign in", |_, _| {})
                .autohides()
        );
        assert!(!Toast::error("Undo failed").undo(|_, _| {}).autohides());
        assert!(
            Toast::warning("Didn't quit cleanly")
                .extra("Show Logs", |_, _| {})
                .autohides()
        );
    }

    /// Undo goes first however it's added, and a way forward beside it keeps the toast up.
    #[test]
    fn undo_is_first_and_a_way_forward_beside_it_stays() {
        let labels = |t: &Toast| -> Vec<SharedString> {
            t.action
                .iter()
                .chain(&t.more)
                .map(|a| a.label.clone())
                .collect()
        };
        let after = Toast::warning("Signed out")
            .action("Sign in", |_, _| {})
            .undo(|_, _| {});
        let before = Toast::warning("Signed out")
            .undo(|_, _| {})
            .action("Sign in", |_, _| {});
        for t in [&after, &before] {
            assert_eq!(labels(t), ["Undo", "Sign in"]);
            assert!(!t.autohides());
        }
    }

    struct Host;
    impl gpui_kit::Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut gpui_kit::Context<Self>) -> impl IntoElement {
            div()
        }
    }

    /// An Undo toast waits while the window is in the background, then goes; Locate… stays.
    #[gpui_kit::test]
    fn the_countdown_waits_for_the_window_and_ways_forward_stay(cx: &mut gpui_kit::TestAppContext) {
        use gpui_kit::AppContext as _;
        use std::time::Duration;
        crate::test_support::config_dir("toast-test");
        cx.update(crate::init);
        let (_, cx) = cx.add_window_view(|window, cx| {
            let host = cx.new(|_| Host);
            gpui_kit::base::Root::new(gpui_kit::AnyView::from(host), window, cx)
        });
        let shown = |cx: &mut gpui_kit::VisualTestContext| {
            cx.update(|window, cx| window.notifications(cx).len())
        };
        cx.update(|window, cx| {
            assert!(!window.is_window_active());
            Toast::success("Discarded a.rs")
                .undo(|_, _| {})
                .show(window, cx);
            Toast::warning("Not found")
                .action("Locate…", |_, _| {})
                .show(window, cx);
        });
        cx.background_executor
            .advance_clock(Duration::from_secs(60));
        cx.run_until_parked();
        assert_eq!(shown(cx), 2);
        cx.update(|window, _| window.activate_window());
        cx.run_until_parked();
        cx.background_executor
            .advance_clock(Duration::from_secs(10));
        cx.run_until_parked();
        assert_eq!(shown(cx), 1);
        cx.background_executor
            .advance_clock(Duration::from_secs(60));
        cx.run_until_parked();
        assert_eq!(shown(cx), 1);
    }

    #[test]
    fn plain_text_stays_text() {
        assert_eq!(
            pre_html("a <b> & c\n  d"),
            "<pre>\na &lt;b&gt; &amp; c\n  d</pre>"
        );
    }
}
