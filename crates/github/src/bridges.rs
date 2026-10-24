//! What other features show of GitHub, through their seams: a branch's pull request in the
//! branch and worktree pickers (gitviber-refs), a commit's checks in History, and the requests
//! this crate makes of others (worktrees, a file, a guided review). The app calls
//! `install_bridges` once, after every feature's init.

use std::rc::Rc;
use std::time::Duration;

use gitviber_core::api;
use gitviber_history::Fork;
use gitviber_refs::hooks::{BranchPull, Checks, PullState as RefsPull};
use gitviber_refs::worktrees::{IssueAsk, PullAsk, Remember};
use gitviber_ui::workspace::{GuideOf, Selection, Workspace};
use gpui_kit::assets::IconName;
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::{
    AnyElement, App, FontWeight, Global, IntoElement, ParentElement as _, Styled as _, Window, div,
    px, rems,
};

use crate::cache::Cache;
use crate::logic::checks::{check_note, checks_summary, commit_summary, is_failing, sort_checks};
use crate::model::{CiState, CommitChecks, PullItem, PullState};
use crate::requests::{GithubRequest, set_request_handler};
use crate::ui::common::{ci_badge, theme};

/// A commit's checks are read again this often while one runs, rarely once all are done.
const RUNNING: Duration = Duration::from_secs(15);
const SETTLED: Duration = Duration::from_secs(600);

pub fn install_bridges(cx: &mut App) {
    gitviber_refs::hooks::set_pull_provider(cx, Rc::new(pull_of), Rc::new(open_pull_url));
    gitviber_history::set_commit_checks(cx, Rc::new(commit_checks));
    gitviber_refs::hooks::set_push_access(cx, Rc::new(push_access));
    gitviber_history::set_fork_of(cx, Rc::new(fork_of));
    // Every read lands in the cache: History hears only when the fork itself changed.
    cx.observe_global::<Cache>(|cx| {
        let now = fork_of(cx);
        if cx.try_global::<LastFork>().map(|l| &l.0) != Some(&now) {
            cx.set_global(LastFork(now));
            gitviber_history::fork_news(cx);
        }
    })
    .detach();
    set_request_handler(cx, Rc::new(handle));
}

/// The fork History was last told of.
struct LastFork(Option<Fork>);
impl Global for LastFork {}

/// What History's fork pane needs: origin's original on GitHub and both default branches. Sync
/// fork pushes origin's, so it needs push rights there.
fn fork_of(cx: &App) -> Option<Fork> {
    let a = crate::account::account(cx)?;
    let parent = a.parent.as_ref()?;
    let origin = a.origin.as_ref();
    let fork_branch = origin.and_then(|o| o.default_branch.clone());
    Some(Fork {
        original: parent.full_name(),
        branch: parent.default_branch.clone(),
        can_sync: origin.is_some_and(|o| o.push) && fork_branch.is_some(),
        fork_branch,
    })
}

/// Whether the account may push to `repo` (owner/name): known for origin and its original only.
fn push_access(repo: &str, cx: &App) -> Option<bool> {
    let a = crate::account::account(cx)?;
    [a.origin.as_ref(), a.parent.as_ref()]
        .into_iter()
        .flatten()
        .find(|r| r.full_name().eq_ignore_ascii_case(repo))
        .map(|r| r.push)
}

fn checks(state: Option<CiState>) -> Option<Checks> {
    Some(match state? {
        CiState::Pending => Checks::Pending,
        CiState::Success => Checks::Passed,
        CiState::Failure => Checks::Failed,
    })
}

/// A branch's pull request on origin, from the lists read so far, with its head's checks.
/// `fresh` (the picker is open) can't ask from here: origin's open list is read as the
/// project opens, and the PRs panel keeps the rest.
fn pull_of(branch: &str, head: Option<&str>, _fresh: bool, cx: &App) -> Option<BranchPull> {
    let origin = crate::account::account(cx)?.origin.as_ref()?.full_name();
    let p = crate::account::branch_pull(cx, branch, &origin, head)?;
    Some(BranchPull {
        number: p.number,
        title: p.title.clone(),
        url: p.url.clone(),
        state: match p.state {
            PullState::Open if p.draft => RefsPull::Draft,
            PullState::Open => RefsPull::Open,
            PullState::Closed => RefsPull::Closed,
            PullState::Merged => RefsPull::Merged,
        },
        checks: checks(crate::ci::state(cx, &None, &p.head_sha)),
        head_sha: p.head_sha,
    })
}

