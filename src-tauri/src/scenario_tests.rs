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
    assert_eq!(log(b, 0, 1).unwrap()[0].parents.len(), 2);
    assert_eq!(
        fs::read_to_string(b.join("a.txt")).unwrap(),
        "one\ntwo\nthree\nfour\n"
    );
}

#[test]
fn pull_rebase_conflict_then_abort_restores() {
    let sb = Sandbox::new("pullrb");
    let c = sb.remote_with_clones(2);
    let (a, b) = (&c[0], &c[1]);
    write_commit(a, "a.txt", "one\nTWO-a\nthree\n", "a edits");
    run(a, &["push", "-q"]).unwrap();
    write_commit(b, "a.txt", "one\nTWO-b\nthree\n", "b edits");
    let before = log(b, 0, 1).unwrap()[0].sha.clone();

    assert!(pull(b, "rebase").unwrap(), "should stop on the conflict");
    assert_eq!(operation(b).unwrap().kind, "rebase");
    assert_eq!(status(b).unwrap().conflicted.len(), 1);
    op_abort(b).unwrap();
    assert!(operation(b).is_none());
    assert_eq!(log(b, 0, 1).unwrap()[0].sha, before);
}

#[test]
fn publish_sets_upstream() {
    let sb = Sandbox::new("publish");
    let c = sb.remote_with_clones(1);
    let a = &c[0];
    switch_branch(a, "feat/new-thing", true).unwrap();
    write_commit(a, "n.txt", "n\n", "new");
    assert!(status(a).unwrap().upstream.is_none());
    push(a).unwrap();
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
    assert!(log(&r, 0, 10).unwrap().is_empty());
    assert!(branches(&r).unwrap().is_empty());
    write_commit(&r, "a.txt", "a\n", "one");
    write_commit(&r, "a.txt", "b\n", "two");
    let first = log(&r, 0, 10).unwrap()[1].sha.clone();
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
    let head = &log(&r, 0, 5).unwrap()[0];
    assert_eq!(head.subject, "original message");
    assert_eq!(log(&r, 0, 5).unwrap().len(), 1);
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
    checkout(b, 1, "feat", true).unwrap();
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
    assert!(checkout(b, 1, "feat", true).is_err());
    assert!(run(b, &["log", "--oneline", "feat"])
        .map(|o| String::from_utf8_lossy(&o).contains("local only"))
        .unwrap());

    // Fork PRs land on pr/<n>; checking out again while on it just fast-forwards.
    run(a, &["push", "-q", "origin", "HEAD:refs/pull/7/head"]).unwrap();
    checkout(b, 7, "someones-branch", false).unwrap();
    assert_eq!(status(b).unwrap().branch.as_deref(), Some("pr/7"));
    checkout(b, 7, "someones-branch", false).unwrap();
}
