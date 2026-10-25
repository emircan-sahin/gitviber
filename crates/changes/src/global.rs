//! The panel's commands from anywhere: focus in a terminal, a diff or the palette, they still
//! reach the panel, built if it wasn't yet.

use crate::ChangesPanel;
use gitviber_ui::actions::{changes, git, review};
use gitviber_ui::workspace::{ChangesOf, Workspace};
use gpui_kit::{Action, App, Context, Window};

type Act = fn(&mut ChangesPanel, &mut Window, &mut Context<ChangesPanel>);

pub fn install(cx: &mut App) {
    // ⌃⌘C: the panel shows and its commit message takes the focus.
    on::<changes::FocusMessage>(cx, true, |p, window, cx| p.focus_message(window, cx));
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
}

/// `act` on the panel of the active window's workspace; `show`: the panel comes into view first.
fn on<A: Action>(cx: &mut App, show: bool, act: Act) {
    cx.on_action(move |_: &A, cx| {
        let Some(handle) = cx.active_window() else {
            return;
        };
        // The window is in the middle of dispatching the action: it can be updated after.
        cx.defer(move |cx| {
            handle
                .update(cx, |_, window, cx| {
                    let Some(ws) = Workspace::for_window(window, cx) else {
                        return;
                    };
                    if show {
                        ws.update(cx, |ws, cx| ws.show_panel("changes", cx));
                    }
                    let panel = Workspace::ensure_panel(&ws, "changes", window, cx)
                        .and_then(|p| p.view().downcast::<ChangesPanel>().ok());
                    if let Some(panel) = panel {
                        panel.update(cx, |p, cx| act(p, window, cx));
                    }
                })
                .ok();
        });
    });
}
