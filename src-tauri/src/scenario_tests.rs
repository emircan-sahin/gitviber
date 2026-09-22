//! End-to-end git scenarios against real repositories and a local bare "remote".

use crate::{fs as vfs, git::*};
use std::fs;
use std::path::{Path, PathBuf};

struct Sandbox(PathBuf);

impl Sandbox {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("gitviber-scn-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Sandbox(dir)
    }
    fn path(&self, p: &str) -> PathBuf {
        self.0.join(p)
    }
    /// A bare origin with one commit on main, plus `n` clones.
    fn remote_with_clones(&self, n: usize) -> Vec<PathBuf> {
        let origin = self.path("origin.git");
        run(
            &self.0,
            &[
                "init",
                "-q",
                "--bare",
                "-b",
                "main",
                origin.to_str().unwrap(),
            ],
        )
        .unwrap();
        let seed = self.clone_of("seed");
        write_commit(&seed, "a.txt", "one\ntwo\nthree\n", "base");
        run(&seed, &["push", "-q", "-u", "origin", "main"]).unwrap();
        (0..n).map(|i| self.clone_of(&format!("c{i}"))).collect()
    }
    fn clone_of(&self, name: &str) -> PathBuf {
        let dir = self.path(name);
        run(
            &self.0,
            &[
                "clone",
                "-q",
                self.path("origin.git").to_str().unwrap(),
                dir.to_str().unwrap(),
            ],
        )
        .unwrap();
        identity(&dir);
        dir
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn identity(repo: &Path) {
    for (k, v) in [
        ("user.name", "T"),
        ("user.email", "t@example.com"),
        ("commit.gpgsign", "false"),
        ("pull.rebase", "false"),
    ] {
        run(repo, &["config", k, v]).unwrap();
    }
}

fn init(dir: &Path) {
    fs::create_dir_all(dir).unwrap();
    run(dir, &["init", "-q", "-b", "main"]).unwrap();
    identity(dir);
}

fn write_commit(repo: &Path, path: &str, content: &str, msg: &str) {
    if let Some(parent) = Path::new(path).parent() {
        fs::create_dir_all(repo.join(parent)).unwrap();
    }
    fs::write(repo.join(path), content).unwrap();
    stage(repo, &[path.into()]).unwrap();
    commit(repo, msg, false).unwrap();
}

#[test]
fn pull_modes_on_diverged_branches() {
    let sb = Sandbox::new("pull");
    let c = sb.remote_with_clones(2);
    let (a, b) = (&c[0], &c[1]);
    write_commit(a, "a.txt", "one\ntwo\nthree\nfour\n", "a appends");
    run(a, &["push", "-q"]).unwrap();
    write_commit(b, "b.txt", "b\n", "b adds a file");
    fetch(b).unwrap();
    let st = status(b).unwrap();
    assert_eq!((st.ahead, st.behind), (1, 1));

    // Fast-forward only must refuse, and must not leave an operation behind.
    assert!(pull(b, "ff").is_err());
    assert!(operation(b).is_none());
    // A clean merge finishes without stopping.
    assert!(!pull(b, "merge").unwrap());
    assert_eq!(log(b, None, 0, 1).unwrap()[0].parents.len(), 2);
    assert_eq!(
        fs::read_to_string(b.join("a.txt")).unwrap(),
        "one\ntwo\nthree\nfour\n"
    );
}

/// After an amend the plain push is rejected as non-fast-forward (the UI keys on those words);
/// the lease push replaces the old commit, but not over someone else's unfetched push.
#[test]
fn force_push_with_lease_after_amend() {
    let sb = Sandbox::new("lease");
    let c = sb.remote_with_clones(2);
    let (a, b) = (&c[0], &c[1]);
    write_commit(a, "a.txt", "mine\n", "mine");
    push(a, false, None).unwrap();
    commit(a, "mine, reworded", true).unwrap();
    let err = push(a, false, None).unwrap_err();
    assert!(err.contains("non-fast-forward"), "{err}");
    push(a, true, None).unwrap();
    // b pushes meanwhile; a, not having fetched it, amends again: the lease refuses.
    fetch(b).unwrap();
    run(b, &["merge", "-q", "--ff-only", "origin/main"]).unwrap();
    write_commit(b, "b.txt", "b\n", "theirs");
    push(b, false, None).unwrap();
    commit(a, "mine, again", true).unwrap();
    assert!(push(a, true, None).is_err());
}

/// A PR is titled like GitHub does: one commit gives its message, more the branch name.
#[test]
fn pull_draft_counts_commits_against_the_base() {
    let sb = Sandbox::new("draft");
    let c = sb.remote_with_clones(1);
    let a = &c[0];
    run(a, &["switch", "-q", "-c", "feat"]).unwrap();
    fs::write(a.join("f.txt"), "f\n").unwrap();
    stage(a, &["f.txt".into()]).unwrap();
    commit(a, "Add f\n\nWhy it matters.", false).unwrap();
    let d = pull_draft(a, "refs/remotes/origin/main").unwrap();
    assert_eq!(
        (d.commits, d.subject.as_deref(), d.body.as_deref()),
        (1, Some("Add f"), Some("Why it matters."))
    );
    write_commit(a, "g.txt", "g\n", "Add g");
    let d = pull_draft(a, "refs/remotes/origin/main").unwrap();
    assert_eq!((d.commits, d.subject), (2, None));
    assert!(pull_draft(a, "main").is_err());
    assert!(pull_draft(a, "refs/remotes/--all").is_err());
}

/// The fork workflow git documents: pull from upstream, push to origin (remote.pushDefault).
#[test]
fn push_target_follows_push_default_not_the_upstream() {
    let sb = Sandbox::new("pushdef");
    let c = sb.remote_with_clones(2);
    let (a, b) = (&c[0], &c[1]);
    // b plays the fork: "upstream" is the original (the shared bare repo), origin its own.
    let url = git_url(b);
    run(b, &["remote", "rename", "origin", "upstream"]).unwrap();
    let fork = sb.path("fork.git");
    run(&sb.0, &["init", "-q", "--bare", fork.to_str().unwrap()]).unwrap();
    run(b, &["remote", "add", "origin", fork.to_str().unwrap()]).unwrap();
    assert_eq!(url, git_url_of(b, "upstream"));
    let st = status(b).unwrap();
    assert_eq!(st.upstream.as_deref(), Some("upstream/main"));
    // Without a push default, pushes follow the upstream: into the original.
    assert_eq!(st.push.unwrap().remote, "upstream");

    set_push_default(b, "origin").unwrap();
    write_commit(b, "b.txt", "b\n", "fork work");
    let p = status(b).unwrap().push.unwrap();
    assert_eq!((p.remote.as_str(), p.branch.as_deref()), ("origin", None));
    push(b, false, None).unwrap();
    let st = status(b).unwrap();
    let p = st.push.unwrap();
    assert_eq!((p.branch.as_deref(), p.ahead), (Some("origin/main"), 0));
    // Still pulls from the original, and the original didn't get the commit.
    assert_eq!(st.upstream.as_deref(), Some("upstream/main"));
    assert_eq!(st.ahead, 1);
    run(a, &["pull", "-q"]).unwrap();
    assert!(log(a, None, 0, 5)
        .unwrap()
        .iter()
        .all(|c| c.subject != "fork work"));
    assert!(set_push_default(b, "nope").is_err());
}

/// A fork has origin/x and upstream/x alike: `git switch x` refuses, so the picker names one.
#[test]
fn switching_to_a_remote_branch_tracks_that_remote() {
    let sb = Sandbox::new("track");
    let c = sb.remote_with_clones(2);
    let (a, b) = (&c[0], &c[1]);
    run(a, &["switch", "-q", "-c", "feat"]).unwrap();
    write_commit(a, "f.txt", "f\n", "feat");
    run(a, &["push", "-q", "-u", "origin", "feat"]).unwrap();
    let url = git_url(b);
    run(b, &["remote", "add", "upstream", &url]).unwrap();
    run(b, &["fetch", "-q", "--all"]).unwrap();
    assert!(run(b, &["switch", "feat"]).is_err());
    switch_tracking(b, "upstream/feat").unwrap();
    let tracked = run(b, &["rev-parse", "--abbrev-ref", "@{upstream}"]).unwrap();
    assert_eq!(String::from_utf8_lossy(&tracked).trim(), "upstream/feat");
    // Options and local refs are refused.
    assert!(switch_tracking(b, "--orphan=x").is_err());
    assert!(switch_tracking(b, "main").is_err());
}

/// A fork's view of its original: another branch's history, marking what HEAD lacks.
#[test]
fn log_of_a_remote_branch_marks_what_head_lacks() {
    let sb = Sandbox::new("logrev");
    let c = sb.remote_with_clones(2);
    let (a, b) = (&c[0], &c[1]);
    write_commit(a, "a.txt", "new\n", "upstream moved on");
    run(a, &["push", "-q"]).unwrap();
    fetch(b).unwrap();
    let theirs = log(b, Some("refs/remotes/origin/main"), 0, 10).unwrap();
    assert_eq!(theirs[0].subject, "upstream moved on");
    assert!(theirs[0].not_in_head && !theirs[1].not_in_head);
    assert!(theirs.iter().all(|c| !c.unpushed));
    // HEAD's own log never marks anything.
    assert!(log(b, None, 0, 10).unwrap().iter().all(|c| !c.not_in_head));
    // Only remote-tracking branches, never an option or a local ref.
    assert!(log(b, Some("--all"), 0, 10).is_err());
    assert!(log(b, Some("refs/heads/main"), 0, 10).is_err());
    assert!(log(b, Some("refs/remotes/--output=x/y"), 0, 10).is_err());
}

#[test]
fn pull_rebase_conflict_then_abort_restores() {
    let sb = Sandbox::new("pullrb");
    let c = sb.remote_with_clones(2);
    let (a, b) = (&c[0], &c[1]);
    write_commit(a, "a.txt", "one\nTWO-a\nthree\n", "a edits");
    run(a, &["push", "-q"]).unwrap();
    write_commit(b, "a.txt", "one\nTWO-b\nthree\n", "b edits");
    let before = log(b, None, 0, 1).unwrap()[0].sha.clone();

    assert!(pull(b, "rebase").unwrap(), "should stop on the conflict");
    assert_eq!(operation(b).unwrap().kind, "rebase");
    assert_eq!(status(b).unwrap().conflicted.len(), 1);
    op_abort(b).unwrap();
    assert!(operation(b).is_none());
    assert_eq!(log(b, None, 0, 1).unwrap()[0].sha, before);
}

#[test]
fn publish_sets_upstream() {
    let sb = Sandbox::new("publish");
    let c = sb.remote_with_clones(1);
    let a = &c[0];
    switch_branch(a, "feat/new-thing", true).unwrap();
    write_commit(a, "n.txt", "n\n", "new");
    assert!(status(a).unwrap().upstream.is_none());
    push(a, false, None).unwrap();
    let st = status(a).unwrap();
    assert_eq!(st.upstream.as_deref(), Some("origin/feat/new-thing"));
    assert_eq!(st.ahead, 0);
    // The remote branch is listed and switching to its short name works from another clone.
    let b = sb.clone_of("late");
    assert!(branches(&b)
        .unwrap()
        .iter()
        .any(|x| x.remote && x.name == "origin/feat/new-thing"));
    switch_branch(&b, "feat/new-thing", false).unwrap();
    assert_eq!(
        status(&b).unwrap().branch.as_deref(),
        Some("feat/new-thing")
    );
}

#[test]
fn publish_picks_the_remote_instead_of_assuming_origin() {
    let sb = Sandbox::new("pubremote");
    let c = sb.remote_with_clones(1);
    let a = &c[0];
    run(a, &["remote", "rename", "origin", "gh"]).unwrap();
    switch_branch(a, "feat", true).unwrap();
    // The only remote, whatever its name.
    assert_eq!(status(a).unwrap().publish.as_deref(), Some("gh"));
    push(a, false, None).unwrap();
    assert_eq!(status(a).unwrap().upstream.as_deref(), Some("gh/feat"));

    // Several remotes and none is origin: the user picks.
    let other = sb.path("other.git");
    run(&sb.0, &["init", "-q", "--bare", other.to_str().unwrap()]).unwrap();
    run(a, &["remote", "add", "other", other.to_str().unwrap()]).unwrap();
    switch_branch(a, "feat2", true).unwrap();
    let st = status(a).unwrap();
    assert_eq!((st.publish.as_deref(), st.remotes.len()), (None, 2));
    assert!(push(a, false, None)
        .unwrap_err()
        .contains("several remotes"));
    assert!(push(a, false, Some("nope")).is_err());
    push(a, false, Some("other")).unwrap();
    assert_eq!(status(a).unwrap().upstream.as_deref(), Some("other/feat2"));

    // remote.pushDefault decides when set.
    set_push_default(a, "other").unwrap();
    switch_branch(a, "feat3", true).unwrap();
    assert_eq!(status(a).unwrap().publish.as_deref(), Some("other"));

    // No remote at all: a clear message, not a raw git error.
    let lone = sb.path("lone");
    init(&lone);
    write_commit(&lone, "x.txt", "x\n", "x");
    assert!(push(&lone, false, None).unwrap_err().contains("no remote"));
}

/// Clicking Stage on many rows at once used to fail on index.lock for most of them.
#[test]
fn index_writes_are_serialized_and_retry_a_brief_lock() {
    let sb = Sandbox::new("indexlock");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "seed", "s\n", "seed");
    for i in 0..30 {
        fs::write(r.join(format!("f{i}.txt")), "x\n").unwrap();
    }
    let lock = std::sync::Arc::new(std::sync::Mutex::new(()));
    let stages: Vec<_> = (0..30)
        .map(|i| {
            let (r, lock) = (r.clone(), lock.clone());
            std::thread::spawn(move || {
                crate::with_index_lock(&lock, &r, |r| stage(r, &[format!("f{i}.txt")]))
            })
        })
        .collect();
    for s in stages {
        s.join().unwrap().unwrap();
    }
    assert_eq!(status(&r).unwrap().staged.len(), 30);

    // Another git (an agent, the terminal) holding the lock for a moment.
    let held = r.join(".git/index.lock");
    fs::write(&held, "").unwrap();
    let release = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(150));
        fs::remove_file(held).unwrap();
    });
    fs::write(r.join("late.txt"), "x\n").unwrap();
    crate::with_index_lock(&lock, &r, |r| stage(r, &["late.txt".into()])).unwrap();
    release.join().unwrap();
}

