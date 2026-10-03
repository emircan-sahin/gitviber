//! The Compare screen: two points of history against each other.

use super::*;

fn names(c: &Comparison) -> Vec<(&str, &str)> {
    c.files
        .iter()
        .map(|f| (f.path.as_str(), f.status.as_str()))
        .collect()
}

fn subjects(log: Vec<Commit>) -> Vec<String> {
    log.into_iter().map(|c| c.subject).collect()
}

/// main: base, then "on main" after feature left it; feature: "f1", "f2".
fn parted(r: &Path) {
    init(r);
    write_commit(r, "shared.txt", "one\ntwo\n", "base");
    run(r, &["switch", "-q", "-c", "feature"]).unwrap();
    write_commit(r, "f.txt", "f\n", "f1");
    write_commit(r, "shared.txt", "one\ntwo\nthree\n", "f2");
    run(r, &["switch", "-q", "main"]).unwrap();
    write_commit(r, "m.txt", "m\n", "on main");
}

#[test]
fn three_dot_is_what_a_pull_request_shows_and_two_dot_is_everything_that_differs() {
    let sb = Sandbox::new("cmp-dots");
    let r = sb.path("r");
    parted(&r);
    let (main, feature) = ("refs/heads/main", "refs/heads/feature");

    let pr = compare(&r, main, feature, true).unwrap();
    assert_eq!((pr.ahead, pr.behind, pr.unrelated), (2, 1, false));
    assert_eq!(names(&pr), [("f.txt", "A"), ("shared.txt", "M")]);
    assert_eq!(pr.from, rev(&r, "main~1"));
    assert_eq!(
        (pr.base.as_str(), pr.head.as_str()),
        (rev(&r, "main").as_str(), rev(&r, "feature").as_str())
    );

    // Two-dot also shows what main gained since, as undone by feature.
    let all = compare(&r, main, feature, false).unwrap();
    assert_eq!(all.from, all.base);
    assert_eq!(
        names(&all),
        [("f.txt", "A"), ("m.txt", "D"), ("shared.txt", "M")]
    );
    assert_eq!((all.ahead, all.behind), (2, 1));

    // Swapped: what main changed since they parted, and the counts turn round.
    let back = compare(&r, feature, main, true).unwrap();
    assert_eq!(names(&back), [("m.txt", "A")]);
    assert_eq!((back.ahead, back.behind), (1, 2));
}

#[test]
fn identical_points_have_nothing_between_them() {
    let sb = Sandbox::new("cmp-same");
    let r = sb.path("r");
    parted(&r);
    for merge_base in [true, false] {
        let same = compare(&r, "refs/heads/main", "HEAD", merge_base).unwrap();
        assert_eq!((same.ahead, same.behind), (0, 0));
        assert!(same.files.is_empty());
        assert_eq!(same.base, same.head);
    }
    // Another branch at the same commit.
    run(&r, &["branch", "twin"]).unwrap();
    let twin = compare(&r, "refs/heads/main", "refs/heads/twin", true).unwrap();
    assert!(twin.files.is_empty() && (twin.ahead, twin.behind) == (0, 0));
}

#[test]
fn points_are_branches_remote_branches_tags_commits_and_head() {
    let sb = Sandbox::new("cmp-points");
    let c = sb.remote_with_clones(1);
    let a = &c[0];
    run(a, &["switch", "-q", "-c", "feat"]).unwrap();
    write_commit(a, "feat.txt", "f\n", "on feat");
    run(a, &["push", "-q", "-u", "origin", "feat"]).unwrap();
    run(a, &["tag", "v1"]).unwrap();
    write_commit(a, "later.txt", "l\n", "later");
    let first = rev(a, "feat~1");

    // A remote-tracking branch, a tag, a full id and a short one, HEAD.
    let remote = compare(a, "refs/heads/main", "refs/remotes/origin/feat", true).unwrap();
    assert_eq!(names(&remote), [("feat.txt", "A")]);
    let tag = compare(a, "refs/tags/v1", "HEAD", true).unwrap();
    assert_eq!((tag.ahead, tag.behind), (1, 0));
    assert_eq!(names(&tag), [("later.txt", "A")]);
    let by_id = compare(a, &first, &rev(a, "HEAD"), true).unwrap();
    assert_eq!(names(&by_id), [("later.txt", "A")]);
    let short = compare(a, &first[..8], "HEAD", true).unwrap();
    assert_eq!(short.base, first);

    // Anything else is refused before git reads it as an option or an expression.
    for bad in [
        "main",
        "HEAD~1",
        "HEAD..feat",
        "refs/heads/main..refs/heads/feat",
        "--output=x",
        "refs/heads/--x",
        "refs/heads/feat^{tree}",
        "",
        "zzzz",
    ] {
        assert!(compare(a, bad, "HEAD", true).is_err(), "base {bad:?}");
        assert!(compare(a, "HEAD", bad, true).is_err(), "head {bad:?}");
    }
    // One that isn't here says so (not fetched, or deleted).
    let gone = compare(a, "refs/remotes/origin/nope", "HEAD", true).err();
    assert!(gone.unwrap().contains("origin/nope doesn't exist"));
    assert!(compare(a, "deadbeef", "HEAD", true).is_err());
}

#[test]
fn branches_with_no_history_in_common_fall_back_to_the_plain_difference() {
    let sb = Sandbox::new("cmp-orphan");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "a\n", "base");
    run(&r, &["switch", "-q", "--orphan", "lonely"]).unwrap();
    write_commit(&r, "b.txt", "b\n", "alone");

    let c = compare(&r, "refs/heads/main", "refs/heads/lonely", true).unwrap();
    assert!(c.unrelated);
    assert_eq!(c.from, c.base);
    assert_eq!(names(&c), [("a.txt", "D"), ("b.txt", "A")]);
    assert_eq!((c.ahead, c.behind), (1, 1));
    let two = compare(&r, "refs/heads/main", "refs/heads/lonely", false).unwrap();
    assert_eq!(names(&two), names(&c));

    let check = merge_check(&r, "refs/heads/main").unwrap();
    assert!(check.unrelated && check.incoming == 1);
}

