//! The ask before tabs' unsaved edits would be lost: closing them, a switch to another worktree
//! or project, a quit. Save, Don't Save or Cancel, as the old app's settleEdits asked. Save
//! writes through each tab's own save; Don't Save drops the edits wherever the tab kept them.

use std::rc::Rc;

use gitviber_ui::Workspace;
use gitviber_ui::workspace::{Toast, set_close_guard, set_settle_edits};
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::{App, ParentElement as _, SharedString, WeakEntity, Window, px};

use crate::help::toast;

type Then = Box<dyn FnOnce(bool, &mut App)>;

pub fn install(cx: &mut App) {
    // Closing tabs: once they're settled, they close.
    set_close_guard(
        cx,
        Rc::new(|keys, ws, cx| {
            let (close, weak) = (keys.clone(), ws.clone());
            let then: Then = Box::new(move |go, cx| {
                if go && let Some(ws) = weak.upgrade() {
                    ws.update(cx, |ws, cx| ws.force_close_tabs(&close, cx));
                }
            });
            ask(keys, ws, then, cx)
        }),
    );
    set_settle_edits(cx, Rc::new(ask));
}

/// What the ask says, and whether it offers Save: only when every tab can save what it has (a
/// comment's draft can't), or Save would quietly drop some.
pub fn wording(names: &[SharedString], savable: bool) -> (String, bool) {
    let what = match names {
        [one] => format!("Do you want to save the changes you made to {one}?"),
        many => format!(
            "Do you want to save the changes you made to {} tabs?",
            many.len()
        ),
    };
    if savable {
        (what, true)
    } else {
        let why = match names {
            [_] => "It can't be saved from here (a comment's draft): Don't Save drops it.",
            _ => "Some can't be saved from here (a comment's draft): Don't Save drops them all.",
        };
        (format!("{what} {why}"), false)
    }
}

/// Asks about `keys`' unsaved edits in the main window, then `then(true)` once they're saved or
/// dropped (and the tabs closed), or `then(false)` when the user keeps them. After the update
/// that asked: a quit's own dialog button may be what's running.
fn ask(keys: Vec<SharedString>, ws: WeakEntity<Workspace>, then: Then, cx: &mut App) {
    cx.defer(move |cx| ask_now(keys, ws, then, cx));
}

fn ask_now(keys: Vec<SharedString>, ws: WeakEntity<Workspace>, then: Then, cx: &mut App) {
    let Some(workspace) = ws.upgrade() else {
        return then(true, cx);
    };
    let handles: Vec<_> = keys
        .iter()
        .filter_map(|k| {
            let entry = workspace.read(cx).tabs().get(k)?;
            Some((
                k.clone(),
                entry.item.title(cx),
                entry.item.handle()?.clone(),
            ))
        })
        .collect();
    if handles.is_empty() {
        return then(true, cx);
    }
    let names: Vec<SharedString> = handles.iter().map(|(_, n, _)| n.clone()).collect();
    let savable = handles.iter().all(|(_, _, h)| h.can_save(cx));
    let (message, offer_save) = wording(&names, savable);
    // The workspace's own window: the ask is about its tabs.
    let own = cx.windows().into_iter().find(|h| {
        h.update(cx, |_, window, cx| {
            Workspace::for_window(window, cx).is_some_and(|w| w == workspace)
        })
        .unwrap_or(false)
    });
    let Some(window) = own
        .or_else(|| crate::workspace_view::main_window(cx))
        .or_else(|| cx.active_window())
    else {
        return then(false, cx);
    };
    let answer = Answer::new(then);
    // A button's choice: the dialog's own close (Escape, the overlay) is Cancel only without one.
    let decided = Rc::new(std::cell::Cell::new(false));
    let fallback = answer.clone();
    let opened = window.update(cx, |_, window, cx| {
        window.activate_window();
        let (handles, ws, message) = (Rc::new(handles), ws.clone(), SharedString::from(message));
        window.open_dialog(cx, move |dialog, _, _| {
            let button = |id: &'static str, label: &'static str, choice: Choice| {
                let (handles, ws, answer, decided) =
                    (handles.clone(), ws.clone(), answer.clone(), decided.clone());
                Button::new(id).label(label).on_click(move |_, window, cx| {
                    decided.set(true);
                    window.close_dialog(cx);
                    choose(choice, &handles, &ws, answer.clone(), window, cx);
                })
            };
            let mut footer = DialogFooter::new()
                .child(button("unsaved-cancel", "Cancel", Choice::Cancel))
                .child(button("unsaved-discard", "Don't Save", Choice::Discard).danger());
            if offer_save {
                footer = footer.child(button("unsaved-save", "Save", Choice::Save).primary());
            }
            let (on_close, decided) = (answer.clone(), decided.clone());
            dialog
                .title("Unsaved changes")
                .w(px(448.))
                .child(message.clone())
                .footer(footer)
                .on_close(move |_, _, cx| {
                    if !decided.get() {
                        on_close.give(false, cx)
                    }
                })
        });
    });
    if opened.is_err() {
        fallback.give(false, cx);
    }
}

