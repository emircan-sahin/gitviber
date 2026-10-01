//! Linked worktrees: listing, nesting, locks and removal.

use super::*;

/// Worktrees made beside the project share `<project>.worktrees`; removing the last one
/// takes the folder along, while one still inside keeps it.
#[test]
fn removing_the_last_worktree_removes_its_folder() {
    let sb = Sandbox::new("wtdir");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "a\n", "base");
    run(&r, &["branch", "one"]).unwrap();
    run(&r, &["branch", "two"]).unwrap();
    let one = add_worktree(&r, "one", None, None).unwrap();
    let two = add_worktree(&r, "two", None, None).unwrap();
    let dir = sb.path("r.worktrees");
    assert!(dir.is_dir());

    remove_worktree(&r, &one, false).unwrap();
    assert!(dir.is_dir(), "two is still in there");
    remove_worktree(&r, &two, false).unwrap();
    assert!(!dir.exists());
}

#[test]
fn worktree_list_detached_prunable_and_counts() {
    let sb = Sandbox::new("wtlist");
    let r = repo_with_worktrees(&sb);
    let agent = r.join(".claude/worktrees/agent");
    fs::write(agent.join("a.txt"), "changed\n").unwrap();
    fs::write(agent.join("new.txt"), "new\n").unwrap();

    let list = worktrees(&r).unwrap();
    assert_eq!(list.len(), 4);
    let find = |dir: &Path| list.iter().find(|w| same_dir(&w.path, dir)).unwrap();
    let main = find(&r);
    assert!(main.main && main.current && main.branch.as_deref() == Some("main"));
    let a = find(&agent);
    assert!(!a.main && !a.current && a.branch.as_deref() == Some("agent"));
    let det = find(&sb.path("det"));
    assert!(det.detached && det.branch.is_none() && det.head.is_some());
    let gone = list.iter().find(|w| w.path.ends_with("/gone")).unwrap();
    assert!(gone.prunable);

    assert_eq!(worktree_state(&r, &a.path, true).unwrap().uncommitted, 2);
    let d = worktree_state(&r, &det.path, true).unwrap();
    assert!(d.uncommitted == 0 && d.commits == 0 && !d.merged);
    assert!(worktree_state(&r, &gone.path, true).is_err());
    // Only listed worktrees: never an arbitrary folder.
    assert!(worktree_state(&r, sb.path("det/..").to_str().unwrap(), true).is_err());

    // Untouched is not merged; committed then merged into a local, unpushed main is.
    assert!(!worktree_state(&r, &a.path, true).unwrap().merged);
    write_commit(&agent, "b.txt", "b\n", "agent work");
    let s = worktree_state(&r, &a.path, true).unwrap();
    assert!(s.commits == 1 && !s.merged);
    // Merged upstream only (a fork's PR landed in the original) is merged too.
    run(&r, &["update-ref", "refs/remotes/upstream/main", "agent"]).unwrap();
    let s = worktree_state(&r, &a.path, true).unwrap();
    assert!(s.commits == 0 && s.merged);
    run(&r, &["update-ref", "-d", "refs/remotes/upstream/main"]).unwrap();
    run(&r, &["merge", "-q", "agent"]).unwrap();
    let s = worktree_state(&r, &a.path, true).unwrap();
    assert!(s.commits == 0 && s.merged);
    // The agent worktree lives inside the main one: it's not an untracked file there.
    let m = worktree_state(&r, &main.path, true).unwrap();
    assert!(m.uncommitted == 0 && !m.merged);

    // From inside a linked worktree the main one is still the project.
    assert!(same_dir(&main_worktree(&agent).unwrap(), &r));
    assert!(worktrees(&agent)
        .unwrap()
        .iter()
        .any(|w| w.current && w.branch.as_deref() == Some("agent")));
    // Branches checked out elsewhere say where, so the UI can open that worktree instead.
    let br = branches(&r).unwrap();
    let wt = |n: &str| br.iter().find(|b| b.name == n).unwrap().worktree.clone();
    assert!(same_dir(&wt("agent").unwrap(), &agent));
    assert_eq!(wt("main"), None, "the current branch is not 'elsewhere'");
    assert!(switch_branch(&r, "agent", false).is_err());

    // Removing: never the main or the open one; a dirty one only when forced; a missing
    // folder just drops the entry. The branch stays.
    assert!(remove_worktree(&r, &main.path, true).is_err());
    assert!(remove_worktree(&agent, &a.path, true).is_err());
    assert!(remove_worktree(&r, sb.path("det/..").to_str().unwrap(), true).is_err());
    let err = remove_worktree(&r, &a.path, false).unwrap_err();
    assert!(err.contains("modified or untracked"), "{err}");
    run(&r, &["worktree", "lock", &a.path]).unwrap();
    remove_worktree(&r, &a.path, true).unwrap();
    assert!(!agent.exists());
    remove_worktree(&r, &gone.path, false).unwrap();
    assert_eq!(worktrees(&r).unwrap().len(), 2);
    assert!(branches(&r).unwrap().iter().any(|b| b.name == "agent"));
}