#[test]
fn multi_commit_rebase_with_skip() {
    let sb = Sandbox::new("skip");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "base\n", "base");
    switch_branch(&r, "feature", true).unwrap();
    write_commit(&r, "a.txt", "feature\n", "f1 conflicts");
    write_commit(&r, "f.txt", "f\n", "f2 clean");
    switch_branch(&r, "main", false).unwrap();
    write_commit(&r, "a.txt", "main\n", "main edit");
    switch_branch(&r, "feature", false).unwrap();

    assert!(rebase(&r, "main").unwrap());
    let op = operation(&r).unwrap();
    assert_eq!((op.step, op.total), (Some(1), Some(2)));
    assert!(!rebase_skip(&r).unwrap(), "second commit applies cleanly");
    assert!(operation(&r).is_none());
    assert_eq!(fs::read_to_string(r.join("a.txt")).unwrap(), "main\n");
    assert!(r.join("f.txt").exists());
}

#[test]
fn resolve_side_never_deletes_a_file_that_exists_on_that_side() {
    let sb = Sandbox::new("side");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "base\n", "base");
    switch_branch(&r, "feature", true).unwrap();
    write_commit(&r, "a.txt", "feature\n", "f");
    switch_branch(&r, "main", false).unwrap();
    write_commit(&r, "a.txt", "main\n", "m");
    assert!(merge(&r, "feature").unwrap());
    resolve_side(&r, "a.txt", "ours").unwrap();
    assert_eq!(fs::read_to_string(r.join("a.txt")).unwrap(), "main\n");
    // Not a conflicted path: must error, not `git rm` it.
    fs::write(r.join("clean.txt"), "x\n").unwrap();
    stage(&r, &["clean.txt".into()]).unwrap();
    assert!(resolve_side(&r, "clean.txt", "theirs").is_err());
    assert!(r.join("clean.txt").exists());
}

