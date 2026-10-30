//! A commit-graph for big histories. Without one, each `--topo-order` walk of a history page
//! loads all of history before it prints a line (0.6 s for 200k commits, three walks a page;
//! 0.01 s with one), and only `gc --auto` writes one: a fresh clone has none.

use super::{git_dirs, run, run_text, ConfigList};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// Objects, loose and packed, from which a repo counts as big. Below, a walk takes a few
/// tens of milliseconds and the graph isn't worth a write.
const BIG: u64 = 100_000;

/// Once per repo and launch, off the caller's thread: writes the commit-graph of a big repo
/// that has none. Additive and under git's own lock, so it runs beside any other git.
pub fn ensure_commit_graph(repo: &Path) {
    static TRIED: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();
    let tried = TRIED.get_or_init(Default::default);
    if !tried
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(repo.to_path_buf())
    {
        return;
    }
    let repo = repo.to_path_buf();
    std::thread::spawn(move || {
        if wants_commit_graph(&repo) {
            // Changed-path filters speed up file history and `path:` searches as much.
            let _ = run(
                &repo,
                &["commit-graph", "write", "--reachable", "--changed-paths"],
            );
        }
    });
}

pub(crate) fn wants_commit_graph(repo: &Path) -> bool {
    let Some(dirs) = git_dirs(repo) else {
        return false;
    };
    let info = dirs.common.join("objects/info");
    // git uses no graph in a shallow clone.
    if info.join("commit-graph").exists()
        || info.join("commit-graphs").exists()
        || dirs.common.join("shallow").exists()
    {
        return false;
    }
    let Ok(config) = ConfigList::read(repo) else {
        return false;
    };
    // The user turned graphs off; a partial clone would fetch every missing tree for the filters.
    if config.bool("core.commitgraph") == Some(false)
        || config.value("extensions.partialclone").is_some()
    {
        return false;
    }
    objects(repo).is_some_and(|n| n >= BIG)
}

/// `count-objects -v`'s loose and packed objects together.
fn objects(repo: &Path) -> Option<u64> {
    let out = run_text(repo, &["count-objects", "-v"]).ok()?;
    let field = |name: &str| -> Option<u64> {
        out.lines()
            .find_map(|l| l.strip_prefix(name)?.trim().parse().ok())
    };
    Some(field("count:")? + field("in-pack:")?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_big_repo_without_a_graph_gets_one() {
        let dir = std::env::temp_dir().join(format!("gitviber-graph-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        run(&dir, &["init", "-q", "-b", "main"]).unwrap();
        let commit = ["-c", "user.name=T", "-c", "user.email=t@example.com"];
        run(
            &dir,
            &[&commit[..], &["commit", "-q", "--allow-empty", "-m", "one"]].concat(),
        )
        .unwrap();
        assert_eq!(objects(&dir), Some(2));
        assert!(!wants_commit_graph(&dir), "small");
        // Written, as gc would, it isn't asked for again.
        run(&dir, &["commit-graph", "write", "--reachable"]).unwrap();
        assert!(dir.join(".git/objects/info/commit-graph").exists());
        assert!(!wants_commit_graph(&dir));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
