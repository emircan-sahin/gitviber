//! What the panel's git actions say when they end: a success with Undo when the action went into
//! the journal (⌘Z's history), a failure git explains with what it means and the way out, git's
//! own words under Details.

use gitviber_core::{Core, Res, api};
use gitviber_refs::RepoState;
use gitviber_refs::errors::explain_git_error;
use gitviber_refs::repo::undo_action;
use gitviber_refs::toast::{Fixes, git_failed};
use gitviber_ui::workspace::{Toast, Workspace};
use gpui_kit::{App, SharedString, WeakEntity};
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

/// A failed git action's toast. A failure git's words name (another git holds index.lock, a hook
/// stopped it…) says what it means and offers the way out, from `fixes` or one that always fits
/// (Remove lock); the rest shows git's words as they are.
pub(crate) fn git_error(title: impl Into<SharedString>, e: &str, fixes: &Fixes) -> Toast {
    let title = title.into();
    let toast = match explain_git_error(e) {
        Some(help) => Toast::error(format!("{}. {}", help.title, help.explanation))
            .detail(e.trim().to_string()),
        None => Toast::error(e.trim().to_string()),
    }
    .title(title.clone());
    match git_failed(title, e, fixes).action {
        Some(a) => toast.action(a.label, move |window, cx| (a.run)(window, cx)),
        None => toast,
    }
}

/// Shows a toast in `ws`'s window, if it's still open.
pub(crate) fn show(ws: &WeakEntity<Workspace>, toast: Toast, cx: &mut App) {
    if let Some(ws) = ws.upgrade() {
        ws.update(cx, |ws, cx| ws.toast(toast, cx));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_held_index_lock_is_explained_with_a_way_out() {
        let e = "fatal: Unable to create '/tmp/r/.git/index.lock': File exists.\n\nAnother git process seems to be running in this repository.";
        let t = git_error("Stage failed", e, &Fixes::new());
        assert_eq!(t.title.as_deref(), Some("Stage failed"));
        assert!(
            t.message
                .starts_with("Another git command holds the index lock."),
            "{}",
            t.message
        );
        assert_eq!(t.detail.as_deref(), Some(e));
        assert_eq!(t.action.map(|a| a.label), Some("Remove lock".into()));
        // What git doesn't explain shows as it said it.
        let t = git_error(
            "Stage failed",
            "fatal: pathspec 'x' did not match any files\n",
            &Fixes::new(),
        );
        assert_eq!(
            t.message.as_ref(),
            "fatal: pathspec 'x' did not match any files"
        );
        assert!(t.action.is_none() && t.detail.is_none());
    }
}
