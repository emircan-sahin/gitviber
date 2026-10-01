//! Branches and tags: switching, creating, renaming, deleting, upstreams.

use super::*;

/// A fork has origin/x and upstream/x alike: `git switch x` refuses, so the picker names one.
#[test]
fn switching_to_a_remote_branch_tracks_that_remote() {
    let sb = Sandbox::new("track");
    let c = sb.remote_with_clones(2);
    let (a, b) = (&c[0], &c[1]);
    run(a, &["switch", "-q", "-c", "feat"]).unwrap();
    write_commit(a, "f.txt", "f\n", "feat");
    run(a, &["push", "-q", "-u", "origin", "feat"]).unwrap();
    let url = git_url(b);
    run(b, &["remote", "add", "upstream", &url]).unwrap();
    run(b, &["fetch", "-q", "--all"]).unwrap();
    assert!(run(b, &["switch", "feat"]).is_err());
    switch_tracking(b, "upstream/feat").unwrap();
    let tracked = run(b, &["rev-parse", "--abbrev-ref", "@{upstream}"]).unwrap();
    assert_eq!(String::from_utf8_lossy(&tracked).trim(), "upstream/feat");
    // Options and local refs are refused.
    assert!(switch_tracking(b, "--orphan=x").is_err());
    assert!(switch_tracking(b, "main").is_err());
}

/// A terminal tab in another project names its branch; nothing when detached or outside a repo.
#[test]
fn current_branch_of_any_folder() {
    let sb = Sandbox::new("curbr");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "d/a.txt", "a\n", "base");
    run(&r, &["switch", "-q", "-c", "feat"]).unwrap();
    assert_eq!(current_branch(&r).as_deref(), Some("feat"));
    assert_eq!(current_branch(&r.join("d")).as_deref(), Some("feat"));
    run(&r, &["switch", "-q", "--detach"]).unwrap();
    assert_eq!(current_branch(&r), None);
    let plain = sb.path("plain");
    std::fs::create_dir_all(&plain).unwrap();
    assert_eq!(current_branch(&plain), None);
}

#[test]
fn merged_branches_and_deleting_them() {
    let sb = Sandbox::new("brdel");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "a\n", "base");
    run(&r, &["branch", "done"]).unwrap();
    run(&r, &["switch", "-q", "-c", "wip"]).unwrap();
    write_commit(&r, "b.txt", "b\n", "unmerged work");
    run(&r, &["switch", "-q", "-c", "feature"]).unwrap();
    let merged = |r: &Path| -> Vec<String> {
        let mut m: Vec<String> = branches(r)
            .unwrap()
            .into_iter()
            .filter(|b| b.merged)
            .map(|b| b.name)
            .collect();
        m.sort();
        m
    };
    // main is the default branch: merged into the feature, but never offered for cleanup.
    assert_eq!(merged(&r), ["done", "wip"]);

    run(&r, &["switch", "-q", "main"]).unwrap();
    assert_eq!(merged(&r), ["done"]);
    // -d refuses commits found nowhere else; -D takes them.
    assert!(delete_branches(&r, &["wip".into()], false).is_err());
    delete_branches(&r, &["done".into(), "wip".into()], true).unwrap();
    assert!(delete_branches(&r, &["-D".into()], true).is_err());
    let left: Vec<String> = branches(&r).unwrap().into_iter().map(|b| b.name).collect();
    assert_eq!(left.len(), 2, "{left:?}");
}