#[test]
fn detached_head_and_empty_repo() {
    let sb = Sandbox::new("detached");
    let r = sb.path("r");
    init(&r);
    let st = status(&r).unwrap();
    assert!(st.head.is_none() && st.branch.as_deref() == Some("main"));
    assert!(log(&r, None, 0, 10).unwrap().is_empty());
    assert!(branches(&r).unwrap().is_empty());
    write_commit(&r, "a.txt", "a\n", "one");
    write_commit(&r, "a.txt", "b\n", "two");
    let first = log(&r, None, 0, 10).unwrap()[1].sha.clone();
    run(&r, &["checkout", "-q", &first]).unwrap();
    let st = status(&r).unwrap();
    assert!(st.branch.is_none());
    assert_eq!(st.head.as_deref(), Some(&first[..7]));
}

#[test]
fn binary_crlf_and_missing_trailing_newline() {
    let sb = Sandbox::new("content");
    let r = sb.path("r");
    init(&r);
    fs::write(r.join("bin.dat"), [0u8, 1, 2, 3, 0, 5]).unwrap();
    write_commit(&r, "crlf.txt", "a\r\nb\r\nc\r\n", "crlf");
    write_commit(&r, "nonl.txt", "x\ny", "no newline");
    stage(&r, &["bin.dat".into()]).unwrap();
    commit(&r, "bin", false).unwrap();

    fs::write(r.join("bin.dat"), [0u8, 9, 9]).unwrap();
    fs::write(r.join("crlf.txt"), "a\r\nB\r\nc\r\n").unwrap();
    fs::write(r.join("nonl.txt"), "x\ny\n").unwrap();
    let wt = |p: &str| vfs::read_file(&r, p);

    let bin = diff_pair(&r, "unstaged", "bin.dat", None, None, None, wt).unwrap();
    assert!(bin.modified.binary && bin.rows.is_empty());

    let crlf = diff_pair(&r, "unstaged", "crlf.txt", None, None, None, wt).unwrap();
    let kinds: Vec<u8> = crlf.rows.iter().map(|x| x.k).collect();
    assert_eq!(kinds, vec![0, 2, 1, 0]);
    // Emphasis must not include the \r.
    assert!(crlf.rows[2].e.iter().all(|[_, end]| *end <= 1));

    let nonl = diff_pair(&r, "unstaged", "nonl.txt", None, None, None, wt).unwrap();
    assert!(
        nonl.rows.iter().all(|x| x.o <= 2 && x.n <= 2),
        "line counts match the frontend's split"
    );
}

#[test]
fn paths_with_spaces_unicode_and_renames() {
    let sb = Sandbox::new("paths");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "dir with space/ümlaut é.txt", "1\n2\n", "odd names");
    run(
        &r,
        &[
            "mv",
            "dir with space/ümlaut é.txt",
            "dir with space/renamed ü.txt",
        ],
    )
    .unwrap();
    fs::write(r.join("dir with space/renamed ü.txt"), "1\n2\n3\n").unwrap();
    let st = status(&r).unwrap();
    let staged = st
        .staged
        .iter()
        .find(|f| f.status == "R")
        .expect("staged rename");
    assert_eq!(staged.path, "dir with space/renamed ü.txt");
    assert_eq!(
        staged.old_path.as_deref(),
        Some("dir with space/ümlaut é.txt")
    );
    let unstaged = st
        .unstaged
        .iter()
        .find(|f| f.path == "dir with space/renamed ü.txt")
        .expect("worktree edit");
    assert_eq!((unstaged.additions, unstaged.deletions), (Some(1), Some(0)));
    let listing = vfs::list_dir(&r, "dir with space").unwrap();
    assert_eq!(listing[0].name, "renamed ü.txt");
}

#[cfg(unix)]
#[test]
fn symlinks_cannot_escape_the_repo() {
    let sb = Sandbox::new("escape");
    let r = sb.path("r");
    init(&r);
    let outside = sb.path("outside");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("secret.txt"), "secret").unwrap();
    std::os::unix::fs::symlink(&outside, r.join("link")).unwrap();

    assert!(
        !vfs::read_file(&r, "link/secret.txt").exists,
        "read through a symlink"
    );
    assert!(
        vfs::write_file(&r, "link/secret.txt", "pwned").is_err(),
        "overwrite through a symlink"
    );
    assert!(
        vfs::write_file(&r, "link/new.txt", "pwned").is_err(),
        "create through a symlinked parent"
    );
    assert!(!outside.join("new.txt").exists());
    assert_eq!(
        fs::read_to_string(outside.join("secret.txt")).unwrap(),
        "secret"
    );
    // Normal writes, including new files in new-ish places, still work.
    vfs::write_file(&r, "ok.txt", "fine").unwrap();
}

#[test]
fn discard_treats_paths_literally() {
    let sb = Sandbox::new("literal");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "app/[id].tsx", "a\n", "route");
    write_commit(&r, "app/i.tsx", "b\n", "other");
    fs::write(r.join("app/[id].tsx"), "changed\n").unwrap();
    fs::write(r.join("app/i.tsx"), "keep my edit\n").unwrap();
    discard(&r, &["app/[id].tsx".into()]).unwrap();
    assert_eq!(fs::read_to_string(r.join("app/[id].tsx")).unwrap(), "a\n");
    assert_eq!(
        fs::read_to_string(r.join("app/i.tsx")).unwrap(),
        "keep my edit\n",
        "glob must not match app/i.tsx"
    );
}

#[test]
fn git_internals_are_off_limits() {
    let sb = Sandbox::new("dotgit");
    let r = sb.path("r");
    init(&r);
    for p in [
        ".git/config",
        ".GIT/config",
        "sub/.git/hooks/pre-commit",
        ".git/hooks/pre-commit",
    ] {
        assert!(vfs::write_file(&r, p, "x").is_err(), "{p}");
    }
    assert!(!vfs::read_file(&r, ".git/config").exists);
    assert_ne!(fs::read_to_string(r.join(".git/config")).unwrap(), "x");
}

#[test]
fn continue_reports_hook_failures_instead_of_conflicts() {
    let sb = Sandbox::new("hook");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "base\n", "base");
    switch_branch(&r, "feature", true).unwrap();
    write_commit(&r, "a.txt", "feature\n", "f");
    switch_branch(&r, "main", false).unwrap();
    write_commit(&r, "a.txt", "main\n", "m");
    assert!(merge(&r, "feature").unwrap());
    resolve_side(&r, "a.txt", "ours").unwrap();

    let hook = r.join(".git/hooks/commit-msg");
    fs::write(&hook, "#!/bin/sh\necho 'rejected by hook' >&2\nexit 1\n").unwrap();
    std::process::Command::new("chmod")
        .args(["+x", hook.to_str().unwrap()])
        .status()
        .unwrap();

    let err = op_continue(&r).expect_err("a failing hook is an error, not 'stopped on conflicts'");
    assert!(err.contains("rejected by hook"), "{err}");
    fs::remove_file(&hook).unwrap();
    assert!(!op_continue(&r).unwrap());
}