/// The settle's `then`, given its answer once.
#[derive(Clone)]
pub struct Answer(Rc<std::cell::RefCell<Option<Then>>>);

impl Answer {
    pub fn new(then: Then) -> Self {
        Self(Rc::new(std::cell::RefCell::new(Some(then))))
    }

    fn give(&self, go: bool, cx: &mut App) {
        if let Some(then) = self.0.borrow_mut().take() {
            then(go, cx);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    Save,
    Discard,
    Cancel,
}

/// What a button does: Save writes them all and closes them, Don't Save drops and closes them,
/// Cancel keeps everything as it is.
pub fn choose(
    choice: Choice,
    handles: &Handles,
    ws: &WeakEntity<Workspace>,
    answer: Answer,
    window: &mut Window,
    cx: &mut App,
) {
    match choice {
        Choice::Cancel => answer.give(false, cx),
        Choice::Discard => {
            for (_, _, h) in handles {
                h.discard(cx);
            }
            close(ws, handles, cx);
            answer.give(true, cx);
        }
        Choice::Save => save_all(handles, ws, answer, window, cx),
    }
}

pub type Handles = [(
    SharedString,
    SharedString,
    gitviber_ui::workspace::TabHandle,
)];

/// The settled tabs close: their edits were written, or let go.
fn close(ws: &WeakEntity<Workspace>, handles: &Handles, cx: &mut App) {
    let keys: Vec<SharedString> = handles.iter().map(|(k, _, _)| k.clone()).collect();
    if let Some(ws) = ws.upgrade() {
        ws.update(cx, |ws, cx| ws.force_close_tabs(&keys, cx));
    }
}

/// Saves each tab; all written, they close and `answer(true)`; one that wasn't keeps everything
/// where it is (its own error says why).
fn save_all(
    handles: &Handles,
    ws: &WeakEntity<Workspace>,
    answer: Answer,
    window: &mut Window,
    cx: &mut App,
) {
    let saves: Vec<_> = handles
        .iter()
        .map(|(_, name, h)| (name.clone(), h.save(window, cx)))
        .collect();
    let (ws, handles) = (ws.clone(), handles.to_vec());
    cx.spawn(async move |cx| {
        let mut failed: Vec<SharedString> = Vec::new();
        for (name, save) in saves {
            let ok = match save {
                Some(task) => task.await,
                None => false,
            };
            if !ok {
                failed.push(name);
            }
        }
        cx.update(|cx| {
            if failed.is_empty() {
                close(&ws, &handles, cx);
                answer.give(true, cx);
            } else {
                let names: Vec<&str> = failed.iter().map(|n| n.as_ref()).collect();
                toast(
                    Toast::warning(format!("Not saved: {}.", names.join(", ")))
                        .title("Nothing closed"),
                    cx,
                );
                answer.give(false, cx);
            }
        });
    })
    .detach();
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::cell::{Cell, RefCell};

    use gitviber_ui::workspace::TabItem;
    use gpui_kit::{
        AnyWindowHandle, AppContext as _, Context, Entity, IntoElement, Render, Task,
        TestAppContext, div,
    };

    /// A tab with typing in it: `saves` is how its save goes.
    struct Typed {
        dirty: Rc<Cell<bool>>,
        saves: bool,
        can: bool,
        dropped: Rc<Cell<bool>>,
    }
    impl Render for Typed {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
        }
    }
    impl TabItem for Typed {
        fn tab_title(&self, _: &App) -> SharedString {
            "notes.md".into()
        }
        fn is_dirty(&self, _: &App) -> bool {
            self.dirty.get()
        }
        fn can_save(&self, _: &App) -> bool {
            self.can
        }
        fn save(&mut self, _: &mut Window, _: &mut Context<Self>) -> Option<Task<bool>> {
            if self.saves {
                self.dirty.set(false);
            }
            Some(Task::ready(self.saves))
        }
        fn discard(&mut self, _: &mut Context<Self>) {
            self.dropped.set(true);
        }
    }

    /// Keeps the workspace alive with its window, as the app's view does.
    struct Host(#[allow(dead_code)] Entity<Workspace>);
    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
        }
    }

    struct Setup {
        window: AnyWindowHandle,
        ws: Entity<Workspace>,
        dirty: Rc<Cell<bool>>,
        dropped: Rc<Cell<bool>>,
    }

