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

    assert_eq!(worktree_changes(&r, &a.path).unwrap(), 2);
    assert_eq!(worktree_changes(&r, &det.path).unwrap(), 0);
    assert!(worktree_changes(&r, &gone.path).is_err());
    // Only listed worktrees: never an arbitrary folder.
    assert!(worktree_changes(&r, sb.path("det/..").to_str().unwrap()).is_err());

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
    let commits = log(&r, 0, 5).unwrap();
    // Only the commit the user saw as HEAD may be undone.
    assert!(undo_commit(&r, &commits[1].sha).is_err());
    undo_commit(&r, &commits[0].sha).unwrap();
    assert_eq!(log(&r, 0, 5).unwrap().len(), 1);
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
    let add_b = log(&r, 0, 1).unwrap()[0].sha.clone();
    assert!(!revert(&r, &add_b).unwrap());
    assert!(!r.join("b.txt").exists());
    assert!(log(&r, 0, 1).unwrap()[0].subject.starts_with("Revert"));

    // Reverting x after y changed the same line conflicts and uses the normal op flow.
    write_commit(&r, "a.txt", "1\nx\n3\n", "x");
    let x = log(&r, 0, 1).unwrap()[0].sha.clone();
    write_commit(&r, "a.txt", "1\ny\n3\n", "y");
    let y = log(&r, 0, 1).unwrap()[0].sha.clone();
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
    assert!(log(&r, 0, 1).unwrap()[0]
        .subject
        .starts_with("Revert \"x\""));

    // Merge commits revert against their first parent.
    switch_branch(&r, "feature", true).unwrap();
    write_commit(&r, "f.txt", "f\n", "feature");
    switch_branch(&r, "main", false).unwrap();
    write_commit(&r, "m.txt", "m\n", "main");
    run(&r, &["merge", "-q", "--no-edit", "feature"]).unwrap();
    let merge = log(&r, 0, 1).unwrap()[0].sha.clone();
    assert!(!revert(&r, &merge).unwrap());
    assert!(!r.join("f.txt").exists() && r.join("m.txt").exists());
}

#[test]
fn reset_modes() {
    let sb = Sandbox::new("reset");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "1\n", "base");
    let base = log(&r, 0, 1).unwrap()[0].sha.clone();
    write_commit(&r, "a.txt", "1\n2\n", "two");
    let two = log(&r, 0, 1).unwrap()[0].sha.clone();
    assert!(reset(&r, &base, "--hard", &two).is_err());
    // The HEAD the user saw must still be HEAD, or an agent's newer commit would be dropped.
    assert!(reset(&r, &base, "hard", &base).is_err());
    assert_eq!(log(&r, 0, 5).unwrap().len(), 2);

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
    assert_eq!(log(&r, 0, 5).unwrap().len(), 1);
}

#[test]
fn checkout_branch_and_tag_at_a_commit() {
    let sb = Sandbox::new("refs");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "1\n", "base");
    let base = log(&r, 0, 1).unwrap()[0].sha.clone();
    write_commit(&r, "a.txt", "2\n", "two");

    assert!(create_tag(&r, "-f", &base).is_err());
    assert!(create_tag(&r, "bad..name", &base).is_err());
    create_tag(&r, "v1.0", &base).unwrap();
    assert!(
        create_tag(&r, "v1.0", &base).is_err(),
        "existing tag is not moved"
    );
    assert!(log(&r, 0, 2).unwrap()[1]
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

    let commits = log(b, 0, 10).unwrap();
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
    push(a).unwrap();
    run(a, &["push", "-q", "origin", "--delete", "feat"]).unwrap();
    fetch(a).unwrap();
    write_commit(a, "g.txt", "g\n", "after the branch was deleted");

    let commits = log(a, 0, 10).unwrap();
    assert!(commits.iter().all(|x| !x.unpushed));
    assert!(!drops_pushed(a, &commits[2].sha).unwrap());
    // "feature" was only ever on the deleted branch; base is still on origin/main.
    let on: Vec<bool> = commits.iter().map(|x| x.on_origin).collect();
    assert_eq!(on, [false, false, true]);

    let local = sb.path("local");
    init(&local);
    write_commit(&local, "x.txt", "x\n", "x");
    assert!(!log(&local, 0, 5).unwrap()[0].on_origin);
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
