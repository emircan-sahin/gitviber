//! Push, pull, fetch, publishing, remotes, clone and submodule updates.

use super::*;

#[test]
fn pull_modes_on_diverged_branches() {
    let sb = Sandbox::new("pull");
    let c = sb.remote_with_clones(2);
    let (a, b) = (&c[0], &c[1]);
    write_commit(a, "a.txt", "one\ntwo\nthree\nfour\n", "a appends");
    run(a, &["push", "-q"]).unwrap();
    write_commit(b, "b.txt", "b\n", "b adds a file");
    fetch(b, &Net::default()).unwrap();
    let st = status(b).unwrap();
    assert_eq!((st.ahead, st.behind), (1, 1));

    // Fast-forward only must refuse, and must not leave an operation behind.
    assert!(pull(b, PullMode::Ff, false, &Net::default()).is_err());
    assert!(operation(b).is_none());
    // A clean merge finishes without stopping.
    assert!(!pull(b, PullMode::Merge, false, &Net::default()).unwrap());
    assert_eq!(log(b, None, 0, 1).unwrap()[0].parents.len(), 2);
    assert_eq!(
        fs::read_to_string(b.join("a.txt")).unwrap(),
        "one\ntwo\nthree\nfour\n"
    );
}

/// Uncommitted changes to a file the pull touches: every mode refuses, in the words gitErrors.ts
/// keys on, and autostash sets them aside and puts them back. Reapplied onto a conflict, they
/// stop as conflicts and stay in the stash.
#[test]
fn pull_with_autostash_over_uncommitted_changes() {
    let sb = Sandbox::new("autostash");
    let c = sb.remote_with_clones(2);
    let (a, b) = (&c[0], &c[1]);
    write_commit(a, "a.txt", "one\ntwo\nthree\nfour\n", "a appends");
    run(a, &["push", "-q"]).unwrap();
    fs::write(b.join("a.txt"), "zero\none\ntwo\nthree\n").unwrap();

    let refused = pull(b, PullMode::Ff, false, &Net::default()).unwrap_err();
    assert!(
        refused.contains("would be overwritten by merge"),
        "{refused}"
    );
    let refused = pull(b, PullMode::Rebase, false, &Net::default()).unwrap_err();
    assert!(refused.contains("cannot pull with rebase"), "{refused}");

    assert!(!pull(b, PullMode::Ff, true, &Net::default()).unwrap());
    assert_eq!(
        fs::read_to_string(b.join("a.txt")).unwrap(),
        "zero\none\ntwo\nthree\nfour\n"
    );
    assert!(stashes(b).unwrap().is_empty());

    write_commit(
        a,
        "a.txt",
        "one\ntwo\nthree\nfour\nfive\n",
        "a appends again",
    );
    run(a, &["push", "-q"]).unwrap();
    fs::write(b.join("a.txt"), "zero\none\ntwo\nthree\nfour\nmine\n").unwrap();
    assert!(pull(b, PullMode::Rebase, true, &Net::default()).unwrap());
    assert_eq!(status(b).unwrap().conflicted.len(), 1);
    assert_eq!(stashes(b).unwrap().len(), 1);
}

/// A merge pull that stops on conflicts holds the autostashed changes (MERGE_AUTOSTASH) out of
/// the worktree, not in Stashes; the banner's Continue (`commit --no-edit`) brings them back.
#[test]
fn autostash_waits_out_a_conflicted_merge_pull() {
    let sb = Sandbox::new("mergeautostash");
    let c = sb.remote_with_clones(2);
    let (a, b) = (&c[0], &c[1]);
    write_commit(a, "a.txt", "a\n", "a edits");
    run(a, &["push", "-q"]).unwrap();
    write_commit(b, "a.txt", "b\n", "b edits");
    write_commit(b, "notes.txt", "kept\n", "notes");
    fs::write(b.join("notes.txt"), "uncommitted\n").unwrap();

    assert!(pull(b, PullMode::Merge, true, &Net::default()).unwrap());
    assert_eq!(operation(b).unwrap().kind, "merge");
    assert_eq!(fs::read_to_string(b.join("notes.txt")).unwrap(), "kept\n");
    assert!(stashes(b).unwrap().is_empty());

    fs::write(b.join("a.txt"), "a and b\n").unwrap();
    stage(b, &["a.txt".into()]).unwrap();
    assert!(!op_continue(b).unwrap());
    assert!(operation(b).is_none());
    assert_eq!(
        fs::read_to_string(b.join("notes.txt")).unwrap(),
        "uncommitted\n"
    );
}

