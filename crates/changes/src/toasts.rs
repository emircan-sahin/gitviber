//! What the panel's git actions say when they end: a success with Undo when the action went into
//! the journal (⌘Z's history), a failure git explains with what it means and the way out, git's
//! own words under Details.

use gitviber_core::{Core, Res, api};
use gitviber_refs::RepoState;
use gitviber_refs::repo::undo_action;
use gitviber_ui::workspace::{Toast, Workspace};
use gpui_kit::{App, WeakEntity};
use std::path::PathBuf;

/// Runs `work`, and gives the journal entry it recorded, if it moved anything.
pub(crate) fn tracked<T>(core: &Core, work: impl FnOnce(&Core) -> Res<T>) -> Res<(T, Option<u64>)> {
    let before = api::journal::journal_last(core).ok().flatten();
    let value = work(core)?;
    let after = api::journal::journal_last(core).ok().flatten();
    Ok((value, after.filter(|a| Some(*a) != before)))
}

/// `toast` with an Undo button for journal entry `id` of the worktree `root`; it does nothing
/// there once another worktree is open.
pub(crate) fn with_undo(
    toast: Toast,
    id: Option<u64>,
    root: Option<PathBuf>,
    ws: WeakEntity<Workspace>,
) -> Toast {
    let (Some(id), Some(root)) = (id, root) else {
        return toast;
    };
    toast.action("Undo", move |window, cx| {
        if crate::core(cx).repo().ok().as_ref() != Some(&root) {
            let note =
                Toast::info("It was for the worktree open before this one.").title("Undo not run");
            return show(&ws, note, cx);
        }
        let Some(project) = ws.upgrade().and_then(|ws| ws.read(cx).project().cloned()) else {
            return;
        };
        let state = RepoState::get(&project, cx);
        (undo_action(state.downgrade(), id).run)(window, cx);
    })
}

pub(crate) use gitviber_refs::toast::git_toast as git_error;

/// Shows a toast in `ws`'s window, if it's still open.
pub(crate) fn show(ws: &WeakEntity<Workspace>, toast: Toast, cx: &mut App) {
    if let Some(ws) = ws.upgrade() {
        ws.update(cx, |ws, cx| ws.toast(toast, cx));
    }
}
