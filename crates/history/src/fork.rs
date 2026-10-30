//! A fork's second History pane: the original's default branch, with what your branch doesn't
//! have yet marked, and the ways to catch up (fetch it, sync the fork on GitHub, merge it in).
//! What GitHub knows of the fork comes through a hook the GitHub crate sets.

use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gitviber_core::{api, git};
use gitviber_refs::RepoState;
use gitviber_refs::toast::Note;
use gitviber_ui::memory::Live;
use gitviber_ui::{ActiveAppTheme, Workspace};
use gpui_kit::component::button::Button;
use gpui_kit::component::{Disableable as _, Sizable as _};
use gpui_kit::{
    App, AppContext as _, Context, Entity, Global, IntoElement, ParentElement as _, Render,
    SharedString, Styled as _, Task, WeakEntity, Window, div, prelude::FluentBuilder as _, rems,
};

use crate::list::{CommitList, ListOptions, Source};

/// What GitHub knows of a fork: the repository origin was forked from, its default branch, and
/// origin's own default branch with whether you may push it (Sync fork).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fork {
    /// owner/name.
    pub original: String,
    pub branch: Option<String>,
    pub fork_branch: Option<String>,
    pub can_sync: bool,
}

/// None while origin isn't a fork, or GitHub isn't connected.
pub type ForkOf = Rc<dyn Fn(&App) -> Option<Fork>>;

#[derive(Default)]
struct ForkHook(Option<ForkOf>);
impl Global for ForkHook {}

/// For the GitHub crate: where the fork's GitHub side comes from.
pub fn set_fork_of(cx: &mut App, fork_of: ForkOf) {
    cx.set_global(ForkHook(Some(fork_of)));
}

pub(crate) fn fork_of(cx: &App) -> Option<Fork> {
    let f = cx.try_global::<ForkHook>()?.0.clone()?;
    f(cx)
}

/// Set again when what GitHub knows may have changed: History panels look whether the fork did.
#[derive(Default)]
pub struct ForkNews;
impl Global for ForkNews {}

pub fn fork_news(cx: &mut App) {
    cx.set_global(ForkNews);
}

/// A fetch of the original is a network round trip: not on every look at History.
const REFETCH_AFTER: Duration = Duration::from_secs(5 * 60);

/// When each original was last fetched, for every window's History.
#[derive(Default)]
pub(crate) struct Fetched(pub(crate) HashMap<String, Instant>);
impl Global for Fetched {}

pub struct OriginalPane {
    repo: Entity<RepoState>,
    workspace: WeakEntity<Workspace>,
    fork: Fork,
    /// None while looking; Some(None) when no remote points at the original yet.
    remote: Option<Option<String>>,
    list: Option<Entity<CommitList>>,
    error: Option<SharedString>,
    loading: bool,
    adding: bool,
    syncing: bool,
    task: Option<Task<()>>,
    _list: Option<gpui_kit::Subscription>,
    _repo: gpui_kit::Subscription,
    _live: Live,
}

impl OriginalPane {
    pub fn new(
        repo: Entity<RepoState>,
        workspace: WeakEntity<Workspace>,
        fork: Fork,
        cx: &mut Context<Self>,
    ) -> Self {
        let stale = cx
            .try_global::<Fetched>()
            .and_then(|f| f.0.get(&fork.original))
            .is_none_or(|at| at.elapsed() > REFETCH_AFTER);
        // The merge button reads the branch and whether a git action runs.
        let _repo = cx.observe(&repo, |_, _, cx| cx.notify());
        let mut pane = OriginalPane {
            repo,
            workspace,
            fork,
            remote: None,
            list: None,
            error: None,
            loading: false,
            adding: false,
            syncing: false,
            task: None,
            _list: None,
            _repo,
            _live: Live::new("OriginalPane"),
        };
        pane.load(stale, cx);
        pane
    }

    /// The same original and branch: only what you may do with it changed.
    pub(crate) fn set_fork(&mut self, fork: Fork, cx: &mut Context<Self>) {
        self.fork = fork;
        cx.notify();
    }

