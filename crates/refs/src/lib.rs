//! GitViber's branches, worktrees and tags, and the top bar's git actions: the branch and
//! worktree pickers with their dialogs, Pull and Push with their menus, undo and redo, the
//! background fetch, network progress and git's credential prompts.
//!
//! What History shares with it lives here too: `RepoState` (status, branches, worktrees and the
//! journal of a project, and `run` for a git action with its toasts), ref names, git errors
//! explained, and the confirmation dialogs.

// The GPUI derive macros name `gpui::…`; the Kit answers to that name.
extern crate gpui_kit as gpui;

mod actions;
pub mod ask;
pub mod askpass;
pub mod branches;
pub mod errors;
pub mod folder;
pub mod hooks;
#[cfg(test)]
mod leaks;
pub mod names;
pub mod net;
pub mod ops;
pub mod points;
pub mod remote_tags;
pub mod repo;
pub mod requests;
pub mod store;
pub mod toast;
pub mod topbar;
pub mod ui;
pub mod worktrees;

pub use names::Branch;
pub use repo::{GitAction, RepoEvent, RepoState};

use std::rc::{Rc, Weak};

use gpui_kit::{App, Entity, Global, SharedString};

/// Registers the top bar's items and the git commands' handlers, and listens for git's
/// credential prompts. Once at startup, after the core is set up.
pub fn init(cx: &mut App) {
    store::import_legacy(cx);
    topbar::register(cx);
    actions::install(cx);
    askpass::install(cx);
    // Core has one repo open: a switch mid-action would carry the rest of it (Sync's push) into
    // the other worktree.
    gitviber_ui::workspace::set_switch_guard(cx, Rc::new(switch_refusal));
}

fn switch_refusal(
    project: &Entity<gitviber_ui::workspace::Project>,
    cx: &App,
) -> Option<SharedString> {
    let busy = held(cx).or_else(|| {
        let state = RepoState::existing(project, cx)?;
        state.read(cx).busy().cloned()
    })?;
    Some(format!("Wait for {busy} to finish.").into())
}

/// Keeps the window's repo where it is while it lives, as an action RepoState runs does: for
/// work run outside it, a commit whose hooks can take minutes and whose push is for its worktree.
pub struct RepoHold {
    _token: Rc<()>,
}

#[derive(Default)]
struct Holds(Vec<(Weak<()>, SharedString)>);
impl Global for Holds {}

pub fn hold_repo(label: impl Into<SharedString>, cx: &mut App) -> RepoHold {
    let token = Rc::new(());
    let holds = cx.default_global::<Holds>();
    holds.0.retain(|(t, _)| t.strong_count() > 0);
    holds.0.push((Rc::downgrade(&token), label.into()));
    RepoHold { _token: token }
}

/// The newest hold still kept.
fn held(cx: &App) -> Option<SharedString> {
    cx.try_global::<Holds>()?
        .0
        .iter()
        .rev()
        .find(|(t, _)| t.strong_count() > 0)
        .map(|(_, label)| label.clone())
}

#[cfg(test)]
mod hold_tests {
    use super::*;
    use gpui_kit::{AppContext as _, TestAppContext};

    #[gpui_kit::test]
    fn a_hold_keeps_the_repo_until_dropped(cx: &mut TestAppContext) {
        let project = cx.update(|cx| {
            cx.new(|cx| gitviber_ui::workspace::Project::new("/nonexistent/gitviber-hold", cx))
        });
        let refusal = |cx: &mut TestAppContext| cx.update(|cx| switch_refusal(&project, cx));
        assert_eq!(refusal(cx), None);
        let hold = cx.update(|cx| hold_repo("Commit", cx));
        assert_eq!(refusal(cx).as_deref(), Some("Wait for Commit to finish."));
        drop(hold);
        assert_eq!(refusal(cx), None);
    }
}
