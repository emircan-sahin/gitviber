//! Coming back to the window: the watcher can miss a change (a network volume, dropped events),
//! and nothing would read the repo again until the next one. So a return after a while away asks
//! git, off the UI thread, for a fingerprint of what the views show (status, the refs, the changed
//! files' sizes and times), and only when it differs from the last one do the views read again.

use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use gitviber_core::git;
use gitviber_ui::memory::Live;
use gitviber_ui::workspace::Project;
use gpui_kit::{AppContext as _, Context, Entity, Task};

/// Focus flickers (a dialog, a click on the Dock) don't ask git again.
pub const GAP: Duration = Duration::from_secs(2);
/// The changed files stat'ed for their times; past this many, the status alone says enough.
const STATS: usize = 2000;

pub struct Wake {
    left_at: Option<Instant>,
    /// The fingerprint last seen, and the repo it's of.
    seen: Option<(PathBuf, u64)>,
    _look: Option<Task<()>>,
    _live: Live,
}

impl Wake {
    pub fn new() -> Self {
        Wake {
            left_at: None,
            seen: None,
            _look: None,
            _live: Live::new("Wake"),
        }
    }

    /// The window lost focus.
    pub fn left(&mut self, cx: &mut Context<Self>) {
        self.left_at = Some(cx.background_executor().now());
    }

    /// The window has focus again: after GAP away, `project`'s repo is looked at, and when it
    /// changed since it last was, its views read it again.
    pub fn back(&mut self, project: Option<Entity<Project>>, cx: &mut Context<Self>) {
        let now = cx.background_executor().now();
        let away = self
            .left_at
            .take()
            .map(|at| now.saturating_duration_since(at));
        let Some(project) = project.filter(|_| away.is_some_and(|a| a >= GAP)) else {
            return;
        };
        let root = project.read(cx).root().to_path_buf();
        let at = root.clone();
        let look = cx.background_spawn(async move { fingerprint(&at) });
        let project = project.downgrade();
        self._look = Some(cx.spawn(async move |this, cx| {
            let Some(print) = look.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                this._look = None;
                if this.seen.as_ref() == Some(&(root.clone(), print)) {
                    return;
                }
                this.seen = Some((root.clone(), print));
                // Still the window's repo: a switch meanwhile reads its own.
                if let Some(project) = project.upgrade()
                    && project.read(cx).root() == root
                {
                    project.update(cx, |p, cx| p.repo_changed(cx));
                }
            });
        }));
    }
}

/// What the views of `root` show, hashed: status (files, HEAD, upstream, a merge or rebase),
/// every ref, and each changed file's size and time, so a file edited again counts too.
pub fn fingerprint(root: &Path) -> Option<u64> {
    let status = git::run(
        root,
        &[
            "status",
            "--porcelain=v2",
            "-z",
            "--branch",
            "--untracked-files=all",
        ],
    )
    .ok()?;
    let refs = git::run(root, &["for-each-ref", "--format=%(objectname) %(refname)"]).ok()?;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    status.hash(&mut h);
    refs.hash(&mut h);
    for path in changed_paths(&status).take(STATS) {
        if let Ok(meta) = std::fs::symlink_metadata(root.join(path)) {
            meta.len().hash(&mut h);
            meta.modified().ok().hash(&mut h);
        }
    }
    Some(h.finish())
}

/// The worktree paths `git status --porcelain=v2 -z` lists, a rename by its new one.
fn changed_paths(status: &[u8]) -> impl Iterator<Item = &str> {
    let mut records = status.split(|b| *b == 0);
    std::iter::from_fn(move || {
        loop {
            let record = std::str::from_utf8(records.next()?).unwrap_or_default();
            // The fields before the path: ordinary 8, a rename or copy 9 (its old path is the
            // next record), unmerged 10, untracked 1.
            let fields = match record.as_bytes().first() {
                Some(b'1') => 8,
                Some(b'2') => {
                    records.next();
                    9
                }
                Some(b'u') => 10,
                Some(b'?') => 1,
                _ => continue,
            };
            if let Some(path) = record.splitn(fields + 1, ' ').nth(fields) {
                return Some(path);
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::TestAppContext;

    fn git(dir: &Path, args: &[&str]) {
        let ok = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@t")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@t")
            .status()
            .is_ok_and(|s| s.success());
        assert!(ok, "git {args:?}");
    }

    #[test]
    fn status_records_give_their_worktree_paths() {
        let status = b"# branch.oid abc\0\
1 .M N... 100644 100644 100644 a b src/a b.rs\0\
2 R. N... 100644 100644 100644 a b R100 new.rs\0old.rs\0\
u UU N... 100644 100644 100644 100644 a b c c.rs\0\
? new dir/x.txt\0";
        let paths: Vec<&str> = changed_paths(status).collect();
        assert_eq!(paths, ["src/a b.rs", "new.rs", "c.rs", "new dir/x.txt"]);
    }

    /// The window coming back reads the repo again only when it changed while away: a commit
    /// made elsewhere, a file edited again; a quick focus flicker asks nothing.
    #[gpui_kit::test]
    fn coming_back_rereads_only_what_changed_while_away(cx: &mut TestAppContext) {
        let dir = gitviber_ui::test_support::temp_dir("wake");
        git(&dir, &["init", "-q"]);
        std::fs::write(dir.join("a.txt"), "one\n").unwrap();
        git(&dir, &["add", "."]);
        git(&dir, &["commit", "-qm", "first"]);
        let root = dir.canonicalize().unwrap();
        let project = cx.new(|cx| Project::new(root.clone(), cx));
        let wake = cx.new(|_| Wake::new());
        let revision = |cx: &mut TestAppContext| project.read_with(cx, |p, _| p.revision());
        let away_and_back = |gone: Duration, cx: &mut TestAppContext| {
            wake.update(cx, |w, cx| w.left(cx));
            cx.executor().advance_clock(gone);
            wake.update(cx, |w, cx| w.back(Some(project.clone()), cx));
            cx.run_until_parked();
        };
        // The first return has nothing to compare with: it reads once.
        away_and_back(GAP, cx);
        assert_eq!(revision(cx), 1);
        away_and_back(GAP, cx);
        assert_eq!(revision(cx), 1, "nothing changed");
        std::fs::write(dir.join("a.txt"), "two\n").unwrap();
        away_and_back(Duration::from_millis(500), cx);
        assert_eq!(revision(cx), 1, "a flicker asks nothing");
        away_and_back(GAP, cx);
        assert_eq!(revision(cx), 2, "an edit the watcher missed");
        std::fs::write(dir.join("a.txt"), "three, longer\n").unwrap();
        away_and_back(GAP, cx);
        assert_eq!(revision(cx), 3, "the same file edited again");
        git(&dir, &["commit", "-qam", "second"]);
        away_and_back(GAP, cx);
        assert_eq!(revision(cx), 4, "a commit made elsewhere");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
