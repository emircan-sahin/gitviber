//! Merge, rebase, revert, cherry-pick and reset, and continuing or aborting them.

use super::*;

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
        pull(b, PullMode::Rebase, &Net::default()).unwrap(),
        "should stop on the conflict"
    );
    assert_eq!(operation(b).unwrap().kind, "rebase");
    assert_eq!(status(b).unwrap().conflicted.len(), 1);
    op_abort(b).unwrap();
    assert!(operation(b).is_none());
    assert_eq!(log(b, None, 0, 1).unwrap()[0].sha, before);
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
    assert!(merge(&r, "feature", MergeKind::Ff).unwrap());
    resolve_side(&r, "a.txt", Side::Ours).unwrap();
    assert_eq!(fs::read_to_string(r.join("a.txt")).unwrap(), "main\n");
    // Not a conflicted path: must error, not `git rm` it.
    fs::write(r.join("clean.txt"), "x\n").unwrap();
    stage(&r, &["clean.txt".into()]).unwrap();
    assert!(resolve_side(&r, "clean.txt", Side::Theirs).is_err());
    assert!(r.join("clean.txt").exists());
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
    assert!(merge(&r, "feature", MergeKind::Ff).unwrap());
    resolve_side(&r, "a.txt", Side::Ours).unwrap();

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
    resolve_side(&r, "a.txt", Side::Theirs).unwrap();
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
    // The HEAD the user saw must still be HEAD, or an agent's newer commit would be dropped.
    assert!(reset(&r, &base, ResetMode::Hard, &base).is_err());
    assert_eq!(log(&r, None, 0, 5).unwrap().len(), 2);

    reset(&r, &base, ResetMode::Soft, &two).unwrap();
    let st = status(&r).unwrap();
    assert_eq!((st.staged.len(), st.unstaged.len()), (1, 0));

    reset(&r, &two, ResetMode::Mixed, &base).unwrap();
    reset(&r, &base, ResetMode::Mixed, &two).unwrap();
    let st = status(&r).unwrap();
    assert_eq!((st.staged.len(), st.unstaged.len()), (0, 1));

    reset(&r, &base, ResetMode::Hard, &base).unwrap();
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
    resolve_side(&r, "a.txt", Side::Theirs).unwrap();
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
    resolve_side(&target, "a.txt", Side::Theirs).unwrap();
    assert!(!j.record(&target, action(), op_continue).unwrap());
    assert_eq!(j.view(&target).undo.len(), 1);
    assert!(j.view(&b).undo.is_empty());
    step(&j, &target, false).unwrap();
    assert_eq!(rev(&r, "HEAD"), before);
}

#[test]
fn merge_kinds_fast_forward_no_ff_and_squash() {
    let sb = Sandbox::new("merge-kinds");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "a\n", "base");
    run(&r, &["switch", "-q", "-c", "feature"]).unwrap();
    write_commit(&r, "b.txt", "b\n", "add b");
    write_commit(&r, "c.txt", "c\n", "add c");
    run(&r, &["switch", "-q", "main"]).unwrap();
    let parents = |rev: &str| {
        run_text(&r, &["rev-list", "--parents", "-n1", rev])
            .unwrap()
            .split_whitespace()
            .count()
            - 1
    };

    // No fast-forward: a merge commit although main could just move.
    assert!(!merge(&r, "feature", MergeKind::NoFf).unwrap());
    assert_eq!(parents("HEAD"), 2);
    run(&r, &["reset", "-q", "--hard", "HEAD~1"]).unwrap();

    // Squash: one ordinary commit with everything, listing what it took.
    assert!(!merge(&r, "feature", MergeKind::Squash).unwrap());
    assert_eq!(parents("HEAD"), 1);
    assert_eq!(
        run_text(&r, &["log", "-1", "--format=%B"]).unwrap().trim(),
        "Squash merge feature\n\n- add b\n- add c"
    );
    assert!(r.join("b.txt").exists() && r.join("c.txt").exists());
    // Again: nothing new, no empty commit.
    let head = run_text(&r, &["rev-parse", "HEAD"]).unwrap();
    assert!(!merge(&r, "feature", MergeKind::Squash).unwrap());
    assert_eq!(run_text(&r, &["rev-parse", "HEAD"]).unwrap(), head);

    // Plain: fast-forwards.
    run(&r, &["reset", "-q", "--hard", "HEAD~1"]).unwrap();
    assert!(!merge(&r, "feature", MergeKind::Ff).unwrap());
    assert_eq!(
        run_text(&r, &["rev-parse", "HEAD"]).unwrap(),
        run_text(&r, &["rev-parse", "feature"]).unwrap()
    );
}
