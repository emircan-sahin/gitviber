//! End-to-end git scenarios against real repositories and a local bare "remote".

use crate::network::Net;
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
    commit(repo, msg, &CommitOptions::default()).unwrap();
}

const AMEND: CommitOptions = CommitOptions {
    amend: true,
    sign_off: false,
    no_verify: false,
    co_authors: Vec::new(),
};

#[test]
fn pull_modes_on_diverged_branches() {
    let sb = Sandbox::new("pull");
    let c = sb.remote_with_clones(2);
    let (a, b) = (&c[0], &c[1]);
    write_commit(a, "a.txt", "one\ntwo\nthree\nfour\n", "a appends");
    run(a, &["push", "-q"]).unwrap();
    write_commit(b, "b.txt", "b\n", "b adds a file");
    fetch(b, &Net::default()).unwrap();
    let st = status(b).unwrap();
    assert_eq!((st.ahead, st.behind), (1, 1));

    // Fast-forward only must refuse, and must not leave an operation behind.
    assert!(pull(b, "ff", &Net::default()).is_err());
    assert!(operation(b).is_none());
    // A clean merge finishes without stopping.
    assert!(!pull(b, "merge", &Net::default()).unwrap());
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
    push(a, false, None, &Net::default()).unwrap();
    commit(a, "mine, reworded", &AMEND).unwrap();
    let err = push(a, false, None, &Net::default()).unwrap_err();
    assert!(err.contains("non-fast-forward"), "{err}");
    push(a, true, None, &Net::default()).unwrap();
    // b pushes meanwhile; a, not having fetched it, amends again: the lease refuses.
    fetch(b, &Net::default()).unwrap();
    run(b, &["merge", "-q", "--ff-only", "origin/main"]).unwrap();
    write_commit(b, "b.txt", "b\n", "theirs");
    push(b, false, None, &Net::default()).unwrap();
    commit(a, "mine, again", &AMEND).unwrap();
    assert!(push(a, true, None, &Net::default()).is_err());
}

/// While "Force push?" is asked, someone pushes and the background fetch brings their commit
/// in, which moves the lease onto it. The force push must still refuse to drop it.
#[test]
fn force_push_after_a_background_fetch_keeps_their_commit() {
    let sb = Sandbox::new("lease-fetch");
    let c = sb.remote_with_clones(2);
    let (a, b) = (&c[0], &c[1]);
    write_commit(a, "a.txt", "mine\n", "mine");
    push(a, false, None, &Net::default()).unwrap();
    commit(a, "mine, reworded", &AMEND).unwrap();
    let err = push(a, false, None, &Net::default()).unwrap_err();
    assert!(err.contains("non-fast-forward"), "{err}");

    fetch(b, &Net::default()).unwrap();
    run(b, &["merge", "-q", "--ff-only", "origin/main"]).unwrap();
    write_commit(b, "b.txt", "b\n", "theirs");
    push(b, false, None, &Net::default()).unwrap();
    let theirs = run_text(b, &["rev-parse", "HEAD"]).unwrap();
    fetch(a, &Net::default()).unwrap();

    assert!(push(a, true, None, &Net::default()).is_err());
    let remote = run_text(&sb.path("origin.git"), &["rev-parse", "main"]).unwrap();
    assert_eq!(remote, theirs);
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
    commit(a, "Add f\n\nWhy it matters.", &CommitOptions::default()).unwrap();
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
    push(b, false, None, &Net::default()).unwrap();
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
    fetch(b, &Net::default()).unwrap();
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

/// Blame: each line's commit with its whole message, lines not committed yet, files git
/// doesn't have yet, and the user's blame.ignoreRevsFile.
#[test]
fn blame_attributes_lines() {
    let sb = Sandbox::new("blame");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "f.txt", "a\nb\n", "First");
    write_commit(
        &r,
        "f.txt",
        "a\nB\n",
        "Second\n\nWhy it changed.\n\nCo-Authored-By: Claude <noreply@anthropic.com>",
    );
    fs::write(r.join("f.txt"), "a\nB\nc\n").unwrap();

    let b = blame(&r, "f.txt").unwrap();
    let at = |b: &Blame, i: usize| b.commits[b.lines[i] as usize].message.clone();
    assert_eq!(b.lines.len(), 3);
    assert_eq!(at(&b, 0), "First");
    assert!(at(&b, 1).starts_with("Second\n\nWhy it changed."));
    assert!(at(&b, 1).ends_with("Co-Authored-By: Claude <noreply@anthropic.com>"));
    let new = &b.commits[b.lines[2] as usize];
    assert_eq!(new.sha, "0".repeat(40));
    let second = &b.commits[b.lines[1] as usize];
    assert_eq!(
        (second.author_name.as_str(), second.path.as_str()),
        ("T", "f.txt")
    );

    // Untracked, or only staged: every line is new, not an error.
    fs::write(r.join("u.txt"), "x\n").unwrap();
    assert!(blame(&r, "u.txt").unwrap().lines.is_empty());
    fs::write(r.join("s.txt"), "x\n").unwrap();
    stage(&r, &["s.txt".into()]).unwrap();
    let staged = blame(&r, "s.txt").unwrap();
    assert!(staged.lines.is_empty() || staged.commits.iter().all(|c| c.sha == "0".repeat(40)));

    // git skips the revisions the user listed to ignore.
    let sha = second.sha.clone();
    fs::write(r.join(".git-blame-ignore-revs"), format!("{sha}\n")).unwrap();
    run(
        &r,
        &["config", "blame.ignoreRevsFile", ".git-blame-ignore-revs"],
    )
    .unwrap();
    assert_eq!(at(&blame(&r, "f.txt").unwrap(), 1), "First");
    assert!(blame(&r, "f.txt").unwrap().unavailable.is_none());

    // A Git LFS file: git has only its pointer, whose lines say nothing about the file's.
    let pointer = format!(
        "version https://git-lfs.github.com/spec/v1\noid sha256:{}\nsize 12345\n",
        "a".repeat(64)
    );
    write_commit(&r, "big.bin", &pointer, "Add big file");
    let lfs = blame(&r, "big.bin").unwrap();
    assert!(lfs.unavailable.is_some() && lfs.lines.is_empty());
}

