//! Seams to features this crate doesn't depend on: History sets the Compare screen's opener and
//! where a commit is shown, GitHub what a branch's pull request is, the terminal how many shells
//! and which agents run in a folder. Each is one global callback, set at startup.

use std::rc::Rc;

use gitviber_ui::widgets::Look;
use gpui_kit::{App, Global, Window};

use crate::points::ComparePoint;

/// Opens the Compare screen on these sides; a side left out is the screen's default.
pub type OpenCompare =
    Rc<dyn Fn(Option<ComparePoint>, Option<ComparePoint>, &mut Window, &mut App)>;

/// Shows commit `sha` in History: selected, scrolled to, pages read until it's listed.
pub type ShowCommit = Rc<dyn Fn(String, &mut Window, &mut App)>;

#[derive(Default)]
struct Hooks {
    open_compare: Option<OpenCompare>,
    show_commit: Option<ShowCommit>,
}

impl Global for Hooks {}

pub fn set_open_compare(cx: &mut App, open: OpenCompare) {
    cx.default_global::<Hooks>().open_compare = Some(open);
}

pub fn set_show_commit(cx: &mut App, show: ShowCommit) {
    cx.default_global::<Hooks>().show_commit = Some(show);
}

/// Shows a commit in History, when History is there; false when it isn't.
pub fn show_commit(sha: String, window: &mut Window, cx: &mut App) -> bool {
    match cx.try_global::<Hooks>().and_then(|h| h.show_commit.clone()) {
        Some(show) => {
            show(sha, window, cx);
            true
        }
        None => false,
    }
}

/// Opens the Compare screen, when History is there to show it.
pub fn open_compare(
    base: Option<ComparePoint>,
    head: Option<ComparePoint>,
    window: &mut Window,
    cx: &mut App,
) {
    if let Some(open) = cx
        .try_global::<Hooks>()
        .and_then(|h| h.open_compare.clone())
    {
        open(base, head, window, cx);
    }
}

/// How many terminals run inside a folder: the terminal crate sets it. Clean up and removing a
/// worktree hold back for one.
pub type TerminalsIn = Rc<dyn Fn(&str, &App) -> usize>;

/// A branch's pull request as the pickers show it: the GitHub crate sets the provider.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BranchPull {
    pub number: u64,
    pub title: String,
    pub url: String,
    pub state: PullState,
    pub head_sha: String,
    /// Its head's checks, when read.
    pub checks: Option<Checks>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PullState {
    Open,
    Draft,
    Closed,
    Merged,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Checks {
    Pending,
    Passed,
    Failed,
}

/// The pull request of `branch` (checked out at `head`, a short id), from what GitHub's lists
/// cached; `fresh`: the picker is open, so the lists may be read again.
pub type PullOf = Rc<dyn Fn(&str, Option<&str>, bool, &App) -> Option<BranchPull>>;
/// Opens a pull request in the app, by its URL.
pub type OpenPull = Rc<dyn Fn(&str, &mut Window, &mut App)>;

/// The most urgent look of the agents in a folder's terminals (any window's): working, done,
/// asking, news not looked at.
pub type AgentsIn = Rc<dyn Fn(&str, &App) -> Option<Look>>;

#[derive(Default)]
struct Providers {
    terminals_in: Option<TerminalsIn>,
    agents_in: Option<AgentsIn>,
    pull_of: Option<PullOf>,
    open_pull: Option<OpenPull>,
}

impl Global for Providers {}

pub fn set_terminals_in(cx: &mut App, f: TerminalsIn) {
    cx.default_global::<Providers>().terminals_in = Some(f);
}

pub fn terminals_in(path: &str, cx: &App) -> usize {
    cx.try_global::<Providers>()
        .and_then(|p| p.terminals_in.clone())
        .map_or(0, |f| f(path, cx))
}

pub fn set_agents_in(cx: &mut App, f: AgentsIn) {
    cx.default_global::<Providers>().agents_in = Some(f);
}

pub fn agents_in(path: &str, cx: &App) -> Option<Look> {
    cx.try_global::<Providers>()
        .and_then(|p| p.agents_in.clone())
        .and_then(|f| f(path, cx))
}

/// Set again whenever an agent's look changed, by the terminal: what shows looks observes it
/// (`cx.observe_global::<AgentLooks>`), as nothing polls.
#[derive(Default)]
pub struct AgentLooks;

impl Global for AgentLooks {}

pub fn agents_changed(cx: &mut App) {
    cx.set_global(AgentLooks);
}

/// The most urgent of several looks: it asked, news not looked at, working, done.
pub fn most_urgent(looks: impl IntoIterator<Item = Option<Look>>) -> Option<Look> {
    let rank = |l: &Look| match l {
        Look::Needs => 0,
        Look::Unread => 1,
        Look::Working => 2,
        Look::Done => 3,
    };
    looks.into_iter().flatten().min_by_key(rank)
}

/// News for the user: what another worktree's mark on the chip tells.
pub fn is_news(look: Option<Look>) -> bool {
    matches!(look, Some(Look::Needs | Look::Unread))
}

/// For a tooltip: the color is never the only telling.
pub fn look_label(look: Look) -> &'static str {
    match look {
        Look::Needs => "needs you",
        Look::Unread => "not viewed",
        Look::Working => "agent working",
        Look::Done => "agent finished",
    }
}

pub fn set_pull_provider(cx: &mut App, pull_of: PullOf, open_pull: OpenPull) {
    let p = cx.default_global::<Providers>();
    p.pull_of = Some(pull_of);
    p.open_pull = Some(open_pull);
}

pub fn pull_of(branch: &str, head: Option<&str>, fresh: bool, cx: &App) -> Option<BranchPull> {
    cx.try_global::<Providers>()
        .and_then(|p| p.pull_of.clone())
        .and_then(|f| f(branch, head, fresh, cx))
}

pub fn open_pull(url: &str, window: &mut Window, cx: &mut App) {
    if let Some(f) = cx
        .try_global::<Providers>()
        .and_then(|p| p.open_pull.clone())
    {
        f(url, window, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_question_outranks_news_and_news_outranks_work() {
        assert_eq!(
            most_urgent([Some(Look::Done), None, Some(Look::Working)]),
            Some(Look::Working)
        );
        assert_eq!(
            most_urgent([Some(Look::Working), Some(Look::Unread), Some(Look::Needs)]),
            Some(Look::Needs)
        );
        assert_eq!(most_urgent([None, None]), None);
        assert!(is_news(Some(Look::Unread)) && is_news(Some(Look::Needs)));
        assert!(!is_news(Some(Look::Working)) && !is_news(Some(Look::Done)) && !is_news(None));
    }

    #[gpui_kit::test]
    fn what_shows_looks_hears_each_change(cx: &mut gpui_kit::TestAppContext) {
        let heard = Rc::new(std::cell::Cell::new(0));
        let count = heard.clone();
        let _sub =
            cx.update(|cx| cx.observe_global::<AgentLooks>(move |_| count.set(count.get() + 1)));
        cx.update(|cx| {
            set_agents_in(
                cx,
                Rc::new(|dir, _| (dir == "/w/a").then_some(Look::Working)),
            );
            agents_changed(cx);
        });
        cx.update(agents_changed);
        assert_eq!(heard.get(), 2);
        cx.update(|cx| {
            assert_eq!(agents_in("/w/a", cx), Some(Look::Working));
            assert_eq!(agents_in("/w/b", cx), None);
        });
    }
}