#[test]
fn amend_without_message_keeps_the_old_one() {
    let sb = Sandbox::new("amend");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "a\n", "original message");
    fs::write(r.join("b.txt"), "b\n").unwrap();
    stage(&r, &["b.txt".into()]).unwrap();
    commit(&r, "  ", true).unwrap();
    let head = &log(&r, None, 0, 5).unwrap()[0];
    assert_eq!(head.subject, "original message");
    assert_eq!(log(&r, None, 0, 5).unwrap().len(), 1);
}

#[test]
fn special_and_non_utf8_files_are_safe() {
    let sb = Sandbox::new("special");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "x.txt", "x\n", "base");
    // An untracked FIFO would block a naive read forever.
    assert!(std::process::Command::new("mkfifo")
        .arg(r.join("pipe"))
        .status()
        .unwrap()
        .success());
    // git skips non-regular files; what matters is that nothing blocks on the pipe.
    status(&r).unwrap();
    assert!(!vfs::read_file(&r, "pipe").exists);

    fs::write(r.join("latin1.txt"), [b'c', b'a', b'f', 0xE9, b'\n']).unwrap();
    let f = vfs::read_file(&r, "latin1.txt");
    assert!(f.exists && f.lossy);
    assert!(!vfs::read_file(&r, "x.txt").lossy);
}

#[test]
fn git_am_is_detected_and_abortable() {
    let sb = Sandbox::new("am");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "base\n", "base");
    switch_branch(&r, "feature", true).unwrap();
    write_commit(&r, "a.txt", "feature\n", "f");
    let patch = run(&r, &["format-patch", "-1", "--stdout"]).unwrap();
    switch_branch(&r, "main", false).unwrap();
    write_commit(&r, "a.txt", "main\n", "m");
    fs::write(sb.path("p.patch"), patch).unwrap();
    assert!(run(&r, &["am", "-3", sb.path("p.patch").to_str().unwrap()]).is_err());
    assert_eq!(operation(&r).unwrap().kind, "am");
    op_abort(&r).unwrap();
    assert!(operation(&r).is_none());
}

#[test]
fn pr_checkout_fast_forwards_and_never_resets() {
    use crate::github::checkout;
    let sb = Sandbox::new("prco");
    let c = sb.remote_with_clones(2);
    let (a, b) = (&c[0], &c[1]);
    // The PR branch as it exists on the remote.
    switch_branch(a, "feat", true).unwrap();
    write_commit(a, "f.txt", "1\n", "f1");
    run(a, &["push", "-q", "-u", "origin", "feat"]).unwrap();
    // b has a stale local copy of it…
    run(b, &["fetch", "-q"]).unwrap();
    switch_branch(b, "feat", false).unwrap();
    switch_branch(b, "main", false).unwrap();
    write_commit(a, "f.txt", "1\n2\n", "f2");
    run(a, &["push", "-q"]).unwrap();
    checkout(b, "origin", None, 1, "feat", true).unwrap();
    assert_eq!(
        fs::read_to_string(b.join("f.txt")).unwrap(),
        "1\n2\n",
        "stale branch fast-forwarded"
    );

    // …and a local commit that diverges from the PR must be reported, not reset.
    write_commit(b, "mine.txt", "mine\n", "local only");
    write_commit(a, "f.txt", "1\n2\n3\n", "f3");
    run(a, &["push", "-q"]).unwrap();
    switch_branch(b, "main", false).unwrap();
    assert!(checkout(b, "origin", None, 1, "feat", true).is_err());
    assert!(run(b, &["log", "--oneline", "feat"])
        .map(|o| String::from_utf8_lossy(&o).contains("local only"))
        .unwrap());

    // Fork PRs land on pr/<n>; checking out again while on it just fast-forwards.
    run(a, &["push", "-q", "origin", "HEAD:refs/pull/7/head"]).unwrap();
    checkout(b, "origin", None, 7, "someones-branch", false).unwrap();
    assert_eq!(status(b).unwrap().branch.as_deref(), Some("pr/7"));
    checkout(b, "origin", None, 7, "someones-branch", false).unwrap();
}

/// A repo with an agent-style worktree inside it (.claude/worktrees/agent), a detached one
/// next to it, and one whose folder was deleted (prunable).
fn repo_with_worktrees(sb: &Sandbox) -> PathBuf {
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "a\n", "base");
    let add = |args: &[&str]| {
        let mut all = vec!["worktree", "add", "-q"];
        all.extend(args);
        run(&r, &all).unwrap();
    };
    add(&["-b", "agent", ".claude/worktrees/agent"]);
    add(&["--detach", sb.path("det").to_str().unwrap()]);
    add(&["-b", "gone", sb.path("gone").to_str().unwrap()]);
    fs::remove_dir_all(sb.path("gone")).unwrap();
    r
}

fn git_url(repo: &Path) -> String {
    git_url_of(repo, "origin")
}

fn git_url_of(repo: &Path, remote: &str) -> String {
    let out = run(repo, &["remote", "get-url", remote]).unwrap();
    String::from_utf8_lossy(&out).trim().to_string()
}

fn same_dir(a: &str, b: &Path) -> bool {
    Path::new(a)
        .canonicalize()
        .is_ok_and(|a| a == b.canonicalize().unwrap())
}

#[test]
fn worktree_list_detached_prunable_and_counts() {
    let sb = Sandbox::new("wtlist");
    let r = repo_with_worktrees(&sb);
    let agent = r.join(".claude/worktrees/agent");
    fs::write(agent.join("a.txt"), "changed\n").unwrap();
    fs::write(agent.join("new.txt"), "new\n").unwrap();

    let list = worktrees(&r).unwrap();
    assert_eq!(list.len(), 4);
    let find = |dir: &Path| list.iter().find(|w| same_dir(&w.path, dir)).unwrap();
    let main = find(&r);
    assert!(main.main && main.current && main.branch.as_deref() == Some("main"));
    let a = find(&agent);
    assert!(!a.main && !a.current && a.branch.as_deref() == Some("agent"));
    let det = find(&sb.path("det"));
    assert!(det.detached && det.branch.is_none() && det.head.is_some());
    let gone = list.iter().find(|w| w.path.ends_with("/gone")).unwrap();
    assert!(gone.prunable);

    assert_eq!(worktree_state(&r, &a.path).unwrap().uncommitted, 2);
    let d = worktree_state(&r, &det.path).unwrap();
    assert!(d.uncommitted == 0 && d.commits == 0 && !d.merged);
    assert!(worktree_state(&r, &gone.path).is_err());
    // Only listed worktrees: never an arbitrary folder.
    assert!(worktree_state(&r, sb.path("det/..").to_str().unwrap()).is_err());

    // Untouched is not merged; committed then merged into a local, unpushed main is.
    assert!(!worktree_state(&r, &a.path).unwrap().merged);
    write_commit(&agent, "b.txt", "b\n", "agent work");
    let s = worktree_state(&r, &a.path).unwrap();
    assert!(s.commits == 1 && !s.merged);
    // Merged upstream only (a fork's PR landed in the original) is merged too.
    run(&r, &["update-ref", "refs/remotes/upstream/main", "agent"]).unwrap();
    let s = worktree_state(&r, &a.path).unwrap();
    assert!(s.commits == 0 && s.merged);
    run(&r, &["update-ref", "-d", "refs/remotes/upstream/main"]).unwrap();
    run(&r, &["merge", "-q", "agent"]).unwrap();
    let s = worktree_state(&r, &a.path).unwrap();
    assert!(s.commits == 0 && s.merged);
    // The agent worktree lives inside the main one: it's not an untracked file there.
    let m = worktree_state(&r, &main.path).unwrap();
    assert!(m.uncommitted == 0 && !m.merged);

    // From inside a linked worktree the main one is still the project.
    assert!(same_dir(&main_worktree(&agent).unwrap(), &r));
    assert!(worktrees(&agent)
        .unwrap()
        .iter()
        .any(|w| w.current && w.branch.as_deref() == Some("agent")));
    // Branches checked out elsewhere say where, so the UI can open that worktree instead.
    let br = branches(&r).unwrap();
    let wt = |n: &str| br.iter().find(|b| b.name == n).unwrap().worktree.clone();
    assert!(same_dir(&wt("agent").unwrap(), &agent));
    assert_eq!(wt("main"), None, "the current branch is not 'elsewhere'");
    assert!(switch_branch(&r, "agent", false).is_err());

    // Removing: never the main or the open one; a dirty one only when forced; a missing
    // folder just drops the entry. The branch stays.
    assert!(remove_worktree(&r, &main.path, true).is_err());
    assert!(remove_worktree(&agent, &a.path, true).is_err());
    assert!(remove_worktree(&r, sb.path("det/..").to_str().unwrap(), true).is_err());
    let err = remove_worktree(&r, &a.path, false).unwrap_err();
    assert!(err.contains("modified or untracked"), "{err}");
    run(&r, &["worktree", "lock", &a.path]).unwrap();
    remove_worktree(&r, &a.path, true).unwrap();
    assert!(!agent.exists());
    remove_worktree(&r, &gone.path, false).unwrap();
    assert_eq!(worktrees(&r).unwrap().len(), 2);
    assert!(branches(&r).unwrap().iter().any(|b| b.name == "agent"));
}

