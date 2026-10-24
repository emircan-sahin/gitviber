//! GitHub: the account, forks, pull requests and issues.

use crate::journal::{Action, Mode};
use crate::state::{Core, Res};
use crate::{git, github, network};
use std::path::PathBuf;

/// The commit to link `path` (or its `lines`) to on GitHub, so the link keeps showing that code.
pub fn github_permalink(
    core: &Core,
    path: String,
    lines: Option<(u32, u32)>,
) -> Res<git::Permalink> {
    core.in_repo(move |r| git::permalink(r, &path, lines))
}

pub fn gh_account(core: &Core) -> Res<github::Account> {
    core.with_github(github::account)
}

/// Looks for a GitHub token now, as the user asked to: gh's, and with `store` git's credential
/// store too (which may show the OS's password dialog). Returns where it came from. Nothing else
/// asks the store, so a screen load never raises that dialog.
pub fn gh_connect(core: &Core, store: bool) -> Res<String> {
    core.with_github(move |gh, r| gh.connect(r, store).map(str::to_string))
}

/// Set as a project opens, from what the user chose before: whether git's credential store may be
/// asked once a session without a click.
pub fn gh_allow_store(core: &Core, allow: bool) {
    core.github.allow_store(allow);
}

/// gh's accounts on github.com; asked for when the repository's settings open.
pub fn gh_accounts(_core: &Core) -> Res<Vec<github::GhAccount>> {
    Ok(github::accounts())
}

/// `login`: the gh account the open repository's GitHub calls use; None for gh's active one. Sent
/// as each repository opens, before its GitHub calls start, and when the pick changes.
pub fn gh_use_account(core: &Core, login: Option<String>) -> Res<()> {
    github::use_account(&core.github, login)
}

pub fn gh_original_remote(
    core: &Core,
    original: String,
    fetch: bool,
    op: String,
    progress: impl Fn(network::Progress) + Send + Sync + 'static,
) -> Res<Option<String>> {
    let net = core.watch_network(op, progress);
    core.in_repo(move |r| github::original_remote(r, &original, fetch, &net))
}

pub fn gh_sync_fork(
    core: &Core,
    branch: String,
    op: String,
    progress: impl Fn(network::Progress) + Send + Sync + 'static,
) -> Res<String> {
    let net = core.watch_network(op, progress);
    core.with_github(move |gh, r| github::sync_fork(gh, r, &branch, &net))
}

pub fn pull_draft(core: &Core, base: String) -> Res<git::PullDraft> {
    core.in_repo(move |r| git::pull_draft(r, &base))
}

pub fn gh_remotes(core: &Core) -> Res<Vec<github::Remote>> {
    core.in_repo(move |r| Ok(github::remotes(r)))
}

pub fn gh_add_original_remote(
    core: &Core,
    op: String,
    progress: impl Fn(network::Progress) + Send + Sync + 'static,
) -> Res<String> {
    let net = core.watch_network(op, progress);
    core.with_github(move |gh, r| github::add_original_remote(gh, r, &net))
}

pub fn gh_protected_branches(core: &Core) -> Res<Vec<String>> {
    core.with_github(github::protected_branches)
}

pub fn pr_list(
    core: &Core,
    target: Option<String>,
    filter: String,
    pages: usize,
    narrow: github::ListFilter,
) -> Res<Vec<github::Pull>> {
    core.with_github(move |gh, r| github::list(gh, r, target.as_deref(), &filter, pages, &narrow))
}

pub fn pr_counts(
    core: &Core,
    target: Option<String>,
    narrow: github::ListFilter,
) -> Res<github::StateCounts> {
    core.with_github(move |gh, r| github::pull_counts(gh, r, target.as_deref(), &narrow))
}

pub fn pr_review_comments(
    core: &Core,
    target: Option<String>,
    number: u64,
) -> Res<Vec<github::ReviewComment>> {
    core.with_github(move |gh, r| github::review_comments(gh, r, target.as_deref(), number))
}

