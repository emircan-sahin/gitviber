//! Watches the open repo and tells the UI to refresh, so changes an agent makes
//! show up without clicking anything.

use crate::errors;
use crate::events::{Event, EventSink};
use notify::event::{AccessKind, AccessMode};
use notify::{recommended_watcher, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::Serialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

/// A running watcher. FSEvents starts and stops with round trips to fseventsd, which took up to
/// 23 s while builds kept it busy: neither opening a repo nor dropping this waits for them, so
/// closing a repo or quitting doesn't either. Its events stop at once.
pub struct Watching {
    /// What it watches.
    root: PathBuf,
    /// Empty while `start` is still bringing the watcher up.
    watcher: Arc<Mutex<Option<RecommendedWatcher>>>,
    /// What the watcher's thread checks before it tells the UI anything.
    live: Arc<AtomicBool>,
    /// The repo was shown (`opened`).
    shown: Arc<AtomicBool>,
}

impl Watching {
    pub(crate) fn new(root: PathBuf, watcher: RecommendedWatcher, live: Arc<AtomicBool>) -> Self {
        Watching {
            root,
            watcher: Arc::new(Mutex::new(Some(watcher))),
            live,
            shown: Arc::default(),
        }
    }

    /// open_repo answered: what the UI reads from now on is newer than anything a watcher still
    /// coming up has missed, so one that's up only later catches up with a refresh.
    pub(crate) fn opened(&self) {
        self.shown.store(true, Ordering::SeqCst);
    }

    pub(crate) fn root(&self) -> &Path {
        &self.root
    }
}

impl Drop for Watching {
    fn drop(&mut self) {
        self.live.store(false, Ordering::Relaxed);
        let taken = lock(&self.watcher).take();
        if let Some(watcher) = taken {
            // Without a thread to spare it stops here, as it always did.
            let _ = std::thread::Builder::new()
                .name("watcher stop".into())
                .spawn(move || drop(watcher));
        }
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct RepoChanged {
    /// The repo this watcher is for, as open_repo returned it.
    pub root: String,
    /// Files in the working tree changed.
    pub worktree: bool,
    /// HEAD, refs or the index changed (commit, checkout, stage from the terminal...).
    pub git: bool,
}

#[derive(Debug, PartialEq)]
pub(crate) enum Kind {
    Worktree,
    Git,
}

pub(crate) fn classify(root: &Path, path: &Path) -> Option<Kind> {
    let rel = path.strip_prefix(root).ok()?;
    if let Ok(inside) = rel.strip_prefix(".git") {
        return git_file(inside, true).then_some(Kind::Git);
    }
    if rel.components().any(|c| c.as_os_str() == "node_modules") || in_nested_repo(root, path) {
        return None;
    }
    Some(Kind::Worktree)
}

/// Whether a path inside a git dir changes what the window shows; objects/ and *.lock churn
/// constantly. `own`: the dir is this worktree's, so its index and a stopped merge or rebase
/// count too; in a linked worktree's window, the main worktree's don't.
pub(crate) fn git_file(rel: &Path, own: bool) -> bool {
    let parts: Vec<_> = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect();
    let parts: Vec<&str> = parts.iter().map(|p| p.as_ref()).collect();
    match parts.as_slice() {
        // reftable/: where a `--ref-format=reftable` repo keeps every ref.
        ["HEAD" | "refs" | "packed-refs" | "reftable", ..] => true,
        // Upstreams and remotes: `git branch -u`, `git remote set-url`. Ignore rules too.
        ["config"] | ["info", "exclude"] => true,
        // BISECT_LOG and sequencer/: a bisect or a paused multi-pick started in a terminal.
        ["index" | "MERGE_HEAD" | "CHERRY_PICK_HEAD" | "REVERT_HEAD" | "BISECT_LOG"
        | "rebase-merge" | "rebase-apply" | "sequencer", ..] => own,
        // Another worktree added or removed, or switched to a branch it now holds. Its index
        // and logs are that worktree's business.
        ["worktrees", _] | ["worktrees", _, "HEAD"] => true,
        _ => false,
    }
}

/// Agents work in worktrees under the repo (.claude/worktrees/*); their writes and git
/// churn would otherwise refresh this window nonstop. Git shows such a folder as one
/// untracked entry, and creating or removing it is still an event on the folder itself.
fn in_nested_repo(root: &Path, path: &Path) -> bool {
    path.ancestors()
        .skip(1)
        .take_while(|dir| *dir != root && dir.starts_with(root))
        .any(is_untracked_repo_root)
}

/// A nested repo has a `.git` dir, a linked worktree a `.git` file pointing at
/// `<repo>/.git/worktrees/<name>`. A submodule's points into `.git/modules/`: it is part of
/// this repo, and edits in it are ours to show.
fn is_untracked_repo_root(dir: &Path) -> bool {
    let git = dir.join(".git");
    let Ok(meta) = std::fs::metadata(&git) else {
        return false;
    };
    if meta.is_dir() {
        return true;
    }
    // Only a file is read: opening a FIFO named .git waited for a writer forever, in FSEvents'
    // callback, and the watcher heard nothing more.
    meta.is_file()
        && std::fs::read_to_string(&git).ok().is_some_and(|s| {
            s.strip_prefix("gitdir:")
                .and_then(|d| {
                    Path::new(d.trim())
                        .parent()?
                        .file_name()
                        .map(|n| n == "worktrees")
                })
                .unwrap_or(false)
        })
}

/// Git dirs that live outside the worktree (linked worktrees: `.git` is a file and HEAD/index
/// sit under the main repo's .git/worktrees/<name>, refs and config in its common dir).
pub(crate) struct ExternalGitDirs {
    pub(crate) own: Option<PathBuf>,
    pub(crate) common: Option<PathBuf>,
}

impl ExternalGitDirs {
    pub(crate) fn find(root: &Path) -> Self {
        let dirs = crate::git::git_dirs(root);
        ExternalGitDirs {
            own: dirs
                .as_ref()
                .map(|d| d.dir.clone())
                .filter(|d| !d.starts_with(root)),
            common: dirs.map(|d| d.common).filter(|d| !d.starts_with(root)),
        }
    }

    pub(crate) fn classify(&self, path: &Path) -> Option<Kind> {
        // Lock files churn during every git command; the real file follows.
        if path.extension().is_some_and(|e| e == "lock") {
            return None;
        }
        // A submodule's (or any separate) git dir is both the own and the common one. As in a
        // plain .git, only what git_file names counts: its objects and logs churn.
        let own = self.own == self.common;
        // A linked worktree's own dir sits inside the common one's worktrees/, so it goes first.
        // Only what git_file names: each status with core.fsmonitor writes and deletes a cookie
        // under fsmonitor--daemon/ here, and taking it for a change refreshed nonstop.
        if let Some(rel) = self
            .own
            .as_ref()
            .filter(|_| !own)
            .and_then(|d| path.strip_prefix(d).ok())
        {
            let counts = rel.as_os_str().is_empty() || git_file(rel, true);
            return counts.then_some(Kind::Git);
        }
        let rel = path.strip_prefix(self.common.as_ref()?).ok()?;
        git_file(rel, own).then_some(Kind::Git)
    }
}

/// Worktree paths that count: not ignored by .gitignore. Build output (`target/`, `dist/`)
/// is written about every second during a build, and each event would cost a full status
/// refresh and a re-highlight of the open diff. Tracked files never count as ignored.
pub(crate) fn not_ignored(root: &Path, paths: &HashSet<PathBuf>) -> bool {
    let rels: Vec<String> = paths
        .iter()
        .filter_map(|p| p.strip_prefix(root).ok())
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    if rels.is_empty() {
        return false;
    }
    let ignored: HashSet<String> = crate::git::ignored(root, &rels).into_iter().collect();
    rels.iter().any(|r| !ignored.contains(r))
}

/// A file opened, or closed without a write: inotify reports both (FSEvents neither). Taken for
/// a change, each read made on hearing one was heard again, a refresh loop on Linux.
pub(crate) fn only_read(event: &notify::Event) -> bool {
    matches!(event.kind, EventKind::Access(kind) if kind != AccessKind::Close(AccessMode::Write))
}

/// What one watcher event means for the window, path by path.
pub(crate) fn route(
    root: &Path,
    external: &ExternalGitDirs,
    event: &notify::Event,
) -> Vec<(Kind, PathBuf)> {
    if only_read(event) {
        return vec![];
    }
    // Events were dropped (FSEvents' MustScanSubDirs, inotify's overflow): for the repo or a git
    // dir itself, which classify to nothing, anything may have changed. A folder in the worktree
    // counts as written to, so ignored build output and nested worktrees still stay out.
    let rescan = event.need_rescan();
    if rescan && event.paths.is_empty() {
        return vec![(Kind::Git, root.to_path_buf())];
    }
    let whole = |p: &Path| {
        p == root
            || p == root.join(".git")
            || [&external.own, &external.common]
                .iter()
                .any(|d| d.as_deref() == Some(p))
    };
    event
        .paths
        .iter()
        .filter_map(|p| {
            let kind = if rescan && whole(p) {
                Some(Kind::Git)
            } else if p.starts_with(root) {
                classify(root, p)
            } else {
                external.classify(p)
            };
            Some((kind?, p.clone()))
        })
        .collect()
}

/// Hands `emit` the events that come in a burst (agents write files in bursts): once things go
/// quiet for 150 ms, but at least every second while something keeps writing (a build, a log
/// file). Returns when every sender is gone.
pub(crate) fn debounce<T>(rx: mpsc::Receiver<T>, mut emit: impl FnMut(Vec<T>)) {
    while let Ok(first) = rx.recv() {
        let started = Instant::now();
        let mut batch = vec![first];
        while started.elapsed() <= Duration::from_secs(1) {
            match rx.recv_timeout(Duration::from_millis(150)) {
                Ok(next) => batch.push(next),
                Err(_) => break,
            }
        }
        emit(batch);
    }
}

/// Watches the repo at `root` from a thread of its own: FSEvents' start waited 1–10 s on a busy
/// fseventsd, and opening the repo doesn't wait for it. What changed before it was up is caught
/// up with one refresh (`Watching::opened`).
pub fn start(events: EventSink, root: PathBuf) -> Watching {
    let watching = Watching {
        root: root.clone(),
        watcher: Arc::default(),
        live: Arc::new(AtomicBool::new(true)),
        shown: Arc::default(),
    };
    let slot = watching.watcher.clone();
    let live = watching.live.clone();
    let shown = watching.shown.clone();
    let told = events.clone();
    let spawned = std::thread::Builder::new()
        .name("repo watcher".into())
        .spawn(move || {
            #[cfg(test)]
            held::wait(&root);
            // Closed already: a quick A→B→C doesn't start B's.
            if !live.load(Ordering::Relaxed) {
                return;
            }
            let (tx, rx) = mpsc::channel();
            let watcher = match watcher_for(&root, tx) {
                Ok(w) => w,
                Err(mut e) => {
                    // The event names the repo; notify's " about [path]" would say it again.
                    e.paths.clear();
                    if live.load(Ordering::Relaxed) {
                        failed(&events, &root, e.to_string());
                    }
                    return;
                }
            };
            {
                let mut kept = lock(&slot);
                if !live.load(Ordering::Relaxed) {
                    // Closed while it came up: it stops here, on this thread.
                    return;
                }
                *kept = Some(watcher);
            }
            if shown.load(Ordering::SeqCst) && live.load(Ordering::Relaxed) {
                events(Event::RepoChanged(RepoChanged {
                    root: root.to_string_lossy().into_owned(),
                    worktree: true,
                    git: true,
                }));
            }
            // Ends when the watcher (and with it the sender) is gone.
            relay(rx, &root, &live, &events);
        });
    if let Err(e) = spawned {
        failed(&told, &watching.root, e.to_string());
    }
    watching
}

/// The repo stays open, without live updates: the app says so and can `api::repo::watch_again`.
fn failed(events: &EventSink, root: &Path, error: String) {
    errors::write("repo watcher", &format!("{}: {error}", root.display()));
    events(Event::RepoWatchFailed {
        root: root.to_string_lossy().into_owned(),
        error,
    });
}

/// A watcher on the worktree and the git dirs outside it, started once for all of them: FSEvents
/// restarts its stream for every `watch`.
fn watcher_for(
    root: &Path,
    tx: mpsc::Sender<(Kind, PathBuf)>,
) -> notify::Result<RecommendedWatcher> {
    let external = ExternalGitDirs::find(root);
    let dirs: Vec<(PathBuf, RecursiveMode)> = [
        external
            .own
            .clone()
            .map(|d| (d, RecursiveMode::NonRecursive)),
        external
            .common
            .clone()
            .map(|d| (d, RecursiveMode::Recursive)),
    ]
    .into_iter()
    .flatten()
    .collect();
    let watch_root = root.to_path_buf();
    let mut watcher = recommended_watcher(move |res: notify::Result<notify::Event>| {
        if let Ok(event) = res {
            for change in route(&watch_root, &external, &event) {
                let _ = tx.send(change);
            }
        }
    })?;
    let mut paths = watcher.paths_mut();
    paths.add(root, RecursiveMode::Recursive)?;
    for (dir, mode) in dirs {
        // Best effort: a git dir that can't be watched shouldn't leave the worktree unwatched.
        let _ = paths.add(&dir, mode);
    }
    paths.commit()?;
    Ok(watcher)
}

/// Tells the UI what the watcher heard, a burst at a time, while the repo is open.
fn relay(rx: mpsc::Receiver<(Kind, PathBuf)>, root: &Path, live: &AtomicBool, events: &EventSink) {
    debounce(rx, |batch| {
        let mut change = RepoChanged {
            root: root.to_string_lossy().into_owned(),
            ..Default::default()
        };
        let mut touched = HashSet::new();
        for (kind, path) in batch {
            match kind {
                Kind::Worktree => {
                    touched.insert(path);
                }
                Kind::Git => change.git = true,
            }
        }
        // A git change reloads the status anyway; no need to ask git which files count.
        change.worktree = if change.git {
            !touched.is_empty()
        } else {
            not_ignored(root, &touched)
        };
        if (change.worktree || change.git) && live.load(Ordering::Relaxed) {
            events(Event::RepoChanged(change));
        }
    })
}

/// How long a test waits for a watcher to come up. fseventsd starts streams one at a time, and
/// one asked for by a binary in target/debug/deps (557K stale .o files there) took 1–25 s; the
/// same binary elsewhere, 1 ms.
#[cfg(test)]
pub(crate) const UP: Duration = Duration::from_secs(240);

/// Holds a repo watcher's start the way a busy fseventsd did, which a test can't make happen.
#[cfg(test)]
pub(crate) mod held {
    use std::path::{Path, PathBuf};
    use std::sync::{mpsc, Mutex};

    static HELD: Mutex<Vec<(PathBuf, mpsc::Receiver<()>)>> = Mutex::new(Vec::new());

    /// `root`'s next watcher comes up once the returned sender is dropped.
    pub(crate) fn hold(root: &Path) -> mpsc::Sender<()> {
        let (tx, rx) = mpsc::channel();
        super::lock(&HELD).push((root.to_path_buf(), rx));
        tx
    }

    pub(super) fn wait(root: &Path) {
        let rx = {
            let mut held = super::lock(&HELD);
            let at = held.iter().position(|(r, _)| r == root);
            at.map(|i| held.swap_remove(i).1)
        };
        if let Some(rx) = rx {
            let _ = rx.recv();
        }
    }
}