#[test]
fn nested_worktrees_stay_out_of_status_and_are_never_staged() {
    let sb = Sandbox::new("wtnested");
    let r = repo_with_worktrees(&sb);
    init(&r.join("vendor/lib"));
    fs::write(r.join("vendor/lib/x.txt"), "x\n").unwrap();
    fs::write(r.join("plain.txt"), "p\n").unwrap();

    let st = status(&r).unwrap();
    let entry = |p: &str| st.unstaged.iter().find(|f| f.path == p);
    assert!(
        entry(".claude/worktrees/agent/").is_none(),
        "own worktrees aren't changes"
    );
    let lib = entry("vendor/lib/")
        .unwrap()
        .nested
        .as_ref()
        .expect("nested repo");
    assert!(same_dir(&lib.path, &r.join("vendor/lib")));
    assert!(entry("plain.txt").unwrap().nested.is_none());

    // Staging one directly, or a folder above it, is refused; plain files still stage.
    for p in [".claude/worktrees/agent/", ".claude", "vendor/lib"] {
        let err = stage(&r, &[p.into()]).unwrap_err();
        assert!(err.contains("separate git repository"), "{p}: {err}");
    }
    assert!(stage(&r, &["plain.txt".into(), "vendor/lib/".into()]).is_err());
    assert!(
        status(&r).unwrap().staged.is_empty(),
        "a refusal stages nothing"
    );
    stage(&r, &["plain.txt".into()]).unwrap();
    // Explicitly allowed, it does what git does: a gitlink to its commit, not its files.
    write_commit(&r.join("vendor/lib"), "x.txt", "x\n", "lib");
    stage_with(&r, &["vendor/lib".into()], true).unwrap();
    let ls = run(&r, &["ls-files", "-s", "vendor/lib"]).unwrap();
    assert!(String::from_utf8_lossy(&ls).starts_with("160000"));
}

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

/// `r` with a submodule at `sub` (its `.git` is a file pointing into .git/modules/).
fn repo_with_submodule(sb: &Sandbox) -> PathBuf {
    let lib = sb.path("lib");
    init(&lib);
    write_commit(&lib, "l.txt", "l\n", "lib");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "a\n", "base");
    let url = lib.to_str().unwrap();
    let add = [
        "-c",
        "protocol.file.allow=always",
        "submodule",
        "add",
        "-q",
        url,
        "sub",
    ];
    run(&r, &add).unwrap();
    commit(&r, "add sub", false).unwrap();
    r
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

#[test]
fn submodule_bump_diffs_as_subproject_commits() {
    let sb = Sandbox::new("subdiff");
    let r = repo_with_submodule(&sb);
    let sub = r.join("sub");
    identity(&sub);
    let old = run(&sub, &["rev-parse", "HEAD"]).unwrap();
    let old = String::from_utf8_lossy(&old).trim().to_string();
    write_commit(&sub, "l.txt", "l2\n", "bump");
    let new = run(&sub, &["rev-parse", "HEAD"]).unwrap();
    let new = String::from_utf8_lossy(&new).trim().to_string();
    let read = |p: &str| vfs::read_file(&r, p);

    let pair = diff_pair(&r, "unstaged", "sub", None, None, None, read).unwrap();
    assert_eq!(pair.original.text, format!("Subproject commit {old}\n"));
    assert_eq!(pair.modified.text, format!("Subproject commit {new}\n"));
    assert!(pair.rows.iter().any(|row| row.k != 0));

    fs::write(sub.join("l.txt"), "dirty\n").unwrap();
    let pair = diff_pair(&r, "unstaged", "sub", None, None, None, read).unwrap();
    assert_eq!(
        pair.modified.text,
        format!("Subproject commit {new}-dirty\n")
    );

    stage(&r, &["sub".into()]).unwrap();
    commit(&r, "bump sub", false).unwrap();
    let head = log(&r, None, 0, 1).unwrap().remove(0).sha;
    let pair = diff_pair(&r, "commit", "sub", None, Some(&head), None, read).unwrap();
    assert_eq!(pair.original.text, format!("Subproject commit {old}\n"));
    assert_eq!(pair.modified.text, format!("Subproject commit {new}\n"));
    // A plain directory is still not a submodule.
    assert!(
        !diff_pair(&r, "unstaged", "nope", None, None, None, read)
            .unwrap()
            .modified
            .exists
    );
}

#[test]
fn stage_refuses_a_tracked_path_that_became_a_repo() {
    let sb = Sandbox::new("wtgitlink");
    let r = repo_with_submodule(&sb);
    write_commit(&r, "dep", "a file\n", "dep");
    fs::remove_file(r.join("dep")).unwrap();
    init(&r.join("dep"));
    write_commit(&r.join("dep"), "y.txt", "y\n", "inner");

    // git shows a type change to a gitlink, not an untracked folder.
    let st = status(&r).unwrap();
    let dep = st.unstaged.iter().find(|f| f.path == "dep").unwrap();
    assert_eq!(dep.status, "T");
    assert!(dep.nested.is_some());
    assert!(stage(&r, &["dep".into()]).is_err());
    assert!(stage(&r, &[".".into()]).is_err());
    assert!(status(&r).unwrap().staged.is_empty());

    // A real submodule already is a gitlink: recording its new commit is fine.
    write_commit(&r.join("sub"), "l.txt", "l2\n", "lib moves on");
    stage(&r, &["sub".into()]).unwrap();
    assert!(status(&r).unwrap().staged.iter().any(|f| f.path == "sub"));
}

#[test]
fn staging_no_paths_stages_nothing() {
    let sb = Sandbox::new("wtempty");
    let r = repo_with_worktrees(&sb);
    fs::write(r.join("new.txt"), "n\n").unwrap();
    stage(&r, &[]).unwrap();
    assert!(
        status(&r).unwrap().staged.is_empty(),
        "`git add -A --` would take everything, nested repos too"
    );
}