#[test]
fn nested_worktrees_stay_out_of_status_and_are_never_staged() {
    let sb = Sandbox::new("wtnested");
    let r = repo_with_worktrees(&sb);
    init(&r.join("vendor/lib"));
    fs::write(r.join("vendor/lib/x.txt"), "x\n").unwrap();
    fs::write(r.join("plain.txt"), "p\n").unwrap();

    let st = status(&r).unwrap();
    let entry = |p: &str| st.unstaged.iter().find(|f| f.path == p);
    assert!(
        entry(".claude/worktrees/agent/").is_none(),
        "own worktrees aren't changes"
    );
    let lib = entry("vendor/lib/")
        .unwrap()
        .nested
        .as_ref()
        .expect("nested repo");
    assert!(same_dir(&lib.path, &r.join("vendor/lib")));
    assert!(entry("plain.txt").unwrap().nested.is_none());

    // Staging one directly, or a folder above it, is refused; plain files still stage.
    for p in [".claude/worktrees/agent/", ".claude", "vendor/lib"] {
        let err = stage(&r, &[p.into()]).unwrap_err();
        assert!(err.contains("separate git repository"), "{p}: {err}");
    }
    assert!(stage(&r, &["plain.txt".into(), "vendor/lib/".into()]).is_err());
    assert!(
        status(&r).unwrap().staged.is_empty(),
        "a refusal stages nothing"
    );
    stage(&r, &["plain.txt".into()]).unwrap();
    // Explicitly allowed, it does what git does: a gitlink to its commit, not its files.
    write_commit(&r.join("vendor/lib"), "x.txt", "x\n", "lib");
    stage_with(&r, &["vendor/lib".into()], true).unwrap();
    let ls = run(&r, &["ls-files", "-s", "vendor/lib"]).unwrap();
    assert!(String::from_utf8_lossy(&ls).starts_with("160000"));
}

#[test]
fn bare_main_repo_is_not_a_worktree_to_open() {
    let sb = Sandbox::new("wtbare");
    sb.remote_with_clones(0);
    let origin = sb.path("origin.git");
    let wt = sb.path("wt");
    run(
        &origin,
        &["worktree", "add", "-q", "-b", "feat", wt.to_str().unwrap()],
    )
    .unwrap();

    let list = worktrees(&wt).unwrap();
    assert!(list[0].main && list[0].bare);
    assert_eq!(
        main_worktree(&wt),
        None,
        "the projects list keys by the worktree then"
    );
    let br = branches(&wt).unwrap();
    let main = br.iter().find(|b| b.name == "main").unwrap();
    assert_eq!(
        main.worktree, None,
        "bare HEAD holds main but can't be opened"
    );

    // An unborn branch has no HEAD to show (git prints all zeros).
    let empty = sb.path("empty");
    init(&empty);
    assert_eq!(worktrees(&empty).unwrap()[0].head, None);
}

#[test]
fn worktree_of_a_moved_repo_is_still_nested() {
    let sb = Sandbox::new("wtmoved");
    let r = repo_with_worktrees(&sb);
    let moved = sb.path("moved");
    fs::rename(&r, &moved).unwrap();

    // The worktree's .git now points at the old place; git lists its files as untracked.
    let st = status(&moved).unwrap();
    let agent: Vec<_> = st
        .unstaged
        .iter()
        .filter(|f| f.path.starts_with(".claude/worktrees/agent"))
        .collect();
    assert_eq!(agent.len(), 1, "one entry for the folder, not its files");
    assert_eq!(agent[0].path, ".claude/worktrees/agent/");
    assert!(agent[0].nested.is_some());
    let err = stage(&moved, &[".claude/worktrees/agent/a.txt".into()]).unwrap_err();
    assert!(err.contains("separate git repository"), "{err}");
    assert!(stage(&moved, &[".claude".into()]).is_err());
}

