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
pub mod hooks;
#[cfg(test)]
mod leaks;
pub mod names;
pub mod net;
pub mod ops;
pub mod points;
pub mod repo;
pub mod requests;
pub mod store;
pub mod toast;
pub mod topbar;
pub mod ui;
pub mod worktrees;

pub use names::Branch;
pub use repo::{GitAction, RepoEvent, RepoState};

use gpui_kit::App;

/// Registers the top bar's items and the git commands' handlers, and listens for git's
/// credential prompts. Once at startup, after the core is set up.
pub fn init(cx: &mut App) {
    store::import_legacy(cx);
    topbar::register(cx);
    actions::install(cx);
    askpass::install(cx);
    // Core has one repo open: a switch mid-action would carry the rest of it (Sync's push) into
    // the other worktree.
    gitviber_ui::workspace::set_switch_guard(
        cx,
        std::rc::Rc::new(|project, cx| {
            let state = RepoState::existing(project, cx)?;
            let busy = state.read(cx).busy()?;
            Some(format!("Wait for {busy} to finish.").into())
        }),
    );
}
