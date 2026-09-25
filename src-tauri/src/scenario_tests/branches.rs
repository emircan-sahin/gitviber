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
