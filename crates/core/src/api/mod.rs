//! The commands the page called through src/lib/api/, one file per part of the app, as plain
//! blocking functions: same names, arguments and results.

pub mod app;
pub mod branches;
pub mod changes;
pub mod files;
pub mod github;
pub mod history;
pub mod journal;
pub mod obsidian;
pub mod patch;
pub mod repo;
pub mod stash;
pub mod sync;
pub mod worktrees;
