//! The stash.

use super::*;

/// Stash with untracked files, list and show it, pop it back. Actions name a stash by its
/// commit, so one pushed meanwhile (stash@{0} moving) can't redirect them.
#[test]
fn stash_push_and_pop_with_untracked_files() {
    let sb = Sandbox::new("stash");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "a\n", "base");
    assert!(
        stash_push(
            &r,
            "",
            StashWhat {
                untracked: false,
                staged: false,
                paths: &[]
            }
        )
        .is_err(),
        "nothing to stash"
    );
    fs::write(r.join("a.txt"), "a changed\n").unwrap();
    fs::write(r.join("new.txt"), "new\n").unwrap();
    stash_push(
        &r,
        "wip: both",
        StashWhat {
            untracked: true,
            staged: false,
            paths: &[],
        },
    )
    .unwrap();
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
    stash_push(
        &r,
        "other",
        StashWhat {
            untracked: false,
            staged: false,
            paths: &[],
        },
    )
    .unwrap();
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
    stash_push(
        &r,
        "mine",
        StashWhat {
            untracked: false,
            staged: false,
            paths: &[],
        },
    )
    .unwrap();
    write_commit(&r, "a.txt", "committed\n", "moved on");
    let sha = stashes(&r).unwrap()[0].sha.clone();
    assert!(stash_apply(&r, &sha, true).unwrap());
    let st = status(&r).unwrap();
    assert_eq!(st.conflicted.len(), 1);
    assert!(st.operation.is_none());
    assert_eq!(stashes(&r).unwrap().len(), 1);
    resolve_side(&r, "a.txt", Side::Theirs).unwrap();
    assert_eq!(fs::read_to_string(r.join("a.txt")).unwrap(), "stashed\n");
    stash_drop(&r, &sha).unwrap();
}

#[test]
fn switching_with_conflicting_changes_works_after_a_stash() {
    let sb = Sandbox::new("switch-stash");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "a\n", "base");
    run(&r, &["switch", "-q", "-c", "other"]).unwrap();
    write_commit(&r, "a.txt", "other\n", "other");
    run(&r, &["switch", "-q", "main"]).unwrap();
    fs::write(r.join("a.txt"), "mine\n").unwrap();
    // The UI looks for this to offer Stash and Switch.
    let e = switch_branch(&r, "other", false).unwrap_err();
    assert!(e.contains("would be overwritten by checkout"), "{e}");
    stash_push(
        &r,
        "Left on main when switching to other",
        StashWhat {
            untracked: true,
            staged: false,
            paths: &[],
        },
    )
    .unwrap();
    switch_branch(&r, "other", false).unwrap();
    assert_eq!(fs::read_to_string(r.join("a.txt")).unwrap(), "other\n");
    assert_eq!(stashes(&r).unwrap().len(), 1);
}

#[test]
fn stash_some_files_only_staged_and_a_branch_from_a_stash() {
    let sb = Sandbox::new("stash-more");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "a\n", "base");
    write_commit(&r, "b [1].txt", "b\n", "b");
    fs::write(r.join("a.txt"), "a2\n").unwrap();
    fs::write(r.join("b [1].txt"), "b2\n").unwrap();
    // One file, by a name that would be a glob.
    let only = ["b [1].txt".to_string()];
    stash_push(
        &r,
        "just b",
        StashWhat {
            untracked: false,
            staged: false,
            paths: &only,
        },
    )
    .unwrap();
    assert_eq!(fs::read_to_string(r.join("b [1].txt")).unwrap(), "b\n");
    assert_eq!(fs::read_to_string(r.join("a.txt")).unwrap(), "a2\n");

    // Only what's staged: the unstaged part stays.
    stage(&r, &["a.txt".into()]).unwrap();
    fs::write(r.join("c.txt"), "untracked\n").unwrap();
    stash_push(
        &r,
        "staged a",
        StashWhat {
            untracked: false,
            staged: true,
            paths: &[],
        },
    )
    .unwrap();
    assert_eq!(fs::read_to_string(r.join("a.txt")).unwrap(), "a\n");
    assert!(r.join("c.txt").exists());
    assert_eq!(stashes(&r).unwrap().len(), 2);

    // The older stash (b) as a branch: made where it was, applied there, and dropped.
    write_commit(&r, "b [1].txt", "b3\n", "b moved on");
    let b = stashes(&r)
        .unwrap()
        .into_iter()
        .find(|s| s.message.ends_with("just b"))
        .unwrap();
    assert!(!stash_branch(&r, "from-stash", &b.sha).unwrap());
    assert_eq!(
        run_text(&r, &["branch", "--show-current"]).unwrap().trim(),
        "from-stash"
    );
    assert_eq!(fs::read_to_string(r.join("b [1].txt")).unwrap(), "b2\n");
    assert_eq!(stashes(&r).unwrap().len(), 1);
    assert!(stash_branch(&r, "from-stash", &b.sha).is_err(), "gone now");
}