#[test]
fn worktree_locks_by_a_live_process_mean_in_use() {
    let sb = Sandbox::new("wtlock");
    let r = repo_with_worktrees(&sb);
    let agent = r.join(".claude/worktrees/agent");
    let det = sb.path("det");
    let live = format!(
        "claude session agent (pid {} start now)",
        std::process::id()
    );
    run(
        &r,
        &[
            "worktree",
            "lock",
            "--reason",
            &live,
            agent.to_str().unwrap(),
        ],
    )
    .unwrap();
    // A pid far past any real one: the session that took the lock is gone.
    let dead = "claude session det (pid 999999999 start then)";
    run(
        &r,
        &["worktree", "lock", "--reason", dead, det.to_str().unwrap()],
    )
    .unwrap();

    let list = with_live_locks(worktrees(&r).unwrap());
    let find = |dir: &Path| list.iter().find(|w| same_dir(&w.path, dir)).unwrap();
    let (a, d) = (find(&agent), find(&det));
    assert!(a.locked && a.in_use && a.lock_reason.as_deref() == Some(live.as_str()));
    assert!(d.locked && !d.in_use);
    assert!(!find(&r).locked && !find(&r).in_use);
}

/// `.worktreeinclude` copies what it matches and git ignores, from the main worktree whichever
/// one makes the new one, also out of folders holding nothing else; tracked, unlisted and
/// unignored files and links stay behind, and node_modules isn't walked for a bare `.env`.
#[test]
fn a_new_worktree_gets_the_ignored_files_worktreeinclude_lists() {
    let sb = Sandbox::new("wtinclude");
    let r = sb.path("r");
    init(&r);
    let ignore = ".env\n*.pem\nnode_modules/\nconfig/local/\nscratch.txt\n";
    write_commit(&r, ".gitignore", ignore, "ignore");
    write_commit(&r, ".env.example", "tracked\n", "example");
    write_commit(&r, "sub/a.txt", "a\n", "sub");
    run(&r, &["branch", "one"]).unwrap();
    run(&r, &["branch", "two"]).unwrap();
    let bare = add_worktree(&r, "one", None, None).unwrap();
    assert!(
        !Path::new(&bare).join(".env").exists(),
        "no .worktreeinclude yet"
    );

    // `local/` is unanchored, and apps/, certs/ and config/ hold only ignored files.
    let include = ".env\n*.pem\n.env.example\nlocal/\nnotes.txt\n# scratch.txt\n";
    fs::write(r.join(".worktreeinclude"), include).unwrap();
    for (path, text) in [
        (".env", "SECRET=1\n"),
        ("sub/.env", "SUB=1\n"),
        ("apps/web/.env", "WEB=1\n"),
        ("certs/dev.pem", "pem\n"),
        ("node_modules/pkg/.env", "dep\n"),
        ("config/local/secrets.json", "{}\n"),
        ("scratch.txt", "ignored, unlisted\n"),
        ("notes.txt", "listed, not ignored\n"),
        (".env.example", "tracked, edited\n"),
    ] {
        fs::create_dir_all(r.join(path).parent().unwrap()).unwrap();
        fs::write(r.join(path), text).unwrap();
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink("../../../outside", r.join("config/local/link")).unwrap();

    let mut listed = worktree_includes(&r).unwrap();
    listed.retain(|p| p != "config/local/link");
    listed.sort();
    let want = [
        ".env",
        "apps/web/.env",
        "certs/dev.pem",
        "config/local/secrets.json",
        "sub/.env",
    ];
    assert_eq!(listed, want);
    // The dialog's count leaves the link out, as the copy does.
    assert_eq!(include_count(&r).unwrap(), want.len());

    // Made from the linked worktree: the files still come from the main one.
    let two = PathBuf::from(add_worktree(Path::new(&bare), "two", None, None).unwrap());
    for path in want {
        assert_eq!(
            fs::read(two.join(path)).unwrap(),
            fs::read(r.join(path)).unwrap(),
            "{path}"
        );
    }
    assert_eq!(
        fs::read_to_string(two.join(".env.example")).unwrap(),
        "tracked\n"
    );
    for gone in [
        "node_modules",
        "scratch.txt",
        "notes.txt",
        "config/local/link",
    ] {
        assert!(two.join(gone).symlink_metadata().is_err(), "{gone} copied");
    }
}

/// A branch that exists checks out in a new worktree as it is, with no base to start from;
/// one checked out already, here or in another worktree, is refused and leaves no folder.
#[test]
fn a_new_worktree_checks_out_an_existing_branch() {
    let sb = Sandbox::new("wtexisting");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "a\n", "base");
    run(&r, &["switch", "-q", "-c", "feat"]).unwrap();
    write_commit(&r, "b.txt", "b\n", "feat adds b");
    let tip = rev(&r, "feat");
    run(&r, &["switch", "-q", "main"]).unwrap();

    let wt = PathBuf::from(add_worktree(&r, "feat", None, None).unwrap());
    assert_eq!(on_branch(&wt), "feat");
    assert_eq!(rev(&wt, "HEAD"), tip);
    assert!(wt.join("b.txt").is_file());
    assert_eq!(rev(&r, "feat"), tip, "the branch itself didn't move");

    let elsewhere = sb.path("elsewhere");
    for held in ["main", "feat"] {
        assert!(add_worktree(&r, held, None, Some(elsewhere.to_str().unwrap())).is_err());
        assert!(!elsewhere.join(held).exists());
    }
}