/// History search: words, author, pickaxe, a path, a file followed through a rename, a SHA.
#[test]
fn log_search_narrows_and_pages() {
    let sb = Sandbox::new("logsearch");
    let c = sb.remote_with_clones(1);
    let a = &c[0];
    write_commit(
        a,
        "src/auth.rs",
        "fn login() {}\n",
        "Fix auth (login) [urgent]",
    );
    run(a, &["push", "-q"]).unwrap();
    write_commit(
        a,
        "src/auth.rs",
        "fn login() {}\nfn logout() {}\n",
        "Add logout",
    );
    run(a, &["mv", "src/auth.rs", "src/session.rs"]).unwrap();
    commit(a, "Rename auth to session", &CommitOptions::default()).unwrap();
    run(
        a,
        &[
            "-c",
            "user.name=Other",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "Tidy",
        ],
    )
    .unwrap();

    let subjects = |f: &LogFilter| -> Vec<String> {
        let log = log_filtered(a, None, 0, 50, f).unwrap();
        log.into_iter().map(|c| c.subject).collect()
    };
    // Regex characters are taken as typed, case is ignored, and every word must match.
    let words = |w: &[&str]| LogFilter {
        grep: w.iter().map(|s| s.to_string()).collect(),
        ..Default::default()
    };
    assert_eq!(
        subjects(&words(&["(LOGIN) [urgent"])),
        ["Fix auth (login) [urgent]"]
    );
    assert_eq!(
        subjects(&words(&["auth", "rename"])),
        ["Rename auth to session"]
    );
    let author = LogFilter {
        author: vec!["other".into()],
        ..Default::default()
    };
    assert_eq!(subjects(&author), ["Tidy"]);
    let code = LogFilter {
        code: Some("logout".into()),
        ..Default::default()
    };
    assert_eq!(subjects(&code), ["Add logout"]);

    // A path alone stops at the rename; followed, it goes on under the old name.
    let mut path = LogFilter {
        paths: vec!["src/session.rs".into()],
        ..Default::default()
    };
    assert_eq!(subjects(&path), ["Rename auth to session"]);
    path.follow = true;
    let followed = log_filtered(a, None, 0, 50, &path).unwrap();
    let files: Vec<_> = followed.iter().map(|c| c.file.as_deref()).collect();
    assert_eq!(
        files,
        [
            Some("src/session.rs"),
            Some("src/auth.rs"),
            Some("src/auth.rs")
        ]
    );
    // The flags hold deep in a filtered history: only the first auth commit was pushed.
    assert!(followed[1].unpushed && !followed[1].on_origin);
    assert!(!followed[2].unpushed && followed[2].on_origin);
    // Pages are pages of the matches (git's own --skip counts every commit with -S or --follow).
    assert!(log_filtered(a, None, 1, 1, &code).unwrap().is_empty());
    assert_eq!(
        log_filtered(a, None, 1, 1, &path).unwrap()[0].subject,
        "Add logout"
    );

    // A SHA prefix finds its commit; unknown or malformed ones find nothing.
    let head = log(a, None, 0, 1).unwrap()[0].sha.clone();
    assert_eq!(find_commit(a, &head[..8]).unwrap().unwrap().sha, head);
    assert!(find_commit(a, "0000000").unwrap().is_none());
    assert!(find_commit(a, "--all").unwrap().is_none());
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

    assert!(
        pull(b, "rebase", &Net::default()).unwrap(),
        "should stop on the conflict"
    );
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
    push(a, false, None, &Net::default()).unwrap();
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
    push(a, false, None, &Net::default()).unwrap();
    assert_eq!(status(a).unwrap().upstream.as_deref(), Some("gh/feat"));

    // Several remotes and none is origin: the user picks.
    let other = sb.path("other.git");
    run(&sb.0, &["init", "-q", "--bare", other.to_str().unwrap()]).unwrap();
    run(a, &["remote", "add", "other", other.to_str().unwrap()]).unwrap();
    switch_branch(a, "feat2", true).unwrap();
    let st = status(a).unwrap();
    assert_eq!((st.publish.as_deref(), st.remotes.len()), (None, 2));
    assert!(push(a, false, None, &Net::default())
        .unwrap_err()
        .contains("several remotes"));
    assert!(push(a, false, Some("nope"), &Net::default()).is_err());
    push(a, false, Some("other"), &Net::default()).unwrap();
    assert_eq!(status(a).unwrap().upstream.as_deref(), Some("other/feat2"));

    // remote.pushDefault decides when set.
    set_push_default(a, "other").unwrap();
    switch_branch(a, "feat3", true).unwrap();
    assert_eq!(status(a).unwrap().publish.as_deref(), Some("other"));

    // No remote at all: a clear message, not a raw git error.
    let lone = sb.path("lone");
    init(&lone);
    write_commit(&lone, "x.txt", "x\n", "x");
    assert!(push(&lone, false, None, &Net::default())
        .unwrap_err()
        .contains("no remote"));
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
    commit(&r, "bin", &CommitOptions::default()).unwrap();

    fs::write(r.join("bin.dat"), [0u8, 9, 9]).unwrap();
    fs::write(r.join("crlf.txt"), "a\r\nB\r\nc\r\n").unwrap();
    fs::write(r.join("nonl.txt"), "x\ny\n").unwrap();
    let wt = |p: &str| vfs::read_file(&r, p);

    let bin = diff_pair(&r, "unstaged", "bin.dat", None, None, None, None, wt).unwrap();
    assert!(bin.modified.binary && bin.rows.is_empty());

    let crlf = diff_pair(&r, "unstaged", "crlf.txt", None, None, None, None, wt).unwrap();
    let kinds: Vec<u8> = crlf.rows.iter().map(|x| x.k).collect();
    assert_eq!(kinds, vec![0, 2, 1, 0]);
    // Emphasis must not include the \r.
    assert!(crlf.rows[2].e.iter().all(|[_, end]| *end <= 1));

    let nonl = diff_pair(&r, "unstaged", "nonl.txt", None, None, None, None, wt).unwrap();
    assert!(
        nonl.rows.iter().all(|x| x.o <= 2 && x.n <= 2),
        "line counts match the frontend's split"
    );
}

#[test]
fn autocrlf_diffs_like_git() {
    let sb = Sandbox::new("autocrlf");
    let r = sb.path("r");
    init(&r);
    run(&r, &["config", "core.autocrlf", "true"]).unwrap();
    // Stored with LF, checked out (here: written) with CRLF.
    write_commit(&r, "a.txt", "a\r\nb\r\nc\r\n", "crlf");
    assert_eq!(
        run_text(&r, &["cat-file", "blob", "HEAD:a.txt"]).unwrap(),
        "a\nb\nc\n"
    );
    fs::write(r.join("a.txt"), "a\r\nB\r\nc\r\n").unwrap();
    let wt = |p: &str| vfs::read_file(&r, p);
    for kind in ["unstaged", "worktree"] {
        let pair = diff_pair(&r, kind, "a.txt", None, None, None, None, wt).unwrap();
        let kinds: Vec<u8> = pair.rows.iter().map(|x| x.k).collect();
        assert_eq!(kinds, vec![0, 2, 1, 0], "{kind}: only the changed line");
        assert!(!pair.eol_only);
    }
    stage(&r, &["a.txt".into()]).unwrap();
    commit(&r, "B", &CommitOptions::default()).unwrap();
    let head = rev(&r, "HEAD");
    let pair = diff_pair(&r, "commit", "a.txt", None, Some(&head), None, None, wt).unwrap();
    assert_eq!(pair.modified.text, "a\r\nB\r\nc\r\n");
    assert_eq!(pair.rows.iter().filter(|x| x.k != 0).count(), 2);

    // Without the setting a CRLF copy of an LF file is a real change, of line endings only.
    run(&r, &["config", "core.autocrlf", "false"]).unwrap();
    write_commit(&r, "lf.txt", "x\ny\n", "lf");
    fs::write(r.join("lf.txt"), "x\r\ny\r\n").unwrap();
    let pair = diff_pair(&r, "unstaged", "lf.txt", None, None, None, None, wt).unwrap();
    assert!(pair.eol_only && pair.rows.iter().any(|x| x.k != 0));
    // Ignoring whitespace hides it, and says so.
    let pair = diff_pair(
        &r,
        "unstaged",
        "lf.txt",
        None,
        None,
        None,
        Some("amount"),
        wt,
    )
    .unwrap();
    assert!(pair.whitespace_hidden && pair.rows.iter().all(|x| x.k == 0));
}

const LFS_OID: &str = "4d7a214614ab2935c943f9e0ff69d22eadbb8f32b1258daaa5e2ca24d17e2393";

fn lfs_pointer(oid: &str, size: usize) -> String {
    format!("version https://git-lfs.github.com/spec/v1\noid sha256:{oid}\nsize {size}\n")
}

