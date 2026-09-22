//! Watches the open repo and tells the UI to refresh, so changes an agent makes
//! show up without clicking anything.

use notify::{recommended_watcher, RecommendedWatcher, RecursiveMode, Watcher};
use serde::Serialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct RepoChanged {
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
        ["HEAD" | "refs" | "packed-refs", ..] => true,
        // Upstreams and remotes: `git branch -u`, `git remote set-url`.
        ["config"] => true,
        ["index" | "MERGE_HEAD" | "CHERRY_PICK_HEAD" | "REVERT_HEAD" | "rebase-merge"
        | "rebase-apply", ..] => own,
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
    if git.is_dir() {
        return true;
    }
    std::fs::read_to_string(&git).ok().is_some_and(|s| {
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
        let dir = |flag: &str| {
            crate::git::run(root, &["rev-parse", flag])
                .ok()
                .map(|o| PathBuf::from(String::from_utf8_lossy(&o).trim()))
        };
        ExternalGitDirs {
            own: dir("--absolute-git-dir").filter(|d| !d.starts_with(root)),
            common: dir("--git-common-dir")
                .map(|d| if d.is_absolute() { d } else { root.join(d) })
                .filter(|d| !d.starts_with(root)),
        }
    }

    pub(crate) fn classify(&self, path: &Path) -> Option<Kind> {
        // Lock files churn during every git command; the real file follows.
        if path.extension().is_some_and(|e| e == "lock") {
            return None;
        }
        // The own dir sits inside the common one's worktrees/, so it goes first.
        if self.own.as_ref().is_some_and(|d| path.starts_with(d)) {
            return Some(Kind::Git);
        }
        let rel = path.strip_prefix(self.common.as_ref()?).ok()?;
        git_file(rel, false).then_some(Kind::Git)
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

pub fn start(app: AppHandle, root: PathBuf) -> Result<RecommendedWatcher, String> {
    let (tx, rx) = mpsc::channel::<(Kind, PathBuf)>();
    let watch_root = root.clone();
    let external = ExternalGitDirs::find(&root);
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
    let mut watcher = recommended_watcher(move |res: notify::Result<notify::Event>| {
        if let Ok(event) = res {
            for p in &event.paths {
                let kind = if p.starts_with(&watch_root) {
                    classify(&watch_root, p)
                } else {
                    external.classify(p)
                };
                if let Some(kind) = kind {
                    let _ = tx.send((kind, p.clone()));
                }
            }
        }
    })
    .map_err(|e| e.to_string())?;
    watcher
        .watch(&root, RecursiveMode::Recursive)
        .map_err(|e| e.to_string())?;
    for (dir, mode) in dirs {
        // Best effort: a git dir that can't be watched shouldn't fail opening the repo.
        let _ = watcher.watch(&dir, mode);
    }

    // Debounce: agents write files in bursts; emit once things go quiet for 150ms, but at
    // least every second while something keeps writing (a build, a log file).
    // The thread ends when the watcher (and with it the sender) is dropped.
    std::thread::spawn(move || {
        while let Ok(first) = rx.recv() {
            let started = Instant::now();
            let mut change = RepoChanged::default();
            let mut touched = HashSet::new();
            let mut next = Some(first);
            while let Some((kind, path)) = next {
                match kind {
                    Kind::Worktree => {
                        touched.insert(path);
                    }
                    Kind::Git => change.git = true,
                }
                if started.elapsed() > Duration::from_secs(1) {
                    break;
                }
                next = rx.recv_timeout(Duration::from_millis(150)).ok();
            }
            change.worktree = not_ignored(&root, &touched);
            if change.worktree || change.git {
                let _ = app.emit("repo-changed", change);
            }
        }
    });
    Ok(watcher)
}
