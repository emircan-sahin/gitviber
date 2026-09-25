//! Reviewing a branch against its base: commits and uncommitted work as one diff.

use super::*;

/// (path, status, old path) of each file under review.
fn listed(review: &BranchReview) -> Vec<(&str, &str, Option<&str>)> {
    review
        .files
        .iter()
        .map(|f| (f.path.as_str(), f.status.as_str(), f.old_path.as_deref()))
        .collect()
}

#[test]
fn branch_review_is_everything_since_the_merge_base() {
    let sb = Sandbox::new("review");
    let r = sb.path("r");
    init(&r);
    let long = "one\ntwo\nthree\nfour\nfive\nsix\n";
    write_commit(&r, "a.txt", "a\n", "base");
    write_commit(&r, "keep.txt", "keep\n", "base");
    write_commit(&r, "gone.txt", "gone\n", "base");
    write_commit(&r, "back.txt", "back\n", "base");
    write_commit(&r, "keep-disk.txt", "k\n", "base");
    write_commit(&r, "old.txt", long, "base");
    run(&r, &["switch", "-q", "-c", "feature"]).unwrap();

    // Committed: an edit, a rename, and an edit the working tree takes back.
    write_commit(&r, "a.txt", "a\ncommitted\n", "edit a");
    run(&r, &["mv", "old.txt", "new.txt"]).unwrap();
    commit(&r, "rename", &CommitOptions::default()).unwrap();
    write_commit(&r, "back.txt", "changed\n", "edit back");
    // main moves on after the branch left it: not this branch's change.
    run(&r, &["switch", "-q", "main"]).unwrap();
    write_commit(&r, "main-only.txt", "m\n", "on main");
    run(&r, &["switch", "-q", "feature"]).unwrap();
    fs::write(r.join("back.txt"), "back\n").unwrap();

    // Uncommitted: staged, unstaged, deleted and untracked.
    fs::write(r.join("staged.txt"), "s\n").unwrap();
    stage(&r, &["staged.txt".into()]).unwrap();
    fs::write(r.join("keep.txt"), "keep\nunstaged\n").unwrap();
    fs::remove_file(r.join("gone.txt")).unwrap();
    fs::write(r.join("untracked.txt"), "u1\nu2\n").unwrap();
    // Out of the index but still on disk: one row, untracked, not a deletion as well.
    run(&r, &["rm", "-q", "--cached", "keep-disk.txt"]).unwrap();

    let review = branch_review(&r, "refs/heads/main").unwrap();
    assert_eq!(review.base, rev(&r, "main~1"));
    assert_eq!(
        listed(&review),
        [
            ("a.txt", "M", None),
            ("gone.txt", "D", None),
            ("keep-disk.txt", "?", None),
            ("keep.txt", "M", None),
            ("new.txt", "R", Some("old.txt")),
            ("staged.txt", "A", None),
            ("untracked.txt", "?", None),
        ]
    );
    let counts = |p: &str| {
        let f = review.files.iter().find(|f| f.path == p).unwrap();
        (f.additions, f.deletions)
    };
    assert_eq!(counts("a.txt"), (Some(1), Some(0)));
    assert_eq!(counts("untracked.txt"), (Some(2), Some(0)));
    assert!(review
        .files
        .iter()
        .all(|f| f.oid.is_some() == (f.status != "D")));

    // Each file: the merge base's version against the one on disk.
    let wt = |p: &str| vfs::read_file(&r, p);
    let pair = |path: &str, old: Option<&str>| {
        diff_pair(&r, "base", path, old, None, Some(&review.base), None, wt).unwrap()
    };
    let a = pair("a.txt", None);
    assert_eq!(
        (a.original.text.as_str(), a.modified.text.as_str()),
        ("a\n", "a\ncommitted\n")
    );
    let renamed = pair("new.txt", Some("old.txt"));
    assert!(renamed.original.text == long && renamed.rows.iter().all(|r| r.k == 0));
    let gone = pair("gone.txt", None);
    assert!(gone.original.exists && !gone.modified.exists);
    let untracked = pair("untracked.txt", None);
    assert!(!untracked.original.exists && untracked.modified.text == "u1\nu2\n");

    // Names from the page: a full ref for the base, a commit id for the diff.
    assert!(branch_review(&r, "main").is_err());
    // A branch that isn't here says so, here and in History's compare.
    let missing = "refs/remotes/origin/main";
    let err = branch_review(&r, missing).err().unwrap();
    assert!(err.contains("origin/main doesn't exist"), "{err}");
    assert_eq!(compare_files(&r, missing).err(), Some(err));
    assert!(branch_review(&r, "refs/heads/--output=x").is_err());
    assert!(diff_pair(&r, "base", "a.txt", None, None, Some("HEAD"), None, wt).is_err());
}

#[test]
fn branch_review_in_a_linked_worktree() {
    let sb = Sandbox::new("review-wt");
    let r = repo_with_worktrees(&sb);
    let agent = r.join(".claude/worktrees/agent");
    write_commit(&agent, "b.txt", "b\n", "agent work");
    fs::write(agent.join("a.txt"), "changed\n").unwrap();

    let review = branch_review(&agent, "refs/heads/main").unwrap();
    assert_eq!(review.base, rev(&r, "main"));
    assert_eq!(
        listed(&review),
        [("a.txt", "M", None), ("b.txt", "A", None)]
    );
    let wt = |p: &str| vfs::read_file(&agent, p);
    let a = diff_pair(
        &agent,
        "base",
        "a.txt",
        None,
        None,
        Some(&review.base),
        None,
        wt,
    )
    .unwrap();
    assert_eq!(
        (a.original.text.as_str(), a.modified.text.as_str()),
        ("a\n", "changed\n")
    );

    // From the main worktree, the agent's folder inside it is another repository: left out.
    assert!(branch_review(&r, "refs/heads/agent")
        .unwrap()
        .files
        .is_empty());
}
