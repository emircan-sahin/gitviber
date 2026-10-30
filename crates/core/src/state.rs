//! What every command shares: the open repo, the locks around it, and where events go. The
//! helpers block: the UI calls core from a background thread, never its own.

use crate::events::{Event, EventSink};
use crate::journal::{self, Action};
use crate::{
    agents, askpass, errors, git, github, network, notifications, opened, pty, shell, watch,
};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub struct Core {
    pub(crate) repo: Mutex<Option<PathBuf>>,
    /// The open repo's git dirs, read once it's asked: a refresh needs them, and open_repo or
    /// close_repo starts over.
    pub(crate) dirs: Mutex<Option<(PathBuf, git::GitDirs)>>,
    pub(crate) watcher: Mutex<Option<watch::Watching>>,
    /// The Obsidian vault the explorer shows, watched while it does.
    pub(crate) vault_watcher: Mutex<Option<watch::Watching>>,
    pub github: github::Session,
    pub ptys: pty::Ptys,
    /// The agents running in terminals, and their state files watched.
    pub agents: agents::Agents,
    pub network: network::Running,
    /// Held by commands that write the index: two `git add`s at once fail on index.lock.
    pub(crate) index: Arc<Mutex<()>>,
    pub(crate) journal: Arc<journal::Journal>,
    /// `git --version`, checked once; the page asks again after the user installs git.
    pub(crate) git: Mutex<Option<git::GitInfo>>,
    pub(crate) suggest: crate::suggest::Suggester,
    /// Paths handed to the app from outside until it takes them (opened.rs).
    pub(crate) opened: opened::Opened,
    events: EventSink,
}

pub type Res<T> = Result<T, String>;

// The app shares one Core among its background threads.
const _: () = {
    const fn shared<T: Send + Sync>() {}
    shared::<Core>();
};

impl Core {
    /// Nothing starts here: no thread, no socket, no global hook. See `start`.
    pub fn new(events: EventSink) -> Self {
        Core {
            repo: Mutex::default(),
            dirs: Mutex::default(),
            watcher: Mutex::default(),
            vault_watcher: Mutex::default(),
            github: github::Session::default(),
            ptys: pty::Ptys::default(),
            agents: agents::Agents::default(),
            network: network::Running::default(),
            index: Arc::default(),
            journal: Arc::default(),
            git: Mutex::default(),
            suggest: crate::suggest::Suggester::default(),
            opened: opened::Opened::default(),
            events,
        }
    }

    /// Once at launch, before the first window: the login shell's PATH starts resolving, errors
    /// and panics go to the log, git's prompts and notification clicks reach the sink, and the
    /// paths this launch was given (`gitviber <path>`) are queued.
    pub fn start(&self) {
        shell::resolve_in_background();
        if let Some(dir) = crate::paths::log_dir() {
            errors::init(dir);
        }
        askpass::serve(self.events.clone());
        notifications::setup(self.events.clone());
        let args: Vec<std::ffi::OsString> = std::env::args_os().collect();
        if let Ok(cwd) = std::env::current_dir() {
            opened::push(self, opened::from_args(&args, &cwd));
        }
    }

    pub fn emit(&self, event: Event) {
        (self.events)(event)
    }

    pub(crate) fn events(&self) -> EventSink {
        self.events.clone()
    }

    pub fn repo(&self) -> Res<PathBuf> {
        self.repo
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .ok_or_else(|| "No repository is open.".to_string())
    }

    /// `f` with the open repo.
    pub fn in_repo<T>(&self, f: impl FnOnce(&Path) -> Res<T>) -> Res<T> {
        f(&self.repo()?)
    }

    /// `in_repo` for the reads a refresh does: each git call in `f` times out (git::reading).
    pub fn read_repo<T>(&self, f: impl FnOnce(&Path) -> Res<T>) -> Res<T> {
        let r = self.repo()?;
        git::reading(|| f(&r))
    }

    /// `repo`'s git dirs, read once while it's the open repo. None when git can't say.
    pub(crate) fn git_dirs(&self, repo: &Path) -> Option<git::GitDirs> {
        let lock = || self.dirs.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((root, dirs)) = lock().as_ref() {
            if root == repo {
                return Some(dirs.clone());
            }
        }
        let dirs = git::git_dirs(repo)?;
        // Only for the repo still open: one that changed meanwhile reads its own.
        if self.repo().is_ok_and(|open| open == repo) {
            *lock() = Some((repo.to_path_buf(), dirs.clone()));
        }
        Some(dirs)
    }

    /// `f` with the GitHub session and the open repo.
    pub fn with_github<T>(&self, f: impl FnOnce(&github::Session, &Path) -> Res<T>) -> Res<T> {
        f(&self.github, &self.repo()?)
    }

    /// Runs an index-writing git command one at a time. An agent or the terminal can hold
    /// index.lock for a moment too, so that failure is retried once.
    pub fn indexed<T>(&self, f: impl Fn(&Path) -> Res<T>) -> Res<T> {
        with_index_lock(&self.index, &self.repo()?, f)
    }

    /// `indexed` without the retry, for commands that do several steps: a stash push or pop that
    /// hit index.lock partway has already done part of its work, and running it again would
    /// stash or apply twice. The user retries once they see why it failed.
    pub fn indexed_once<T>(&self, f: impl FnOnce(&Path) -> Res<T>) -> Res<T> {
        let r = self.repo()?;
        let _held = self.index.lock().unwrap_or_else(|e| e.into_inner());
        f(&r)
    }

    /// Runs a command that may move HEAD or local branches, recording what it moved for undo.
    /// A switch or pull rewrites files as fast as git writes the index, so the stat cache is
    /// refreshed after it (git::refresh_stat_cache).
    pub fn journaled<T>(&self, action: Action, f: impl FnOnce(&Path) -> Res<T>) -> Res<T> {
        let repo = self.repo()?;
        let out = self.journal.record(&repo, action, f)?;
        self.refresh_stat_cache(&repo);
        Ok(out)
    }

    /// git::refresh_stat_cache, skipped while one of our own commands writes the index: a
    /// commit's hooks may hold it for minutes, and this is only a speed-up.
    fn refresh_stat_cache(&self, repo: &Path) {
        if let Ok(_held) = self.index.try_lock() {
            git::refresh_stat_cache(repo);
        }
    }

    /// Registers a network command under the page's id, streaming its progress to `progress`.
    pub fn watch_network(
        &self,
        op: String,
        progress: impl Fn(network::Progress) + Send + Sync + 'static,
    ) -> network::Net {
        self.network.start(op, progress)
    }
}

pub(crate) fn with_index_lock<T>(
    lock: &Mutex<()>,
    repo: &Path,
    f: impl Fn(&Path) -> Res<T>,
) -> Res<T> {
    let _held = lock.lock().unwrap_or_else(|e| e.into_inner());
    match f(repo) {
        Err(e) if e.contains("index.lock") => {
            std::thread::sleep(std::time::Duration::from_millis(400));
            f(repo)
        }
        done => done,
    }
}

/// "Commit" and the like read better with the commit's short id.
pub(crate) fn short(sha: &str) -> &str {
    &sha[..sha.len().min(7)]
}

/// The first `max` of `items` for a message: "a, b, c and 4 more".
pub(crate) fn some_of(items: &[&str], max: usize) -> String {
    let shown = items
        .iter()
        .take(max)
        .copied()
        .collect::<Vec<_>>()
        .join(", ");
    match items.len().saturating_sub(max) {
        0 => shown,
        more => format!("{shown} and {more} more"),
    }
}
