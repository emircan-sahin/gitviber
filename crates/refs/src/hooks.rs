//! Seams to features this crate doesn't depend on: History sets the Compare screen's opener,
//! GitHub will set what a branch's pull request is. Each is one global callback, set at startup.

use std::rc::Rc;

use gpui_kit::{App, Global, Window};

use crate::points::ComparePoint;

/// Opens the Compare screen on these sides; a side left out is the screen's default.
pub type OpenCompare =
    Rc<dyn Fn(Option<ComparePoint>, Option<ComparePoint>, &mut Window, &mut App)>;

#[derive(Default)]
struct Hooks {
    open_compare: Option<OpenCompare>,
}

impl Global for Hooks {}

pub fn set_open_compare(cx: &mut App, open: OpenCompare) {
    cx.default_global::<Hooks>().open_compare = Some(open);
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

#[derive(Default)]
struct Providers {
    terminals_in: Option<TerminalsIn>,
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