/// A comment on a line of the PR's diff, or a reply in a thread (`reply_to`).
#[allow(clippy::too_many_arguments)]
pub fn pr_comment_line(
    core: &Core,
    target: Option<String>,
    number: u64,
    commit: String,
    path: String,
    line: u64,
    side: String,
    reply_to: Option<u64>,
    body: String,
) -> Res<github::ReviewComment> {
    core.with_github(move |gh, r| {
        github::comment_line(
            gh,
            r,
            target.as_deref(),
            number,
            &commit,
            &path,
            line,
            &side,
            reply_to,
            &body,
        )
    })
}

/// The GitHub account's repositories, for cloning one; works with no repository open.
pub fn gh_own_repos(core: &Core) -> Res<Vec<github::OwnRepo>> {
    let cwd = core
        .repo()
        .ok()
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .unwrap_or_else(std::env::temp_dir);
    github::own_repos(&core.github, &cwd)
}

/// CI's rollup for each commit GitHub has checks on (see github::ci_states).
pub fn ci_states(
    core: &Core,
    target: Option<String>,
    shas: Vec<String>,
) -> Res<std::collections::HashMap<String, String>> {
    core.with_github(move |gh, r| github::ci_states(gh, r, target.as_deref(), &shas))
}

/// Commit `sha`'s check runs and statuses, for its header in the viewer.
pub fn commit_checks(
    core: &Core,
    target: Option<String>,
    sha: String,
) -> Res<github::CommitChecks> {
    core.with_github(move |gh, r| github::commit_checks(gh, r, target.as_deref(), &sha))
}

/// Why check run `id` failed: its output, annotations and job log's tail. Asked for from its row.
pub fn check_failure(core: &Core, target: Option<String>, id: u64) -> Res<github::CheckFailure> {
    core.with_github(move |gh, r| github::check_failure(gh, r, target.as_deref(), id))
}

pub fn pr_detail(core: &Core, target: Option<String>, number: u64) -> Res<github::PullDetail> {
    core.with_github(move |gh, r| github::detail(gh, r, target.as_deref(), number))
}

pub fn pr_attachments(
    core: &Core,
    target: Option<String>,
    number: u64,
) -> Res<std::collections::HashMap<String, String>> {
    core.with_github(move |gh, r| github::attachments(gh, r, target.as_deref(), number))
}

#[allow(clippy::too_many_arguments)]
pub fn pr_files(
    core: &Core,
    target: Option<String>,
    number: u64,
    base_ref: String,
    base_sha: String,
    head_sha: String,
    op: String,
    progress: impl Fn(network::Progress) + Send + Sync + 'static,
) -> Res<github::PullFiles> {
    let net = core.watch_network(op, progress);
    core.with_github(move |gh, r| {
        github::files(
            gh,
            r,
            target.as_deref(),
            number,
            &base_ref,
            &base_sha,
            &head_sha,
            &net,
        )
    })
}

#[allow(clippy::too_many_arguments)]
pub fn pr_create(
    core: &Core,
    target: Option<String>,
    title: String,
    body: String,
    head: String,
    base: String,
    draft: bool,
    maintainer_edits: bool,
) -> Res<github::Pull> {
    core.with_github(move |gh, r| {
        github::create(
            gh,
            r,
            target.as_deref(),
            &title,
            &body,
            &head,
            &base,
            draft,
            maintainer_edits,
        )
    })
}

pub fn pr_merge(core: &Core, target: Option<String>, number: u64, method: String) -> Res<()> {
    core.with_github(move |gh, r| github::merge(gh, r, target.as_deref(), number, &method))
}

pub fn pr_set_open(
    core: &Core,
    target: Option<String>,
    number: u64,
    open: bool,
) -> Res<github::Pull> {
    core.with_github(move |gh, r| github::set_open(gh, r, target.as_deref(), number, open))
}

pub fn pr_review(
    core: &Core,
    target: Option<String>,
    number: u64,
    event: String,
    body: String,
) -> Res<()> {
    core.with_github(move |gh, r| github::review(gh, r, target.as_deref(), number, &event, &body))
}