#[test]
fn bare_main_repo_is_not_a_worktree_to_open() {
    let sb = Sandbox::new("wtbare");
    sb.remote_with_clones(0);
    let origin = sb.path("origin.git");
    let wt = sb.path("wt");
    run(
        &origin,
        &["worktree", "add", "-q", "-b", "feat", wt.to_str().unwrap()],
    )
    .unwrap();

    let list = worktrees(&wt).unwrap();
    assert!(list[0].main && list[0].bare);
    assert_eq!(
        main_worktree(&wt),
        None,
        "the projects list keys by the worktree then"
    );
    let br = branches(&wt).unwrap();
    let main = br.iter().find(|b| b.name == "main").unwrap();
    assert_eq!(
        main.worktree, None,
        "bare HEAD holds main but can't be opened"
    );

    // An unborn branch has no HEAD to show (git prints all zeros).
    let empty = sb.path("empty");
    init(&empty);
    assert_eq!(worktrees(&empty).unwrap()[0].head, None);
}

#[test]
fn worktree_of_a_moved_repo_is_still_nested() {
    let sb = Sandbox::new("wtmoved");
    let r = repo_with_worktrees(&sb);
    let moved = sb.path("moved");
    fs::rename(&r, &moved).unwrap();

    // The worktree's .git now points at the old place; git lists its files as untracked.
    let st = status(&moved).unwrap();
    let agent: Vec<_> = st
        .unstaged
        .iter()
        .filter(|f| f.path.starts_with(".claude/worktrees/agent"))
        .collect();
    assert_eq!(agent.len(), 1, "one entry for the folder, not its files");
    assert_eq!(agent[0].path, ".claude/worktrees/agent/");
    assert!(agent[0].nested.is_some());
    let err = stage(&moved, &[".claude/worktrees/agent/a.txt".into()]).unwrap_err();
    assert!(err.contains("separate git repository"), "{err}");
    assert!(stage(&moved, &[".claude".into()]).is_err());
}

#[test]
fn undo_last_commit_keeps_its_changes_staged() {
    let sb = Sandbox::new("undo");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "one\n", "base");
    write_commit(&r, "a.txt", "one\ntwo\n", "second");
    let commits = log(&r, None, 0, 5).unwrap();
    // Only the commit the user saw as HEAD may be undone.
    assert!(undo_commit(&r, &commits[1].sha).is_err());
    undo_commit(&r, &commits[0].sha).unwrap();
    assert_eq!(log(&r, None, 0, 5).unwrap().len(), 1);
    assert_eq!(status(&r).unwrap().staged.len(), 1);
    assert_eq!(fs::read_to_string(r.join("a.txt")).unwrap(), "one\ntwo\n");
}

#[test]
fn revert_clean_conflicting_empty_and_merge() {
    let sb = Sandbox::new("revert");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "1\n2\n3\n", "base");
    write_commit(&r, "b.txt", "b\n", "add b");
    let add_b = log(&r, None, 0, 1).unwrap()[0].sha.clone();
    assert!(!revert(&r, &add_b).unwrap());
    assert!(!r.join("b.txt").exists());
    assert!(log(&r, None, 0, 1).unwrap()[0]
        .subject
        .starts_with("Revert"));

    // Reverting x after y changed the same line conflicts and uses the normal op flow.
    write_commit(&r, "a.txt", "1\nx\n3\n", "x");
    let x = log(&r, None, 0, 1).unwrap()[0].sha.clone();
    write_commit(&r, "a.txt", "1\ny\n3\n", "y");
    let y = log(&r, None, 0, 1).unwrap()[0].sha.clone();
    assert!(revert(&r, &x).unwrap(), "should stop on the conflict");
    assert_eq!(operation(&r).unwrap().kind, "revert");
    assert_eq!(status(&r).unwrap().conflicted.len(), 1);
    assert!(revert(&r, &y).is_err(), "busy repo refuses a second revert");
    op_abort(&r).unwrap();
    assert!(operation(&r).is_none());

    // Reverting something already undone is an error that leaves no operation behind.
    assert!(!revert(&r, &y).unwrap());
    let err = revert(&r, &y).unwrap_err();
    assert!(err.contains("already undone"), "{err}");
    assert!(operation(&r).is_none());

    // Resolved conflicts finish through the same Continue as merges.
    write_commit(&r, "a.txt", "1\nz\n3\n", "z");
    assert!(revert(&r, &x).unwrap());
    resolve_side(&r, "a.txt", "theirs").unwrap();
    assert!(!op_continue(&r).unwrap());
    assert!(operation(&r).is_none());
    assert_eq!(fs::read_to_string(r.join("a.txt")).unwrap(), "1\n2\n3\n");
    assert!(log(&r, None, 0, 1).unwrap()[0]
        .subject
        .starts_with("Revert \"x\""));

    // Merge commits revert against their first parent.
    switch_branch(&r, "feature", true).unwrap();
    write_commit(&r, "f.txt", "f\n", "feature");
    switch_branch(&r, "main", false).unwrap();
    write_commit(&r, "m.txt", "m\n", "main");
    run(&r, &["merge", "-q", "--no-edit", "feature"]).unwrap();
    let merge = log(&r, None, 0, 1).unwrap()[0].sha.clone();
    assert!(!revert(&r, &merge).unwrap());
    assert!(!r.join("f.txt").exists() && r.join("m.txt").exists());
}

#[test]
fn reset_modes() {
    let sb = Sandbox::new("reset");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "1\n", "base");
    let base = log(&r, None, 0, 1).unwrap()[0].sha.clone();
    write_commit(&r, "a.txt", "1\n2\n", "two");
    let two = log(&r, None, 0, 1).unwrap()[0].sha.clone();
    assert!(reset(&r, &base, "--hard", &two).is_err());
    // The HEAD the user saw must still be HEAD, or an agent's newer commit would be dropped.
    assert!(reset(&r, &base, "hard", &base).is_err());
    assert_eq!(log(&r, None, 0, 5).unwrap().len(), 2);

    reset(&r, &base, "soft", &two).unwrap();
    let st = status(&r).unwrap();
    assert_eq!((st.staged.len(), st.unstaged.len()), (1, 0));

    reset(&r, &two, "mixed", &base).unwrap();
    reset(&r, &base, "mixed", &two).unwrap();
    let st = status(&r).unwrap();
    assert_eq!((st.staged.len(), st.unstaged.len()), (0, 1));

    reset(&r, &base, "hard", &base).unwrap();
    let st = status(&r).unwrap();
    assert!(st.staged.is_empty() && st.unstaged.is_empty());
    assert_eq!(fs::read_to_string(r.join("a.txt")).unwrap(), "1\n");
    assert_eq!(log(&r, None, 0, 5).unwrap().len(), 1);
}

#[test]
fn checkout_branch_and_tag_at_a_commit() {
    let sb = Sandbox::new("refs");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "1\n", "base");
    let base = log(&r, None, 0, 1).unwrap()[0].sha.clone();
    write_commit(&r, "a.txt", "2\n", "two");

    assert!(create_tag(&r, "-f", &base).is_err());
    assert!(create_tag(&r, "bad..name", &base).is_err());
    create_tag(&r, "v1.0", &base).unwrap();
    assert!(
        create_tag(&r, "v1.0", &base).is_err(),
        "existing tag is not moved"
    );
    assert!(log(&r, None, 0, 2).unwrap()[1]
        .refs
        .iter()
        .any(|x| x == "tag: v1.0"));

    checkout_commit(&r, &base).unwrap();
    assert!(status(&r).unwrap().branch.is_none());
    assert_eq!(fs::read_to_string(r.join("a.txt")).unwrap(), "1\n");

    switch_branch(&r, "main", false).unwrap();
    create_branch_at(&r, "from-base", &base).unwrap();
    assert_eq!(status(&r).unwrap().branch.as_deref(), Some("from-base"));
    assert_eq!(fs::read_to_string(r.join("a.txt")).unwrap(), "1\n");
    assert!(create_branch_at(&r, "--evil", &base).is_err());
    assert!(create_branch_at(&r, "@", &base).is_err());
    assert!(create_tag(&r, "@", &base).is_err());
}

