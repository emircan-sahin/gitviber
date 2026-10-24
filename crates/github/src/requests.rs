//! What the GitHub views ask of other features: a worktree for a PR or an issue, the offer to
//! remove one after a merge, a file at a line, a guided review. The app sets one handler; until
//! it does, a PR's worktree is made by core directly and the rest say they're not here yet.

use std::rc::Rc;

use gitviber_ui::workspace::{Toast, Workspace};
use gpui_kit::{App, Global, Window};

use crate::model::{PullItem, Target};

/// Where Checkout puts a PR (see `logic` pull_source).
#[derive(Clone, Debug, PartialEq)]
pub struct PullSource {
    /// owner/name of the repository the PR is on.
    pub target: String,
    pub number: u64,
    pub head_ref: String,
    /// The PR's branch lives on origin: checked out under its own name.
    pub same_repo: bool,
    /// The local branch it lands on.
    pub branch: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum GithubRequest {
    /// A PR was merged: offer to remove the worktree its branch is checked out in, if any.
    OfferWorktreeRemoval {
        branch: String,
        number: u64,
        head_sha: String,
    },
    /// "Check out in new worktree…": the worktree dialog for this PR.
    NewWorktreeForPull(PullSource),
    /// "Start in a worktree…": a new branch for the issue (its proposed name), in its own worktree.
    /// Whoever makes it calls `store` set_branch_issue so a PR from it closes the issue.
    StartIssue {
        number: u64,
        title: String,
        url: String,
        origin: String,
        branch: String,
    },
    /// A check's annotation: the file at a line.
    OpenFile { path: String, line: Option<u32> },
    /// A guided review of a PR.
    GuidedReview { pull: PullItem, target: Target },
}

pub type RequestHandler = Rc<dyn Fn(GithubRequest, &mut Window, &mut App)>;

struct Handler(RequestHandler);
impl Global for Handler {}

/// Set by the app once the features that serve these are in.
pub fn set_request_handler(cx: &mut App, handler: RequestHandler) {
    cx.set_global(Handler(handler));
}

pub(crate) fn request(req: GithubRequest, window: &mut Window, cx: &mut App) {
    if let Some(handler) = cx.try_global::<Handler>().map(|h| h.0.clone()) {
        return handler(req, window, cx);
    }
    let what = match &req {
        GithubRequest::OfferWorktreeRemoval { .. } => return,
        GithubRequest::NewWorktreeForPull(source) => {
            return crate::ui::pull_view::checkout_worktree(source.clone(), None, window, cx);
        }
        GithubRequest::StartIssue { .. } => "Starting an issue in a worktree",
        GithubRequest::OpenFile { .. } => "Opening the file",
        GithubRequest::GuidedReview { .. } => "Guided review",
    };
    toast(
        Toast::info(format!(
            "{what} comes with another part of GitViber, not here yet."
        )),
        window,
        cx,
    );
}

/// A toast in this window.
pub(crate) fn toast(toast: Toast, window: &mut Window, cx: &mut App) {
    match Workspace::for_window(window, cx) {
        Some(ws) => ws.update(cx, |ws, cx| ws.toast(toast, cx)),
        None => toast.show(window, cx),
    }
}
