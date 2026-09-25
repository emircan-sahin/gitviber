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