    /// Fetching the original or syncing the fork: its buttons wait.
    pub fn busy(&self) -> bool {
        self.loading || self.syncing
    }

    #[cfg(test)]
    pub(crate) fn remote(&self) -> Option<Option<&str>> {
        self.remote.as_ref().map(Option::as_deref)
    }

    #[cfg(test)]
    pub(crate) fn list(&self) -> Option<&Entity<CommitList>> {
        self.list.as_ref()
    }

    fn branch(&self) -> String {
        self.fork.branch.clone().unwrap_or_else(|| "main".into())
    }

    /// The original's branch as fetched: `refs/remotes/<remote>/<branch>`.
    fn rev(&self) -> Option<String> {
        Some(format!(
            "refs/remotes/{}/{}",
            self.remote.clone()??,
            self.branch()
        ))
    }

    /// Finds the original's remote, fetching it first with `fetch`. Offline or refused, the last
    /// fetched state still shows.
    pub fn load(&mut self, fetch: bool, cx: &mut Context<Self>) {
        let core = gitviber_ui::core(cx);
        let original = self.fork.original.clone();
        self.loading = true;
        cx.notify();
        let find = cx.background_spawn(async move {
            let op = gitviber_refs::net::NetOp::quiet();
            let fetched = fetch.then(|| {
                api::github::gh_original_remote(
                    &core,
                    original.clone(),
                    true,
                    op.id.clone(),
                    op.progress(),
                )
            });
            match fetched {
                Some(Ok(name)) => (Ok(name), None),
                Some(Err(e)) => (
                    api::github::gh_original_remote(
                        &core,
                        original,
                        false,
                        op.id.clone(),
                        op.progress(),
                    ),
                    Some(e),
                ),
                None => (
                    api::github::gh_original_remote(
                        &core,
                        original,
                        false,
                        op.id.clone(),
                        op.progress(),
                    ),
                    None,
                ),
            }
        });
        self.task = Some(cx.spawn(async move |this, cx| {
            let (found, fetch_failed) = find.await;
            let _ = this.update(cx, |this, cx| {
                this.loading = false;
                if fetch {
                    cx.default_global::<Fetched>()
                        .0
                        .insert(this.fork.original.clone(), Instant::now());
                }
                if let Some(e) = fetch_failed {
                    let _ = this.workspace.update(cx, |ws, cx| {
                        ws.toast(
                            gitviber_ui::workspace::Toast::error(e)
                                .title("Could not fetch the original"),
                            cx,
                        )
                    });
                }
                match found {
                    Ok(remote) => {
                        this.error = None;
                        this.remote = Some(remote);
                        this.show_list(cx);
                    }
                    Err(e) => this.error = Some(e.into()),
                }
                cx.notify();
            });
        }));
    }

    fn show_list(&mut self, cx: &mut Context<Self>) {
        let Some(rev) = self.rev() else {
            self.list = None;
            return;
        };
        let options = ListOptions {
            empty: "No commits yet.".into(),
            github: Some(self.fork.original.clone()),
            ..Default::default()
        };
        match &self.list {
            Some(list) => list.update(cx, |l, cx| l.set_source(Source::Rev(rev), options, cx)),
            None => {
                let (repo, ws) = (self.repo.clone(), self.workspace.clone());
                let list = cx.new(|cx| CommitList::new(repo, ws, Source::Rev(rev), options, cx));
                self._list = Some(cx.observe(&list, |_, _, cx| cx.notify()));
                self.list = Some(list);
            }
        }
    }

