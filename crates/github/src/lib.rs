//! GitViber's GitHub side: the PRs and Issues panels, a pull request's and an issue's tab, a PR
//! file's diff with its line comments, checks, and the account they're read with.
//!
//! For other features, data only: `ci` (a commit's verdict, a commit's checks), `branch_pull`
//! (the PR a branch's badge shows), `account` (who's signed in, a fork's original), and the
//! requests this crate can't serve itself (`requests`: worktrees, opening a file).

// The GPUI derive macros name `gpui::…`; the Kit answers to that name.
extern crate gpui_kit as gpui;

pub mod account;
pub mod cache;
pub mod ci;
pub mod logic;
pub mod model;
pub mod requests;
pub mod store;
pub mod ui;

use std::sync::Arc;

use gitviber_core::Core;
use gpui_kit::App;

pub use account::{branch_pull, current_account};
pub use requests::{GithubRequest, set_request_handler};
pub use ui::{
    IssueView, IssuesPanel, PullFileView, PullView, PullsPanel, open_issue, open_pull,
    open_pull_file,
};

/// The app's one Core; the only place the crate reads it.
pub(crate) fn core(cx: &App) -> Arc<Core> {
    gitviber_ui::core(cx)
}

/// Registers the PRs and Issues panels and the pull, pr-file and issue openers; call once at
/// startup, after gitviber_ui::init.
pub fn init(cx: &mut App) {
    store::init(cx);
    cache::init(cx);
    ci::init(cx);
    ui::init(cx);
}

#[cfg(test)]
mod leak_tests;
