//! GitViber's backend: git, GitHub, terminals and coding agents, with no UI in it. The app
//! makes one `Core`, calls `api::*` from background threads, and hears back through `Event`s.
//!
//! The `gitviber` binary dispatches before any window: `askpass::helper()` first (git and ssh
//! run it as their askpass), then the GUI.

pub mod agents;
pub mod api;
pub mod askpass;
pub mod cli;
pub mod clipboard;
pub mod conversations;
pub mod definitions;
pub mod diff;
pub mod errors;
pub mod events;
pub mod external;
pub mod fs;
pub mod git;
pub mod github;
pub mod grep;
pub mod handoff;
pub mod journal;
pub mod launch;
pub mod lfs;
pub mod lines;
pub mod local_socket;
pub mod network;
pub mod notifications;
#[cfg(target_os = "macos")]
mod objc;
pub mod obsidian;
pub mod open_in;
pub mod opened;
pub mod patch;
pub mod paths;
pub mod process;
pub mod procinfo;
pub mod pty;
pub mod revert;
pub mod rewrite;
#[cfg(test)]
mod scenario_tests;
pub mod scratch;
pub mod shell;
pub mod shell_integration;
mod state;
pub mod suggest;
#[cfg(target_os = "linux")]
pub mod trash;
pub mod watch;

pub use events::{Event, EventSink};
pub use state::{Core, Res};
