//! What other features show of GitHub, through their seams: a branch's pull request in the
//! branch and worktree pickers (gitviber-refs), a commit's checks in History, and the requests
//! this crate makes of others (worktrees, a file, a guided review). The app calls
//! `install_bridges` once, after every feature's init.

use std::rc::Rc;
use std::time::Duration;

use gitviber_core::api;
use gitviber_refs::hooks::{BranchPull, Checks, PullState as RefsPull};
use gitviber_ui::workspace::{GuideOf, Selection, Toast, Workspace};
use gpui_kit::assets::IconName;
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::{
    AnyElement, App, AppContext as _, FontWeight, IntoElement, ParentElement as _, Styled as _,
    Window, div, px, rems,
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
    set_request_handler(cx, Rc::new(handle));
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

/// The main worktree of the window's project, where new worktrees' folder is kept by.
fn main_of(window: &Window, cx: &App) -> Option<String> {
    let ws = Workspace::for_window(window, cx)?;
    let project = ws.read(cx).project()?.read(cx);
    Some(
        project
            .main_worktree()
            .unwrap_or(project.root())
            .to_string_lossy()
            .into_owned(),
    )
}

fn handle(req: GithubRequest, window: &mut Window, cx: &mut App) {
    match req {
        GithubRequest::OfferWorktreeRemoval {
            branch,
            number,
            head_sha,
        } => gitviber_refs::worktrees::offer_worktree_removal(branch, number, head_sha, window, cx),
        GithubRequest::NewWorktreeForPull(source) => {
            let dir =
                main_of(window, cx).and_then(|main| gitviber_refs::ops::worktree_dir(&main, cx));
            crate::ui::pull_view::checkout_worktree(source, dir, window, cx);
        }
        GithubRequest::StartIssue {
            number,
            title,
            url,
            origin,
            branch,
        } => start_issue(number, title, url, origin, branch, window, cx),
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

/// "Start in a worktree…": the issue's branch, made from origin's default branch in a worktree
/// of its own, remembered as the issue's (a PR from it closes the issue), and switched to.
fn start_issue(
    number: u64,
    title: String,
    url: String,
    origin: String,
    branch: String,
    window: &mut Window,
    cx: &mut App,
) {
    let dir = main_of(window, cx).and_then(|main| gitviber_refs::ops::worktree_dir(&main, cx));
    let base = crate::account::account(cx)
        .and_then(|a| a.origin.as_ref()?.default_branch.clone())
        .map_or_else(|| "HEAD".to_string(), |b| format!("origin/{b}"));
    let message = format!(
        "Start #{number} \"{title}\" on a new branch, {branch}, from {base}, in a worktree of its own?"
    );
    crate::ui::confirm(
        "Start in a worktree",
        message,
        "Start",
        window,
        cx,
        move |window, cx| {
            let (core, branch, base, dir, url, origin) = (
                crate::core(cx),
                branch.clone(),
                base.clone(),
                dir.clone(),
                url.clone(),
                origin.clone(),
            );
            let ws = Workspace::for_window(window, cx).map(|w| w.downgrade());
            cx.spawn(async move |cx| {
                let b = branch.clone();
                let made = cx
                    .background_spawn(async move {
                        api::worktrees::add_worktree(&core, b, Some(base), false, dir)
                    })
                    .await;
                cx.update(|cx| {
                    if made.is_ok() {
                        crate::store::Store::update(cx, |s| {
                            s.set_branch_issue(&origin, &branch, &url)
                        });
                    }
                    let Some(ws) = ws.and_then(|w| w.upgrade()) else {
                        return;
                    };
                    ws.update(cx, |ws, cx| match made {
                        Ok(path) => {
                            ws.toast(Toast::success(format!("#{number} starts in {path}")), cx);
                            ws.switch_worktree(path.into(), cx);
                        }
                        Err(e) => {
                            ws.toast(Toast::error(e).title("Could not make the worktree"), cx)
                        }
                    });
                });
            })
            .detach();
        },
    );
}