/// Pointers committed as plain files: no git-lfs needed to read what the store has or lacks.
#[test]
fn lfs_pointers_show_the_object_or_its_size() {
    let sb = Sandbox::new("lfs-pointer");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "big.bin", &lfs_pointer(LFS_OID, 13_002_342), "pointer");
    write_commit(&r, "pic.png", &lfs_pointer(LFS_OID, 5), "pointer");
    let head = rev(&r, "HEAD");
    let none = |_: &str| FileText::default();
    let pair = diff_pair(&r, "commit", "big.bin", None, Some(&head), None, None, none).unwrap();
    assert_eq!(
        pair.modified.lfs_missing.as_deref(),
        Some("LFS object not downloaded (12.4 MB)")
    );
    assert!(pair.rows.is_empty());
    let got = media(
        &r,
        "commit",
        "pic.png",
        None,
        Some(&head),
        None,
        false,
        |_| Err("unused".into()),
    );
    assert_eq!(got.unwrap_err(), "LFS object not downloaded (5 B)");

    // Once downloaded, the object stands in for the pointer on every side.
    let dir = r.join(format!(
        ".git/lfs/objects/{}/{}",
        &LFS_OID[..2],
        &LFS_OID[2..4]
    ));
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join(LFS_OID), "hello").unwrap();
    let got = media(
        &r,
        "commit",
        "pic.png",
        None,
        Some(&head),
        None,
        false,
        |_| Err("unused".into()),
    );
    assert_eq!(got.unwrap(), b"hello");
    let wt = |p: &str| vfs::read_file(&r, p);
    let pair = diff_pair(&r, "worktree", "pic.png", None, None, None, None, wt).unwrap();
    assert_eq!(
        (pair.original.text.as_str(), pair.modified.text.as_str()),
        ("hello", "hello")
    );
}

/// The real thing, when git-lfs is installed: the stored side is the object, never a download.
#[test]
fn lfs_tracked_file_diffs_as_its_content() {
    let sb = Sandbox::new("lfs");
    let r = sb.path("r");
    init(&r);
    if run(&r, &["lfs", "install", "--local"]).is_err() {
        eprintln!("git-lfs is not installed; skipping");
        return;
    }
    write_commit(
        &r,
        ".gitattributes",
        "*.png filter=lfs diff=lfs merge=lfs -text\n",
        "lfs",
    );
    write_commit(&r, "pic.png", "\u{89}PNG old", "pic");
    let stored = run(&r, &["cat-file", "blob", "HEAD:pic.png"]).unwrap();
    assert!(
        crate::lfs::pointer(&stored).is_some(),
        "stored as a pointer"
    );
    fs::write(r.join("pic.png"), "\u{89}PNG new").unwrap();
    let wt = |p: &str| vfs::read_media(&r, p);
    let before = media(&r, "unstaged", "pic.png", None, None, None, true, wt).unwrap();
    assert_eq!(before, "\u{89}PNG old".as_bytes());

    // Gone from the store (a partial clone, say): it says so rather than fetching it.
    let oid = crate::lfs::pointer(&stored).unwrap().oid;
    let common = r.join(".git/lfs/objects");
    fs::remove_file(common.join(&oid[..2]).join(&oid[2..4]).join(&oid)).unwrap();
    let err = media(&r, "unstaged", "pic.png", None, None, None, true, wt).unwrap_err();
    assert!(err.starts_with("LFS object not downloaded"), "{err}");
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

/// git runs in English (the app reads its messages) and UTF-8, so a translated git can't
/// break the checks, and non-ASCII names and messages still come back as written.
#[test]
fn english_git_keeps_utf8_names_and_messages() {
    use std::ffi::OsStr;
    let cmd = command(Path::new("."), &["status"]);
    let env: Vec<_> = cmd.get_envs().collect();
    let lc_all = OsStr::new("LC_ALL");
    let language = OsStr::new("LANGUAGE");
    assert!(env.contains(&(lc_all, Some(OsStr::new("en_US.UTF-8")))));
    assert!(env.contains(&(language, None)), "LANGUAGE is cleared");

    let sb = Sandbox::new("utf8");
    let r = sb.path("r");
    init(&r);
    let (path, message) = ("şehir/ağaç 🌳.txt", "Grüße, çay ve 日本語 🎉");
    write_commit(&r, path, "x\n", message);
    let head = &log(&r, None, 0, 1).unwrap()[0];
    assert_eq!(head.subject, message);
    let files = commit_files(&r, &head.sha).unwrap();
    assert_eq!(files[0].path, path);
    fs::write(r.join(path), "y\n").unwrap();
    assert_eq!(status(&r).unwrap().unstaged[0].path, path);
    let outside = sb.path("plain");
    fs::create_dir_all(&outside).unwrap();
    assert_eq!(toplevel(&outside).unwrap_err(), NOT_A_REPO);

    // A German setup (Homebrew's git ships its translations) still gets English.
    let mut german = std::process::Command::new("git");
    german
        .env("LANGUAGE", "de")
        .env("LANG", "de_DE.UTF-8")
        .env("LC_ALL", "de_DE.UTF-8")
        .env("PATH", search_path())
        .current_dir(&outside)
        .args(["rev-parse", "--show-toplevel"]);
    let out = in_english(&mut german).output().unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("not a git repository"), "{err}");
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

fn executable(path: &Path, script: &str) {
    fs::write(path, script).unwrap();
    std::process::Command::new("chmod")
        .args(["+x", path.to_str().unwrap()])
        .status()
        .unwrap();
}

/// Launched from Finder the app has a bare PATH; a hook calling a tool only the login shell
/// adds (node from nvm, say) failed with 127. The "login shell" here prepends one folder.
#[cfg(unix)]
#[test]
fn hooks_find_tools_on_the_login_shell_path() {
    use std::ffi::OsStr;
    let sb = Sandbox::new("login-path");
    let tools = sb.path("tools");
    fs::create_dir_all(&tools).unwrap();
    executable(&tools.join("gitviber-lint"), "#!/bin/sh\nexit 0\n");
    let shell = sb.path("login-sh");
    // Called as `login-sh -ilc <command>`.
    executable(
        &shell,
        &format!(
            "#!/bin/sh\necho 'Welcome back!'\nPATH=\"{}:$PATH\"; export PATH\nexec /bin/sh -c \"$2\"\n",
            tools.display()
        ),
    );
    let r = sb.path("r");
    init(&r);
    executable(
        &r.join(".git/hooks/pre-commit"),
        "#!/bin/sh\nexec gitviber-lint\n",
    );
    fs::write(r.join("a.txt"), "a\n").unwrap();
    stage(&r, &["a.txt".into()]).unwrap();
    let commit_with = |path: &OsStr| {
        let mut cmd = command(&r, &["commit", "-q", "-m", "hooked"]);
        cmd.env("PATH", path);
        exec(cmd, "git commit", &[], None, None)
    };

    let app_path = OsStr::new("/usr/bin:/bin:/usr/sbin:/sbin");
    let err = commit_with(&merge_paths(None, app_path)).unwrap_err();
    assert!(err.contains("gitviber-lint"), "{err}");

    let login = crate::shell::probe_path(&shell, std::time::Duration::from_secs(3)).unwrap();
    let merged = merge_paths(Some(&login), app_path);
    assert!(merged
        .to_str()
        .unwrap()
        .starts_with(tools.to_str().unwrap()));
    commit_with(&merged).unwrap();
    assert_eq!(log(&r, None, 0, 1).unwrap()[0].subject, "hooked");
}

#[test]
fn amend_without_message_keeps_the_old_one() {
    let sb = Sandbox::new("amend");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "a\n", "original message");
    fs::write(r.join("b.txt"), "b\n").unwrap();
    stage(&r, &["b.txt".into()]).unwrap();
    commit(&r, "  ", &AMEND).unwrap();
    let head = &log(&r, None, 0, 5).unwrap()[0];
    assert_eq!(head.subject, "original message");
    assert_eq!(log(&r, None, 0, 5).unwrap().len(), 1);
}