/// A host squash- or rebase-merges a pull request and deletes its branch: after a pruning
/// fetch the local branch counts as merged upstream. A gone upstream alone doesn't.
#[test]
fn squash_and_rebase_merged_branches_whose_upstream_is_gone() {
    let sb = Sandbox::new("landed");
    let c = sb.remote_with_clones(2);
    let (a, host) = (&c[0], &c[1]);
    let push = |name: &str, commits: &[(&str, &str)]| {
        run(a, &["switch", "-q", "-c", name, "main"]).unwrap();
        for (path, content) in commits {
            write_commit(a, path, content, &format!("{name}: {path}"));
        }
        run(a, &["push", "-q", "-u", "origin", name]).unwrap();
    };
    push("squashed", &[("s.txt", "s1\n"), ("s.txt", "s2\n")]);
    push("rebased", &[("r1.txt", "r1\n"), ("r2.txt", "r2\n")]);
    let nine = "1\n2\n3\n4\n5\n6\n7\n8\n9\n";
    push("later", &[("l.txt", "1\n"), ("l.txt", nine)]);
    push("partial", &[("p1.txt", "p1\n"), ("p2.txt", "p2\n")]);
    push("unmerged", &[("u.txt", "u\n")]);
    push("alive", &[("v.txt", "v\n")]);
    push("held", &[("h.txt", "h\n")]);
    // Its only change differs from main's in whitespace alone: not merged.
    push("spaces", &[("w.txt", "a  b\n")]);
    // Landed, then a merge brought in more work: e.txt, which main has, and precious.txt.
    push("stacked", &[("k.txt", "k\n")]);
    run(a, &["switch", "-q", "-c", "extra", "main"]).unwrap();
    write_commit(a, "e.txt", "e\n", "extra");
    run(a, &["switch", "-q", "stacked"]).unwrap();
    run(a, &["merge", "-q", "--no-ff", "--no-commit", "extra"]).unwrap();
    fs::write(a.join("precious.txt"), "p\n").unwrap();
    stage(a, &["precious.txt".into()]).unwrap();
    commit(a, "merge extra", &CommitOptions::default(), &Net::default()).unwrap();
    run(a, &["switch", "-q", "main"]).unwrap();

    // The host: main moves on, then each pull request lands its own way.
    run(host, &["fetch", "-q"]).unwrap();
    write_commit(host, "a.txt", "one\ntwo\nthree\nfour\n", "other work");
    let git = |args: &[&str]| run(host, args).unwrap();
    for b in ["squashed", "later", "alive", "held", "stacked"] {
        git(&["merge", "-q", "--squash", &format!("origin/{b}")]);
        git(&["commit", "-q", "-m", &format!("{b} (#1)")]);
    }
    git(&["cherry-pick", "main..origin/rebased"]);
    git(&["cherry-pick", "origin/partial~1"]);
    // After the squash, main edits the same file: only the squash commit's patch still matches.
    write_commit(host, "l.txt", &format!("{nine}10\n"), "more");
    write_commit(host, "w.txt", "a b\n", "w");
    write_commit(host, "e.txt", "e\n", "e");
    git(&["push", "-q", "origin", "main"]);
    for b in [
        "squashed", "rebased", "later", "partial", "unmerged", "held", "spaces", "stacked",
    ] {
        git(&["push", "-q", "origin", "--delete", b]);
    }

    run(a, &["fetch", "-q", "--prune"]).unwrap();
    run(a, &["merge", "-q", "--ff-only", "origin/main"]).unwrap();
    run(
        a,
        &[
            "worktree",
            "add",
            "-q",
            sb.path("held").to_str().unwrap(),
            "held",
        ],
    )
    .unwrap();
    let mut found = merged_upstream(a);
    found.sort();
    assert_eq!(found, ["held", "later", "rebased", "squashed"]);
    // git itself doesn't see them as merged, so the plain rule leaves them be.
    assert!(!branches(a).unwrap().iter().any(|b| b.merged));
    // The worktree holding one is merged too, with nothing to merge back.
    let held = worktrees(a)
        .unwrap()
        .into_iter()
        .find(|w| same_dir(&w.path, &sb.path("held")))
        .unwrap();
    let s = worktree_state(a, &held.path, true).unwrap();
    assert!(s.commits == 0 && s.merged);
    // git's own -d refuses them; they're checked again, at the commit they're at now, then -D.
    assert!(delete_branches(a, &["squashed".into()], false).is_err());
    run(a, &["switch", "-q", "rebased"]).unwrap();
    write_commit(a, "new.txt", "new\n", "new work");
    run(a, &["switch", "-q", "main"]).unwrap();
    let both = ["squashed".to_string(), "rebased".to_string()];
    assert!(delete_merged(a, &[], &both).is_err());
    assert!(exists(a, "squashed") && exists(a, "rebased"));
    delete_merged(a, &[], &both[..1]).unwrap();
    assert!(!exists(a, "squashed"));
    // Its settings go with it, as with git branch -D.
    assert!(run(a, &["config", "--get", "branch.squashed.remote"]).is_err());
}

