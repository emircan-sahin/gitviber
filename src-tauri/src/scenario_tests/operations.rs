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
        pull(b, PullMode::Rebase, false, &Net::default()).unwrap(),
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

    assert!(rebase(&r, "main", false, None).unwrap());
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
    assert!(merge(&r, "feature", MergeKind::Ff, false).unwrap());
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
    assert!(merge(&r, "feature", MergeKind::Ff, false).unwrap());
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
    assert!(!merge(&r, "feature", MergeKind::NoFf, false).unwrap());
    assert_eq!(parents("HEAD"), 2);
    run(&r, &["reset", "-q", "--hard", "HEAD~1"]).unwrap();

    // Squash: one ordinary commit with everything, listing what it took.
    assert!(!merge(&r, "feature", MergeKind::Squash, false).unwrap());
    assert_eq!(parents("HEAD"), 1);
    assert_eq!(
        run_text(&r, &["log", "-1", "--format=%B"]).unwrap().trim(),
        "Squash merge feature\n\n- add b\n- add c"
    );
    assert!(r.join("b.txt").exists() && r.join("c.txt").exists());
    // Again: nothing new, no empty commit.
    let head = run_text(&r, &["rev-parse", "HEAD"]).unwrap();
    assert!(!merge(&r, "feature", MergeKind::Squash, false).unwrap());
    assert_eq!(run_text(&r, &["rev-parse", "HEAD"]).unwrap(), head);

    // Plain: fast-forwards.
    run(&r, &["reset", "-q", "--hard", "HEAD~1"]).unwrap();
    assert!(!merge(&r, "feature", MergeKind::Ff, false).unwrap());
    assert_eq!(
        run_text(&r, &["rev-parse", "HEAD"]).unwrap(),
        run_text(&r, &["rev-parse", "feature"]).unwrap()
    );
}

/// Changes in the way of a merge or rebase from the branch picker: refused as git says, then
/// set aside and brought back with `autostash`, as Pull's retry does.
#[test]
fn merge_and_rebase_autostash_uncommitted_changes() {
    let sb = Sandbox::new("op-autostash");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "1\n2\n3\n4\n5\n", "base");
    switch_branch(&r, "feature", true).unwrap();
    write_commit(&r, "a.txt", "1\n2\n3\n4\nfive\n", "feature edit");
    switch_branch(&r, "main", false).unwrap();
    fs::write(r.join("a.txt"), "one\n2\n3\n4\n5\n").unwrap();

    let e = merge(&r, "feature", MergeKind::Ff, false).unwrap_err();
    assert!(e.contains("would be overwritten by merge"), "{e}");
    assert!(!merge(&r, "feature", MergeKind::Ff, true).unwrap());
    assert_eq!(
        fs::read_to_string(r.join("a.txt")).unwrap(),
        "one\n2\n3\n4\nfive\n"
    );

    run(&r, &["reset", "-q", "--hard", "HEAD~1"]).unwrap();
    write_commit(&r, "b.txt", "b\n", "main moves on");
    switch_branch(&r, "feature", false).unwrap();
    fs::write(r.join("a.txt"), "one\n2\n3\n4\nfive\n").unwrap();
    let e = rebase(&r, "main", false, None).unwrap_err();
    assert!(
        e.contains("error: cannot rebase: You have unstaged changes."),
        "{e}"
    );
    assert!(!rebase(&r, "main", true, None).unwrap());
    assert!(r.join("b.txt").exists());
    assert_eq!(
        fs::read_to_string(r.join("a.txt")).unwrap(),
        "one\n2\n3\n4\nfive\n"
    );
    assert!(run_text(&r, &["stash", "list"]).unwrap().is_empty());
}

