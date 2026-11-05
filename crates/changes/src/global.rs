//! The panel's commands from anywhere: focus in a terminal, a diff or the palette, they still
//! reach the panel, built if it wasn't yet.

use crate::ChangesPanel;
use crate::panel::{Act, has_steps, skips};
use gitviber_core::git::Operation;
use gitviber_ui::actions::{changes, git, review};
use gitviber_ui::workspace::{ChangesOf, Workspace};
use gpui_kit::{Action, App, InteractiveElement, Window};

pub fn install(cx: &mut App) {
    // ⌃⌘C: the panel shows and its commit message takes the focus; again, focus goes back.
    on::<changes::FocusMessage>(cx, true, |p, window, cx| p.toggle_message_focus(window, cx));
    on::<git::ApplyPatch>(cx, false, |p, window, cx| p.apply_patch(window, cx));
    on::<review::OpenAll>(cx, false, |p, window, cx| {
        p.open_all(ChangesOf::Unstaged, window, cx)
    });
    on::<review::OpenAllStaged>(cx, false, |p, window, cx| {
        p.open_all(ChangesOf::Staged, window, cx)
    });
    // Its files are the review's: one starts when none is in progress.
    on::<review::OpenAllBranch>(cx, true, |p, _, cx| p.review_branch(true, cx));
    on::<review::Branch>(cx, true, |p, _, cx| p.review_branch(false, cx));
    // These check what the panel lists, so they wait for its first read.
    on::<git::Stash>(cx, false, |p, window, cx| {
        p.once_read("Stash", |p, w, cx| p.stash_command(w, cx), window, cx)
    });
    on::<git::PopStash>(cx, false, |p, window, cx| {
        p.once_read("Pop", |p, w, cx| p.pop_latest_stash(w, cx), window, cx)
    });
    on::<git::CommitAndPush>(cx, true, |p, window, cx| {
        p.once_read(
            "Commit & Push",
            |p, w, cx| p.commit_and_push(w, cx),
            window,
            cx,
        )
    });
    on::<git::ToggleAmend>(cx, true, |p, window, cx| {
        p.once_read("Amend", |p, w, cx| p.toggle_amend(w, cx), window, cx)
    });
}

/// Continue, Abort and Skip on a window's root, there only while `op` is stopped for them: the
/// palette and the menu bar offer what something listens for.
pub fn operation_actions<E: InteractiveElement>(el: E, op: Option<&Operation>) -> E {
    let Some(op) = op.filter(|o| has_steps(&o.kind)) else {
        return el;
    };
    let mut el = el
        .on_action(|_: &git::Continue, window, cx| {
            on_panel(window, cx, true, |p, window, cx| {
                p.once_read(
                    "Continue",
                    |p, w, cx| p.operation_command("continue", w, cx),
                    window,
                    cx,
                )
            })
        })
        .on_action(|_: &git::Abort, window, cx| {
            on_panel(window, cx, true, |p, window, cx| {
                p.once_read(
                    "Abort",
                    |p, w, cx| p.operation_command("abort", w, cx),
                    window,
                    cx,
                )
            })
        });
    if skips(&op.kind, op.split) {
        el = el.on_action(|_: &git::Skip, window, cx| {
            on_panel(window, cx, true, |p, window, cx| {
                p.once_read(
                    "Skip",
                    |p, w, cx| p.operation_command("skip", w, cx),
                    window,
                    cx,
                )
            })
        });
    }
    el
}

/// `act` on the panel of the active window's workspace, once its repo is open if it's opening;
/// `show`: the panel comes into view first.
fn on<A: Action>(cx: &mut App, show: bool, act: Act) {
    cx.on_action(move |_: &A, cx| {
        let Some(handle) = cx.active_window() else {
            return;
        };
        // The window is in the middle of dispatching the action: it can be updated after.
        cx.defer(move |cx| {
            handle
                .update(cx, |_, window, cx| on_panel(window, cx, show, act))
                .ok();
        });
    });
}

fn on_panel(window: &mut Window, cx: &mut App, show: bool, act: Act) {
    let Some(ws) = Workspace::for_window(window, cx) else {
        return;
    };
    if show {
        ws.update(cx, |ws, cx| ws.show_panel("changes", cx));
    }
    Workspace::with_panel(&ws, "changes", window, cx, move |p, window, cx| {
        if let Ok(panel) = p.view().downcast::<ChangesPanel>() {
            panel.update(cx, |p, cx| act(p, window, cx));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{
        Context, FocusHandle, IntoElement, Render, TestAppContext, VisualTestContext, div,
    };

    struct Root {
        op: Option<Operation>,
        focus: FocusHandle,
    }

    impl Render for Root {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            operation_actions(div().track_focus(&self.focus), self.op.as_ref())
        }
    }

    fn op(kind: &str, split: bool) -> Option<Operation> {
        Some(Operation {
            kind: kind.into(),
            subject: None,
            step: None,
            total: None,
            split,
        })
    }

    /// What the palette and the menu bar offer: (Continue, Abort, Skip).
    fn offered(op: Option<Operation>, cx: &mut TestAppContext) -> (bool, bool, bool) {
        let window = cx.add_window(|_, cx| Root {
            op,
            focus: cx.focus_handle(),
        });
        let cx = &mut VisualTestContext::from_window(*window, cx);
        cx.update(|window, cx| {
            let focus = window
                .root::<Root>()
                .flatten()
                .unwrap()
                .read(cx)
                .focus
                .clone();
            focus.focus(window, cx);
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            (
                window.is_action_available(&git::Continue, cx),
                window.is_action_available(&git::Abort, cx),
                window.is_action_available(&git::Skip, cx),
            )
        })
    }

    #[gpui_kit::test]
    fn continue_abort_and_skip_are_offered_only_while_they_apply(cx: &mut TestAppContext) {
        assert_eq!(offered(None, cx), (false, false, false));
        assert_eq!(offered(op("merge", false), cx), (true, true, false));
        assert_eq!(offered(op("rebase", false), cx), (true, true, true));
        // A split's pieces are committed, not skipped; a bisect has its own bar.
        assert_eq!(offered(op("rebase", true), cx), (true, true, false));
        assert_eq!(offered(op("bisect", false), cx), (false, false, false));
    }
}