/// After an amend the plain push is rejected as non-fast-forward (the UI keys on those words);
/// the lease push replaces the old commit, but not over someone else's unfetched push.
#[test]
fn force_push_with_lease_after_amend() {
    let sb = Sandbox::new("lease");
    let c = sb.remote_with_clones(2);
    let (a, b) = (&c[0], &c[1]);
    write_commit(a, "a.txt", "mine\n", "mine");
    push(a, false, None, &Net::default()).unwrap();
    commit(a, "mine, reworded", &AMEND).unwrap();
    let err = push(a, false, None, &Net::default()).unwrap_err();
    assert!(err.contains("non-fast-forward"), "{err}");
    push(a, true, None, &Net::default()).unwrap();
    // b pushes meanwhile; a, not having fetched it, amends again: the lease refuses.
    fetch(b, &Net::default()).unwrap();
    run(b, &["merge", "-q", "--ff-only", "origin/main"]).unwrap();
    write_commit(b, "b.txt", "b\n", "theirs");
    push(b, false, None, &Net::default()).unwrap();
    commit(a, "mine, again", &AMEND).unwrap();
    assert!(push(a, true, None, &Net::default()).is_err());
}

/// While "Force push?" is asked, someone pushes and the background fetch brings their commit
/// in, which moves the lease onto it. The force push must still refuse to drop it.
#[test]
fn force_push_after_a_background_fetch_keeps_their_commit() {
    let sb = Sandbox::new("lease-fetch");
    let c = sb.remote_with_clones(2);
    let (a, b) = (&c[0], &c[1]);
    write_commit(a, "a.txt", "mine\n", "mine");
    push(a, false, None, &Net::default()).unwrap();
    commit(a, "mine, reworded", &AMEND).unwrap();
    let err = push(a, false, None, &Net::default()).unwrap_err();
    assert!(err.contains("non-fast-forward"), "{err}");

    fetch(b, &Net::default()).unwrap();
    run(b, &["merge", "-q", "--ff-only", "origin/main"]).unwrap();
    write_commit(b, "b.txt", "b\n", "theirs");
    push(b, false, None, &Net::default()).unwrap();
    let theirs = run_text(b, &["rev-parse", "HEAD"]).unwrap();
    fetch(a, &Net::default()).unwrap();

    assert!(push(a, true, None, &Net::default()).is_err());
    let remote = run_text(&sb.path("origin.git"), &["rev-parse", "main"]).unwrap();
    assert_eq!(remote, theirs);
}

/// A PR is titled like GitHub does: one commit gives its message, more the branch name.
#[test]
fn pull_draft_counts_commits_against_the_base() {
    let sb = Sandbox::new("draft");
    let c = sb.remote_with_clones(1);
    let a = &c[0];
    run(a, &["switch", "-q", "-c", "feat"]).unwrap();
    fs::write(a.join("f.txt"), "f\n").unwrap();
    stage(a, &["f.txt".into()]).unwrap();
    commit(a, "Add f\n\nWhy it matters.", &CommitOptions::default()).unwrap();
    let d = pull_draft(a, "refs/remotes/origin/main").unwrap();
    assert_eq!(
        (d.commits, d.subject.as_deref(), d.body.as_deref()),
        (1, Some("Add f"), Some("Why it matters."))
    );
    write_commit(a, "g.txt", "g\n", "Add g");
    let d = pull_draft(a, "refs/remotes/origin/main").unwrap();
    assert_eq!((d.commits, d.subject), (2, None));
    assert!(pull_draft(a, "main").is_err());
    assert!(pull_draft(a, "refs/remotes/--all").is_err());
}

