//! Checking a pull request out without resetting local work or reading FETCH_HEAD.

use super::*;

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

/// A PR checked out in a new worktree: this one's HEAD stays, the branch is set up as
/// `checkout` would, and an existing one is only ever fast-forwarded.
#[test]
fn pr_checkout_worktree_leaves_head_and_fast_forwards() {
    use crate::github::checkout_worktree;
    let sb = Sandbox::new("prwt");
    let c = sb.remote_with_clones(2);
    let (a, b) = (&c[0], &c[1]);
    let dir = sb.path("wts");
    let d = dir.to_str().unwrap();
    let net = Net::default();
    let listed = |branch: &str| {
        worktrees(b)
            .unwrap()
            .into_iter()
            .find(|w| w.branch.as_deref() == Some(branch))
            .map(|w| w.path)
    };
    switch_branch(a, "feat", true).unwrap();
    write_commit(a, "f.txt", "1\n", "f1");
    run(a, &["push", "-q", "-u", "origin", "feat"]).unwrap();

    // Same repo: the real branch, tracking origin's.
    let wt = checkout_worktree(b, "origin", None, 1, "feat", true, Some(d), &net).unwrap();
    assert_eq!(Path::new(&wt), dir.join("feat"));
    assert_eq!(fs::read_to_string(dir.join("feat/f.txt")).unwrap(), "1\n");
    assert_eq!(status(b).unwrap().branch.as_deref(), Some("main"));
    let upstream = run_text(b, &["rev-parse", "--abbrev-ref", "feat@{upstream}"]).unwrap();
    assert_eq!(upstream.trim(), "origin/feat");
    // Already out in a worktree: said so, not fetched into.
    let again = checkout_worktree(b, "origin", None, 1, "feat", true, Some(d), &net);
    assert!(again.unwrap_err().contains("already checked out"));
    // Gone again, the stale local branch is fast-forwarded into the next one.
    remove_worktree(b, &listed("feat").unwrap(), false).unwrap();
    write_commit(a, "f.txt", "1\n2\n", "f2");
    run(a, &["push", "-q"]).unwrap();
    checkout_worktree(b, "origin", None, 1, "feat", true, Some(d), &net).unwrap();
    assert_eq!(
        fs::read_to_string(dir.join("feat/f.txt")).unwrap(),
        "1\n2\n"
    );

    // A fork's PR: pr/<n>, following refs/pull/<n>/head.
    run(a, &["push", "-q", "origin", "HEAD:refs/pull/7/head"]).unwrap();
    // A taken folder is refused before the fetch: no branch, no tracking left behind.
    fs::create_dir_all(dir.join("pr-7")).unwrap();
    assert!(checkout_worktree(b, "origin", None, 7, "theirs", false, Some(d), &net).is_err());
    assert!(run(b, &["rev-parse", "--verify", "-q", "refs/heads/pr/7"]).is_err());
    assert!(run(b, &["config", "branch.pr/7.merge"]).is_err());
    fs::remove_dir(dir.join("pr-7")).unwrap();
    let wt = checkout_worktree(b, "origin", None, 7, "theirs", false, Some(d), &net).unwrap();
    assert_eq!(Path::new(&wt), dir.join("pr-7"));
    let merge = run_text(b, &["config", "branch.pr/7.merge"]).unwrap();
    assert_eq!(merge.trim(), "refs/pull/7/head");
    // A local commit that diverges from the PR is reported, not reset.
    write_commit(&dir.join("pr-7"), "mine.txt", "mine\n", "local only");
    remove_worktree(b, &listed("pr/7").unwrap(), false).unwrap();
    write_commit(a, "f.txt", "3\n", "f3");
    run(a, &["push", "-q", "origin", "HEAD:refs/pull/7/head"]).unwrap();
    let diverged = checkout_worktree(b, "origin", None, 7, "theirs", false, Some(d), &net);
    let e = diverged.unwrap_err();
    assert!(e.contains("diverged"), "{e}");
    let log = run_text(b, &["log", "--oneline", "pr/7"]).unwrap();
    assert!(log.contains("local only"));
}