/// "Open in <tool>": a tool set up in git's config runs on the path, and git stages what it merged.
#[test]
fn configured_merge_and_diff_tools_run_on_one_path() {
    let sb = Sandbox::new("tools");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "base\n", "base");
    write_commit(&r, "b.txt", "b\n", "b");
    switch_branch(&r, "feature", true).unwrap();
    write_commit(&r, "a.txt", "feature\n", "f");
    switch_branch(&r, "main", false).unwrap();
    write_commit(&r, "a.txt", "main\n", "m");
    assert!(merge(&r, "feature", MergeKind::Ff, false).unwrap());

    // Stand-ins for a GUI tool: take the incoming side; note which file the diff got. The gui keys,
    // which come first, so a tool in the user's global config can't take over.
    let seen = sb.path("seen.txt");
    run(&r, &["config", "merge.guitool", "fake"]).unwrap();
    run(
        &r,
        &["config", "mergetool.fake.cmd", "cp \"$REMOTE\" \"$MERGED\""],
    )
    .unwrap();
    run(&r, &["config", "mergetool.fake.trustExitCode", "true"]).unwrap();
    run(&r, &["config", "mergetool.keepBackup", "false"]).unwrap();
    run(&r, &["config", "diff.guitool", "look"]).unwrap();
    let look = format!("cat \"$REMOTE\" > '{}'", seen.display());
    run(&r, &["config", "difftool.look.cmd", &look]).unwrap();
    assert_eq!(
        external_tools(&r),
        ExternalTools {
            merge: Some("fake".into()),
            diff: Some("look".into()),
        }
    );

    open_merge_tool(&r, "a.txt").unwrap();
    assert_eq!(fs::read_to_string(r.join("a.txt")).unwrap(), "feature\n");
    assert!(status(&r).unwrap().conflicted.is_empty());

    fs::write(r.join("b.txt"), "b2\n").unwrap();
    open_diff_tool(&r, "b.txt", false).unwrap();
    assert_eq!(fs::read_to_string(&seen).unwrap(), "b2\n");
    // Staged: the index's copy, against HEAD.
    stage(&r, &["b.txt".into()]).unwrap();
    fs::write(r.join("b.txt"), "b3\n").unwrap();
    open_diff_tool(&r, "b.txt", true).unwrap();
    assert_eq!(fs::read_to_string(&seen).unwrap(), "b2\n");
}

/// Whatever conflictstyle wrote the file, each conflict's base is built from the index stages.
#[test]
fn conflict_base_from_the_index_stages() {
    let sb = Sandbox::new("cbase");
    let r = sb.path("r");
    init(&r);
    run(&r, &["config", "merge.conflictStyle", "merge"]).unwrap();
    write_commit(&r, "a.txt", "one\ntwo\nthree\n", "base");
    switch_branch(&r, "feature", true).unwrap();
    write_commit(&r, "a.txt", "one\nTWO-f\nthree\n", "f");
    write_commit(&r, "new.txt", "f\n", "add f");
    switch_branch(&r, "main", false).unwrap();
    write_commit(&r, "a.txt", "one\nTWO-m\nthree\n", "m");
    write_commit(&r, "new.txt", "m\n", "add m");
    assert!(merge(&r, "feature", MergeKind::Ff, false).unwrap());
    // The file has no base section; the rebuilt one does.
    assert!(!fs::read_to_string(r.join("a.txt"))
        .unwrap()
        .contains("|||||||"));
    assert_eq!(
        conflict_base(&r, "a.txt").unwrap().as_deref(),
        Some("one\n<<<<<<< current\nTWO-m\n||||||| base\ntwo\n=======\nTWO-f\n>>>>>>> incoming\nthree\n")
    );
    // Added on both sides: an empty base.
    assert_eq!(
        conflict_base(&r, "new.txt").unwrap().as_deref(),
        Some("<<<<<<< current\nm\n||||||| base\n=======\nf\n>>>>>>> incoming\n")
    );
    assert_eq!(conflict_base(&r, "missing.txt").unwrap(), None);
}

#[cfg(unix)]
fn fake_tool(path: &Path, script: &str) {
    fs::write(path, script).unwrap();
    std::process::Command::new("chmod")
        .args(["+x", path.to_str().unwrap()])
        .status()
        .unwrap();
}