/// A folder picked through a symlink, or typed in another case: the new worktree's path is
/// the one `worktree list` reports, which the picker and the terminals compare it with.
#[cfg(unix)]
#[test]
fn a_new_worktree_through_a_symlink_has_its_real_path() {
    let sb = Sandbox::new("wtlink");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "a\n", "base");
    fs::create_dir_all(sb.path("real")).unwrap();
    std::os::unix::fs::symlink(sb.path("real"), sb.path("link")).unwrap();

    let link = sb.path("link");
    let wt = add_worktree(&r, "feat", Some("refs/heads/main"), link.to_str()).unwrap();
    assert_eq!(
        Path::new(&wt),
        sb.path("real").canonicalize().unwrap().join("feat")
    );
    assert!(worktrees(&r).unwrap().iter().any(|w| w.path == wt));

    // A folder typed in another case, where the disk ignores case: git keeps the typed case.
    fs::create_dir_all(sb.path("Cased")).unwrap();
    let typed = sb.path("cased");
    if typed.exists() {
        let wt = add_worktree(&r, "other", Some("refs/heads/main"), typed.to_str()).unwrap();
        assert!(worktrees(&r).unwrap().iter().any(|w| w.path == wt), "{wt}");
    }
}

/// A branch only on a remote gets a local branch tracking that remote's, also where two
/// remotes have it (git's own DWIM gives up there); a new name from it tracks nothing.
#[test]
fn a_new_worktree_from_a_remote_only_branch_tracks_it() {
    let sb = Sandbox::new("wtremote");
    let c = sb.remote_with_clones(2);
    let (a, b) = (&c[0], &c[1]);
    switch_branch(a, "feat", true).unwrap();
    write_commit(a, "f.txt", "f\n", "feat");
    run(a, &["push", "-q", "-u", "origin", "feat"]).unwrap();
    run(b, &["remote", "add", "upstream", &git_url(b)]).unwrap();
    run(b, &["fetch", "-q", "--all"]).unwrap();
    assert!(!exists(b, "feat"));
    let upstream = |wt: &str| {
        run_text(Path::new(wt), &["rev-parse", "--abbrev-ref", "@{upstream}"])
            .map(|u| u.trim().to_string())
    };

    let dir = sb.path("wts");
    let d = dir.to_str();
    let wt = add_worktree(b, "feat", Some("refs/remotes/upstream/feat"), d).unwrap();
    assert_eq!(on_branch(Path::new(&wt)), "feat");
    assert_eq!(rev(Path::new(&wt), "HEAD"), rev(a, "feat"));
    assert_eq!(upstream(&wt).unwrap(), "upstream/feat");

    let other = add_worktree(b, "other", Some("refs/remotes/origin/feat"), d).unwrap();
    assert!(upstream(&other).is_err(), "a new name tracks nothing");
}
