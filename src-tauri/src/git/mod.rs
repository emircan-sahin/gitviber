//! Thin wrapper over the `git` CLI. We shell out instead of linking libgit2 so the
//! user's config, hooks, credential helpers and signing all behave exactly like the
//! terminal — GitViber never keeps state of its own inside the repo.

mod blame;
mod branch;
mod cmd;
mod commit;
mod config;
mod content;
mod external_tool;
mod history;
mod index;
mod install;
mod main_back;
mod operation;
mod pair;
mod permalink;
mod remote;
mod repo;
mod review;
mod rewind;
mod stash;
mod status;
mod submodule;
mod sync;
mod tag;
#[cfg(test)]
mod tests;
mod tree;
mod validate;
mod worktree;
mod worktree_include;

pub use blame::*;
pub use branch::*;
pub use cmd::*;
pub use commit::*;
pub use config::*;
pub use content::*;
pub use external_tool::*;
pub use history::*;
pub use index::*;
pub use install::*;
pub use main_back::*;
pub use operation::*;
pub use pair::*;
pub use permalink::*;
pub use remote::*;
pub use repo::*;
pub use review::*;
pub use rewind::*;
pub use stash::*;
pub use status::*;
pub use submodule::*;
pub use sync::*;
pub use tag::*;
pub use tree::*;
pub(crate) use validate::*;
pub use worktree::*;
pub use worktree_include::*;