/// Co-authors become trailers git itself formats, sign-off adds the committer's line and
/// --no-verify gets past a failing hook; the commit header reads the trailers back.
#[test]
fn commit_options_trailers_sign_off_and_skipped_hooks() {
    let sb = Sandbox::new("commit-options");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "a\n", "base");
    let hook = r.join(".git/hooks/pre-commit");
    fs::write(&hook, "#!/bin/sh\necho 'lint failed' >&2\nexit 1\n").unwrap();
    std::process::Command::new("chmod")
        .args(["+x", hook.to_str().unwrap()])
        .status()
        .unwrap();
    fs::write(r.join("b.txt"), "b\n").unwrap();
    stage(&r, &["b.txt".into()]).unwrap();

    let err = commit(&r, "Add b", &CommitOptions::default()).unwrap_err();
    assert!(err.contains("lint failed"), "{err}");
    let claude = "Claude <noreply@anthropic.com>";
    let opts = CommitOptions {
        sign_off: true,
        no_verify: true,
        co_authors: vec![claude.into()],
        ..Default::default()
    };
    commit(&r, "Add b\n\nWhy it matters.", &opts).unwrap();
    let head = &log(&r, None, 0, 1).unwrap()[0];
    assert_eq!(head.subject, "Add b");
    assert_eq!(
        head.body,
        format!("Why it matters.\n\nSigned-off-by: T <t@example.com>\nCo-authored-by: {claude}")
    );
    let details = commit_details(&r, &head.sha).unwrap();
    assert_eq!(
        (details.signature.as_str(), details.sign_expected),
        ("N", false)
    );
    assert_eq!(
        details.trailers,
        [
            ("Signed-off-by".to_string(), "T <t@example.com>".to_string()),
            ("Co-authored-by".to_string(), claude.to_string()),
        ]
    );

    // Amending without a new message keeps it and still takes a new co-author; the hook
    // runs again once --no-verify is off.
    let ada = "Ada <ada@example.com>";
    let amend = |no_verify| CommitOptions {
        amend: true,
        no_verify,
        co_authors: vec![ada.into()],
        ..Default::default()
    };
    assert!(commit(&r, "", &amend(false)).is_err());
    commit(&r, "", &amend(true)).unwrap();
    let head = &log(&r, None, 0, 1).unwrap()[0];
    assert!(
        head.body.ends_with(&format!("Co-authored-by: {ada}")),
        "{}",
        head.body
    );
    assert_eq!(log(&r, None, 0, 5).unwrap().len(), 2);

    // A co-author can't smuggle in a line of its own.
    let bad = CommitOptions {
        co_authors: vec!["Eve <e@x>\nSigned-off-by: Mallory <m@x>".into()],
        no_verify: true,
        ..Default::default()
    };
    assert!(commit(&r, "x", &bad).is_err());

    // Suggestions: co-authors and authors, newest first, never the user.
    assert_eq!(recent_authors(&r).unwrap(), [claude, ada]);
}

/// The template's text comes back the way git starts the editor with it, comments gone.
#[test]
fn commit_template_is_read_without_comments() {
    let sb = Sandbox::new("template");
    let r = sb.path("r");
    init(&r);
    assert_eq!(commit_template(&r), None);
    fs::write(
        r.join(".git/msg"),
        "\nWhy:\n# say why, not what\n\n\nRefs:\n",
    )
    .unwrap();
    run(&r, &["config", "commit.template", ".git/msg"]).unwrap();
    assert_eq!(commit_template(&r).as_deref(), Some("Why:\n\nRefs:"));
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
    checkout(b, "origin", None, 1, "feat", true, &Net::default()).unwrap();
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
    assert!(checkout(b, "origin", None, 1, "feat", true, &Net::default()).is_err());
    assert!(run(b, &["log", "--oneline", "feat"])
        .map(|o| String::from_utf8_lossy(&o).contains("local only"))
        .unwrap());

    // Fork PRs land on pr/<n>; checking out again while on it just fast-forwards.
    run(a, &["push", "-q", "origin", "HEAD:refs/pull/7/head"]).unwrap();
    checkout(
        b,
        "origin",
        None,
        7,
        "someones-branch",
        false,
        &Net::default(),
    )
    .unwrap();
    assert_eq!(status(b).unwrap().branch.as_deref(), Some("pr/7"));
    write_commit(a, "f.txt", "1\n2\n3\n4\n", "f4");
    run(a, &["push", "-q", "origin", "HEAD:refs/pull/7/head"]).unwrap();
    checkout(
        b,
        "origin",
        None,
        7,
        "someones-branch",
        false,
        &Net::default(),
    )
    .unwrap();
    assert_eq!(fs::read_to_string(b.join("f.txt")).unwrap(), "1\n2\n3\n4\n");
    // Leaving it and coming back fast-forwards it by fetching straight into the branch.
    write_commit(a, "f.txt", "5\n", "f5");
    run(a, &["push", "-q", "origin", "HEAD:refs/pull/7/head"]).unwrap();
    switch_branch(b, "main", false).unwrap();
    checkout(
        b,
        "origin",
        None,
        7,
        "someones-branch",
        false,
        &Net::default(),
    )
    .unwrap();
    assert_eq!(fs::read_to_string(b.join("f.txt")).unwrap(), "5\n");
}