/// A branch merged upstream that's checked out here or in another worktree stays: deleting
/// its ref would leave that HEAD unborn, every file staged as added.
#[test]
fn merged_upstream_branches_checked_out_anywhere_are_never_deleted() {
    let sb = Sandbox::new("landedheld");
    let c = sb.remote_with_clones(2);
    let (a, host) = (&c[0], &c[1]);
    for b in ["here", "there"] {
        run(a, &["switch", "-q", "-c", b, "main"]).unwrap();
        write_commit(a, &format!("{b}.txt"), "x\n", b);
        run(a, &["push", "-q", "-u", "origin", b]).unwrap();
    }
    run(host, &["fetch", "-q"]).unwrap();
    for b in ["here", "there"] {
        run(host, &["merge", "-q", "--squash", &format!("origin/{b}")]).unwrap();
        run(host, &["commit", "-q", "-m", b]).unwrap();
    }
    run(host, &["push", "-q", "origin", "main", ":here", ":there"]).unwrap();
    run(a, &["switch", "-q", "here"]).unwrap();
    run(a, &["fetch", "-q", "--prune"]).unwrap();
    let wt = sb.path("there");
    run(a, &["worktree", "add", "-q", wt.to_str().unwrap(), "there"]).unwrap();
    let mut found = merged_upstream(a);
    found.sort();
    assert_eq!(found, ["here", "there"]);

    for b in ["here", "there"] {
        assert!(delete_merged(a, &[], &[b.to_string()]).is_err());
        assert!(exists(a, b));
    }
    assert_eq!(on_branch(a), "here");
    // Once nothing holds it, it goes.
    run(a, &["switch", "-q", "main"]).unwrap();
    delete_merged(a, &[], &["here".to_string()]).unwrap();
    assert!(!exists(a, "here"));
}

/// The user's diff settings shape the patches patch-id reads; none may make an unmerged
/// branch look merged, and a real squash still counts under all of them.
#[test]
fn diff_settings_dont_make_a_branch_look_merged_upstream() {
    let sb = Sandbox::new("landedcfg");
    let r = repo_with_submodule(&sb);
    let sub = r.join("sub");
    let ten: String = (1..=10).map(|n| format!("{n}\n")).collect();
    let blank = "a\nb\n\nc\nd\ne\nf\ng\nh\ni\nj\nk\nl\nm\n";
    for (f, text) in [
        ("g.txt", ten.as_str()),
        ("h.txt", blank),
        ("f4.txt", &ten),
        ("f6.txt", &ten),
        ("r.txt", &ten),
    ] {
        write_commit(&r, f, text, f);
    }
    run(
        &r,
        &["remote", "add", "origin", sb.path("none").to_str().unwrap()],
    )
    .unwrap();
    let git = |args: &[&str]| run(&r, args).unwrap();
    let branch = |name: &str| git(&["switch", "-q", "-c", name, "main"]);
    // A commit in the submodule, for a branch to move its pointer to.
    let bump = |msg: &str| {
        write_commit(&sub, "l.txt", &format!("{msg}\n"), msg);
        stage(&r, &["sub".into()]).unwrap();
    };
    let edit = |f: &str, from: &str, to: &str| {
        let text = fs::read_to_string(r.join(f)).unwrap().replacen(from, to, 1);
        fs::write(r.join(f), text).unwrap();
        stage(&r, &[f.into()]).unwrap();
    };
    let done = |msg: &str| commit(&r, msg, &CommitOptions::default(), &Net::default()).unwrap();

    // Each branch's own work, and on main something that matches only part of it or only
    // under the setting.
    branch("ctx");
    edit("g.txt", "3\n", "3\nfoo\n");
    done("foo after 3");
    branch("blank");
    edit("h.txt", "b\n", "B\n");
    edit("h.txt", "m\n", "M\n");
    write_commit(&r, "x.txt", "extra\n", "B, M and x");
    branch("hidden");
    edit("f4.txt", "5\n", "five\n");
    bump("hidden");
    done("f4 and sub");
    branch("sublog");
    edit("f6.txt", "5\n", "five\n");
    bump("sublog");
    fs::write(r.join("z.txt"), "important\n").unwrap();
    stage(&r, &["z.txt".into()]).unwrap();
    done("f6, sub and z");
    branch("real");
    edit("r.txt", "2\n", "two\n");
    done("r1");
    edit("r.txt", "4\n", "four\n");
    done("r2");

    git(&["switch", "-q", "main"]);
    git(&["submodule", "update", "-q"]);
    edit("g.txt", "7\n", "7\nfoo\n");
    done("foo after 7");
    edit("h.txt", "b\n", "B\n");
    edit("h.txt", "m\n", "DIFFERENT\n");
    done("B and different");
    edit("f4.txt", "5\n", "five\n");
    done("f4 without sub");
    edit("f6.txt", "5\n", "five\n");
    done("f6 only");
    git(&["merge", "-q", "--squash", "real"]);
    done("real (#1)");
    // Base moves on over each file, so their contents differ from the branches'.
    for f in ["g.txt", "h.txt", "f6.txt", "r.txt"] {
        edit(f, "10\n", "ten\n");
        edit(f, "l\n", "L\n");
        done(&format!("later {f}"));
    }

    // One at a time: only a branch whose upstream is gone is asked about.
    let gone = |b: &str, config: &[(&str, &str)]| -> bool {
        for (k, v) in config {
            git(&["config", k, v]);
        }
        git(&["config", &format!("branch.{b}.remote"), "origin"]);
        git(&[
            "config",
            &format!("branch.{b}.merge"),
            &format!("refs/heads/{b}"),
        ]);
        let found = merged_upstream(&r).contains(&b.to_string());
        git(&["config", "--remove-section", &format!("branch.{b}")]);
        for (k, _) in config {
            git(&["config", "--unset", k]);
        }
        found
    };
    assert!(!gone("ctx", &[("diff.context", "0")]));
    assert!(!gone("blank", &[("diff.suppressBlankEmpty", "true")]));
    assert!(!gone("hidden", &[("diff.ignoreSubmodules", "all")]));
    assert!(!gone("sublog", &[("diff.submodule", "log")]));
    let all = [
        ("diff.context", "0"),
        ("diff.suppressBlankEmpty", "true"),
        ("diff.ignoreSubmodules", "all"),
        ("diff.submodule", "log"),
        ("diff.noprefix", "true"),
        ("color.diff", "always"),
    ];
    assert!(gone("real", &all));
}