/// main and feature both rewrote `path` (created in the base commit) with different text.
fn conflicted_on(r: &Path, path: &str) {
    init(r);
    write_commit(r, path, "base\n", "base");
    switch_branch(r, "feature", true).unwrap();
    write_commit(r, path, "feature\n", "f");
    switch_branch(r, "main", false).unwrap();
    write_commit(r, path, "main\n", "m");
    assert!(merge(r, "feature", MergeKind::Ff, false).unwrap());
}

fn untracked(r: &Path) -> Vec<String> {
    status(r)
        .unwrap()
        .unstaged
        .into_iter()
        .filter(|f| f.status == "?")
        .map(|f| f.path)
        .collect()
}

/// A team's shared config (an include, or ~/.gitconfig) names one tool and the repo another: git
/// takes the last value it reads, so the button has to name that one, the tool that opens.
#[test]
fn the_tool_named_is_the_tool_git_opens() {
    let sb = Sandbox::new("tool-last");
    let r = sb.path("r");
    conflicted_on(&r, "a.txt");
    let ran = sb.path("ran.txt");
    let shared = sb.path("shared.gitconfig");
    fs::write(
        &shared,
        "[merge]\n\ttool = shared-tool\n[diff]\n\ttool = shared-tool\n",
    )
    .unwrap();
    run(&r, &["config", "include.path", shared.to_str().unwrap()]).unwrap();
    run(&r, &["config", "merge.tool", "repo-tool"]).unwrap();
    run(&r, &["config", "diff.tool", "repo-tool"]).unwrap();
    for name in ["shared-tool", "repo-tool"] {
        let cmd = format!(
            "echo {name} > '{}'; cp \"$REMOTE\" \"$MERGED\"",
            ran.display()
        );
        run(&r, &["config", &format!("mergetool.{name}.cmd"), &cmd]).unwrap();
        run(
            &r,
            &["config", &format!("mergetool.{name}.trustExitCode"), "true"],
        )
        .unwrap();
    }
    run(&r, &["config", "mergetool.keepBackup", "false"]).unwrap();
    open_merge_tool(&r, "a.txt").unwrap();
    let opened = fs::read_to_string(&ran).unwrap().trim().to_string();
    assert_eq!(opened, "repo-tool");
    assert_eq!(
        external_tools(&r),
        ExternalTools {
            merge: Some(opened.clone()),
            diff: Some(opened),
        }
    );
}

/// Paths as people name them: spaces, an apostrophe, `$`, backticks, non-ASCII, a folder. (A
/// double quote or a tab in the name: `git mergetool` itself answers "file not found".)
#[cfg(unix)]
#[test]
fn tools_open_files_with_odd_names() {
    let sb = Sandbox::new("tool-odd");
    let r = sb.path("r");
    let path = "dir with space/it's ü $HOME `x`.txt";
    conflicted_on(&r, path);
    run(&r, &["config", "merge.guitool", "take"]).unwrap();
    run(
        &r,
        &["config", "mergetool.take.cmd", "cp \"$REMOTE\" \"$MERGED\""],
    )
    .unwrap();
    run(&r, &["config", "mergetool.take.trustExitCode", "true"]).unwrap();
    run(&r, &["config", "mergetool.keepBackup", "false"]).unwrap();
    open_merge_tool(&r, path).unwrap();
    assert_eq!(fs::read_to_string(r.join(path)).unwrap(), "feature\n");
    assert!(status(&r).unwrap().conflicted.is_empty());
    assert!(untracked(&r).is_empty(), "{:?}", untracked(&r));

    let seen = sb.path("seen.txt");
    run(&r, &["config", "diff.guitool", "look"]).unwrap();
    let look = format!("cat \"$REMOTE\" > '{}'", seen.display());
    run(&r, &["config", "difftool.look.cmd", &look]).unwrap();
    commit(&r, "merged", &CommitOptions::default(), &Net::default()).unwrap();
    fs::write(r.join(path), "edited\n").unwrap();
    open_diff_tool(&r, path, false).unwrap();
    assert_eq!(fs::read_to_string(&seen).unwrap(), "edited\n");
}

