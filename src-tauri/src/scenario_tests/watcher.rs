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
    // `git remote set-url`, `git worktree add`, a checkout in another worktree, a ref in a
    // reftable repo, a new rule in info/exclude.
    for p in [
        ".git/config",
        ".git/packed-refs",
        ".git/reftable/tables.list",
        ".git/info/exclude",
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
    assert_eq!(
        ext.classify(&common.join("reftable/0x01.ref")),
        Some(Kind::Git)
    );
    assert_eq!(ext.classify(&common.join("info/exclude")), Some(Kind::Git));
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
fn watcher_survives_a_flood_of_ignored_paths() {
    use crate::watch::not_ignored;
    use std::collections::HashSet;
    let sb = Sandbox::new("watchflood");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, ".gitignore", "target/\n", "ignore");
    let r = r.canonicalize().unwrap();
    // A `cargo clean` deletes this many build files at once; git answers ~1 MB while it
    // still reads, which used to hang the watcher thread for good.
    let paths: HashSet<PathBuf> = (0..12_000)
        .map(|i| r.join(format!("target/debug/deps/libsome_crate-{i:016x}.rmeta")))
        .collect();
    let (tx, rx) = std::sync::mpsc::channel();
    let root = r.clone();
    std::thread::spawn(move || tx.send(not_ignored(&root, &paths)));
    let got = rx.recv_timeout(std::time::Duration::from_secs(30));
    assert_eq!(got, Ok(false), "all ignored, and the call returns");
}

#[test]
fn watcher_still_follows_submodules() {
    use crate::watch::{classify, ExternalGitDirs, Kind};
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

    // The submodule opened on its own: its git dir is .git/modules/sub, own and common at once.
    let ext = ExternalGitDirs::find(&r.join("sub"));
    let dir = ext.own.clone().expect("git dir outside the submodule");
    assert_eq!(ext.common.as_ref(), Some(&dir));
    for p in ["HEAD", "index", "refs/heads/main", "MERGE_HEAD"] {
        assert_eq!(ext.classify(&dir.join(p)), Some(Kind::Git), "{p}");
    }
    for p in ["objects/ab/cdef", "logs/HEAD", "FETCH_HEAD"] {
        assert_eq!(ext.classify(&dir.join(p)), None, "{p}");
    }
}

#[test]
fn watcher_reloads_after_dropped_events() {
    use crate::watch::{route, ExternalGitDirs, Kind};
    use notify::event::{Event, EventKind, Flag};
    let sb = Sandbox::new("watchrescan");
    let r = sb.path("r");
    init(&r);
    let r = r.canonicalize().unwrap();
    let ext = ExternalGitDirs::find(&r);
    // On its own, .git is not a file the window shows.
    let plain = Event::new(EventKind::Any).add_path(r.join(".git"));
    assert!(route(&r, &ext, &plain).is_empty());
    // FSEvents' MustScanSubDirs on .git: events were dropped, so everything reloads.
    let rescan = plain.set_flag(Flag::Rescan);
    let got: Vec<Kind> = route(&r, &ext, &rescan).into_iter().map(|c| c.0).collect();
    assert_eq!(got, [Kind::Git]);
    // inotify's queue overflow carries no path at all.
    let overflow = Event::new(EventKind::Other).set_flag(Flag::Rescan);
    assert_eq!(route(&r, &ext, &overflow).len(), 1);
}
