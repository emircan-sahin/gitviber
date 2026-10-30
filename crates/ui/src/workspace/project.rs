//! The repository a window shows, and its session in core: opening runs `open_repo` (which
//! also starts core's watcher), a worktree switch runs it again on the new root, and the
//! watcher's news comes back as `ProjectEvent::RepoChanged`. It holds no repo data: features
//! fetch their own, following the events.

use std::path::{Path, PathBuf};

use gitviber_core::{Event, watch};
use gpui_kit::{AppContext as _, Context, EventEmitter, SharedString, Subscription, Task};

use crate::backend::{self, CoreEvents, has_core};
use crate::memory::Live;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RepoState {
    /// `open_repo` is running.
    Opening,
    /// Open: `main` is the main worktree, which the projects list is keyed by.
    Open { main: PathBuf },
    /// Open, but outside any git repository: files, terminals and agents work, git's views
    /// offer `git init`. A `.git` appearing (Initialize, or `git init` in a terminal) opens it
    /// again as a repository.
    Folder,
    /// git failed (a refusal for the folder's owner, a broken config): what it said.
    Failed(String),
}

#[derive(Clone, Debug)]
pub enum ProjectEvent {
    /// The repo opened; `root` is its worktree's top level.
    Opened,
    /// Another worktree of it is open now.
    Switched { from: PathBuf, to: PathBuf },
    /// The working tree or git's state changed on disk: refresh what you show.
    RepoChanged(watch::RepoChanged),
    /// Opening or switching failed.
    Failed(String),
}

pub struct Project {
    root: PathBuf,
    name: SharedString,
    state: RepoState,
    revision: u64,
    _open: Option<Task<()>>,
    /// A plain folder looking for the `.git` that would make it a repository.
    _recheck: Option<Task<()>>,
    _events: Option<Subscription>,
    _live: Live,
}

impl EventEmitter<ProjectEvent> for Project {}

fn folder_name(root: &Path) -> SharedString {
    root.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| root.to_string_lossy().into_owned())
        .into()
}

impl Project {
    /// A project at `root`, opened in core when there is one (a test may have none).
    pub fn new(root: impl Into<PathBuf>, cx: &mut Context<Self>) -> Self {
        let root = root.into();
        let events = has_core(cx).then(|| {
            cx.subscribe(
                &CoreEvents::global(cx),
                |this: &mut Self, _, event: &Event, cx| {
                    if let Event::RepoChanged(change) = event
                        && Path::new(&change.root) == this.root
                    {
                        if change.git {
                            this.recheck_folder(cx);
                        }
                        this.revision += 1;
                        cx.emit(ProjectEvent::RepoChanged(change.clone()));
                        cx.notify();
                    }
                },
            )
        });
        let mut project = Self {
            name: folder_name(&root),
            root: root.clone(),
            state: RepoState::Opening,
            revision: 0,
            _open: None,
            _recheck: None,
            _events: events,
            _live: Live::new("Project"),
        };
        project.open(root, None, cx);
        project
    }

