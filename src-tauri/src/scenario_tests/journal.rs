//! Undo and redo.

use super::*;

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
            r,
            "feat",
            MergeKind::Ff
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
        pull(r, PullMode::Ff, false, &Net::default())
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