/// The fork workflow git documents: pull from upstream, push to origin (remote.pushDefault).
#[test]
fn push_target_follows_push_default_not_the_upstream() {
    let sb = Sandbox::new("pushdef");
    let c = sb.remote_with_clones(2);
    let (a, b) = (&c[0], &c[1]);
    // b plays the fork: "upstream" is the original (the shared bare repo), origin its own.
    let url = git_url(b);
    run(b, &["remote", "rename", "origin", "upstream"]).unwrap();
    let fork = sb.path("fork.git");
    run(&sb.0, &["init", "-q", "--bare", fork.to_str().unwrap()]).unwrap();
    run(b, &["remote", "add", "origin", fork.to_str().unwrap()]).unwrap();
    assert_eq!(url, git_url_of(b, "upstream"));
    let st = status(b).unwrap();
    assert_eq!(st.upstream.as_deref(), Some("upstream/main"));
    // Without a push default, pushes follow the upstream: into the original.
    assert_eq!(st.push.unwrap().remote, "upstream");

    set_push_default(b, "origin").unwrap();
    write_commit(b, "b.txt", "b\n", "fork work");
    let p = status(b).unwrap().push.unwrap();
    assert_eq!((p.remote.as_str(), p.branch.as_deref()), ("origin", None));
    push(b, false, None, &Net::default()).unwrap();
    let st = status(b).unwrap();
    let p = st.push.unwrap();
    assert_eq!((p.branch.as_deref(), p.ahead), (Some("origin/main"), 0));
    // Still pulls from the original, and the original didn't get the commit.
    assert_eq!(st.upstream.as_deref(), Some("upstream/main"));
    assert_eq!(st.ahead, 1);
    run(a, &["pull", "-q"]).unwrap();
    assert!(log(a, None, 0, 5)
        .unwrap()
        .iter()
        .all(|c| c.subject != "fork work"));
    assert!(set_push_default(b, "nope").is_err());
}

#[test]
fn publish_sets_upstream() {
    let sb = Sandbox::new("publish");
    let c = sb.remote_with_clones(1);
    let a = &c[0];
    switch_branch(a, "feat/new-thing", true).unwrap();
    write_commit(a, "n.txt", "n\n", "new");
    assert!(status(a).unwrap().upstream.is_none());
    push(a, false, None, &Net::default()).unwrap();
    let st = status(a).unwrap();
    assert_eq!(st.upstream.as_deref(), Some("origin/feat/new-thing"));
    assert_eq!(st.ahead, 0);
    // The remote branch is listed and switching to its short name works from another clone.
    let b = sb.clone_of("late");
    assert!(branches(&b)
        .unwrap()
        .iter()
        .any(|x| x.remote && x.name == "origin/feat/new-thing"));
    switch_branch(&b, "feat/new-thing", false).unwrap();
    assert_eq!(
        status(&b).unwrap().branch.as_deref(),
        Some("feat/new-thing")
    );
}

#[test]
fn publish_picks_the_remote_instead_of_assuming_origin() {
    let sb = Sandbox::new("pubremote");
    let c = sb.remote_with_clones(1);
    let a = &c[0];
    run(a, &["remote", "rename", "origin", "gh"]).unwrap();
    switch_branch(a, "feat", true).unwrap();
    // The only remote, whatever its name.
    assert_eq!(status(a).unwrap().publish.as_deref(), Some("gh"));
    push(a, false, None, &Net::default()).unwrap();
    assert_eq!(status(a).unwrap().upstream.as_deref(), Some("gh/feat"));

    // Several remotes and none is origin: the user picks.
    let other = sb.path("other.git");
    run(&sb.0, &["init", "-q", "--bare", other.to_str().unwrap()]).unwrap();
    run(a, &["remote", "add", "other", other.to_str().unwrap()]).unwrap();
    switch_branch(a, "feat2", true).unwrap();
    let st = status(a).unwrap();
    assert_eq!((st.publish.as_deref(), st.remotes.len()), (None, 2));
    assert!(push(a, false, None, &Net::default())
        .unwrap_err()
        .contains("several remotes"));
    assert!(push(a, false, Some("nope"), &Net::default()).is_err());
    push(a, false, Some("other"), &Net::default()).unwrap();
    assert_eq!(status(a).unwrap().upstream.as_deref(), Some("other/feat2"));

    // remote.pushDefault decides when set.
    set_push_default(a, "other").unwrap();
    switch_branch(a, "feat3", true).unwrap();
    assert_eq!(status(a).unwrap().publish.as_deref(), Some("other"));

    // No remote at all: a clear message, not a raw git error.
    let lone = sb.path("lone");
    init(&lone);
    write_commit(&lone, "x.txt", "x\n", "x");
    assert!(push(&lone, false, None, &Net::default())
        .unwrap_err()
        .contains("no remote"));
}

#[test]
fn gone_upstream_is_unknown_not_pushed() {
    let sb = Sandbox::new("gone");
    let c = sb.remote_with_clones(1);
    let a = &c[0];
    switch_branch(a, "feat", true).unwrap();
    write_commit(a, "f.txt", "f\n", "feature");
    push(a, false, None, &Net::default()).unwrap();
    run(a, &["push", "-q", "origin", "--delete", "feat"]).unwrap();
    fetch(a, &Net::default()).unwrap();
    write_commit(a, "g.txt", "g\n", "after the branch was deleted");

    let commits = log(a, None, 0, 10).unwrap();
    assert!(commits.iter().all(|x| !x.unpushed));
    assert!(!drops_pushed(a, &commits[2].sha).unwrap());
    // "feature" was only ever on the deleted branch; base is still on origin/main.
    let on: Vec<bool> = commits.iter().map(|x| x.on_origin).collect();
    assert_eq!(on, [false, false, true]);

    let local = sb.path("local");
    init(&local);
    write_commit(&local, "x.txt", "x\n", "x");
    assert!(!log(&local, None, 0, 5).unwrap()[0].on_origin);
}