/// Checking out a PR never goes through FETCH_HEAD, which another fetch (the background
/// one, a terminal) may rewrite between the fetch and its use: it stays as that fetch left it.
#[test]
fn pr_checkout_leaves_fetch_head_alone() {
    use crate::github::checkout;
    let sb = Sandbox::new("prco-fh");
    let c = sb.remote_with_clones(2);
    let (a, b) = (&c[0], &c[1]);
    write_commit(a, "f.txt", "pr\n", "pr");
    run(a, &["push", "-q", "origin", "HEAD:refs/pull/3/head"]).unwrap();
    let pr = run_text(a, &["rev-parse", "HEAD"]).unwrap();
    switch_branch(a, "feat", true).unwrap();
    write_commit(a, "g.txt", "g\n", "feat");
    run(a, &["push", "-q", "-u", "origin", "feat"]).unwrap();
    let feat = run_text(a, &["rev-parse", "HEAD"]).unwrap();

    run(b, &["fetch", "-q", "origin", "main"]).unwrap();
    let fetch_head = || fs::read_to_string(b.join(".git/FETCH_HEAD")).unwrap();
    let before = fetch_head();
    checkout(b, "origin", None, 3, "theirs", false, &Net::default()).unwrap();
    assert_eq!(run_text(b, &["rev-parse", "HEAD"]).unwrap(), pr);
    checkout(b, "origin", None, 4, "feat", true, &Net::default()).unwrap();
    assert_eq!(run_text(b, &["rev-parse", "HEAD"]).unwrap(), feat);
    assert_eq!(fetch_head(), before);
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

/// Worktrees made beside the project share `<project>.worktrees`; removing the last one
/// takes the folder along, while one still inside keeps it.
#[test]
fn removing_the_last_worktree_removes_its_folder() {
    let sb = Sandbox::new("wtdir");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "a\n", "base");
    run(&r, &["branch", "one"]).unwrap();
    run(&r, &["branch", "two"]).unwrap();
    let one = add_worktree(&r, "one").unwrap();
    let two = add_worktree(&r, "two").unwrap();
    let dir = sb.path("r.worktrees");
    assert!(dir.is_dir());

    remove_worktree(&r, &one, false).unwrap();
    assert!(dir.is_dir(), "two is still in there");
    remove_worktree(&r, &two, false).unwrap();
    assert!(!dir.exists());
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
    commit(&r, "add sub", &CommitOptions::default()).unwrap();
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

    let pair = diff_pair(&r, "unstaged", "sub", None, None, None, None, read).unwrap();
    assert_eq!(pair.original.text, format!("Subproject commit {old}\n"));
    assert_eq!(pair.modified.text, format!("Subproject commit {new}\n"));
    assert!(pair.rows.iter().any(|row| row.k != 0));

    fs::write(sub.join("l.txt"), "dirty\n").unwrap();
    let pair = diff_pair(&r, "unstaged", "sub", None, None, None, None, read).unwrap();
    assert_eq!(
        pair.modified.text,
        format!("Subproject commit {new}-dirty\n")
    );

    stage(&r, &["sub".into()]).unwrap();
    commit(&r, "bump sub", &CommitOptions::default()).unwrap();
    let head = log(&r, None, 0, 1).unwrap().remove(0).sha;
    let pair = diff_pair(&r, "commit", "sub", None, Some(&head), None, None, read).unwrap();
    assert_eq!(pair.original.text, format!("Subproject commit {old}\n"));
    assert_eq!(pair.modified.text, format!("Subproject commit {new}\n"));
    // A plain directory is still not a submodule.
    assert!(
        !diff_pair(&r, "unstaged", "nope", None, None, None, None, read)
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

    assert!(create_tag(&r, "-f", &base, None).is_err());
    assert!(create_tag(&r, "bad..name", &base, None).is_err());
    create_tag(&r, "v1.0", &base, None).unwrap();
    assert!(
        create_tag(&r, "v1.0", &base, None).is_err(),
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
    assert!(create_tag(&r, "@", &base, None).is_err());
}

#[test]
fn drops_pushed_follows_ancestry_not_log_order() {
    let sb = Sandbox::new("drops");
    let c = sb.remote_with_clones(2);
    let (a, b) = (&c[0], &c[1]);
    // P is pushed; merged in as the second parent, "target" lists above it.
    write_commit(a, "p.txt", "p\n", "old pushed");
    run(a, &["push", "-q"]).unwrap();
    write_commit(b, "t.txt", "t\n", "target");
    let t = run_text(b, &["rev-parse", "HEAD"]).unwrap();
    run(b, &["fetch", "-q"]).unwrap();
    run(b, &["reset", "-q", "--hard", "origin/main"]).unwrap();
    run(b, &["merge", "-q", "--no-ff", "--no-edit", t.trim()]).unwrap();

    let commits = log(b, None, 0, 10).unwrap();
    let subjects: Vec<&str> = commits.iter().map(|x| x.subject.as_str()).collect();
    assert_eq!(subjects[1..], ["target", "old pushed", "base"]);
    let (merge, target) = (&commits[0].sha, &commits[1].sha);
    // Only unpushed commits sit above "target", yet resetting to it drops the pushed P.
    assert!(commits[0].unpushed && commits[1].unpushed && !commits[2].unpushed);
    assert!(drops_pushed(b, target).unwrap());
    assert!(!drops_pushed(b, merge).unwrap());
    // Undoing the merge (moving to its first parent, P) drops nothing pushed.
    assert!(!drops_pushed(b, &commits[0].parents[0]).unwrap());
    assert!(commits[2].on_origin && !commits[1].on_origin);
}

#[test]
fn gone_upstream_is_unknown_not_pushed() {
    let sb = Sandbox::new("gone");
    let c = sb.remote_with_clones(1);
    let a = &c[0];
    switch_branch(a, "feat", true).unwrap();
    write_commit(a, "f.txt", "f\n", "feature");
    push(a, false, None, &Net::default()).unwrap();
    run(a, &["push", "-q", "origin", "--delete", "feat"]).unwrap();
    fetch(a, &Net::default()).unwrap();
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
    fetch(a, &Net::default()).unwrap();
    let defaults: Vec<String> = branches(a)
        .unwrap()
        .into_iter()
        .filter(|b| b.remote_default)
        .map(|b| b.name)
        .collect();
    assert_eq!(defaults, ["origin/main"]);
    delete_remote_branch(a, "origin/feat/x", &Net::default()).unwrap();
    let left = String::from_utf8(run(a, &["ls-remote", "--heads", "origin"]).unwrap()).unwrap();
    assert!(!left.contains("feat/x"), "{left}");
    // The tracking ref goes with it, so the picker drops the row without a fetch.
    assert!(!branches(a)
        .unwrap()
        .iter()
        .any(|b| b.name == "origin/feat/x"));
    assert!(delete_remote_branch(a, "origin/main", &Net::default()).is_err());
    assert!(delete_remote_branch(a, "nope/x", &Net::default()).is_err());
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
        commit(r, "base", &CommitOptions::default())
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
        commit(r, "second", &CommitOptions::default())
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
        commit(r, "second, reworded", &CommitOptions::default())
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
    push(a, false, None, &Net::default()).unwrap();
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
    push(a, false, None, &Net::default()).unwrap();
    let before = rev(b, "HEAD");
    let j = Journal::default();
    j.record(b, Action::new("Pull", Mode::Keep), |r| {
        pull(r, "ff", &Net::default())
    })
    .unwrap();
    step(&j, b, false).unwrap();
    assert_eq!(rev(b, "HEAD"), before);
    step(&j, b, true).unwrap();

    fs::write(b.join("y.txt"), "y\n").unwrap();
    stage(b, &["y.txt".into()]).unwrap();
    j.record(b, Action::new("Commit", Mode::Soft), |r| {
        commit(r, "from b", &CommitOptions::default())
    })
    .unwrap();
    assert!(j.view(b).undo_blocked.is_none());
    push(b, false, None, &Net::default()).unwrap();
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

// ---------------------------------------------------------------- branches, tags, stash, cherry-pick

fn upstream_of(repo: &Path, branch: &str) -> Option<String> {
    let spec = format!("{branch}@{{upstream}}");
    run_text(repo, &["rev-parse", "--abbrev-ref", &spec])
        .ok()
        .map(|s| s.trim().to_string())
}

fn on_remote(repo: &Path, branch: &str) -> bool {
    let full = format!("refs/heads/{branch}");
    !run_text(repo, &["ls-remote", "origin", &full])
        .unwrap()
        .trim()
        .is_empty()
}

/// Renaming the checked-out branch with its remote: the new name is pushed and tracked, the
/// old one leaves the remote. Undo brings the old name back with its old upstream settings.
#[test]
fn rename_a_branch_here_and_on_the_remote() {
    let sb = Sandbox::new("rename");
    let c = sb.remote_with_clones(1);
    let a = &c[0];
    switch_branch(a, "feat", true).unwrap();
    write_commit(a, "f.txt", "f\n", "feat");
    push(a, false, None, &Net::default()).unwrap();
    assert!(rename_branch(a, "feat", "--evil", false, &Net::default()).is_err());
    // The remote default branch is refused before anything moves.
    assert!(rename_branch(a, "main", "trunk", true, &Net::default()).is_err());
    assert!(exists(a, "main"));

    let j = Journal::default();
    j.record(a, Action::new("Rename feat to feature", Mode::Keep), |r| {
        rename_branch(r, "feat", "feature", true, &Net::default())
    })
    .unwrap();
    assert_eq!(on_branch(a), "feature");
    assert!(!exists(a, "feat"));
    assert_eq!(upstream_of(a, "feature").as_deref(), Some("origin/feature"));
    assert!(on_remote(a, "feature") && !on_remote(a, "feat"));

    step(&j, a, false).unwrap();
    assert_eq!(on_branch(a), "feat");
    assert!(!exists(a, "feature"));
    let merge = run_text(a, &["config", "branch.feat.merge"]).unwrap();
    assert_eq!(merge.trim(), "refs/heads/feat");
    step(&j, a, true).unwrap();
    assert_eq!(on_branch(a), "feature");
    assert_eq!(upstream_of(a, "feature").as_deref(), Some("origin/feature"));

    // A local-only rename of a branch that isn't checked out.
    switch_branch(a, "side", true).unwrap();
    switch_branch(a, "feature", false).unwrap();
    rename_branch(a, "side", "aside", false, &Net::default()).unwrap();
    assert!(exists(a, "aside") && !exists(a, "side"));
    assert!(
        rename_branch(a, "aside", "x", true, &Net::default()).is_err(),
        "tracks nothing"
    );
}

/// A branch from a remote branch or a tag starts there without tracking it; the upstream is
/// set and unset on its own.
#[test]
fn create_branch_from_a_base_and_set_its_upstream() {
    let sb = Sandbox::new("newfrom");
    let c = sb.remote_with_clones(1);
    let a = &c[0];
    let base = rev(a, "HEAD");
    write_commit(a, "b.txt", "b\n", "local only");
    run(a, &["tag", "v1", &base]).unwrap();
    assert_eq!(tags(a).unwrap(), vec!["v1".to_string()]);

    create_branch(a, "from-remote", "refs/remotes/origin/main", false).unwrap();
    assert_eq!(on_branch(a), "main");
    assert_eq!(rev(a, "from-remote"), base);
    assert_eq!(upstream_of(a, "from-remote"), None);
    create_branch(a, "from-tag", "refs/tags/v1", true).unwrap();
    assert_eq!(on_branch(a), "from-tag");
    assert_eq!(rev(a, "HEAD"), base);
    assert!(create_branch(a, "bad", "main", false).is_err());
    assert!(create_branch(a, "bad", "refs/tags/nope", false).is_err());

    set_upstream(a, "from-tag", Some("origin/main")).unwrap();
    assert_eq!(upstream_of(a, "from-tag").as_deref(), Some("origin/main"));
    assert!(set_upstream(a, "from-tag", Some("origin/nope")).is_err());
    set_upstream(a, "from-tag", None).unwrap();
    assert_eq!(upstream_of(a, "from-tag"), None);
}

/// An annotated tag carries its message and goes along with `--follow-tags`; a lightweight
/// one needs its own push. Creating and deleting tags are undo entries.
/// Tags a fetch brings in while some other action runs (the background fetch during a slow
/// hook, a pull's) are not that action's: undoing it keeps them, and they record nothing.
#[test]
fn tags_fetched_during_an_action_are_not_part_of_it() {
    let sb = Sandbox::new("j-fetched-tag");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "a\n", "base");
    let j = Journal::default();
    fs::write(r.join("a.txt"), "b\n").unwrap();
    stage(&r, &["a.txt".into()]).unwrap();
    j.record(&r, Action::new("Commit", Mode::Soft), |r| {
        run(r, &["tag", "fetched", "HEAD"]).unwrap();
        commit(r, "second", &CommitOptions::default())
    })
    .unwrap();
    step(&j, &r, false).unwrap();
    assert!(tags(&r).unwrap().contains(&"fetched".to_string()));
    // A tag alone moving (a pull that only brought tags) is no entry and keeps Redo.
    j.record(&r, Action::new("Pull", Mode::Keep), |r| {
        run(r, &["tag", "v9", "HEAD"]).map(|_| ())
    })
    .unwrap();
    let v = j.view(&r);
    assert!(v.undo.is_empty() && v.redo.len() == 1);
}

#[test]
fn annotated_tags_push_and_undo() {
    let sb = Sandbox::new("tags");
    let c = sb.remote_with_clones(1);
    let a = &c[0];
    write_commit(a, "b.txt", "b\n", "release");
    let head = rev(a, "HEAD");
    let j = Journal::default();
    j.record(
        a,
        Action::new("Create tag v1", Mode::Keep).with_tags(),
        |r| create_tag(r, "v1", &head, Some("First release\n\nNotes")),
    )
    .unwrap();
    create_tag(a, "light", &head, Some("  ")).unwrap();
    assert!(create_tag(a, "--evil", &head, None).is_err());
    let kind = |t: &str| {
        run_text(a, &["cat-file", "-t", t])
            .unwrap()
            .trim()
            .to_string()
    };
    assert_eq!((kind("v1"), kind("light")), ("tag".into(), "commit".into()));
    let msg = run_text(a, &["tag", "-l", "--format=%(contents)", "v1"]).unwrap();
    assert!(msg.starts_with("First release\n\nNotes"), "{msg}");

    push_with_tags(a, false, None, &Net::default()).unwrap();
    let there = remote_tags(a, &Net::default()).unwrap();
    assert_eq!(
        (there.remote.as_str(), there.names.clone()),
        ("origin", vec!["v1".to_string()])
    );
    assert_eq!(
        push_tags(a, &["light".into()], &Net::default()).unwrap(),
        "origin"
    );
    assert_eq!(
        remote_tags(a, &Net::default()).unwrap().names,
        vec!["light", "v1"]
    );
    delete_remote_tag(a, "light", &Net::default()).unwrap();
    assert_eq!(remote_tags(a, &Net::default()).unwrap().names, vec!["v1"]);

    // Undo takes the tag away and redo brings back the same tag object, message and all.
    let object = rev(a, "refs/tags/v1");
    step(&j, a, false).unwrap();
    assert!(run(a, &["rev-parse", "--verify", "-q", "refs/tags/v1"]).is_err());
    step(&j, a, true).unwrap();
    assert_eq!(rev(a, "refs/tags/v1"), object);

    j.record(
        a,
        Action::new("Delete tag v1", Mode::Keep).with_tags(),
        |r| delete_tag(r, "v1"),
    )
    .unwrap();
    assert!(!tags(a).unwrap().contains(&"v1".to_string()));
    step(&j, a, false).unwrap();
    assert_eq!(rev(a, "refs/tags/v1"), object);
    // Moved outside the app since: no longer safe to undo or redo.
    run(a, &["tag", "-f", "v1", "HEAD~1"]).unwrap();
    assert!(j.view(a).redo_blocked.is_some());
}

/// Stash with untracked files, list and show it, pop it back. Actions name a stash by its
/// commit, so one pushed meanwhile (stash@{0} moving) can't redirect them.
#[test]
fn stash_push_and_pop_with_untracked_files() {
    let sb = Sandbox::new("stash");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "a\n", "base");
    assert!(stash_push(&r, "", false).is_err(), "nothing to stash");
    fs::write(r.join("a.txt"), "a changed\n").unwrap();
    fs::write(r.join("new.txt"), "new\n").unwrap();
    stash_push(&r, "wip: both", true).unwrap();
    // Untracked files leave the worktree too (literal pathspecs once kept them there).
    let left: Vec<String> = status(&r)
        .unwrap()
        .unstaged
        .iter()
        .map(|f| f.path.clone())
        .collect();
    assert!(left.is_empty(), "{left:?}");
    let list = stashes(&r).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].message, "On main: wip: both");
    let files = stash_files(&r, &list[0].sha).unwrap();
    assert_eq!(files.files.len(), 1);
    assert_eq!(files.files[0].path, "a.txt");
    assert_eq!(files.untracked.len(), 1);
    assert_eq!(
        (
            files.untracked[0].path.as_str(),
            files.untracked[0].status.as_str()
        ),
        ("new.txt", "?")
    );
    // The untracked side opens as a diff from nothing.
    let u = files.untracked_sha.unwrap();
    let pair = diff_pair(&r, "commit", "new.txt", None, Some(&u), None, None, |_| {
        FileText::default()
    })
    .unwrap();
    assert_eq!(
        (pair.original.exists, pair.modified.text.as_str()),
        (false, "new\n")
    );

    // Another stash on top: the first is stash@{1} now, still found by its commit.
    fs::write(r.join("a.txt"), "other\n").unwrap();
    stash_push(&r, "other", false).unwrap();
    let now = stashes(&r).unwrap();
    assert_eq!(now.len(), 2);
    stash_drop(&r, &now[0].sha).unwrap();
    assert!(!stash_apply(&r, &list[0].sha, true).unwrap());
    assert_eq!(fs::read_to_string(r.join("a.txt")).unwrap(), "a changed\n");
    assert_eq!(fs::read_to_string(r.join("new.txt")).unwrap(), "new\n");
    assert!(stashes(&r).unwrap().is_empty());
    assert!(stash_drop(&r, &list[0].sha).is_err());
}