#[test]
fn deleting_a_remote_branch() {
    let sb = Sandbox::new("rbdel");
    let c = sb.remote_with_clones(1);
    let a = &c[0];
    run(a, &["push", "-q", "origin", "HEAD:refs/heads/feat/x"]).unwrap();
    fetch(a, &Net::default()).unwrap();
    let defaults: Vec<String> = branches(a)
        .unwrap()
        .into_iter()
        .filter(|b| b.remote_default)
        .map(|b| b.name)
        .collect();
    assert_eq!(defaults, ["origin/main"]);
    delete_remote_branch(a, "origin/feat/x", &Net::default()).unwrap();
    let left = String::from_utf8(run(a, &["ls-remote", "--heads", "origin"]).unwrap()).unwrap();
    assert!(!left.contains("feat/x"), "{left}");
    // The tracking ref goes with it, so the picker drops the row without a fetch.
    assert!(!branches(a)
        .unwrap()
        .iter()
        .any(|b| b.name == "origin/feat/x"));
    assert!(delete_remote_branch(a, "origin/main", &Net::default()).is_err());
    assert!(delete_remote_branch(a, "nope/x", &Net::default()).is_err());
}

fn upstream_of(repo: &Path, branch: &str) -> Option<String> {
    let spec = format!("{branch}@{{upstream}}");
    run_text(repo, &["rev-parse", "--abbrev-ref", &spec])
        .ok()
        .map(|s| s.trim().to_string())
}

fn on_remote(repo: &Path, branch: &str) -> bool {
    let full = format!("refs/heads/{branch}");
    !run_text(repo, &["ls-remote", "origin", &full])
        .unwrap()
        .trim()
        .is_empty()
}

/// Renaming the checked-out branch with its remote: the new name is pushed and tracked, the
/// old one leaves the remote. Undo brings the old name back with its old upstream settings.
#[test]
fn rename_a_branch_here_and_on_the_remote() {
    let sb = Sandbox::new("rename");
    let c = sb.remote_with_clones(1);
    let a = &c[0];
    switch_branch(a, "feat", true).unwrap();
    write_commit(a, "f.txt", "f\n", "feat");
    push(a, false, None, &Net::default()).unwrap();
    assert!(rename_branch(a, "feat", "--evil", false, &Net::default()).is_err());
    // The remote default branch is refused before anything moves.
    assert!(rename_branch(a, "main", "trunk", true, &Net::default()).is_err());
    assert!(exists(a, "main"));

    let j = Journal::default();
    j.record(a, Action::new("Rename feat to feature", Mode::Keep), |r| {
        rename_branch(r, "feat", "feature", true, &Net::default())
    })
    .unwrap();
    assert_eq!(on_branch(a), "feature");
    assert!(!exists(a, "feat"));
    assert_eq!(upstream_of(a, "feature").as_deref(), Some("origin/feature"));
    assert!(on_remote(a, "feature") && !on_remote(a, "feat"));

    step(&j, a, false).unwrap();
    assert_eq!(on_branch(a), "feat");
    assert!(!exists(a, "feature"));
    let merge = run_text(a, &["config", "branch.feat.merge"]).unwrap();
    assert_eq!(merge.trim(), "refs/heads/feat");
    step(&j, a, true).unwrap();
    assert_eq!(on_branch(a), "feature");
    assert_eq!(upstream_of(a, "feature").as_deref(), Some("origin/feature"));

    // A local-only rename of a branch that isn't checked out.
    switch_branch(a, "side", true).unwrap();
    switch_branch(a, "feature", false).unwrap();
    rename_branch(a, "side", "aside", false, &Net::default()).unwrap();
    assert!(exists(a, "aside") && !exists(a, "side"));
    assert!(
        rename_branch(a, "aside", "x", true, &Net::default()).is_err(),
        "tracks nothing"
    );
}