fn commit_dated(repo: &Path, path: &str, content: &str, msg: &str, date: &str) {
    fs::write(repo.join(path), content).unwrap();
    stage(repo, &[path.into()]).unwrap();
    let ok = std::process::Command::new("git")
        .current_dir(repo)
        .args(["commit", "-q", "-m", msg])
        .env("GIT_AUTHOR_DATE", date)
        .env("GIT_COMMITTER_DATE", date)
        .status()
        .unwrap()
        .success();
    assert!(ok);
}

#[test]
fn drops_pushed_follows_ancestry_not_log_order() {
    let sb = Sandbox::new("drops");
    let c = sb.remote_with_clones(2);
    let (a, b) = (&c[0], &c[1]);
    // P is pushed but dated long ago, so the log lists it below newer local commits.
    commit_dated(a, "p.txt", "p\n", "old pushed", "2000-01-01T00:00:00Z");
    run(a, &["push", "-q"]).unwrap();
    write_commit(b, "t.txt", "t\n", "target");
    run(b, &["fetch", "-q"]).unwrap();
    run(b, &["merge", "-q", "--no-edit", "origin/main"]).unwrap();

    let commits = log(b, None, 0, 10).unwrap();
    let subjects: Vec<&str> = commits.iter().map(|x| x.subject.as_str()).collect();
    assert_eq!(subjects[1..], ["target", "base", "old pushed"]);
    let (merge, target) = (&commits[0].sha, &commits[1].sha);
    // Only unpushed commits sit above "target", yet resetting to it drops the pushed P.
    assert!(commits[0].unpushed && commits[1].unpushed && !commits[3].unpushed);
    assert!(drops_pushed(b, target).unwrap());
    assert!(!drops_pushed(b, merge).unwrap());
    // Undoing the merge (moving to its first parent) drops P as well.
    assert!(drops_pushed(b, &commits[0].parents[0]).unwrap());
    assert!(commits[3].on_origin && !commits[1].on_origin);
}

#[test]
fn gone_upstream_is_unknown_not_pushed() {
    let sb = Sandbox::new("gone");
    let c = sb.remote_with_clones(1);
    let a = &c[0];
    switch_branch(a, "feat", true).unwrap();
    write_commit(a, "f.txt", "f\n", "feature");
    push(a, false, None).unwrap();
    run(a, &["push", "-q", "origin", "--delete", "feat"]).unwrap();
    fetch(a).unwrap();
    write_commit(a, "g.txt", "g\n", "after the branch was deleted");

    let commits = log(a, None, 0, 10).unwrap();
    assert!(commits.iter().all(|x| !x.unpushed));
    assert!(!drops_pushed(a, &commits[2].sha).unwrap());
    // "feature" was only ever on the deleted branch; base is still on origin/main.
    let on: Vec<bool> = commits.iter().map(|x| x.on_origin).collect();
    assert_eq!(on, [false, false, true]);

    let local = sb.path("local");
    init(&local);
    write_commit(&local, "x.txt", "x\n", "x");
    assert!(!log(&local, None, 0, 5).unwrap()[0].on_origin);
}

#[test]
fn merged_branches_and_deleting_them() {
    let sb = Sandbox::new("brdel");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "a\n", "base");
    run(&r, &["branch", "done"]).unwrap();
    run(&r, &["switch", "-q", "-c", "wip"]).unwrap();
    write_commit(&r, "b.txt", "b\n", "unmerged work");
    run(&r, &["switch", "-q", "-c", "feature"]).unwrap();
    let merged = |r: &Path| -> Vec<String> {
        let mut m: Vec<String> = branches(r)
            .unwrap()
            .into_iter()
            .filter(|b| b.merged)
            .map(|b| b.name)
            .collect();
        m.sort();
        m
    };
    // main is the default branch: merged into the feature, but never offered for cleanup.
    assert_eq!(merged(&r), ["done", "wip"]);

    run(&r, &["switch", "-q", "main"]).unwrap();
    assert_eq!(merged(&r), ["done"]);
    // -d refuses commits found nowhere else; -D takes them.
    assert!(delete_branches(&r, &["wip".into()], false).is_err());
    delete_branches(&r, &["done".into(), "wip".into()], true).unwrap();
    assert!(delete_branches(&r, &["-D".into()], true).is_err());
    let left: Vec<String> = branches(&r).unwrap().into_iter().map(|b| b.name).collect();
    assert_eq!(left.len(), 2, "{left:?}");
}

#[test]
fn deleting_a_remote_branch() {
    let sb = Sandbox::new("rbdel");
    let c = sb.remote_with_clones(1);
    let a = &c[0];
    run(a, &["push", "-q", "origin", "HEAD:refs/heads/feat/x"]).unwrap();
    fetch(a).unwrap();
    let defaults: Vec<String> = branches(a)
        .unwrap()
        .into_iter()
        .filter(|b| b.remote_default)
        .map(|b| b.name)
        .collect();
    assert_eq!(defaults, ["origin/main"]);
    delete_remote_branch(a, "origin/feat/x").unwrap();
    let left = String::from_utf8(run(a, &["ls-remote", "--heads", "origin"]).unwrap()).unwrap();
    assert!(!left.contains("feat/x"), "{left}");
    // The tracking ref goes with it, so the picker drops the row without a fetch.
    assert!(!branches(a)
        .unwrap()
        .iter()
        .any(|b| b.name == "origin/feat/x"));
    assert!(delete_remote_branch(a, "origin/main").is_err());
    assert!(delete_remote_branch(a, "nope/x").is_err());
}

#[test]
fn worktree_locks_by_a_live_process_mean_in_use() {
    let sb = Sandbox::new("wtlock");
    let r = repo_with_worktrees(&sb);
    let agent = r.join(".claude/worktrees/agent");
    let det = sb.path("det");
    let live = format!(
        "claude session agent (pid {} start now)",
        std::process::id()
    );
    run(
        &r,
        &[
            "worktree",
            "lock",
            "--reason",
            &live,
            agent.to_str().unwrap(),
        ],
    )
    .unwrap();
    // A pid far past any real one: the session that took the lock is gone.
    let dead = "claude session det (pid 999999999 start then)";
    run(
        &r,
        &["worktree", "lock", "--reason", dead, det.to_str().unwrap()],
    )
    .unwrap();

    let list = with_live_locks(worktrees(&r).unwrap());
    let find = |dir: &Path| list.iter().find(|w| same_dir(&w.path, dir)).unwrap();
    let (a, d) = (find(&agent), find(&det));
    assert!(a.locked && a.in_use && a.lock_reason.as_deref() == Some(live.as_str()));
    assert!(d.locked && !d.in_use);
    assert!(!find(&r).locked && !find(&r).in_use);
}

// ---------------------------------------------------------------- undo / redo

use crate::journal::{Action, Journal, Mode};
use std::sync::Mutex;

fn rev(repo: &Path, spec: &str) -> String {
    run_text(repo, &["rev-parse", spec])
        .unwrap()
        .trim()
        .to_string()
}

fn exists(repo: &Path, branch: &str) -> bool {
    let full = format!("refs/heads/{branch}");
    run(repo, &["rev-parse", "--verify", "-q", &full]).is_ok()
}

fn on_branch(repo: &Path) -> String {
    run_text(repo, &["symbolic-ref", "--short", "HEAD"])
        .unwrap()
        .trim()
        .to_string()
}

/// Undo (`forward` false) or redo the next entry.
fn step(j: &Journal, repo: &Path, forward: bool) -> Result<(), String> {
    j.step(repo, forward, None, &Mutex::new(())).map(|_| ())
}

