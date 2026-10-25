//! The tags the remote has, for the menus that say whether a tag is there: the top bar's push
//! menu and History's tag submenus. One lookup at a time for both, its answer reused for a few
//! seconds and forgotten once a push or a tag's deletion ends.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use futures::FutureExt as _;
use futures::channel::oneshot;
use futures::future::Shared;
use gitviber_core::api;
use gpui_kit::{App, AppContext as _, Global, Task};

use crate::net::NetOp;

/// The remote tags are pushed to, and the tags it has.
pub type There = Result<(String, Vec<String>), String>;

const FRESH: Duration = Duration::from_secs(10);
/// A menu gives up on a remote that doesn't answer; git would wait minutes.
const GIVE_UP: Duration = Duration::from_secs(6);

struct Lookup {
    id: u64,
    root: PathBuf,
    /// When the answer came; None while it's asked.
    at: Option<Instant>,
    there: Shared<Task<There>>,
}

#[derive(Default)]
struct RemoteTags {
    next: u64,
    last: Option<Lookup>,
}

impl Global for RemoteTags {}

/// The remote's tags for the worktree `root`: the lookup running or a fresh answer, else a new
/// lookup. A failure isn't kept.
pub fn lookup(root: &Path, cx: &mut App) -> Shared<Task<There>> {
    if let Some(l) = cx.try_global::<RemoteTags>().and_then(|g| g.last.as_ref())
        && l.root == root
        && l.at.is_none_or(|at| at.elapsed() < FRESH)
    {
        return l.there.clone();
    }
    let core = gitviber_ui::core(cx);
    let op = NetOp::quiet().pinned(root.to_path_buf());
    let (c2, op_id) = (core.clone(), op.id.clone());
    let timer = cx.background_executor().timer(GIVE_UP);
    cx.background_spawn(async move {
        timer.await;
        api::sync::cancel_network(&c2, op_id);
    })
    .detach();
    let ask = cx.background_spawn(async move {
        op.still_in(&core)?;
        api::branches::remote_tags(&core, op.id.clone(), op.progress())
            .map(|r| (r.remote, r.names))
            .map_err(|e| {
                if e == gitviber_core::network::CANCELLED {
                    "The remote didn't answer in time.".to_string()
                } else {
                    e
                }
            })
    });
    let tags = cx.default_global::<RemoteTags>();
    tags.next += 1;
    let id = tags.next;
    let there = cx
        .spawn(async move |cx| {
            let there = ask.await;
            let ok = there.is_ok();
            cx.update(|cx| {
                let tags = cx.default_global::<RemoteTags>();
                if let Some(l) = tags.last.as_mut().filter(|l| l.id == id) {
                    if ok {
                        l.at = Some(Instant::now());
                    } else {
                        tags.last = None;
                    }
                }
            });
            there
        })
        .shared();
    cx.default_global::<RemoteTags>().last = Some(Lookup {
        id,
        root: root.to_path_buf(),
        at: None,
        there: there.clone(),
    });
    there
}

/// The remote's tags changed, or may have: the next menu asks again. A lookup still running
/// answers the menus waiting on it but isn't kept.
pub fn forget(cx: &mut App) {
    if cx.has_global::<RemoteTags>() {
        cx.global_mut::<RemoteTags>().last = None;
    }
}

/// `forget` once the push or deletion `done` ends, through or not: a refused push of several
/// tags may have sent some.
pub fn forget_after(done: oneshot::Receiver<bool>, cx: &mut App) {
    cx.spawn(async move |cx| {
        let _ = done.await;
        cx.update(forget);
    })
    .detach();
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use gitviber_core::api;
    use gpui_kit::TestAppContext;

    use super::*;

    fn git(dir: &Path, args: &[&str]) {
        // Not the owner's config: no signing, no hooks.
        let ok = Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "T")
            .env("GIT_AUTHOR_EMAIL", "t@t")
            .env("GIT_COMMITTER_NAME", "T")
            .env("GIT_COMMITTER_EMAIL", "t@t")
            .status()
            .unwrap()
            .success();
        assert!(ok, "git {args:?}");
    }

    #[gpui_kit::test]
    fn both_menus_share_one_lookup_until_a_push_forgets_it(cx: &mut TestAppContext) {
        let dir = gitviber_ui::test_support::temp_dir("refs-tags");
        let (bare, work) = (dir.join("remote.git"), dir.join("work"));
        std::fs::create_dir_all(&work).unwrap();
        git(&dir, &["init", "-q", "--bare", "remote.git"]);
        git(&work, &["init", "-q"]);
        git(&work, &["commit", "-q", "--allow-empty", "-m", "one"]);
        git(&work, &["tag", "v1"]);
        git(&work, &["remote", "add", "origin", bare.to_str().unwrap()]);
        git(&work, &["push", "-q", "origin", "v1"]);
        let work = work.canonicalize().unwrap();
        // SAFETY: the tests that read the config dir all point it at a temporary folder.
        unsafe { std::env::set_var("GITVIBER_CONFIG_DIR", &dir) };
        cx.update(|cx| {
            gitviber_ui::init(cx);
            gitviber_ui::install_core(cx);
        });
        let core = cx.update(|cx| gitviber_ui::core(cx));
        api::repo::open_repo(&core, work.to_string_lossy().into_owned()).unwrap();

        let first = cx.update(|cx| lookup(&work, cx));
        let second = cx.update(|cx| lookup(&work, cx));
        assert!(first.ptr_eq(&second), "asked while one runs");
        cx.run_until_parked();
        let there = futures::executor::block_on(first.clone()).unwrap();
        assert_eq!(there.1, ["v1"]);
        assert!(
            cx.update(|cx| lookup(&work, cx)).ptr_eq(&first),
            "a fresh answer"
        );

        cx.update(forget);
        let after = cx.update(|cx| lookup(&work, cx));
        assert!(!after.ptr_eq(&first));
        // One forgotten while it runs answers its menus but isn't kept.
        cx.update(forget);
        cx.run_until_parked();
        assert!(futures::executor::block_on(after.clone()).is_ok());
        assert!(!cx.update(|cx| lookup(&work, cx)).ptr_eq(&after));

        // Another worktree asks for itself, and a failure is asked again.
        let other = dir.join("elsewhere");
        let failed = cx.update(|cx| lookup(&other, cx));
        cx.run_until_parked();
        assert!(futures::executor::block_on(failed.clone()).is_err());
        assert!(!cx.update(|cx| lookup(&other, cx)).ptr_eq(&failed));
        cx.run_until_parked();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