/// Clone streams progress, tracks origin, and never writes into a folder that holds something.
#[test]
fn clone_from_a_local_bare_remote() {
    let sb = Sandbox::new("clone");
    sb.remote_with_clones(0);
    // file:// takes git's transfer path (a plain path would hardlink), so progress shows.
    let url = format!("file://{}", sb.path("origin.git").display());
    let seen = std::sync::Arc::new(Mutex::new(Vec::new()));
    let sink = seen.clone();
    let running = crate::network::Running::default();
    let net = running.start("clone".into(), move |p| sink.lock().unwrap().push(p.phase));
    let dest = clone(&sb.0, &url, "mine", &net).unwrap();
    assert_eq!(PathBuf::from(&dest), sb.path("mine"));
    let st = status(Path::new(&dest)).unwrap();
    assert_eq!(st.branch.as_deref(), Some("main"));
    assert_eq!(st.upstream.as_deref(), Some("origin/main"));
    assert!(seen
        .lock()
        .unwrap()
        .iter()
        .any(|p| p == "Receiving objects"));

    fs::create_dir_all(sb.path("taken")).unwrap();
    fs::write(sb.path("taken/keep.txt"), "mine\n").unwrap();
    let err = clone(&sb.0, &url, "taken", &Net::default()).unwrap_err();
    assert!(err.contains("isn't empty"), "{err}");
    assert_eq!(fs::read_dir(sb.path("taken")).unwrap().count(), 1);
    fs::create_dir_all(sb.path("empty")).unwrap();
    clone(&sb.0, &url, "empty", &Net::default()).unwrap();
    for bad in ["", "..", "a/b"] {
        assert!(clone(&sb.0, &url, bad, &Net::default()).is_err(), "{bad}");
    }
    let missing = format!("file://{}", sb.path("nope.git").display());
    assert!(clone(&sb.0, &missing, "nope", &Net::default()).is_err());
    assert!(
        !sb.path("nope").exists(),
        "a failed clone leaves its folder behind"
    );
}

/// A fetch from the main worktree counts for its linked worktrees, which keep FETCH_HEADs of
/// their own; otherwise the background fetch would fetch again from each of them.
#[test]
fn last_fetch_counts_the_main_worktree() {
    let sb = Sandbox::new("lastfetch");
    let c = sb.remote_with_clones(1);
    let a = &c[0];
    let linked = sb.path("linked");
    run(
        a,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "side",
            linked.to_str().unwrap(),
        ],
    )
    .unwrap();
    assert_eq!(last_fetch(&linked), None);
    fetch(a, &Net::default()).unwrap();
    assert!(last_fetch(&linked).is_some());
    assert_eq!(last_fetch(&linked), last_fetch(a));
}

#[test]
fn remotes_added_renamed_repointed_and_removed() {
    let sb = Sandbox::new("remotes");
    let c = sb.remote_with_clones(1);
    let r = &c[0];
    let origin = sb.path("origin.git");
    remote_add(r, "backup", origin.to_str().unwrap()).unwrap();
    assert!(remote_add(r, "-x", "u").is_err());
    assert!(remote_add(r, "a/b", "u").is_err());
    assert!(remote_add(r, "ok", "--upload-pack=evil").is_err());
    let names = |r: &Path| {
        remote_list(r)
            .unwrap()
            .into_iter()
            .map(|x| x.name)
            .collect::<Vec<_>>()
    };
    assert_eq!(names(r), ["backup", "origin"]);
    run(r, &["fetch", "-q", "backup"]).unwrap();
    remote_rename(r, "backup", "spare").unwrap();
    assert_eq!(names(r), ["origin", "spare"]);
    assert!(run(
        r,
        &["rev-parse", "--verify", "-q", "refs/remotes/spare/main"]
    )
    .is_ok());
    remote_set_url(r, "spare", "https://example.com/x.git").unwrap();
    let spare = remote_list(r)
        .unwrap()
        .into_iter()
        .find(|x| x.name == "spare")
        .unwrap();
    assert_eq!(spare.url, "https://example.com/x.git");
    remote_remove(r, "spare").unwrap();
    assert_eq!(names(r), ["origin"]);
    assert!(run(
        r,
        &["rev-parse", "--verify", "-q", "refs/remotes/spare/main"]
    )
    .is_err());
}

