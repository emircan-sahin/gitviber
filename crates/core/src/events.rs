//! What core tells the app without being asked: one variant per event the page used to hear.
//! Sent on whichever thread noticed (a watcher's, a socket's, AppKit's main thread), so the
//! sink hands it to the UI thread itself.

use crate::{askpass, obsidian, watch};
use serde::Serialize;
use std::sync::Arc;

pub type EventSink = Arc<dyn Fn(Event) + Send + Sync>;

#[derive(Clone, Debug)]
pub enum Event {
    /// "repo-changed": the open repo's files or its git state changed on disk.
    RepoChanged(watch::RepoChanged),
    /// "vault-changed": the Obsidian vault the explorer shows changed.
    VaultChanged(obsidian::VaultChanged),
    /// "agent-state": a terminal's coding agent changed state (see api::app::pty_agents).
    AgentState(AgentState),
    /// "askpass": git or ssh asks something; answer with api::sync::askpass_answer.
    Askpass(askpass::Prompt),
    /// "askpass-done": that prompt was answered, timed out, or its command stopped waiting.
    AskpassDone(u64),
    /// "notification-click": one of ours was clicked, with the target it was sent with. The app
    /// comes forward too.
    NotificationClick(Option<String>),
    /// "opened": paths were handed to the app from outside; take them with
    /// api::app::take_opened. The app comes forward too.
    Opened,
    /// Worktrees removed, by the folder their browser pages were opened under (canonical, as
    /// api::worktrees::root_key names them): those pages go with them.
    WorktreesRemoved(Vec<String>),
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct AgentState {
    pub id: u32,
    pub state: Option<&'static str>,
}
