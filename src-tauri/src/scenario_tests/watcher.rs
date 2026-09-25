//! What the file watcher reacts to and what it ignores.

use super::*;

#[test]
fn watcher_ignores_nested_worktrees() {
    use crate::watch::{classify, Kind};
    let sb = Sandbox::new("wtwatch");
    let r = repo_with_worktrees(&sb);
    let agent = r.join(".claude/worktrees/agent");
    for p in ["a.txt", "src/deep/x.rs", ".git"] {
        assert_eq!(classify(&r, &agent.join(p)), None, "{p}");
    }
    // The worktree folder appearing or going away does change this repo's status.
    assert_eq!(classify(&r, &agent), Some(Kind::Worktree));
    assert_eq!(
        classify(&r, &r.join(".claude/notes.md")),
        Some(Kind::Worktree)
    );
    assert_eq!(classify(&r, &r.join(".git/HEAD")), Some(Kind::Git));
    // Opened as the repo itself, the worktree's own files count.
    assert_eq!(classify(&agent, &agent.join("a.txt")), Some(Kind::Worktree));
}

#[test]
fn watcher_sees_config_and_other_worktrees() {
    use crate::watch::{classify, ExternalGitDirs, Kind};
    let sb = Sandbox::new("wtgitwatch");
    let r = repo_with_worktrees(&sb);
    let git = |p: &str| classify(&r, &r.join(p));
    // `git remote set-url`, `git worktree add`, a checkout in another worktree.
    for p in [
        ".git/config",
        ".git/packed-refs",
        ".git/worktrees/agent",
        ".git/worktrees/agent/HEAD",
    ] {
        assert_eq!(git(p), Some(Kind::Git), "{p}");
    }
    // Another worktree's staging and reflog are its own.
    for p in [
        ".git/worktrees/agent/index",
        ".git/worktrees/agent/logs/HEAD",
        ".git/config.lock",
        ".git/objects/ab/cdef",
    ] {
        assert_eq!(git(p), None, "{p}");
    }

    // The agent worktree's own window: its git dir is under the main repo's .git.
    let ext = ExternalGitDirs::find(&r.join(".claude/worktrees/agent"));
    let common = ext.common.clone().expect("common dir outside the worktree");
    let own = ext.own.clone().expect("own git dir outside the worktree");
    assert!(own.starts_with(&common));
    for p in [
        own.join("index"),
        own.join("HEAD"),
        common.join("packed-refs"),
    ] {
        assert_eq!(ext.classify(&p), Some(Kind::Git), "{}", p.display());
    }
    assert_eq!(ext.classify(&common.join("config")), Some(Kind::Git));
    assert_eq!(ext.classify(&common.join("refs/heads/x")), Some(Kind::Git));
    // The main worktree's index and another worktree's aren't this window's.
    for p in [
        common.join("index"),
        common.join("MERGE_HEAD"),
        common.join("worktrees/det/index"),
        common.join("refs/heads/x.lock"),
    ] {
        assert_eq!(ext.classify(&p), None, "{}", p.display());
    }
}

#[test]
fn watcher_skips_ignored_build_output() {
    use crate::watch::not_ignored;
    use std::collections::HashSet;
    let sb = Sandbox::new("watchignore");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, ".gitignore", "target/\n*.log\n", "ignore");
    fs::create_dir_all(r.join("target/debug")).unwrap();
    // Tracked despite the pattern: still a change worth showing.
    fs::write(r.join("keep.log"), "k\n").unwrap();
    run(&r, &["add", "-f", "keep.log"]).unwrap();
    let r = r.canonicalize().unwrap();
    let set = |ps: &[&str]| -> HashSet<PathBuf> { ps.iter().map(|p| r.join(p)).collect() };
    assert!(!not_ignored(&r, &set(&["target/debug/out.o", "build.log"])));
    assert!(not_ignored(
        &r,
        &set(&["target/debug/out.o", "src/main.rs"])
    ));
    assert!(not_ignored(&r, &set(&["keep.log"])));
    assert!(!not_ignored(&r, &HashSet::new()));
}

#[test]
fn watcher_still_follows_submodules() {
    use crate::watch::{classify, Kind};
    let sb = Sandbox::new("wtsubwatch");
    let r = repo_with_submodule(&sb);
    assert!(r.join("sub/.git").is_file());
    assert_eq!(
        classify(&r, &r.join("sub/l.txt")),
        Some(Kind::Worktree),
        "edits in a submodule are part of this repo's status"
    );
    // A plain nested repo (a .git dir) is still ignored.
    init(&r.join("vendor/x"));
    assert_eq!(classify(&r, &r.join("vendor/x/f.txt")), None);
}
