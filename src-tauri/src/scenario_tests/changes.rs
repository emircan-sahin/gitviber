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