#[test]
fn undo_and_redo_a_commit() {
    let sb = Sandbox::new("j-commit");
    let r = sb.path("r");
    init(&r);
    let j = Journal::default();
    // The very first commit goes back to an unborn branch.
    fs::write(r.join("a.txt"), "one\n").unwrap();
    stage(&r, &["a.txt".into()]).unwrap();
    j.record(&r, Action::new("Commit", Mode::Soft), |r| {
        commit(r, "base", false)
    })
    .unwrap();
    step(&j, &r, false).unwrap();
    assert!(!exists(&r, "main"));
    assert_eq!(status(&r).unwrap().staged.len(), 1);
    step(&j, &r, true).unwrap();
    let base = rev(&r, "HEAD");

    fs::write(r.join("a.txt"), "one\ntwo\n").unwrap();
    stage(&r, &["a.txt".into()]).unwrap();
    j.record(&r, Action::new("Commit", Mode::Soft), |r| {
        commit(r, "second", false)
    })
    .unwrap();
    let second = rev(&r, "HEAD");
    let id = j.last(&r).unwrap();
    // A toast's Undo names its entry, and nothing else is taken back.
    assert!(j.step(&r, false, Some(id - 1), &Mutex::new(())).is_err());
    j.step(&r, false, Some(id), &Mutex::new(())).unwrap();
    assert_eq!(rev(&r, "HEAD"), base);
    assert_eq!(status(&r).unwrap().staged.len(), 1);
    assert_eq!(fs::read_to_string(r.join("a.txt")).unwrap(), "one\ntwo\n");
    step(&j, &r, true).unwrap();
    assert_eq!(rev(&r, "HEAD"), second);
    assert!(status(&r).unwrap().staged.is_empty());

    // A new action after an undo drops what could be redone.
    step(&j, &r, false).unwrap();
    j.record(&r, Action::new("Commit", Mode::Soft), |r| {
        commit(r, "second, reworded", false)
    })
    .unwrap();
    let v = j.view(&r);
    assert!(v.redo.is_empty());
    assert_eq!(v.undo.len(), 2);
}

#[test]
fn undo_merge_keeps_local_edits_and_stops_after_outside_changes() {
    let sb = Sandbox::new("j-merge");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "a\n", "base");
    run(&r, &["switch", "-q", "-c", "feat"]).unwrap();
    write_commit(&r, "b.txt", "b\n", "feat adds b");
    run(&r, &["switch", "-q", "main"]).unwrap();
    write_commit(&r, "c.txt", "c\n", "main adds c");
    let before = rev(&r, "HEAD");
    let j = Journal::default();
    assert!(!j
        .record(&r, Action::new("Merge feat", Mode::Keep), |r| merge(
            r, "feat"
        ))
        .unwrap());
    let merged = rev(&r, "HEAD");

    // A file the merge didn't touch keeps its edit both ways.
    fs::write(r.join("c.txt"), "edited\n").unwrap();
    step(&j, &r, false).unwrap();
    assert_eq!(rev(&r, "HEAD"), before);
    assert!(!r.join("b.txt").exists());
    assert_eq!(fs::read_to_string(r.join("c.txt")).unwrap(), "edited\n");
    step(&j, &r, true).unwrap();
    assert_eq!(rev(&r, "HEAD"), merged);
    assert!(r.join("b.txt").exists());

    // A commit made in a terminal: the journal no longer describes this state.
    run(&r, &["commit", "-q", "--allow-empty", "-m", "outside"]).unwrap();
    let outside = rev(&r, "HEAD");
    assert!(j.view(&r).undo_blocked.is_some());
    assert!(step(&j, &r, false).is_err());
    assert_eq!(rev(&r, "HEAD"), outside);
}

#[test]
fn undo_branch_delete_restores_its_upstream() {
    let sb = Sandbox::new("j-delete");
    let c = sb.remote_with_clones(1);
    let a = &c[0];
    run(a, &["switch", "-q", "-c", "feat"]).unwrap();
    write_commit(a, "f.txt", "f\n", "feat");
    push(a, false, None).unwrap();
    run(a, &["switch", "-q", "main"]).unwrap();
    let tip = rev(a, "feat");
    let j = Journal::default();
    j.record(a, Action::new("Delete branch feat", Mode::Keep), |r| {
        delete_branches(r, &["feat".into()], true)
    })
    .unwrap();
    assert!(!exists(a, "feat"));
    // Twice: redo deletes it again, and the next undo still knows its upstream.
    for _ in 0..2 {
        step(&j, a, false).unwrap();
        assert_eq!(rev(a, "feat"), tip);
        assert_eq!(rev(a, "feat@{upstream}"), rev(a, "origin/feat"));
        step(&j, a, true).unwrap();
        assert!(!exists(a, "feat"));
    }
}

#[test]
fn undo_create_and_switch_branch() {
    let sb = Sandbox::new("j-switch");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "a\n", "base");
    let j = Journal::default();
    j.record(&r, Action::new("Create branch feat", Mode::Keep), |r| {
        switch_branch(r, "feat", true)
    })
    .unwrap();
    step(&j, &r, false).unwrap();
    assert_eq!(on_branch(&r), "main");
    assert!(!exists(&r, "feat"));
    step(&j, &r, true).unwrap();
    assert_eq!(on_branch(&r), "feat");
}

/// Undoing a pull just leaves the branch behind again; undoing a pushed commit would need
/// a force push, so it's refused.
#[test]
fn undo_takes_back_a_pull_but_not_a_pushed_commit() {
    let sb = Sandbox::new("j-pushed");
    let c = sb.remote_with_clones(2);
    let (a, b) = (&c[0], &c[1]);
    write_commit(a, "x.txt", "x\n", "from a");
    push(a, false, None).unwrap();
    let before = rev(b, "HEAD");
    let j = Journal::default();
    j.record(b, Action::new("Pull", Mode::Keep), |r| pull(r, "ff"))
        .unwrap();
    step(&j, b, false).unwrap();
    assert_eq!(rev(b, "HEAD"), before);
    step(&j, b, true).unwrap();

    fs::write(b.join("y.txt"), "y\n").unwrap();
    stage(b, &["y.txt".into()]).unwrap();
    j.record(b, Action::new("Commit", Mode::Soft), |r| {
        commit(r, "from b", false)
    })
    .unwrap();
    assert!(j.view(b).undo_blocked.is_none());
    push(b, false, None).unwrap();
    let why = j.view(b).undo_blocked.unwrap();
    assert!(why.contains("force push"), "{why}");
    assert!(step(&j, b, false).is_err());
}

/// A rebase that stops on conflicts is one entry once continued, undone to where it started.
#[test]
fn undo_a_rebase_continued_after_conflicts() {
    let sb = Sandbox::new("j-rebase");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "base\n", "base");
    run(&r, &["switch", "-q", "-c", "feat"]).unwrap();
    write_commit(&r, "a.txt", "feat\n", "feat");
    run(&r, &["switch", "-q", "main"]).unwrap();
    write_commit(&r, "a.txt", "main\n", "main");
    run(&r, &["switch", "-q", "feat"]).unwrap();
    let before = rev(&r, "HEAD");
    let j = Journal::default();
    let action = || Action::new("Rebase onto main", Mode::Keep);
    assert!(j.record(&r, action(), |r| rebase(r, "main")).unwrap());
    assert!(j.view(&r).undo.is_empty());
    fs::write(r.join("a.txt"), "both\n").unwrap();
    stage(&r, &["a.txt".into()]).unwrap();
    assert!(!j.record(&r, action(), op_continue).unwrap());
    let v = j.view(&r);
    assert_eq!(v.undo.len(), 1);
    assert_eq!(v.undo[0].label, "Rebase onto main");
    step(&j, &r, false).unwrap();
    assert_eq!(rev(&r, "HEAD"), before);
    assert_eq!(on_branch(&r), "feat");
    assert_eq!(fs::read_to_string(r.join("a.txt")).unwrap(), "feat\n");
}