/// A pop that conflicts keeps the stash and leaves conflicts to resolve like a merge's.
#[test]
fn stash_pop_conflict_keeps_the_stash() {
    let sb = Sandbox::new("stash-conflict");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "base\n", "base");
    fs::write(r.join("a.txt"), "stashed\n").unwrap();
    stash_push(&r, "mine", false).unwrap();
    write_commit(&r, "a.txt", "committed\n", "moved on");
    let sha = stashes(&r).unwrap()[0].sha.clone();
    assert!(stash_apply(&r, &sha, true).unwrap());
    let st = status(&r).unwrap();
    assert_eq!(st.conflicted.len(), 1);
    assert!(st.operation.is_none());
    assert_eq!(stashes(&r).unwrap().len(), 1);
    resolve_side(&r, "a.txt", "theirs").unwrap();
    assert_eq!(fs::read_to_string(r.join("a.txt")).unwrap(), "stashed\n");
    stash_drop(&r, &sha).unwrap();
}

/// A cherry-pick that conflicts goes through the continue flow and is then one undo entry;
/// one whose changes are already there is refused without leaving a pick in progress.
#[test]
fn cherry_pick_with_conflict_then_continue_and_undo() {
    let sb = Sandbox::new("pick");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "base\n", "base");
    run(&r, &["switch", "-q", "-c", "feat"]).unwrap();
    write_commit(&r, "a.txt", "feat\n", "feat edits a");
    let edit = rev(&r, "HEAD");
    write_commit(&r, "b.txt", "b\n", "feat adds b");
    let add = rev(&r, "HEAD");
    run(&r, &["switch", "-q", "main"]).unwrap();
    write_commit(&r, "a.txt", "main\n", "main edits a");
    let before = rev(&r, "HEAD");

    let j = Journal::default();
    let action = || Action::new("Cherry-pick", Mode::Keep);
    assert!(!j.record(&r, action(), |r| cherry_pick(r, &add)).unwrap());
    assert_eq!(fs::read_to_string(r.join("b.txt")).unwrap(), "b\n");
    let err = cherry_pick(&r, &add).unwrap_err();
    assert!(err.contains("already has"), "{err}");
    assert!(operation(&r).is_none());
    step(&j, &r, false).unwrap();
    assert_eq!(rev(&r, "HEAD"), before);

    assert!(j.record(&r, action(), |r| cherry_pick(r, &edit)).unwrap());
    assert_eq!(operation(&r).unwrap().kind, "cherry-pick");
    resolve_side(&r, "a.txt", "theirs").unwrap();
    assert!(!j.record(&r, action(), op_continue).unwrap());
    assert!(operation(&r).is_none());
    assert_eq!(log(&r, None, 0, 1).unwrap()[0].subject, "feat edits a");
    assert_eq!(fs::read_to_string(r.join("a.txt")).unwrap(), "feat\n");
    step(&j, &r, false).unwrap();
    assert_eq!(rev(&r, "HEAD"), before);
}