/// A request for stashing lines, as the view makes it from the unstaged diff.
fn stash_request(repo: &Path, path: &str, removed: &[u32], added: &[u32]) -> crate::lines::Request {
    let pair = diff_pair(repo, "unstaged", path, None, None, None, None, |p| {
        vfs::read_diff_side(repo, p)
    })
    .unwrap();
    let shown = |f: &FileText| f.exists.then(|| f.text.clone());
    crate::lines::Request {
        path: path.into(),
        old_path: None,
        kind: "unstaged".into(),
        action: "stash".into(),
        original: shown(&pair.original),
        modified: shown(&pair.modified),
        removed: removed.to_vec(),
        added: added.to_vec(),
    }
}

/// The index file as bytes: stat data, extensions and all, so any rewrite of it shows.
fn index_bytes(repo: &Path) -> Vec<u8> {
    fs::read(repo.join(".git/index")).unwrap()
}

/// Stashing some lines takes only those, leaves the staged changes (in other files and in the
/// same one) in the index, never writes the index, and applies back as just those lines.
#[test]
fn partial_stash_takes_the_chosen_lines_and_leaves_the_index_alone() {
    let sb = Sandbox::new("stash-lines");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "1\n2\n3\n4\n5\n6\n7\n8\n9\n", "base");
    write_commit(&r, "b.txt", "b\n", "b");
    // b is staged whole; a has one staged change, and two more in the working tree.
    fs::write(r.join("b.txt"), "b staged\n").unwrap();
    stage(&r, &["b.txt".into()]).unwrap();
    fs::write(r.join("a.txt"), "one\n2\n3\n4\n5\n6\n7\n8\n9\n").unwrap();
    stage(&r, &["a.txt".into()]).unwrap();
    fs::write(r.join("a.txt"), "one\n2\nthree\n4\n5\n6\n7\n8\nnine\n").unwrap();
    let before = index_bytes(&r);
    let staged_before = run_text(&r, &["ls-files", "-s"]).unwrap();

    // The change of 3 → three (removed line 3, added line 3).
    let req = stash_request(&r, "a.txt", &[3], &[3]);
    crate::lines::stash(&r, "just three", &req).unwrap();

    assert_eq!(index_bytes(&r), before, "the index file is untouched");
    assert_eq!(run_text(&r, &["ls-files", "-s"]).unwrap(), staged_before);
    assert_eq!(
        fs::read_to_string(r.join("a.txt")).unwrap(),
        "one\n2\n3\n4\n5\n6\n7\n8\nnine\n"
    );
    let list = stashes(&r).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].message, "On main: just three");
    // Shown against its base: only the chosen change, not the staged ones.
    let files = stash_files(&r, &list[0].sha).unwrap();
    assert_eq!(files.files.len(), 1);
    assert_eq!(
        (files.files[0].additions, files.files[0].deletions),
        (Some(1), Some(1))
    );

    // Back on a clean tree: HEAD plus the staged change, the stash then adds just "three".
    run(&r, &["reset", "-q", "--hard"]).unwrap();
    assert!(!stash_apply(&r, &list[0].sha, true).unwrap());
    assert_eq!(
        fs::read_to_string(r.join("a.txt")).unwrap(),
        "1\n2\nthree\n4\n5\n6\n7\n8\n9\n"
    );
}

/// A selection that went stale, or a failure partway, leaves the index file, the working tree
/// and the stash list as they were.
#[test]
fn a_failed_partial_stash_changes_nothing() {
    let sb = Sandbox::new("stash-lines-fail");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "1\n2\n3\n", "base");
    write_commit(&r, "b.txt", "b\n", "b");
    fs::write(r.join("b.txt"), "b2\n").unwrap();
    stage(&r, &["b.txt".into()]).unwrap();
    fs::write(r.join("a.txt"), "1\ntwo\n3\n").unwrap();
    let req = stash_request(&r, "a.txt", &[2], &[2]);
    let before = index_bytes(&r);
    // An agent writes the file after the diff was shown.
    fs::write(r.join("a.txt"), "1\ntwo\nthree\n").unwrap();
    let e = crate::lines::stash(&r, "x", &req).unwrap_err();
    assert!(e.contains("changed since"), "{e}");
    assert_eq!(index_bytes(&r), before);
    assert_eq!(
        fs::read_to_string(r.join("a.txt")).unwrap(),
        "1\ntwo\nthree\n"
    );
    assert!(stashes(&r).unwrap().is_empty());
    // Nothing chosen is no stash either.
    let none = stash_request(&r, "a.txt", &[], &[]);
    assert!(crate::lines::stash(&r, "x", &none).is_err());
    // With an operation under way it won't start.
    let ok = stash_request(&r, "a.txt", &[2], &[2]);
    fs::write(
        r.join(".git/MERGE_HEAD"),
        run_text(&r, &["rev-parse", "HEAD"]).unwrap(),
    )
    .unwrap();
    assert!(crate::lines::stash(&r, "x", &ok).is_err());
    assert_eq!(index_bytes(&r), before);
    assert!(stashes(&r).unwrap().is_empty());
}