    fn setup(saves: bool, cx: &mut TestAppContext) -> Setup {
        gitviber_ui::test_support::config_dir("unsaved");
        cx.update(|cx| {
            gitviber_ui::init(cx);
            install(cx);
        });
        let mut made = None;
        let window: AnyWindowHandle = cx
            .add_window(|window, cx| {
                let ws = cx.new(Workspace::new);
                Workspace::register_window(&ws, window, cx);
                made = Some(ws.clone());
                let host = cx.new(|_| Host(ws));
                gpui_kit::base::Root::new(host, window, cx)
            })
            .into();
        let ws = made.unwrap();
        let (dirty, dropped) = (Rc::new(Cell::new(true)), Rc::new(Cell::new(false)));
        let (d, x) = (dirty.clone(), dropped.clone());
        let _ = window.update(cx, |_, window, cx| {
            ws.update(cx, |ws, cx| {
                ws.open_project("/tmp/gitviber-unsaved-project".into(), window, cx);
                ws.open_tab(
                    "typed",
                    false,
                    move |_, cx| {
                        cx.new(|_| Typed {
                            dirty: d,
                            saves,
                            can: true,
                            dropped: x,
                        })
                    },
                    window,
                    cx,
                );
            })
        });
        Setup {
            window,
            ws,
            dirty,
            dropped,
        }
    }

    fn handles(
        ws: &Entity<Workspace>,
        cx: &App,
    ) -> Vec<(
        SharedString,
        SharedString,
        gitviber_ui::workspace::TabHandle,
    )> {
        let entry = ws.read(cx).tabs().get("typed").unwrap();
        vec![(
            "typed".into(),
            entry.item.title(cx),
            entry.item.handle().unwrap().clone(),
        )]
    }

    fn answer() -> (Answer, Rc<RefCell<Option<bool>>>) {
        let got = Rc::new(RefCell::new(None));
        let put = got.clone();
        (
            Answer::new(Box::new(move |go, _| *put.borrow_mut() = Some(go))),
            got,
        )
    }

    #[gpui_kit::test]
    fn closing_a_tab_with_typing_asks_and_keeps_it(cx: &mut TestAppContext) {
        let s = setup(true, cx);
        let ws = s.ws.clone();
        let _ = s.window.update(cx, |_, _, cx| {
            ws.update(cx, |ws, cx| ws.close_tabs(&["typed".into()], cx))
        });
        cx.run_until_parked();
        let asking = s
            .window
            .update(cx, |_, window, cx| window.has_active_dialog(cx))
            .unwrap();
        assert!(asking);
        assert_eq!(cx.update(|cx| ws.read(cx).tabs().len()), 1);
    }

    #[gpui_kit::test]
    fn save_writes_then_closes_and_a_failed_one_keeps_everything(cx: &mut TestAppContext) {
        for saves in [true, false] {
            let s = setup(saves, cx);
            let ws = s.ws.clone();
            let (give, got) = answer();
            let _ = s.window.update(cx, |_, window, cx| {
                let h = handles(&ws, cx);
                choose(Choice::Save, &h, &ws.downgrade(), give, window, cx)
            });
            cx.run_until_parked();
            assert_eq!(*got.borrow(), Some(saves));
            assert_eq!(
                cx.update(|cx| ws.read(cx).tabs().len()),
                usize::from(!saves)
            );
            assert_eq!(s.dirty.get(), !saves);
        }
    }

    #[gpui_kit::test]
    fn dont_save_drops_and_closes_and_cancel_keeps(cx: &mut TestAppContext) {
        let s = setup(true, cx);
        let ws = s.ws.clone();
        let (give, got) = answer();
        let _ = s.window.update(cx, |_, window, cx| {
            let h = handles(&ws, cx);
            choose(Choice::Cancel, &h, &ws.downgrade(), give, window, cx)
        });
        assert_eq!(*got.borrow(), Some(false));
        assert_eq!(cx.update(|cx| ws.read(cx).tabs().len()), 1);
        assert!(!s.dropped.get());
        let (give, got) = answer();
        let _ = s.window.update(cx, |_, window, cx| {
            let h = handles(&ws, cx);
            choose(Choice::Discard, &h, &ws.downgrade(), give, window, cx)
        });
        assert_eq!(*got.borrow(), Some(true));
        assert!(s.dropped.get());
        assert!(cx.update(|cx| ws.read(cx).tabs().is_empty()));
    }
    #[test]
    fn the_ask_names_one_tab_or_counts_them_and_offers_save_only_when_it_can() {
        let one = [SharedString::from("main.rs")];
        assert_eq!(
            wording(&one, true),
            (
                "Do you want to save the changes you made to main.rs?".into(),
                true
            )
        );
        let two = [SharedString::from("a"), SharedString::from("b")];
        let (text, save) = wording(&two, false);
        assert!(text.starts_with("Do you want to save the changes you made to 2 tabs?"));
        assert!(text.ends_with("Don't Save drops them all."));
        assert!(!save);
    }
}