/// An agent's commit in worktree A lands on the branch of worktree B, git running in B: clean,
/// refused while B's changes are in the way, and stopped on conflicts there for B to finish.
#[test]
fn cherry_pick_into_another_worktree() {
    let sb = Sandbox::new("pick-wt");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "base\n", "base");
    let b = sb.path("b");
    run(
        &r,
        &["worktree", "add", "-q", "-b", "agent", b.to_str().unwrap()],
    )
    .unwrap();
    write_commit(&b, "a.txt", "agent\n", "agent edits a");
    let edit = rev(&b, "HEAD");
    write_commit(&b, "n.txt", "n\n", "agent adds n");
    let add = rev(&b, "HEAD");
    // The window is on worktree B (the agent's); main is checked out in r.
    let target = pick_target(&b, &r.canonicalize().unwrap().to_string_lossy()).unwrap();
    assert!(same_dir(&target.to_string_lossy(), &r));
    assert!(pick_target(&b, &b.canonicalize().unwrap().to_string_lossy()).is_err());
    assert!(pick_target(&b, "/tmp").is_err());

    let j = Journal::default();
    let action = || Action::new("Cherry-pick", Mode::Keep);
    let before = rev(&r, "HEAD");
    // An edit to a file the commit touches is in the way; one elsewhere is not.
    fs::write(r.join("n.txt"), "mine\n").unwrap();
    let err = cherry_pick_into(&target, &add).unwrap_err();
    assert!(err.contains("n.txt") && err.contains("main"), "{err}");
    assert!(operation(&r).is_none());
    fs::remove_file(r.join("n.txt")).unwrap();
    fs::write(r.join("a.txt"), "local\n").unwrap();
    let pick = |sha: &str| j.record(&target, action(), |t| cherry_pick_into(t, sha));
    assert!(!pick(&add).unwrap());
    assert_eq!(log(&r, None, 0, 1).unwrap()[0].subject, "agent adds n");
    assert_eq!(fs::read_to_string(r.join("a.txt")).unwrap(), "local\n");
    assert_eq!(rev(&b, "HEAD"), add, "the source branch doesn't move");
    step(&j, &target, false).unwrap();
    assert_eq!(rev(&r, "HEAD"), before);

    // Conflicting: the pick stays in progress in r, where its status shows it.
    run(&r, &["checkout", "-q", "--", "a.txt"]).unwrap();
    write_commit(&r, "a.txt", "main\n", "main edits a");
    let before = rev(&r, "HEAD");
    assert!(pick(&edit).unwrap());
    let st = status(&r).unwrap();
    assert_eq!(
        st.operation.as_ref().map(|o| o.kind.as_str()),
        Some("cherry-pick")
    );
    assert_eq!(st.conflicted.len(), 1);
    assert!(status(&b).unwrap().operation.is_none());
    // Continued from r's own window: one undo entry in r's history.
    resolve_side(&target, "a.txt", "theirs").unwrap();
    assert!(!j.record(&target, action(), op_continue).unwrap());
    assert_eq!(j.view(&target).undo.len(), 1);
    assert!(j.view(&b).undo.is_empty());
    step(&j, &target, false).unwrap();
    assert_eq!(rev(&r, "HEAD"), before);
}

/// A new repository has no commits yet: every view reads it as empty, and staging, unstaging
/// and the first commit work before HEAD exists.
#[test]
fn init_then_first_commit() {
    let sb = Sandbox::new("init");
    let r = sb.path("new");
    fs::create_dir_all(&r).unwrap();
    crate::git::init(&r).unwrap();
    identity(&r);
    assert!(crate::git::init(&r).is_err(), "already a repository");
    let first = run_text(&r, &["config", "--get", "init.defaultBranch"])
        .map(|b| b.trim().to_string())
        .unwrap_or_else(|_| "main".into());
    let st = status(&r).unwrap();
    assert_eq!(st.branch.as_deref(), Some(first.as_str()));
    assert!(st.head.is_none() && st.upstream.is_none() && st.remotes.is_empty());
    assert!(log(&r, None, 0, 10).unwrap().is_empty());
    branches(&r).unwrap();
    worktrees(&r).unwrap();

    fs::write(r.join("a.txt"), "a\n").unwrap();
    stage(&r, &["a.txt".into()]).unwrap();
    unstage(&r, &["a.txt".into()]).unwrap();
    assert_eq!(status(&r).unwrap().unstaged.len(), 1);
    stage(&r, &["a.txt".into()]).unwrap();
    let j = Journal::default();
    j.record(&r, Action::new("Commit", Mode::Soft), |r| {
        commit(r, "first", &CommitOptions::default())
    })
    .unwrap();
    assert_eq!(log(&r, None, 0, 10).unwrap().len(), 1);
    assert!(publish_remote(&r).unwrap_err().contains("no remote"));
    // Undoing the first commit makes the branch unborn again, its file still staged.
    step(&j, &r, false).unwrap();
    let st = status(&r).unwrap();
    assert!(st.head.is_none() && st.staged.len() == 1);
}

