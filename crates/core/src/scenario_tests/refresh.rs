//! What a Changes refresh reads, in as few git calls as give the same answers.

use super::*;

/// `stash list` reads refs/stash's reflog: with none there is nothing to ask git.
#[test]
fn stashes_are_read_only_where_there_are_some() {
    let sb = Sandbox::new("stashread");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "a\n", "base");
    let common = |repo: &Path| git_dirs(repo).unwrap().common;
    // No git call: git would fail in a folder that isn't there.
    let nowhere = sb.path("nowhere");
    assert!(stashes_in(&nowhere, Some(&common(&r))).unwrap().is_empty());
    assert!(stashes(&r).unwrap().is_empty());

    fs::write(r.join("a.txt"), "b\n").unwrap();
    run(&r, &["stash", "-q"]).unwrap();
    let wt = sb.path("wt");
    run(&r, &["worktree", "add", "-q", wt.to_str().unwrap()]).unwrap();
    let shas = |list: Vec<Stash>| list.into_iter().map(|s| s.sha).collect::<Vec<_>>();
    for repo in [&r, &wt] {
        let listed = shas(stashes_in(repo, Some(&common(repo))).unwrap());
        assert_eq!(listed.len(), 1);
        assert_eq!(listed, shas(stashes(repo).unwrap()));
    }

    // reftable (git 2.45+) keeps the reflog where only git reads it.
    let rt = sb.path("rt");
    if run(
        &sb.0,
        &["init", "-q", "--ref-format=reftable", rt.to_str().unwrap()],
    )
    .is_ok()
    {
        identity(&rt);
        write_commit(&rt, "a.txt", "a\n", "base");
        fs::write(rt.join("a.txt"), "b\n").unwrap();
        run(&rt, &["stash", "-q"]).unwrap();
        assert_eq!(stashes_in(&rt, Some(&common(&rt))).unwrap().len(), 1);
    }
}

/// Amend's message comes from one `git log`, the same as `find_commit`'s nine calls give.
#[test]
fn heads_message_is_the_commits_own() {
    let sb = Sandbox::new("headmsg");
    let r = sb.path("r");
    init(&r);
    assert_eq!(head_message(&r), Ok(None));
    fs::write(r.join("a.txt"), "a\n").unwrap();
    stage(&r, &["a.txt".into()]).unwrap();
    let message = "subject\nwrapped\n\nbody\n\n  more  \n";
    commit(&r, message, &CommitOptions::default(), &Net::default()).unwrap();
    let st = status(&r).unwrap();
    let oid = st.head_oid.clone().unwrap();
    assert_eq!(oid.len(), 40);
    assert!(oid.starts_with(st.head.as_deref().unwrap()));
    let found = find_commit(&r, &oid).unwrap().unwrap();
    let head = head_message(&r).unwrap().unwrap();
    assert_eq!(
        (head.sha, head.subject, head.body),
        (found.sha, found.subject, found.body)
    );
}

/// The open repo's git dirs are read on its first refresh, and closing it forgets them.
#[test]
fn the_open_repos_git_dirs_are_kept_until_it_closes() {
    use std::sync::Arc;
    let sb = Sandbox::new("dirscache");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "a\n", "base");
    let wt = sb.path("wt");
    run(&r, &["worktree", "add", "-q", wt.to_str().unwrap()]).unwrap();
    let wt = wt.canonicalize().unwrap();
    let core = crate::Core::new(Arc::new(|_| {}));
    *core.repo.lock().unwrap() = Some(wt.clone());
    let st = crate::api::changes::status(&core).unwrap();
    assert_eq!(st.root, wt.to_string_lossy());
    let (root, dirs) = core.dirs.lock().unwrap().clone().unwrap();
    assert_eq!(root, wt);
    assert_eq!(Some(dirs.clone()), git_dirs(&wt));
    assert_ne!(
        dirs.dir, dirs.common,
        "a linked worktree's own dir isn't the common one"
    );
    crate::api::repo::close_repo(&core).unwrap();
    assert!(core.dirs.lock().unwrap().is_none());
}