/// Opens a PR's tab by its page: the list's copy when one was read, else just its url.
fn open_pull_url(url: &str, window: &mut Window, cx: &mut App) {
    let Some(ws) = Workspace::for_window(window, cx) else {
        return;
    };
    let item = crate::cache::Cache::rows::<PullItem>(cx, "pulls:")
        .into_iter()
        .find(|p| p.url == url)
        .unwrap_or_else(|| PullItem {
            number: url
                .rsplit('/')
                .next()
                .and_then(|n| n.parse().ok())
                .unwrap_or_default(),
            title: String::new(),
            state: PullState::Open,
            draft: false,
            author: String::new(),
            head_ref: String::new(),
            head_sha: String::new(),
            head_repo: None,
            base_ref: String::new(),
            base_sha: String::new(),
            created_at: String::new(),
            updated_at: String::new(),
            url: url.to_string(),
        });
    crate::ui::open_pull(&ws.downgrade(), item, false, window, cx);
}

/// A commit's checks for History: its row's badge (`header` false), asked with the other rows
/// shown, or its tab's verdict and list, read once its rollup says it has some.
fn commit_checks(sha: &str, header: bool, _: &mut Window, cx: &mut App) -> Option<AnyElement> {
    crate::ci::want(cx, &None, sha);
    let state = crate::ci::state(cx, &None, sha);
    let t = theme(cx);
    if !header {
        return ci_badge(gpui_kit::SharedString::from(format!("ci-{sha}")), state, &t);
    }
    let state = state?;
    let key = format!("commit-checks:{sha}");
    let read = Cache::read::<CommitChecks>(cx, &key);
    let age = if state == CiState::Pending {
        RUNNING
    } else {
        SETTLED
    };
    if !read.loading {
        let sha = sha.to_string();
        Cache::load_held(cx, &key, age, move |core| {
            api::github::commit_checks(core, None, sha.clone()).map(|c| CommitChecks::from(&c))
        });
    }
    let data = read.data;
    let summary = commit_summary(data.as_deref(), state.label());
    let mut col = div()
        .flex()
        .flex_col()
        .gap(px(2.))
        .text_size(rems(0.72))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .children(ci_badge(
                    gpui_kit::SharedString::from(format!("ci-head-{sha}")),
                    Some(state),
                    &t,
                ))
                .child(
                    div().font_weight(FontWeight::MEDIUM).child(
                        if data
                            .as_ref()
                            .is_some_and(|d| !d.checks.is_empty() && d.checks_error.is_none())
                        {
                            checks_summary(&data.as_ref().unwrap().checks)
                        } else {
                            summary
                        },
                    ),
                ),
        );
    if let Some(d) = &data {
        let now = gitviber_ui::widgets::now_unix();
        for c in sort_checks(&d.checks) {
            let (name, color) = if c.state == "success" {
                (IconName::Check, t.added)
            } else if c.state == "pending" {
                (IconName::CircleDashed, t.modified)
            } else if is_failing(&c.state) {
                (IconName::X, t.removed)
            } else {
                (IconName::CircleMinus, t.subtle)
            };
            let note = check_note(c, now);
            col = col.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(Icon::new(name).xsmall().text_color(color))
                    .child(div().min_w_0().truncate().child(c.name.clone()))
                    .child(div().min_w_0().truncate().text_color(t.subtle).child(note)),
            );
        }
        if let Some(e) = &d.checks_error {
            col = col.child(
                div()
                    .text_color(t.removed)
                    .child(format!("Could not load all checks: {e}")),
            );
        }
    }
    Some(col.into_any_element())
}

fn handle(req: GithubRequest, window: &mut Window, cx: &mut App) {
    match req {
        GithubRequest::OfferWorktreeRemoval {
            branch,
            number,
            head_sha,
        } => gitviber_refs::worktrees::offer_worktree_removal(branch, number, head_sha, window, cx),
        GithubRequest::NewWorktreeForPull(s) => {
            let ask = PullAsk {
                target: s.target,
                number: s.number,
                head_ref: s.head_ref,
                same_repo: s.same_repo,
                branch: s.branch,
            };
            gitviber_refs::worktrees::open_worktree_for_pull(ask, window, cx);
        }
        GithubRequest::StartIssue {
            number,
            title,
            url,
            origin,
            branch,
        } => {
            // Remembered as the issue's branch: a PR from it then closes the issue.
            let remember: Remember = Rc::new(move |branch, cx| {
                crate::store::Store::update(cx, |s| s.set_branch_issue(&origin, branch, &url))
            });
            let ask = IssueAsk {
                number,
                title,
                branch,
                remember: Some(remember),
            };
            gitviber_refs::worktrees::open_worktree_for_issue(ask, window, cx);
        }
        GithubRequest::OpenFile { path, line: _ } => {
            if let Some(ws) = Workspace::for_window(window, cx) {
                ws.update(cx, |ws, cx| {
                    ws.open(Selection::File { path }, false, window, cx)
                });
            }
        }
        GithubRequest::GuidedReview { pull, target } => {
            let pull = std::sync::Arc::new(pull.to_core());
            gitviber_review::open_guide(GuideOf::Pull { pull, target }, window, cx);
        }
    }
}