/// A tool that isn't installed where the config says, or one that fails: the file stays in conflict
/// and the click leaves nothing behind in the worktree (an agent's `git add -A` would commit it).
#[test]
fn a_missing_or_failing_tool_leaves_the_worktree_as_it_was() {
    let sb = Sandbox::new("tool-fail");
    let r = sb.path("r");
    conflicted_on(&r, "a.txt");
    run(&r, &["config", "merge.guitool", "kdiff3"]).unwrap();
    run(
        &r,
        &["config", "mergetool.kdiff3.path", "/nonexistent/kdiff3"],
    )
    .unwrap();
    assert_eq!(external_tools(&r).merge.as_deref(), Some("kdiff3"));
    let err = open_merge_tool(&r, "a.txt").unwrap_err();
    assert!(err.contains("not available"), "{err}");
    assert_eq!(status(&r).unwrap().conflicted.len(), 1);
    assert!(untracked(&r).is_empty(), "left behind: {:?}", untracked(&r));

    run(&r, &["config", "merge.guitool", "broken"]).unwrap();
    run(&r, &["config", "mergetool.broken.cmd", "exit 3"]).unwrap();
    assert!(open_merge_tool(&r, "a.txt").is_err());
    assert_eq!(status(&r).unwrap().conflicted.len(), 1);
    assert!(untracked(&r).is_empty(), "left behind: {:?}", untracked(&r));
}