/// A branch from a remote branch or a tag starts there without tracking it; the upstream is
/// set and unset on its own.
#[test]
fn create_branch_from_a_base_and_set_its_upstream() {
    let sb = Sandbox::new("newfrom");
    let c = sb.remote_with_clones(1);
    let a = &c[0];
    let base = rev(a, "HEAD");
    write_commit(a, "b.txt", "b\n", "local only");
    run(a, &["tag", "v1", &base]).unwrap();
    assert_eq!(tags(a).unwrap(), vec!["v1".to_string()]);

    create_branch(a, "from-remote", "refs/remotes/origin/main", false).unwrap();
    assert_eq!(on_branch(a), "main");
    assert_eq!(rev(a, "from-remote"), base);
    assert_eq!(upstream_of(a, "from-remote"), None);
    create_branch(a, "from-tag", "refs/tags/v1", true).unwrap();
    assert_eq!(on_branch(a), "from-tag");
    assert_eq!(rev(a, "HEAD"), base);
    assert!(create_branch(a, "bad", "main", false).is_err());
    assert!(create_branch(a, "bad", "refs/tags/nope", false).is_err());

    set_upstream(a, "from-tag", Some("origin/main")).unwrap();
    assert_eq!(upstream_of(a, "from-tag").as_deref(), Some("origin/main"));
    assert!(set_upstream(a, "from-tag", Some("origin/nope")).is_err());
    set_upstream(a, "from-tag", None).unwrap();
    assert_eq!(upstream_of(a, "from-tag"), None);
}

#[test]
fn annotated_tags_push_and_undo() {
    let sb = Sandbox::new("tags");
    let c = sb.remote_with_clones(1);
    let a = &c[0];
    write_commit(a, "b.txt", "b\n", "release");
    let head = rev(a, "HEAD");
    let j = Journal::default();
    j.record(
        a,
        Action::new("Create tag v1", Mode::Keep).with_tags(),
        |r| create_tag(r, "v1", &head, Some("First release\n\nNotes")),
    )
    .unwrap();
    create_tag(a, "light", &head, Some("  ")).unwrap();
    assert!(create_tag(a, "--evil", &head, None).is_err());
    let kind = |t: &str| {
        run_text(a, &["cat-file", "-t", t])
            .unwrap()
            .trim()
            .to_string()
    };
    assert_eq!((kind("v1"), kind("light")), ("tag".into(), "commit".into()));
    let msg = run_text(a, &["tag", "-l", "--format=%(contents)", "v1"]).unwrap();
    assert!(msg.starts_with("First release\n\nNotes"), "{msg}");

    push_with_tags(a, false, None, &Net::default()).unwrap();
    let there = remote_tags(a, &Net::default()).unwrap();
    assert_eq!(
        (there.remote.as_str(), there.names.clone()),
        ("origin", vec!["v1".to_string()])
    );
    assert_eq!(
        push_tags(a, &["light".into()], &Net::default()).unwrap(),
        "origin"
    );
    assert_eq!(
        remote_tags(a, &Net::default()).unwrap().names,
        vec!["light", "v1"]
    );
    delete_remote_tag(a, "light", &Net::default()).unwrap();
    assert_eq!(remote_tags(a, &Net::default()).unwrap().names, vec!["v1"]);

    // Undo takes the tag away and redo brings back the same tag object, message and all.
    let object = rev(a, "refs/tags/v1");
    step(&j, a, false).unwrap();
    assert!(run(a, &["rev-parse", "--verify", "-q", "refs/tags/v1"]).is_err());
    step(&j, a, true).unwrap();
    assert_eq!(rev(a, "refs/tags/v1"), object);

    j.record(
        a,
        Action::new("Delete tag v1", Mode::Keep).with_tags(),
        |r| delete_tag(r, "v1"),
    )
    .unwrap();
    assert!(!tags(a).unwrap().contains(&"v1".to_string()));
    step(&j, a, false).unwrap();
    assert_eq!(rev(a, "refs/tags/v1"), object);
    // Moved outside the app since: no longer safe to undo or redo.
    run(a, &["tag", "-f", "v1", "HEAD~1"]).unwrap();
    assert!(j.view(a).redo_blocked.is_some());
}
