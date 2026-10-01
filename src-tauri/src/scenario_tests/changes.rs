//! Staging and discarding, and status in edge cases.

use super::*;

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
                crate::state::with_index_lock(&lock, &r, |r| stage(r, &[format!("f{i}.txt")]))
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
    crate::state::with_index_lock(&lock, &r, |r| stage(r, &["late.txt".into()])).unwrap();
    release.join().unwrap();
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
fn unstaging_a_rename_with_its_old_path_undoes_it() {
    let sb = Sandbox::new("unrename");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "a\n", "base");
    run(&r, &["mv", "a.txt", "b.txt"]).unwrap();
    let st = status(&r).unwrap();
    assert_eq!(st.staged[0].old_path.as_deref(), Some("a.txt"));
    unstage(&r, &["b.txt".into(), "a.txt".into()]).unwrap();
    let st = status(&r).unwrap();
    assert!(st.staged.is_empty(), "a.txt's deletion stayed staged");
    let unstaged: Vec<_> = st
        .unstaged
        .iter()
        .map(|f| (f.path.as_str(), f.status.as_str()))
        .collect();
    assert_eq!(unstaged, [("a.txt", "D"), ("b.txt", "?")]);
}

/// Past the OS's argv limit (an agent's unignored node_modules) git couldn't be started at all.
#[test]
fn staging_unstaging_and_discarding_more_paths_than_argv_holds() {
    let sb = Sandbox::new("manypaths");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "seed", "s\n", "seed");
    // 3000 paths of ~890 bytes: 2.6 MB, past macOS's 1 MB and Linux's usual 2 MB. Each file
    // is named 6 times over, as git matches every path against every pathspec.
    let dir = ["d", "e", "f", "g"].map(|c| c.repeat(220)).join("/");
    fs::create_dir_all(r.join(&dir)).unwrap();
    let files: Vec<String> = (0..500).map(|i| format!("{dir}/f{i}")).collect();
    for f in &files {
        fs::write(r.join(f), "a\n").unwrap();
    }
    let many: Vec<String> = (0..6).flat_map(|_| files.clone()).collect();
    // A folder among them takes stage's check for nested repositories.
    let mut with_dir = many.clone();
    with_dir.push(dir.clone());
    stage(&r, &with_dir).unwrap();
    assert_eq!(status(&r).unwrap().staged.len(), 500);
    unstage(&r, &many).unwrap();
    assert!(status(&r).unwrap().staged.is_empty());

    stage(&r, &files).unwrap();
    commit(&r, "many", &CommitOptions::default()).unwrap();
    for f in &files {
        fs::write(r.join(f), "b\n").unwrap();
    }
    discard(&r, &many).unwrap();
    assert!(status(&r).unwrap().unstaged.is_empty());
    assert_eq!(fs::read_to_string(r.join(&files[499])).unwrap(), "a\n");
}

/// chmod +x alone has no text diff; status says what changed.
#[cfg(unix)]
#[test]
fn a_mode_change_is_reported() {
    use std::os::unix::fs::PermissionsExt;
    let sb = Sandbox::new("mode");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "run.sh", "echo\n", "base");
    fs::set_permissions(r.join("run.sh"), fs::Permissions::from_mode(0o755)).unwrap();
    let st = status(&r).unwrap();
    assert_eq!(st.unstaged[0].mode.as_deref(), Some("100644 → 100755"));
    stage(&r, &["run.sh".into()]).unwrap();
    let st = status(&r).unwrap();
    assert_eq!(st.staged[0].mode.as_deref(), Some("100644 → 100755"));
    assert!(st.unstaged.is_empty());
    // A new file has no old mode to compare.
    fs::write(r.join("new.sh"), "x\n").unwrap();
    stage(&r, &["new.sh".into()]).unwrap();
    let st = status(&r).unwrap();
    assert!(st
        .staged
        .iter()
        .find(|f| f.path == "new.sh")
        .unwrap()
        .mode
        .is_none());
}

/// Changes inside a submodule show as the submodule's row, with nothing of its own to diff.
#[test]
fn a_submodule_with_changes_inside_says_so() {
    let sb = Sandbox::new("subinside");
    let r = repo_with_submodule(&sb);
    fs::write(r.join("sub/l.txt"), "edited\n").unwrap();
    fs::write(r.join("sub/new.txt"), "n\n").unwrap();
    fs::write(r.join("a.txt"), "b\n").unwrap();
    let st = status(&r).unwrap();
    let sub = st.unstaged.iter().find(|f| f.path == "sub").unwrap();
    assert_eq!(sub.submodule.as_deref(), Some("S.MU"));
    let a = st.unstaged.iter().find(|f| f.path == "a.txt").unwrap();
    assert!(a.submodule.is_none());
}

/// A mixed reset moves the index under an unstaged change while its letter and its file stay
/// the same: the stacked diff reads the file again by the index blob it's against.
#[test]
fn an_unstaged_change_names_the_index_blob_it_is_against() {
    let sb = Sandbox::new("indexoid");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "1\n", "one");
    write_commit(&r, "a.txt", "2\n", "two");
    fs::write(r.join("a.txt"), "3\n").unwrap();
    let before = status(&r).unwrap().unstaged[0].clone();
    assert_eq!(before.index_oid, Some(rev(&r, ":a.txt")));
    let head = rev(&r, "HEAD");
    reset(&r, &rev(&r, "HEAD~1"), ResetMode::Mixed, &head).unwrap();
    let st = status(&r).unwrap();
    assert!(st.staged.is_empty());
    let after = &st.unstaged[0];
    assert_eq!((&after.status, &after.oid), (&before.status, &before.oid));
    assert_eq!(after.index_oid, Some(rev(&r, ":a.txt")));
    assert_ne!(after.index_oid, before.index_oid);
}

/// A submodule's folder doesn't change as its checkout moves from one new commit to another.
#[test]
fn a_moved_submodule_names_its_commit() {
    let sb = Sandbox::new("submoved");
    let r = repo_with_submodule(&sb);
    let sub = r.join("sub");
    write_commit(&sub, "l.txt", "2\n", "two");
    let oid = |r: &Path| status(r).unwrap().unstaged[0].oid.clone();
    assert_eq!(oid(&r), Some(rev(&sub, "HEAD")));
    write_commit(&sub, "l.txt", "3\n", "three");
    assert_eq!(oid(&r), Some(rev(&sub, "HEAD")));
}