/// A new file stashed whole by its lines leaves the folder; with no index file at all, git's
/// first commit still works from the scratch copy.
#[test]
fn partial_stash_of_a_whole_new_file_removes_it_and_a_deletion_stashes_as_a_deletion() {
    let sb = Sandbox::new("stash-lines-new");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "1\n2\n", "base");
    fs::write(r.join("new.txt"), "n1\nn2\n").unwrap();
    let req = stash_request(&r, "new.txt", &[], &[1, 2]);
    crate::lines::stash(&r, "", &req).unwrap();
    assert!(!r.join("new.txt").exists());
    let list = stashes(&r).unwrap();
    assert!(
        list[0].message.starts_with("WIP on main: "),
        "{}",
        list[0].message
    );

    fs::remove_file(r.join("a.txt")).unwrap();
    let req = stash_request(&r, "a.txt", &[1, 2], &[]);
    crate::lines::stash(&r, "gone", &req).unwrap();
    assert_eq!(fs::read_to_string(r.join("a.txt")).unwrap(), "1\n2\n");
    let top = stashes(&r).unwrap();
    assert_eq!(top[0].message, "On main: gone");
    let files = stash_files(&r, &top[0].sha).unwrap();
    assert_eq!(files.files[0].status, "D");
}

/// Renaming keeps the commit, moves it to the top and drops the old entry; never leaves it missing.
#[test]
fn renaming_a_stash_keeps_its_commit() {
    let sb = Sandbox::new("stash-rename");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "a\n", "base");
    let push = |text: &str, msg: &str| {
        fs::write(r.join("a.txt"), text).unwrap();
        let what = StashWhat {
            untracked: false,
            staged: false,
            paths: &[],
        };
        stash_push(&r, msg, what).unwrap();
    };
    push("1\n", "first");
    push("2\n", "second");
    let list = stashes(&r).unwrap();
    let first = list[1].sha.clone();
    assert_eq!(list[1].message, "On main: first");
    stash_rename(&r, &first, "  better name ").unwrap();
    let now = stashes(&r).unwrap();
    assert_eq!(now.len(), 2);
    assert_eq!(
        (now[0].sha.as_str(), now[0].message.as_str()),
        (first.as_str(), "On main: better name")
    );
    assert_eq!(now[1].sha, list[0].sha);
    assert!(stash_rename(&r, &first, "  ").is_err());
    assert!(stash_rename(&r, "deadbeef", "x").is_err());
    assert_eq!(stashes(&r).unwrap().len(), 2);
}

/// One file taken out of a stash, an untracked one from its third parent, journaled: Undo puts
/// back the version it replaced.
#[test]
fn taking_one_file_from_a_stash_can_be_undone() {
    let sb = Sandbox::new("stash-file");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "a\n", "base");
    write_commit(&r, "b.txt", "b\n", "b");
    fs::write(r.join("a.txt"), "a stashed\n").unwrap();
    fs::write(r.join("b.txt"), "b stashed\n").unwrap();
    fs::write(r.join("new.txt"), "new stashed\n").unwrap();
    let what = StashWhat {
        untracked: true,
        staged: false,
        paths: &[],
    };
    stash_push(&r, "all", what).unwrap();
    let sha = stashes(&r).unwrap()[0].sha.clone();
    fs::write(r.join("a.txt"), "a newer\n").unwrap();
    let j = Journal::default();
    let take = |path: &str, untracked: bool| {
        let (sha, p) = (sha.clone(), path.to_string());
        let write = move |r: &Path| stash_restore_file(r, &sha, &p, untracked).map(|_| vec![]);
        let paths = [path.to_string()];
        j.replace(
            &r,
            "Take".into(),
            "before stash",
            &paths,
            &Mutex::new(()),
            write,
        )
        .unwrap();
    };
    take("a.txt", false);
    assert_eq!(fs::read_to_string(r.join("a.txt")).unwrap(), "a stashed\n");
    // Only that file: b is still as committed, and the index is untouched.
    assert_eq!(fs::read_to_string(r.join("b.txt")).unwrap(), "b\n");
    assert!(run_text(&r, &["diff", "--cached", "--name-only"])
        .unwrap()
        .is_empty());
    take("new.txt", true);
    assert_eq!(
        fs::read_to_string(r.join("new.txt")).unwrap(),
        "new stashed\n"
    );
    step(&j, &r, false).unwrap();
    assert!(!r.join("new.txt").exists());
    step(&j, &r, false).unwrap();
    assert_eq!(fs::read_to_string(r.join("a.txt")).unwrap(), "a newer\n");
    assert_eq!(stashes(&r).unwrap().len(), 1, "the stash stays");
}