/// remote_list reads every remote from one config read, and remote_urls from one
/// `git remote -v`; both must say what `git remote`, `config --get` and `get-url` say.
#[test]
fn remotes_read_at_once_match_one_by_one() {
    let sb = Sandbox::new("remotes-at-once");
    let c = sb.remote_with_clones(1);
    let r = &c[0];
    for args in [
        vec!["remote", "add", "my.fork", "https://github.com/me/fork.git"],
        vec![
            "config",
            "remote.my.fork.pushurl",
            "git@github.com:me/fork.git",
        ],
        // No URL at all, only a fetch refspec.
        vec![
            "config",
            "remote.bare.fetch",
            "+refs/heads/*:refs/remotes/bare/*",
        ],
        vec!["config", "url.git@github.com:.insteadOf", "gh:"],
        vec!["remote", "add", "short", "gh:o/r"],
    ] {
        run(r, &args).unwrap();
    }
    let listed: Vec<String> = run_text(r, &["remote"])
        .unwrap()
        .lines()
        .map(str::to_string)
        .collect();
    let list = remote_list(r).unwrap();
    assert_eq!(
        list.iter().map(|x| x.name.clone()).collect::<Vec<_>>(),
        listed
    );
    let fork = list.iter().find(|x| x.name == "my.fork").unwrap();
    assert_eq!(fork.url, "https://github.com/me/fork.git");
    assert_eq!(fork.push_url.as_deref(), Some("git@github.com:me/fork.git"));
    let bare = list.iter().find(|x| x.name == "bare").unwrap();
    assert_eq!((bare.url.as_str(), bare.push_url.as_deref()), ("", None));
    // The raw URL for editing, not the rewritten one.
    let short = list.iter().find(|x| x.name == "short").unwrap();
    assert_eq!(short.url, "gh:o/r");

    // get-url echoes a URL-less remote's name back.
    let one_by_one: Vec<_> = listed
        .iter()
        .map(|n| (n.clone(), remote_url(r, n).filter(|_| n != "bare")))
        .collect();
    assert_eq!(remote_urls(r), one_by_one);
    let on_github: Vec<_> = crate::github::remotes(r)
        .into_iter()
        .map(|x| (x.name, x.repo))
        .collect();
    assert!(on_github.contains(&("short".into(), Some("o/r".into()))));
    assert!(on_github.contains(&("my.fork".into(), Some("me/fork".into()))));
    assert!(on_github.contains(&("bare".into(), None)));
}

#[test]
fn submodules_listed_and_set_up() {
    let sb = Sandbox::new("submodules");
    let lib = sb.path("lib");
    init(&lib);
    write_commit(&lib, "lib.txt", "lib\n", "lib");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "a\n", "base");
    assert!(submodules(&r).unwrap().is_empty());
    // Local paths as submodule URLs are off by default since git 2.38.
    run(
        &r,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            "-q",
            lib.to_str().unwrap(),
            "vendor/lib",
        ],
    )
    .unwrap();
    identity(&r.join("vendor/lib"));
    commit(&r, "add lib", &CommitOptions::default()).unwrap();
    let subs = submodules(&r).unwrap();
    assert_eq!(
        (subs[0].path.as_str(), subs[0].state.as_str()),
        ("vendor/lib", "ok")
    );

    // Registered but not set up (as in a fresh clone); its repository stays in .git/modules, so
    // setting it up again needs no clone, which local paths wouldn't be allowed for.
    run(&r, &["submodule", "deinit", "-q", "-f", "vendor/lib"]).unwrap();
    assert_eq!(submodules(&r).unwrap()[0].state, "missing");
    assert!(!r.join("vendor/lib/lib.txt").exists());
    submodule_update(&r, &Net::default()).unwrap();
    assert_eq!(submodules(&r).unwrap()[0].state, "ok");
    assert!(r.join("vendor/lib/lib.txt").exists());
    // Moved to another commit inside: "moved" until updated back.
    write_commit(&r.join("vendor/lib"), "lib.txt", "newer\n", "newer");
    assert_eq!(submodules(&r).unwrap()[0].state, "moved");
    submodule_update(&r, &Net::default()).unwrap();
    assert_eq!(submodules(&r).unwrap()[0].state, "ok");
}