/// Clone streams progress, tracks origin, and never writes into a folder that holds something.
#[test]
fn clone_from_a_local_bare_remote() {
    let sb = Sandbox::new("clone");
    sb.remote_with_clones(0);
    // file:// takes git's transfer path (a plain path would hardlink), so progress shows.
    let url = format!("file://{}", sb.path("origin.git").display());
    let seen = std::sync::Arc::new(Mutex::new(Vec::new()));
    let sink = seen.clone();
    let running = crate::network::Running::default();
    let net = running.start("clone".into(), move |p| sink.lock().unwrap().push(p.phase));
    let dest = clone(&sb.0, &url, "mine", &net).unwrap();
    assert_eq!(PathBuf::from(&dest), sb.path("mine"));
    let st = status(Path::new(&dest)).unwrap();
    assert_eq!(st.branch.as_deref(), Some("main"));
    assert_eq!(st.upstream.as_deref(), Some("origin/main"));
    assert!(seen
        .lock()
        .unwrap()
        .iter()
        .any(|p| p == "Receiving objects"));

    fs::create_dir_all(sb.path("taken")).unwrap();
    fs::write(sb.path("taken/keep.txt"), "mine\n").unwrap();
    let err = clone(&sb.0, &url, "taken", &Net::default()).unwrap_err();
    assert!(err.contains("isn't empty"), "{err}");
    assert_eq!(fs::read_dir(sb.path("taken")).unwrap().count(), 1);
    fs::create_dir_all(sb.path("empty")).unwrap();
    clone(&sb.0, &url, "empty", &Net::default()).unwrap();
    for bad in ["", "..", "a/b"] {
        assert!(clone(&sb.0, &url, bad, &Net::default()).is_err(), "{bad}");
    }
    let missing = format!("file://{}", sb.path("nope.git").display());
    assert!(clone(&sb.0, &missing, "nope", &Net::default()).is_err());
    assert!(
        !sb.path("nope").exists(),
        "a failed clone leaves its folder behind"
    );
}

/// `cat` as the "agent": what it prints back is exactly what a real CLI would be sent.
#[test]
fn suggestion_input_follows_what_the_commit_takes() {
    use crate::suggest::{self, Scope};
    use std::sync::atomic::AtomicBool;
    let sb = Sandbox::new("suggest");
    let r = sb.path("r");
    init(&r);
    let go = |scope| suggest::run(&r, "cat", "PROMPT", scope, &AtomicBool::new(false));
    // A root commit amends from the empty tree.
    write_commit(&r, "a.txt", "one\n", "first");
    let sent = go(Scope::Amend).unwrap();
    assert!(sent.starts_with("PROMPT\n\n"), "{sent}");
    assert!(sent.contains("+one"), "{sent}");
    assert!(go(Scope::Staged).unwrap_err().contains("no changes"));

    fs::write(r.join("a.txt"), "two\n").unwrap();
    fs::write(r.join("new.txt"), "fresh\n").unwrap();
    // Commit all: tracked edits and untracked files both.
    let sent = go(Scope::All).unwrap();
    assert!(sent.contains("+two") && sent.contains("+fresh"), "{sent}");
    // Staged: only the index.
    stage(&r, &["new.txt".into()]).unwrap();
    let sent = go(Scope::Staged).unwrap();
    assert!(sent.contains("+fresh") && !sent.contains("+two"), "{sent}");

    // Past the cap the diff is cut and the prompt says so.
    fs::write(r.join("big.txt"), "x\n".repeat(80 * 1024)).unwrap();
    stage(&r, &["big.txt".into()]).unwrap();
    let sent = go(Scope::Staged).unwrap();
    assert!(sent.len() <= suggest::MAX_DIFF + 200, "{}", sent.len());
    assert!(sent.contains("cut off at 100 KB"));

    let err = suggest::run(
        &r,
        "no-such-agent-cli -p",
        "P",
        Scope::Staged,
        &AtomicBool::new(false),
    )
    .unwrap_err();
    assert!(err.contains("Couldn't find \"no-such-agent-cli\""), "{err}");
    let err = suggest::run(
        &r,
        "sh -c 'echo not logged in >&2; exit 3'",
        "P",
        Scope::Staged,
        &AtomicBool::new(false),
    )
    .unwrap_err();
    assert!(
        err.contains("code 3") && err.contains("not logged in"),
        "{err}"
    );
}

#[test]
fn cancelling_a_suggestion_stops_the_command_and_its_children() {
    use crate::suggest::{self, Scope, Suggester};
    let sb = Sandbox::new("suggest-cancel");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "one\n", "first");
    let s = Suggester::default();
    let flag = s.start();
    let started = std::time::Instant::now();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(200));
        s.cancel();
    });
    // The grandchild `sleep` keeps stdout open; only killing the group ends it.
    let err =
        suggest::run(&r, "sh -c 'sleep 30 & sleep 30'", "P", Scope::Amend, &flag).unwrap_err();
    assert_eq!(err, suggest::CANCELLED);
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
}

/// Discard puts the old versions in the Trash (a temporary folder under test), and undo
/// writes them back unless the file changed since.
#[test]
fn undo_and_redo_a_discard() {
    let sb = Sandbox::new("j-discard");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "a\n", "base");
    write_commit(&r, "dir/b.txt", "b\n", "b");
    fs::write(r.join("a.txt"), "agent's work\n").unwrap();
    fs::remove_file(r.join("dir/b.txt")).unwrap();
    let j = Journal::default();
    let paths: Vec<String> = vec!["a.txt".into(), "dir/b.txt".into()];
    j.discard(&r, &paths, || discard(&r, &paths)).unwrap();
    assert_eq!(fs::read_to_string(r.join("a.txt")).unwrap(), "a\n");
    assert!(r.join("dir/b.txt").exists());
    let v = j.view(&r);
    assert_eq!(v.undo[0].label, "Discard 2 files");
    assert!(v.undo_blocked.is_none());

    step(&j, &r, false).unwrap();
    assert_eq!(
        fs::read_to_string(r.join("a.txt")).unwrap(),
        "agent's work\n"
    );
    assert!(!r.join("dir/b.txt").exists(), "deleted again");
    step(&j, &r, true).unwrap();
    assert_eq!(fs::read_to_string(r.join("a.txt")).unwrap(), "a\n");

    // Written after the discard: undoing it now would lose that.
    fs::write(r.join("a.txt"), "newer\n").unwrap();
    assert!(j.view(&r).undo_blocked.is_some());
    assert!(step(&j, &r, false).is_err());
    assert_eq!(fs::read_to_string(r.join("a.txt")).unwrap(), "newer\n");
}

/// A fetch from the main worktree counts for its linked worktrees, which keep FETCH_HEADs of
/// their own; otherwise the background fetch would fetch again from each of them.
#[test]
fn last_fetch_counts_the_main_worktree() {
    let sb = Sandbox::new("lastfetch");
    let c = sb.remote_with_clones(1);
    let a = &c[0];
    let linked = sb.path("linked");
    run(
        a,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "side",
            linked.to_str().unwrap(),
        ],
    )
    .unwrap();
    assert_eq!(last_fetch(&linked), None);
    fetch(a, &Net::default()).unwrap();
    assert!(last_fetch(&linked).is_some());
    assert_eq!(last_fetch(&linked), last_fetch(a));
}

#[test]
fn tree_paths_and_text_at_read_a_commit_and_its_parent() {
    let sb = Sandbox::new("tree-paths");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "src/a.ts", "one\n", "a");
    write_commit(&r, "src/lib/b.ts", "two\n", "b");
    let head = log(&r, None, 0, 1).unwrap()[0].sha.clone();
    assert_eq!(tree_paths(&r, &head).unwrap(), ["src/a.ts", "src/lib/b.ts"]);
    assert_eq!(tree_paths(&r, &format!("{head}^")).unwrap(), ["src/a.ts"]);
    assert_eq!(text_at(&r, &head, "src/lib/b.ts").unwrap().text, "two\n");
    assert!(
        !text_at(&r, &format!("{head}^"), "src/lib/b.ts")
            .unwrap()
            .exists
    );
    assert!(tree_paths(&r, "HEAD").is_err(), "only commit ids");
    assert!(tree_paths(&r, &format!("{head}^^")).is_err());
}
