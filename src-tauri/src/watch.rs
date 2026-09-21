//! Watches the open repo and tells the UI to refresh, so changes an agent makes
//! show up without clicking anything.

use notify::{recommended_watcher, RecommendedWatcher, RecursiveMode, Watcher};
use serde::Serialize;
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

enum Kind {
    Worktree,
    Git,
}

fn classify(root: &Path, path: &Path) -> Option<Kind> {
    let rel = path.strip_prefix(root).ok()?;
    let mut parts = rel.components().map(|c| c.as_os_str().to_string_lossy());
    let first = parts.next()?;
    if first == ".git" {
        // Only the files that change what we display; objects/ and *.lock churn constantly.
        return match parts.next().as_deref() {
            Some(
                "HEAD" | "index" | "refs" | "packed-refs" | "MERGE_HEAD" | "CHERRY_PICK_HEAD"
                | "REVERT_HEAD" | "rebase-merge" | "rebase-apply",
            ) => Some(Kind::Git),
            _ => None,
        };
    }
    if rel.components().any(|c| c.as_os_str() == "node_modules") {
        return None;
    }
    Some(Kind::Worktree)
}

/// Git dirs that live outside the worktree (linked worktrees: `.git` is a file and HEAD/index
/// sit under the main repo's .git/worktrees/<name>, refs under its common dir).
fn external_git_dirs(root: &Path) -> Vec<(PathBuf, RecursiveMode)> {
    let dir = |flag: &str| {
        crate::git::run(root, &["rev-parse", flag])
            .ok()
            .map(|o| PathBuf::from(String::from_utf8_lossy(&o).trim()))
    };
    let mut out = vec![];
    if let Some(git_dir) = dir("--absolute-git-dir").filter(|d| !d.starts_with(root)) {
        out.push((git_dir, RecursiveMode::NonRecursive));
    }
    if let Some(common) = dir("--git-common-dir")
        .map(|d| if d.is_absolute() { d } else { root.join(d) })
        .filter(|d| !d.starts_with(root))
    {
        out.push((common.join("refs"), RecursiveMode::Recursive));
    }
    out
}

pub fn start(app: AppHandle, root: PathBuf) -> Result<RecommendedWatcher, String> {
    let (tx, rx) = mpsc::channel::<Kind>();
    let watch_root = root.clone();
    let external = external_git_dirs(&root);
    let external_roots: Vec<PathBuf> = external.iter().map(|(d, _)| d.clone()).collect();
    let mut watcher = recommended_watcher(move |res: notify::Result<notify::Event>| {
        if let Ok(event) = res {
            for p in &event.paths {
                let kind = if external_roots.iter().any(|d| p.starts_with(d)) {
                    // Lock files churn during every git command; the real file follows.
                    p.extension()
                        .is_none_or(|e| e != "lock")
                        .then_some(Kind::Git)
                } else {
                    classify(&watch_root, p)
                };
                if let Some(kind) = kind {
                    let _ = tx.send(kind);
                }
            }
        }
    })
    .map_err(|e| e.to_string())?;
    watcher
        .watch(&root, RecursiveMode::Recursive)
        .map_err(|e| e.to_string())?;
    for (dir, mode) in external {
        // Best effort: a missing refs dir (packed refs only) shouldn't fail opening the repo.
        let _ = watcher.watch(&dir, mode);
    }

    // Debounce: agents write files in bursts; emit once things go quiet for 150ms, but at
    // least every second while something keeps writing (a build, a log file).
    // The thread ends when the watcher (and with it the sender) is dropped.
    std::thread::spawn(move || {
        while let Ok(first) = rx.recv() {
            let started = Instant::now();
            let mut change = RepoChanged::default();
            let mut next = Some(first);
            while let Some(kind) = next {
                match kind {
                    Kind::Worktree => change.worktree = true,
                    Kind::Git => change.git = true,
                }
                if started.elapsed() > Duration::from_secs(1) {
                    break;
                }
                next = rx.recv_timeout(Duration::from_millis(150)).ok();
            }
            let _ = app.emit("repo-changed", change);
        }
    });
    Ok(watcher)
}