    /// Adds a remote for the original, then shows its history.
    fn add(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let core = gitviber_ui::core(cx);
        self.adding = true;
        cx.notify();
        let original = self.fork.original.clone();
        let add = cx.background_spawn(async move {
            let op = gitviber_refs::net::NetOp::quiet();
            api::github::gh_add_original_remote(&core, op.id.clone(), op.progress())
        });
        self.task = Some(cx.spawn_in(window, async move |this, cx| {
            let added = add.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.adding = false;
                match added {
                    Ok(name) => {
                        Note::success(format!("Added remote {name}"))
                            .detail(format!("It points at {original}."))
                            .show(window, cx);
                        this.load(false, cx);
                    }
                    Err(e) => Note::error("Could not add the remote")
                        .detail(e)
                        .show(window, cx),
                }
                cx.notify();
            });
        }));
    }

    /// GitHub's "Sync fork": origin's default branch catches up with the original, on GitHub.
    pub fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(branch) = self.fork.fork_branch.clone() else {
            return;
        };
        let original = self.fork.original.clone();
        let action = gitviber_refs::GitAction::new("Sync fork")
            .net()
            .untracked()
            .done(format!("origin/{branch} is up to date with {original}"));
        self.syncing = true;
        cx.notify();
        let done = self.repo.update(cx, |r, cx| {
            r.run(
                action,
                move |core, op| {
                    gitviber_refs::repo::plain(api::github::gh_sync_fork(
                        core,
                        branch,
                        op.id.clone(),
                        op.progress(),
                    ))
                },
                window,
                cx,
            )
        });
        self.task = Some(cx.spawn(async move |this, cx| {
            let synced = done.await.unwrap_or(false);
            let _ = this.update(cx, |this, cx| {
                this.syncing = false;
                if synced {
                    this.load(true, cx);
                }
                cx.notify();
            });
        }));
    }

    fn merge(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(remote) = self.remote.clone().flatten() else {
            return;
        };
        let name = format!("{remote}/{}", self.branch());
        gitviber_refs::ops::merge(&self.repo, name, git::MergeKind::Ff, false, window, cx);
    }
}

impl Render for OriginalPane {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.app_theme().clone();
        let text = |s: SharedString| {
            div()
                .px_4()
                .py_3()
                .text_center()
                .text_size(rems(0.75))
                .text_color(t.muted_foreground)
                .child(s)
        };
        match &self.remote {
            // Still looking: an empty list here would read as "no commits".
            None if self.error.is_none() => return div().into_any_element(),
            None => return text(self.error.clone().unwrap_or_default()).into_any_element(),
            Some(None) => {
                return div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .child(text(
                        format!("No remote points at {} yet.", self.fork.original).into(),
                    ))
                    .child(
                        Button::new("add-upstream")
                            .small()
                            .label(if self.adding {
                                "Adding…"
                            } else {
                                "Add it as upstream"
                            })
                            .disabled(self.adding)
                            .on_click(cx.listener(|this, _, window, cx| this.add(window, cx))),
                    )
                    .into_any_element();
            }
            Some(Some(_)) => {}
        }
        let Some(list) = self.list.clone() else {
            return div().into_any_element();
        };
        let l = list.read(cx);
        // What the original has that this branch doesn't, as far as the loaded pages show.
        let missing = l.commits.iter().filter(|c| c.not_in_head).count();
        let more = missing == l.len() && l.has_more();
        let repo = self.repo.read(cx);
        let into = repo.branch().map(str::to_string);
        let off = repo.locked();
        div()
            .size_full()
            .flex()
            .flex_col()
            .when_some(into.filter(|_| missing > 0), |d, into| {
                d.child(
                    div()
                        .flex()
                        .flex_none()
                        .items_center()
                        .gap_2()
                        .px_3()
                        .py_1p5()
                        .border_b_1()
                        .border_color(t.border)
                        .text_size(rems(0.71875))
                        .text_color(t.muted_foreground)
                        .child(div().flex_1().min_w_0().truncate().child(format!(
                            "{missing}{} commit{} not in {into}",
                            if more { "+" } else { "" },
                            if missing == 1 && !more { "" } else { "s" }
                        )))
                        .child(
                            Button::new("merge-original")
                                .small()
                                .label(format!("Merge into {into}"))
                                .disabled(off)
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.merge(window, cx)),
                                ),
                        ),
                )
            })
            .child(div().flex_1().min_h_0().child(list))
            .into_any_element()
    }
}
