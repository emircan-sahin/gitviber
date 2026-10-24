//! The GitHub account the views read with: one cache entry for all of them, connecting when none
//! is found, the gh account a project uses, and a fork's GitHub side for other features.

use std::sync::Arc;
use std::time::Duration;

use gitviber_core::api::github as api;
use gitviber_core::github::GhAccount;
use gpui_kit::{App, AppContext as _, Task};

use crate::cache::{Cache, Read};
use crate::logic::branch_pulls::pull_for_branch;
use crate::model::{Account, PullItem};
use crate::store::Store;

pub const KEY: &str = "account";
/// It changes only with a new sign-in or a new origin (which resets the cache); rechecked
/// every 10 minutes and on every refresh, a 304 when nothing changed.
const AGE: Duration = Duration::from_secs(600);

pub fn load(cx: &mut App, force: bool) {
    Cache::load(cx, KEY, if force { Duration::ZERO } else { AGE }, |core| {
        api::gh_account(core).map(|a| Account::from(&a))
    });
}

/// The signed-in account as last read; `load` asks for it.
pub fn current_account(cx: &App) -> Read<Account> {
    Cache::read::<Account>(cx, KEY)
}

/// As project `main` opens, before any GitHub call: its calls use the account picked for it,
/// and git's credential store is asked by itself only for who connected through it before.
pub fn apply_project_account(main: &str, cx: &mut App) -> Result<(), String> {
    let core = crate::core(cx);
    let saved = Store::get(cx);
    api::gh_allow_store(&core, saved.allow_store());
    api::gh_use_account(&core, saved.account(main))
}

/// Picks project `main`'s account (None: gh's active one), and reads everything again as it.
pub fn pick_account(main: &str, login: Option<String>, cx: &mut App) -> Result<(), String> {
    api::gh_use_account(&crate::core(cx), login.clone())?;
    Store::update(cx, |s| s.set_account(main, login));
    Cache::refetch_all(cx);
    Ok(())
}

/// gh's accounts on github.com, for picking one; asks GitHub about each token, so only on demand.
pub fn accounts(cx: &App) -> Task<Vec<GhAccount>> {
    let core = crate::core(cx);
    cx.background_spawn(async move { api::gh_accounts(&core).unwrap_or_default() })
}

/// Looks for a GitHub login now, as the user asked to. `store`: in git's credential store too,
/// which the OS may guard with a password dialog; a login found there is remembered, so later
/// sessions look once by themselves. Fails with core's NOT_CONNECTED when there is none.
pub fn connect(store: bool, cx: &mut App) -> Task<Result<(), String>> {
    let core = crate::core(cx);
    cx.spawn(async move |cx| {
        let source = cx
            .background_spawn(async move { api::gh_connect(&core, store) })
            .await?;
        cx.update(|cx| {
            if source == "git" {
                Store::update(cx, |s| s.set_allow_store(true));
                api::gh_allow_store(&crate::core(cx), true);
            }
            Cache::refetch_all(cx);
        });
        Ok(())
    })
}

/// The PR a branch's badge shows, out of every PR list read so far: one whose head is `branch` of
/// `head_repo` (owner/name, origin's for a local branch). With the branch's `tip` (a short id), a
/// closed or merged PR must end there. Its CI: `ci::state(cx, &None, &pull.head_sha)`.
pub fn branch_pull(cx: &App, branch: &str, head_repo: &str, tip: Option<&str>) -> Option<PullItem> {
    let rows = Cache::rows::<PullItem>(cx, "pulls:");
    pull_for_branch(&rows, branch, head_repo, tip).cloned()
}

/// Origin's branches under branch protection (names without "origin/"), for the branch picker.
pub fn protected_branches(cx: &App) -> Task<Result<Vec<String>, String>> {
    let core = crate::core(cx);
    cx.background_spawn(async move { api::gh_protected_branches(&core) })
}

/// GitHub's "Sync fork" for origin's `branch`, then a fetch: "fast-forward", "merge" or "none".
pub fn sync_fork(branch: String, cx: &App) -> Task<Result<String, String>> {
    let core = crate::core(cx);
    cx.background_spawn(
        async move { api::gh_sync_fork(&core, branch, "gh-sync-fork".into(), |_| {}) },
    )
}

/// The remote for a fork's original, owner/name (fetched first if `fetch`); None when there is none.
pub fn original_remote(
    original: String,
    fetch: bool,
    cx: &App,
) -> Task<Result<Option<String>, String>> {
    let core = crate::core(cx);
    cx.background_spawn(async move {
        api::gh_original_remote(&core, original, fetch, "gh-original".into(), |_| {})
    })
}

/// Adds the fork's original as a remote ("upstream") and fetches it; its name.
pub fn add_original_remote(cx: &App) -> Task<Result<String, String>> {
    let core = crate::core(cx);
    cx.background_spawn(async move {
        api::gh_add_original_remote(&core, "gh-add-original".into(), |_| {})
    })
}

/// The account as an Arc, for views that keep it a moment.
pub(crate) fn account(cx: &App) -> Option<Arc<Account>> {
    current_account(cx).data
}