pub fn issue_list(
    core: &Core,
    target: Option<String>,
    filter: String,
    narrow: github::ListFilter,
) -> Res<Vec<github::Issue>> {
    core.with_github(move |gh, r| github::issues(gh, r, target.as_deref(), &filter, &narrow))
}

pub fn issue_counts(
    core: &Core,
    target: Option<String>,
    narrow: github::ListFilter,
) -> Res<github::StateCounts> {
    core.with_github(move |gh, r| github::issue_counts(gh, r, target.as_deref(), &narrow))
}

pub fn issue_labels(core: &Core, target: Option<String>) -> Res<Vec<github::Label>> {
    core.with_github(move |gh, r| github::issue_labels(gh, r, target.as_deref()))
}

pub fn issue_detail(core: &Core, target: Option<String>, number: u64) -> Res<github::IssueDetail> {
    core.with_github(move |gh, r| github::issue_detail(gh, r, target.as_deref(), number))
}

pub fn issue_create(
    core: &Core,
    target: Option<String>,
    title: String,
    body: String,
) -> Res<github::Issue> {
    core.with_github(move |gh, r| github::issue_create(gh, r, target.as_deref(), &title, &body))
}

pub fn issue_edit(
    core: &Core,
    target: Option<String>,
    number: u64,
    title: String,
    body: String,
) -> Res<github::Issue> {
    core.with_github(move |gh, r| {
        github::issue_edit(gh, r, target.as_deref(), number, &title, &body)
    })
}

pub fn issue_set_open(
    core: &Core,
    target: Option<String>,
    number: u64,
    open: bool,
    reason: String,
) -> Res<github::Issue> {
    core.with_github(move |gh, r| {
        github::issue_set_open(gh, r, target.as_deref(), number, open, &reason)
    })
}

pub fn issue_set_labels(
    core: &Core,
    target: Option<String>,
    number: u64,
    labels: Vec<String>,
) -> Res<Vec<github::Label>> {
    core.with_github(move |gh, r| {
        github::issue_set_labels(gh, r, target.as_deref(), number, &labels)
    })
}

pub fn issue_delete(core: &Core, target: Option<String>, number: u64) -> Res<()> {
    core.with_github(move |gh, r| github::issue_delete(gh, r, target.as_deref(), number))
}

pub fn issue_comment(core: &Core, target: Option<String>, number: u64, body: String) -> Res<()> {
    core.with_github(move |gh, r| github::issue_comment(gh, r, target.as_deref(), number, &body))
}

pub fn pr_checkout(
    core: &Core,
    target: Option<String>,
    number: u64,
    head_ref: String,
    same_repo: bool,
    op: String,
    progress: impl Fn(network::Progress) + Send + Sync + 'static,
) -> Res<()> {
    let net = core.watch_network(op, progress);
    let r = core.repo()?;
    let action = Action::new(format!("Check out PR #{number}"), Mode::Keep);
    core.journal.record(&r, action, |r| {
        let (remote, owner) = github::pr_source(&core.github, r, target.as_deref())?;
        github::checkout(
            r,
            &remote,
            owner.as_deref(),
            number,
            &head_ref,
            same_repo,
            &net,
        )
    })
}

/// `pr_checkout` into a new worktree; returns its path. This worktree's HEAD doesn't move.
#[allow(clippy::too_many_arguments)]
pub fn pr_checkout_worktree(
    core: &Core,
    target: Option<String>,
    number: u64,
    head_ref: String,
    same_repo: bool,
    dir: Option<String>,
    op: String,
    progress: impl Fn(network::Progress) + Send + Sync + 'static,
) -> Res<String> {
    let net = core.watch_network(op, progress);
    let r = core.repo()?;
    let (remote, owner) = github::pr_source(&core.github, &r, target.as_deref())?;
    github::checkout_worktree(
        &r,
        &remote,
        owner.as_deref(),
        number,
        &head_ref,
        same_repo,
        dir.as_deref(),
        &net,
    )
}