#[test]
fn renames_binaries_and_modes_are_listed_as_a_commit_lists_them() {
    let sb = Sandbox::new("cmp-kinds");
    let r = sb.path("r");
    init(&r);
    let body = "one\ntwo\nthree\nfour\nfive\nsix\nseven\n";
    write_commit(&r, "old name.txt", body, "base");
    write_commit(&r, "img.bin", "x", "base");
    fs::write(r.join("img.bin"), [0u8, 159, 146, 150, 0, 1]).unwrap();
    stage(&r, &["img.bin".into()]).unwrap();
    commit(&r, "binary", &CommitOptions::default(), &Net::default()).unwrap();
    run(&r, &["switch", "-q", "-c", "work"]).unwrap();
    run(&r, &["mv", "old name.txt", "new name.txt"]).unwrap();
    fs::write(r.join("img.bin"), [0u8, 1, 2, 3, 0, 9, 9]).unwrap();
    stage(&r, &["img.bin".into()]).unwrap();
    commit(
        &r,
        "rename and rebinary",
        &CommitOptions::default(),
        &Net::default(),
    )
    .unwrap();

    let c = compare(&r, "refs/heads/main", "refs/heads/work", true).unwrap();
    let renamed = c.files.iter().find(|f| f.status == "R").unwrap();
    assert_eq!(
        (renamed.path.as_str(), renamed.old_path.as_deref()),
        ("new name.txt", Some("old name.txt"))
    );
    let bin = c.files.iter().find(|f| f.path == "img.bin").unwrap();
    assert_eq!(
        (bin.status.as_str(), bin.additions, bin.deletions),
        ("M", None, None)
    );
}

#[test]
fn thousands_of_files_are_one_list() {
    let sb = Sandbox::new("cmp-huge");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "a\n", "base");
    run(&r, &["switch", "-q", "-c", "big"]).unwrap();
    for i in 0..3000 {
        let dir = r.join(format!("d{}", i % 40));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(format!("f{i}.txt")), format!("{i}\n")).unwrap();
    }
    run(&r, &["add", "-A"]).unwrap();
    commit(&r, "many", &CommitOptions::default(), &Net::default()).unwrap();

    let c = compare(&r, "refs/heads/main", "refs/heads/big", true).unwrap();
    assert_eq!(c.files.len(), 3000);
    assert!(c
        .files
        .iter()
        .all(|f| f.status == "A" && f.additions == Some(1)));
    // The list of commits is a page, not all.
    assert_eq!(c.ahead, 1);
}

#[test]
fn the_commits_between_are_newest_first_and_paged() {
    let sb = Sandbox::new("cmp-log");
    let r = sb.path("r");
    parted(&r);
    let (main, feature) = (rev(&r, "main"), rev(&r, "feature"));

    // Ahead: what feature has and main lacks; behind: the other way round.
    assert_eq!(
        subjects(log_between(&r, &main, &feature, 0, 20).unwrap()),
        ["f2", "f1"]
    );
    assert_eq!(
        subjects(log_between(&r, &feature, &main, 0, 20).unwrap()),
        ["on main"]
    );
    // Pages, and none past the end.
    assert_eq!(
        subjects(log_between(&r, &main, &feature, 0, 1).unwrap()),
        ["f2"]
    );
    assert_eq!(
        subjects(log_between(&r, &main, &feature, 1, 1).unwrap()),
        ["f1"]
    );
    assert!(log_between(&r, &main, &feature, 2, 5).unwrap().is_empty());
    // Commit ids only: names and ranges go no further.
    assert!(log_between(&r, "refs/heads/main", &feature, 0, 5).is_err());
    assert!(log_between(&r, &main, "HEAD", 0, 5).is_err());
    assert!(log_between(&r, &main, &format!("{feature}..{main}"), 0, 5).is_err());
}

#[test]
fn a_merge_is_checked_before_it_is_made() {
    let sb = Sandbox::new("cmp-merge");
    let r = sb.path("r");
    parted(&r);

    // Clean: feature's two commits come in, nothing conflicts, and the check changes nothing.
    let head = rev(&r, "HEAD");
    let clean = merge_check(&r, "refs/heads/feature").unwrap();
    assert_eq!((clean.incoming, clean.unrelated), (2, false));
    assert_eq!(clean.conflicts, Some(vec![]));
    assert_eq!(rev(&r, "HEAD"), head);
    assert!(status(&r).unwrap().unstaged.is_empty());

    // Both sides change a line of the same file.
    write_commit(&r, "shared.txt", "one\nTWO\n", "main edits shared");
    let conflicted = merge_check(&r, "refs/heads/feature").unwrap();
    assert_eq!(conflicted.conflicts, Some(vec!["shared.txt".to_string()]));
    assert_eq!(rev(&r, "HEAD"), rev(&r, "main"));

    // Nothing to merge: already in HEAD, or HEAD itself.
    let done = merge_check(&r, "HEAD").unwrap();
    assert_eq!((done.incoming, done.conflicts), (0, Some(vec![])));
    run(&r, &["branch", "behind", "main~1"]).unwrap();
    assert_eq!(merge_check(&r, "refs/heads/behind").unwrap().incoming, 0);
    assert!(merge_check(&r, "main").is_err());
}
