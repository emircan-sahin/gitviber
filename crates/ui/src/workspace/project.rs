//! The repository a window shows, and its session in core: opening runs `open_repo` (which
//! also starts core's watcher), a worktree switch runs it again on the new root, and the
//! watcher's news comes back as `ProjectEvent::RepoChanged`. It holds no repo data: features
//! fetch their own, following the events.

use std::path::{Path, PathBuf};

use gitviber_core::{Event, watch};
use gpui_kit::{Context, EventEmitter, SharedString, Subscription, Task};

use crate::backend::{self, CoreEvents, has_core};
use crate::memory::Live;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RepoState {
    /// `open_repo` is running.
    Opening,
    /// Open: `main` is the main worktree, which the projects list is keyed by.
    Open { main: PathBuf },
    /// Not a repository, or git failed: what it said.
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
        if from.is_none() {
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
                    Ok(repo) => {
                        this.root = PathBuf::from(&repo.root);
                        this.name = folder_name(&this.root);
                        this.state = RepoState::Open {
                            main: PathBuf::from(repo.main),
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
                        // A failed switch leaves core on the worktree it had.
                        if from.is_none() {
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

    pub fn is_open(&self) -> bool {
        matches!(self.state, RepoState::Open { .. })
    }

    /// The main worktree, once open.
    pub fn main_worktree(&self) -> Option<&Path> {
        match &self.state {
            RepoState::Open { main } => Some(main),
            _ => None,
        }
    }

    /// Goes up with every change on disk, for a cheap "is what I show stale" check.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Refresh (a git action finished, or the user asked): as if the watcher had seen a change.
    pub fn repo_changed(&mut self, cx: &mut Context<Self>) {
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