/// A tool left open for an hour: the app goes on reading and staging meanwhile, and the merge
/// lands when it's closed.
#[cfg(unix)]
#[test]
fn an_open_tool_blocks_nothing_else() {
    let sb = Sandbox::new("tool-open");
    let r = sb.path("r");
    conflicted_on(&r, "a.txt");
    let (started, close) = (sb.path("started"), sb.path("close"));
    let tool = sb.path("slow-tool");
    fake_tool(
        &tool,
        &format!(
            "#!/bin/sh\ntouch '{}'\nwhile [ ! -e '{}' ]; do sleep 0.05; done\ncp \"$1\" \"$2\"\n",
            started.display(),
            close.display()
        ),
    );
    run(&r, &["config", "merge.guitool", "slow"]).unwrap();
    let cmd = format!("'{}' \"$REMOTE\" \"$MERGED\"", tool.display());
    run(&r, &["config", "mergetool.slow.cmd", &cmd]).unwrap();
    run(&r, &["config", "mergetool.slow.trustExitCode", "true"]).unwrap();
    run(&r, &["config", "mergetool.keepBackup", "false"]).unwrap();

    let repo = r.clone();
    let open = std::thread::spawn(move || open_merge_tool(&repo, "a.txt"));
    let t0 = std::time::Instant::now();
    while !started.exists() {
        assert!(t0.elapsed().as_secs() < 20, "the tool never started");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(status(&r).unwrap().conflicted.len(), 1);
    assert!(conflict_base(&r, "a.txt").unwrap().is_some());
    fs::write(r.join("b.txt"), "b2\n").unwrap();
    stage(&r, &["b.txt".into()]).unwrap();
    assert!(!open.is_finished());

    fs::write(&close, "").unwrap();
    open.join().unwrap().unwrap();
    assert_eq!(fs::read_to_string(r.join("a.txt")).unwrap(), "feature\n");
    assert!(status(&r).unwrap().conflicted.is_empty());
}

/// Every kind of conflict the view can show: only text both sides changed gets a base, the rest
/// say so without an error.
#[test]
fn conflict_bases_across_conflict_kinds() {
    let sb = Sandbox::new("cbase-kinds");
    let r = sb.path("r");
    init(&r);
    run(&r, &["config", "merge.conflictStyle", "merge"]).unwrap();
    run(&r, &["config", "core.autocrlf", "false"]).unwrap();
    let files: &[(&str, &str, &str, &str)] = &[
        // path, base, feature, main
        (
            "crlf.txt",
            "a\r\nb\r\nc\r\n",
            "a\r\nB-f\r\nc\r\n",
            "a\r\nB-m\r\nc\r\n",
        ),
        ("no-eol.txt", "a\nend", "a\nend-f", "a\nend-m"),
        ("bin.dat", "x\0base", "x\0feat", "x\0main"),
        ("del-here.txt", "base\n", "feature\n", ""),
        ("del-there.txt", "base\n", "", "main\n"),
        (
            "odd \"name\" ü $x.txt",
            "1\n2\n3\n",
            "1\nf\n3\n",
            "1\nm\n3\n",
        ),
    ];
    for (p, base, _, _) in files {
        fs::write(r.join(p), base).unwrap();
    }
    run(&r, &["add", "-A"]).unwrap();
    commit(&r, "base", &CommitOptions::default(), &Net::default()).unwrap();
    for (branch, side) in [("feature", 2), ("main", 3)] {
        if branch == "feature" {
            switch_branch(&r, "feature", true).unwrap();
        } else {
            switch_branch(&r, "main", false).unwrap();
        }
        for f in files {
            let text = if side == 2 { f.2 } else { f.3 };
            if text.is_empty() {
                fs::remove_file(r.join(f.0)).unwrap();
            } else {
                fs::write(r.join(f.0), text).unwrap();
            }
        }
        run(&r, &["add", "-A"]).unwrap();
        commit(&r, branch, &CommitOptions::default(), &Net::default()).unwrap();
    }
    assert!(merge(&r, "feature", MergeKind::Ff, false).unwrap());

    let crlf = conflict_base(&r, "crlf.txt").unwrap().unwrap();
    assert!(crlf.contains("||||||| base\r\nb\r\n"), "{crlf:?}");
    let no_eol = conflict_base(&r, "no-eol.txt").unwrap().unwrap();
    assert!(no_eol.contains("||||||| base\nend\n"), "{no_eol:?}");
    // The merged text ends the marker line with a newline; the sides say neither file had one.
    let sides = conflict_sides(&r, "no-eol.txt").unwrap().unwrap();
    assert!(!sides.ours_newline && !sides.theirs_newline);
    let sides = conflict_sides(&r, "crlf.txt").unwrap().unwrap();
    assert!(sides.ours_newline && sides.theirs_newline);
    // Binary: no merged text, but still how it ends.
    assert!(conflict_sides(&r, "bin.dat")
        .unwrap()
        .unwrap()
        .merged
        .is_none());
    assert_eq!(conflict_base(&r, "bin.dat").unwrap(), None);
    assert_eq!(conflict_base(&r, "del-here.txt").unwrap(), None);
    assert_eq!(conflict_base(&r, "del-there.txt").unwrap(), None);
    let odd = conflict_base(&r, "odd \"name\" ü $x.txt").unwrap().unwrap();
    assert!(odd.contains("||||||| base\n2\n"), "{odd:?}");
}

/// Renamed on one side, edited on both: the conflict sits at the new name, and so do its stages.
/// Renamed apart on both sides: the stages split over two paths, and neither errors.
#[test]
fn conflict_bases_after_renames() {
    let sb = Sandbox::new("cbase-rename");
    let r = sb.path("r");
    init(&r);
    let body: String = (1..=30).map(|i| format!("line {i}\n")).collect();
    write_commit(&r, "old.txt", &body, "base");
    let other: String = (1..=30).map(|i| format!("other {i}\n")).collect();
    write_commit(&r, "split.txt", &other, "base2");
    switch_branch(&r, "feature", true).unwrap();
    write_commit(
        &r,
        "old.txt",
        &body.replace("line 15\n", "feature 15\n"),
        "f",
    );
    run(&r, &["mv", "split.txt", "split-f.txt"]).unwrap();
    commit(&r, "f mv", &CommitOptions::default(), &Net::default()).unwrap();
    switch_branch(&r, "main", false).unwrap();
    run(&r, &["mv", "old.txt", "new.txt"]).unwrap();
    commit(&r, "mv", &CommitOptions::default(), &Net::default()).unwrap();
    write_commit(&r, "new.txt", &body.replace("line 15\n", "main 15\n"), "m");
    run(&r, &["mv", "split.txt", "split-m.txt"]).unwrap();
    commit(&r, "m mv", &CommitOptions::default(), &Net::default()).unwrap();
    assert!(merge(&r, "feature", MergeKind::Ff, false).unwrap());

    let moved = conflict_base(&r, "new.txt").unwrap().unwrap();
    assert!(moved.contains("||||||| base\nline 15\n"), "{moved:?}");
    for p in ["split.txt", "split-f.txt", "split-m.txt"] {
        assert_eq!(conflict_base(&r, p).unwrap(), None, "{p}");
    }
}

/// Two places with the same edit on each side but different originals: each keeps its own base
/// in what the backend builds (the view has to pair them up in order).
#[test]
fn conflict_base_keeps_twin_conflicts_apart() {
    let sb = Sandbox::new("cbase-twin");
    let r = sb.path("r");
    init(&r);
    run(&r, &["config", "merge.conflictStyle", "merge"]).unwrap();
    let text =
        |a: &str, b: &str| format!("fn a() {{\n  {a};\n}}\n1\n2\n3\n4\nfn b() {{\n  {b};\n}}\n");
    write_commit(&r, "f.rs", &text("old_a()", "old_b()"), "base");
    switch_branch(&r, "feature", true).unwrap();
    write_commit(&r, "f.rs", &text("theirs()", "theirs()"), "f");
    switch_branch(&r, "main", false).unwrap();
    write_commit(&r, "f.rs", &text("ours()", "ours()"), "m");
    assert!(merge(&r, "feature", MergeKind::Ff, false).unwrap());
    let rebuilt = conflict_base(&r, "f.rs").unwrap().unwrap();
    let a = rebuilt.find("old_a").unwrap();
    let b = rebuilt.find("old_b").unwrap();
    assert!(a < b);
}

/// A conflicted generated file past the limit: no base, and no scratch folder left behind.
#[test]
fn conflict_base_skips_huge_files_and_cleans_up() {
    let sb = Sandbox::new("cbase-huge");
    let r = sb.path("r");
    init(&r);
    let big: String = (0..600_000).map(|i| format!("row {i}\n")).collect();
    write_commit(&r, "big.txt", &big, "base");
    switch_branch(&r, "feature", true).unwrap();
    write_commit(&r, "big.txt", &big.replace("row 7\n", "feature\n"), "f");
    switch_branch(&r, "main", false).unwrap();
    write_commit(&r, "big.txt", &big.replace("row 7\n", "main\n"), "m");
    assert!(merge(&r, "feature", MergeKind::Ff, false).unwrap());
    assert!(big.len() > 4 << 20);
    assert_eq!(conflict_base(&r, "big.txt").unwrap(), None);
    // Under the limit (one side small enough is not enough): built, then its folder removed.
    let prefix = "gitviber-merge-";
    let left = || {
        fs::read_dir(std::env::temp_dir())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with(prefix))
            .count()
    };
    let t0 = std::time::Instant::now();
    // Other tests in this process build bases too; theirs go within milliseconds.
    while left() > 0 {
        assert!(t0.elapsed().as_secs() < 10, "scratch folders left behind");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

/// Several commits are picked in the order given, one undo entry; one already there is left
/// out; a conflict stops it for continue, skip or abort to take over.
#[test]
fn cherry_pick_many_in_order_with_conflicts() {
    let sb = Sandbox::new("pick-many");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "base\n", "base");
    run(&r, &["switch", "-q", "-c", "feat"]).unwrap();
    write_commit(&r, "n1.txt", "1\n", "feat one");
    write_commit(&r, "a.txt", "feat\n", "feat edits a");
    write_commit(&r, "n3.txt", "3\n", "feat three");
    let picks: Vec<String> = ["HEAD~2", "HEAD~1", "HEAD"]
        .iter()
        .map(|s| rev(&r, s))
        .collect();
    run(&r, &["switch", "-q", "main"]).unwrap();
    let before = rev(&r, "HEAD");
    let j = Journal::default();
    let action = || Action::new("Cherry-pick 3 commits", Mode::Keep);

    assert!(!j
        .record(&r, action(), |r| cherry_pick_many(r, &picks))
        .unwrap());
    let subjects: Vec<String> = log(&r, None, 0, 4)
        .unwrap()
        .into_iter()
        .map(|c| c.subject)
        .collect();
    assert_eq!(subjects, ["feat three", "feat edits a", "feat one", "base"]);
    step(&j, &r, false).unwrap();
    assert_eq!(rev(&r, "HEAD"), before);

    // Again, with "two" conflicting: it stops on it, after one.
    write_commit(&r, "a.txt", "main\n", "main edits a");
    let before = rev(&r, "HEAD");
    assert!(j
        .record(&r, action(), |r| cherry_pick_many(r, &picks))
        .unwrap());
    assert_eq!(operation(&r).unwrap().kind, "cherry-pick");
    assert!(!rev(&r, "HEAD").is_empty() && log(&r, None, 0, 1).unwrap()[0].subject == "feat one");
    // Resolved, continuing picks the last one too, in one undo entry.
    resolve_side(&r, "a.txt", Side::Theirs).unwrap();
    assert!(!j.record(&r, action(), op_continue).unwrap());
    assert!(operation(&r).is_none());
    let subjects: Vec<String> = log(&r, None, 0, 3)
        .unwrap()
        .into_iter()
        .map(|c| c.subject)
        .collect();
    assert_eq!(subjects, ["feat three", "feat edits a", "feat one"]);
    assert_eq!(j.view(&r).undo.len(), 1);
    step(&j, &r, false).unwrap();
    assert_eq!(rev(&r, "HEAD"), before);

    // Called off: back where it started, nothing half-picked.
    assert!(j
        .record(&r, action(), |r| cherry_pick_many(r, &picks))
        .unwrap());
    op_abort(&r).unwrap();
    assert!(operation(&r).is_none());
    assert_eq!(rev(&r, "HEAD"), before);
    assert!(!r.join("n1.txt").exists());

    // Nothing new: refused, and none half-picked.
    run(&r, &["cherry-pick", &picks[0]]).unwrap();
    run(&r, &["reset", "-q", "--hard", &before]).unwrap();
    let err = cherry_pick_many(&r, &[]).unwrap_err();
    assert!(err.contains("No commits"), "{err}");
    write_commit(&r, "n1.txt", "1\n", "same file");
    let err = cherry_pick_many(&r, &picks[..1]).unwrap_err();
    assert!(err.contains("already has"), "{err}");
    assert!(operation(&r).is_none());
}

/// A rebase onto another branch moves the branches it passes along only when asked.
#[test]
fn rebase_moves_stacked_branches_when_asked() {
    let setup = |name: &str| {
        let sb = Sandbox::new(name);
        let r = sb.path("r");
        init(&r);
        write_commit(&r, "a.txt", "a\n", "base");
        run(&r, &["switch", "-q", "-c", "feat"]).unwrap();
        write_commit(&r, "f1.txt", "1\n", "f1");
        run(&r, &["branch", "stack"]).unwrap();
        write_commit(&r, "f2.txt", "2\n", "f2");
        run(&r, &["switch", "-q", "main"]).unwrap();
        write_commit(&r, "m.txt", "m\n", "main moves");
        run(&r, &["switch", "-q", "feat"]).unwrap();
        (sb, r)
    };
    let (_sb, r) = setup("rebase-stack-off");
    let offer = crate::rewrite::stacked_onto(&r, "main").unwrap();
    assert_eq!(offer.branches, ["stack"]);
    let old = rev(&r, "stack");
    assert!(!rebase(&r, "main", false, Some(false)).unwrap());
    assert_eq!(rev(&r, "stack"), old);

    let (_sb, r) = setup("rebase-stack-on");
    assert!(!rebase(&r, "main", false, Some(true)).unwrap());
    assert_eq!(rev(&r, "stack"), rev(&r, "HEAD~1"));
    assert_ne!(rev(&r, "stack"), "");
}