    /// Opens `root` in core (after what core is doing); `from`: the worktree this switches away
    /// from. The root changes only once it's open: a failed switch stays where it was.
    fn open(&mut self, root: PathBuf, from: Option<PathBuf>, cx: &mut Context<Self>) {
        if !has_core(cx) {
            return;
        }
        // A folder becoming a repository stays usable as a folder until it is one.
        let upgrading = self.state == RepoState::Folder;
        if from.is_none() && !upgrading {
            self.state = RepoState::Opening;
        }
        let answer = backend::open_repo(cx, root);
        self._open = Some(cx.spawn(async move |this, cx| {
            // None: a newer open or a close overtook this one.
            let Ok(Some(opened)) = answer.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                match opened {
                    // Still no repository: nothing changed for what shows it.
                    Ok(repo) if upgrading && !repo.git => {}
                    Ok(repo) => {
                        this.root = PathBuf::from(&repo.root);
                        this.name = folder_name(&this.root);
                        this.state = if repo.git {
                            RepoState::Open {
                                main: PathBuf::from(repo.main),
                            }
                        } else {
                            RepoState::Folder
                        };
                        cx.emit(match from {
                            Some(from) => ProjectEvent::Switched {
                                from,
                                to: this.root.clone(),
                            },
                            None => ProjectEvent::Opened,
                        });
                    }
                    Err(err) => {
                        // A failed switch leaves core on the worktree it had; a failed
                        // upgrade, on the folder.
                        if from.is_none() && !upgrading {
                            this.state = RepoState::Failed(err.clone());
                        }
                        cx.emit(ProjectEvent::Failed(err));
                    }
                }
                this._open = None;
                cx.notify();
            });
        }));
    }

    /// Opens another worktree of the repo in its place; `Switched` follows once it's open, and
    /// until then `root` is the one still open.
    pub fn switch_worktree(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        let from = self.root.clone();
        self.open(root, Some(from), cx);
        cx.notify();
    }

    /// The worktree's root folder: the top level once open.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The folder's name, as the top bar shows it.
    pub fn name(&self) -> &SharedString {
        &self.name
    }

    pub fn state(&self) -> &RepoState {
        &self.state
    }

    /// Open, as a repository or as a plain folder: its files can be read.
    pub fn is_open(&self) -> bool {
        matches!(self.state, RepoState::Open { .. } | RepoState::Folder)
    }

    /// Open as a git repository: git's views have something to show.
    pub fn is_repo(&self) -> bool {
        matches!(self.state, RepoState::Open { .. })
    }

    /// Open as a plain folder, outside any repository.
    pub fn is_folder(&self) -> bool {
        self.state == RepoState::Folder
    }

    /// Runs `git init` in the folder, then opens it as the repository it is now. The answer is
    /// git's error, for the caller to show.
    pub fn init_repository(&mut self, cx: &mut Context<Self>) -> Task<Result<(), String>> {
        let core = backend::core(cx);
        let root = self.root.to_string_lossy().into_owned();
        let made =
            cx.background_spawn(async move { gitviber_core::api::repo::init_repo(&core, root) });
        cx.spawn(async move |this, cx| {
            made.await?;
            let _ = this.update(cx, |this, cx| this.recheck_folder(cx));
            Ok(())
        })
    }

    /// The main worktree, once open, which keys the project; a plain folder is its own.
    pub fn main_worktree(&self) -> Option<&Path> {
        match &self.state {
            RepoState::Open { main } => Some(main),
            RepoState::Folder => Some(&self.root),
            _ => None,
        }
    }

    /// Goes up with every change on disk, for a cheap "is what I show stale" check.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// A plain folder that has a `.git` now (`git init` in a terminal) is opened again, as the
    /// repository it is. Only once git agrees: each open restarts core's watcher, whose first
    /// news would ask again, so an empty or broken `.git` reopened it forever. Off the UI
    /// thread: a stalled network volume answers the stat in seconds.
    fn recheck_folder(&mut self, cx: &mut Context<Self>) {
        if !self.is_folder() || self._open.is_some() || self._recheck.is_some() {
            return;
        }
        let root = self.root.clone();
        let probe = root.clone();
        let made = cx.background_spawn(async move {
            probe.join(".git").exists() && gitviber_core::git::toplevel(&probe).is_ok()
        });
        self._recheck = Some(cx.spawn(async move |this, cx| {
            let made = made.await;
            let _ = this.update(cx, |this, cx| {
                this._recheck = None;
                if made && this.is_folder() && this._open.is_none() {
                    this.open(root, None, cx);
                }
            });
        }));
    }

    /// Refresh (a git action finished, or the user asked): as if the watcher had seen a change.
    pub fn repo_changed(&mut self, cx: &mut Context<Self>) {
        self.recheck_folder(cx);
        self.revision += 1;
        let change = watch::RepoChanged {
            root: self.root.to_string_lossy().into_owned(),
            worktree: true,
            git: true,
        };
        cx.emit(ProjectEvent::RepoChanged(change));
        cx.notify();
    }
}
