//! The Compare screen's hard cases: names that share a branch's, odd refs, shallow clones, merges
//! in a range, and a merge check that must never touch the repository.

use super::*;
use std::time::Instant;

/// Everything a merge check must leave as it was: refs, HEAD, index, and the working tree's state.
fn untouched(r: &Path) -> Vec<String> {
    ["for-each-ref", "status", "ls-files", "rev-parse"]
        .iter()
        .map(|cmd| match *cmd {
            "for-each-ref" => run_text(r, &["for-each-ref"]).unwrap(),
            "status" => run_text(r, &["status", "--porcelain=v2", "-uall", "--branch"]).unwrap(),
            "ls-files" => run_text(r, &["ls-files", "-s"]).unwrap(),
            _ => run_text(r, &["rev-parse", "HEAD"]).unwrap(),
        })
        .collect()
}

/// main and a branch that parted from it, each with its own commit.
fn parted(r: &Path, branch: &str) {
    init(r);
    write_commit(r, "shared.txt", "one\ntwo\n", "base");
    run(r, &["switch", "-q", "-c", branch]).unwrap();
    write_commit(r, "f.txt", "f\n", "on the branch");
    run(r, &["switch", "-q", "main"]).unwrap();
    write_commit(r, "m.txt", "m\n", "on main");
}

#[test]
fn a_tag_and_a_branch_of_one_name_are_told_apart_and_merged_by_the_branch_not_the_tag() {
    let sb = Sandbox::new("cmp-same-name");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "shared.txt", "one\n", "base");
    run(&r, &["tag", "v1"]).unwrap();
    run(&r, &["switch", "-q", "-c", "v1"]).unwrap();
    write_commit(&r, "f.txt", "f\n", "on the branch");
    run(&r, &["switch", "-q", "main"]).unwrap();
    write_commit(&r, "m.txt", "m\n", "on main");
    let (tag, branch) = (rev(&r, "refs/tags/v1"), rev(&r, "refs/heads/v1"));

    let c = compare(&r, "refs/tags/v1", "refs/heads/v1", true).unwrap();
    assert_eq!((c.base.as_str(), c.head.as_str()), (&*tag, &*branch));
    assert_eq!((c.ahead, c.behind), (1, 0));
    // The check is of the branch, and so must be the merge it leads to.
    assert_eq!(merge_check(&r, "refs/heads/v1").unwrap().incoming, 1);
    assert_eq!(merge_check(&r, "refs/tags/v1").unwrap().incoming, 0);

    // Git reads a bare `v1` as the tag.
    assert_eq!(rev(&r, "v1"), tag);
    merge(&r, "refs/heads/v1", MergeKind::NoFf, false).unwrap();
    let parents = run_text(&r, &["rev-list", "--parents", "-n1", "HEAD"]).unwrap();
    assert!(parents.contains(&branch), "{parents}");
    let subject = run_text(&r, &["log", "-1", "--format=%s"]).unwrap();
    // Kept whole, as "v1" alone is the tag; a branch nothing shares its name is worded plainly.
    assert_eq!(subject.trim(), "Merge branch 'refs/heads/v1'");
}

#[test]
fn an_annotated_tag_is_its_commit_and_a_squash_names_a_branch_plainly() {
    let sb = Sandbox::new("cmp-annotated");
    let r = sb.path("r");
    parted(&r, "feature");
    run(&r, &["tag", "-a", "v2", "-m", "release", "feature"]).unwrap();
    let object = rev(&r, "refs/tags/v2");
    let commit = rev(&r, "refs/tags/v2^{commit}");
    assert_ne!(object, commit);

    let c = compare(&r, "refs/heads/main", "refs/tags/v2", true).unwrap();
    assert_eq!(c.head, commit);
    assert_eq!(merge_check(&r, "refs/tags/v2").unwrap().incoming, 1);

    merge(&r, "refs/heads/feature", MergeKind::Squash, false).unwrap();
    let subject = run_text(&r, &["log", "-1", "--format=%s"]).unwrap();
    assert_eq!(subject.trim(), "Squash merge feature");
}

#[test]
fn refs_that_look_like_options_or_expressions_are_refused_and_odd_real_ones_work() {
    let sb = Sandbox::new("cmp-odd-refs");
    let r = sb.path("r");
    parted(&r, "feature");
    // Git's own porcelain refuses these names; update-ref doesn't.
    run(&r, &["update-ref", "refs/heads/-x", "feature"]).unwrap();
    run(&r, &["update-ref", "refs/heads/ünï/fé", "feature"]).unwrap();
    run(&r, &["update-ref", "refs/tags/--tag", "feature"]).unwrap();

    for good in ["refs/heads/-x", "refs/heads/ünï/fé", "refs/tags/--tag"] {
        let c =
            compare(&r, "refs/heads/main", good, true).unwrap_or_else(|e| panic!("{good}: {e}"));
        assert_eq!(c.ahead, 1, "{good}");
        assert!(merge_check(&r, good).is_ok(), "{good}");
    }
    for bad in [
        "refs/heads/a b",
        "refs/heads/main@{1}",
        "refs/heads/main@{u}",
        "refs/heads/main^",
        "refs/heads/main~1",
        "refs/heads/main:x",
        "refs/heads/main\n",
        "refs/heads/",
        "refs/tags/",
        "refs/stash",
        "refs/notes/x",
        "refs/../x",
        "refs/heads/..",
        "-",
        "--",
        "--all",
        "-p",
        " HEAD",
        "HEAD ",
        "head",
        "@",
        "@{u}",
        "HEAD@{1}",
        "HEAD^",
        "HEAD:shared.txt",
        "abc",
        "ZZZZ0000",
        "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789",
    ] {
        assert!(compare(&r, bad, "HEAD", true).is_err(), "base {bad:?}");
        assert!(compare(&r, "HEAD", bad, true).is_err(), "head {bad:?}");
        assert!(merge_check(&r, bad).is_err(), "merge {bad:?}");
    }
}

#[test]
fn a_detached_head_is_a_point_and_a_deleted_branch_says_so() {
    let sb = Sandbox::new("cmp-detached");
    let r = sb.path("r");
    parted(&r, "feature");
    run(&r, &["switch", "-q", "--detach", "feature"]).unwrap();
    let c = compare(&r, "refs/heads/main", "HEAD", true).unwrap();
    assert_eq!((c.ahead, c.behind), (1, 1));
    assert_eq!(merge_check(&r, "refs/heads/main").unwrap().incoming, 1);

    // A tab restored after the branch was deleted: its name is gone, its commit is not.
    let tip = rev(&r, "feature");
    run(&r, &["switch", "-q", "main"]).unwrap();
    run(&r, &["branch", "-D", "feature"]).unwrap();
    let gone = compare(&r, "refs/heads/main", "refs/heads/feature", true)
        .err()
        .unwrap();
    assert!(gone.contains("feature doesn't exist"), "{gone}");
    assert!(merge_check(&r, "refs/heads/feature").is_err());
    assert_eq!(compare(&r, "refs/heads/main", &tip, true).unwrap().ahead, 1);
}

#[test]
fn a_branch_rewritten_since_leaves_its_old_commits_readable_by_id() {
    let sb = Sandbox::new("cmp-rewritten");
    let r = sb.path("r");
    parted(&r, "feature");
    let c = compare(&r, "refs/heads/main", "refs/heads/feature", true).unwrap();
    // The branch is reset to main: the comparison read before names commits no branch holds.
    run(&r, &["branch", "-f", "feature", "main"]).unwrap();
    assert_eq!(
        compare(&r, "refs/heads/main", "refs/heads/feature", true)
            .unwrap()
            .ahead,
        0
    );
    assert_eq!(log_between(&r, &c.base, &c.head, 0, 10).unwrap().len(), 1);
    assert_eq!(range_files(&r, &c.from, &c.head).unwrap().len(), 1);
    // Gone for good (pruned): an error to show, not a crash.
    assert!(range_files(&r, &c.from, &"0".repeat(40)).is_err());
    assert!(log_between(&r, &c.base, &"0".repeat(40), 0, 10).is_err());
}

#[test]
fn a_shallow_clone_has_no_merge_base_to_find_and_still_compares() {
    let sb = Sandbox::new("cmp-shallow");
    let c = sb.remote_with_clones(1);
    let a = &c[0];
    write_commit(a, "m.txt", "m\n", "more on main");
    run(a, &["push", "-q", "origin", "main"]).unwrap();
    run(a, &["switch", "-q", "-c", "feat"]).unwrap();
    write_commit(a, "f.txt", "f\n", "on feat");
    run(a, &["push", "-q", "-u", "origin", "feat"]).unwrap();

    let url = format!("file://{}", sb.path("origin.git").display());
    let shallow = sb.path("shallow");
    run(
        &sb.0,
        &[
            "clone",
            "-q",
            "--depth",
            "1",
            "--no-single-branch",
            &url,
            shallow.to_str().unwrap(),
        ],
    )
    .unwrap();
    identity(&shallow);
    let c = compare(
        &shallow,
        "refs/remotes/origin/main",
        "refs/remotes/origin/feat",
        true,
    )
    .unwrap();
    // Cut off before they parted: no common commit is found, so it's the plain difference.
    assert!(c.unrelated);
    assert_eq!(c.from, c.base);
    assert!(!c.files.is_empty());
    let check = merge_check(&shallow, "refs/remotes/origin/feat").unwrap();
    assert!(check.incoming >= 1);
}

#[test]
fn merge_commits_in_a_range_are_counted_listed_and_diffed_against_their_first_parent() {
    let sb = Sandbox::new("cmp-merges");
    let r = sb.path("r");
    parted(&r, "feature");
    let base = rev(&r, "main~1");
    merge(&r, "refs/heads/feature", MergeKind::NoFf, false).unwrap();
    let merged = rev(&r, "HEAD");

    let c = compare(&r, &base, &merged, true).unwrap();
    // main's commit, the branch's, and the merge.
    assert_eq!((c.ahead, c.behind), (3, 0));
    let log = log_between(&r, &base, &merged, 0, 50).unwrap();
    assert_eq!(log.len() as u32, c.ahead);
    assert_eq!(log[0].parents.len(), 2);
    // Open All on the merge: what it brought onto main's side.
    let files = commit_files(&r, &merged).unwrap();
    assert_eq!(
        files.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(),
        ["f.txt"]
    );
}

#[test]
fn open_all_on_a_root_commit_an_empty_one_and_a_huge_one() {
    let sb = Sandbox::new("cmp-open-all");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "a\n", "root");
    let root = rev(&r, "HEAD");
    run(&r, &["commit", "-q", "--allow-empty", "-m", "nothing"]).unwrap();
    let empty = rev(&r, "HEAD");

    let first = commit_files(&r, &root).unwrap();
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].status, "A");
    assert!(commit_files(&r, &empty).unwrap().is_empty());

    for i in 0..5000 {
        let dir = r.join(format!("d{}", i % 50));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(format!("f {i} ünï.txt")), format!("{i}\n")).unwrap();
    }
    run(&r, &["add", "-A"]).unwrap();
    run(&r, &["commit", "-q", "-m", "many"]).unwrap();
    let t = Instant::now();
    let files = commit_files(&r, &rev(&r, "HEAD")).unwrap();
    assert_eq!(files.len(), 5000);
    assert!(files.iter().all(|f| f.additions == Some(1)));
    assert!(t.elapsed().as_secs() < 30, "{:?}", t.elapsed());
}

#[test]
fn thousands_of_commits_page_in_order_within_a_budget() {
    let sb = Sandbox::new("cmp-many-commits");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "a\n", "base");
    let mut stream = String::new();
    for i in 0..3000 {
        let first = if i == 0 { "from refs/heads/main\n" } else { "" };
        stream.push_str(&format!(
            "commit refs/heads/big\ncommitter T <t@example.com> {} +0000\ndata {}\nc{i}\n{first}M 100644 inline n.txt\ndata {}\n{i}\n\n",
            1_700_000_000 + i,
            format!("c{i}").len(),
            i.to_string().len()
        ));
    }
    run_with(
        &r,
        &["fast-import", "--quiet"],
        &[],
        Some(stream.as_bytes()),
    )
    .unwrap();

    let t = Instant::now();
    let c = compare(&r, "refs/heads/main", "refs/heads/big", true).unwrap();
    assert_eq!((c.ahead, c.behind), (3000, 0));
    let (base, head) = (c.base.as_str(), c.head.as_str());
    let mut seen = 0;
    let mut newest_first = vec![];
    while seen < 3000 {
        let page = log_between(&r, base, head, seen, 200).unwrap();
        assert!(!page.is_empty(), "stopped at {seen}");
        newest_first.extend(page.iter().map(|c| c.subject.clone()));
        seen += page.len() as u32;
    }
    assert_eq!(newest_first.len(), 3000);
    assert_eq!(newest_first[0], "c2999");
    assert_eq!(newest_first[2999], "c0");
    assert!(log_between(&r, base, head, 3000, 200).unwrap().is_empty());
    assert!(t.elapsed().as_secs() < 60, "{:?}", t.elapsed());
}

#[test]
fn a_merge_check_names_what_conflicts_and_changes_nothing() {
    let sb = Sandbox::new("cmp-conflicts");
    let r = sb.path("r");
    init(&r);
    let body = "one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\n";
    write_commit(&r, "ünï code $x.txt", "a\nb\nc\n", "base");
    write_commit(&r, "gone.txt", "gone\n", "base");
    write_commit(&r, "old.txt", body, "base");
    write_commit(&r, "pic.bin", "x", "base");
    fs::write(r.join("pic.bin"), [0u8, 1, 2, 0, 3]).unwrap();
    run(&r, &["commit", "-qam", "binary"]).unwrap();
    run(&r, &["switch", "-q", "-c", "other"]).unwrap();
    fs::write(r.join("ünï code $x.txt"), "a\nOTHER\nc\n").unwrap();
    fs::write(r.join("pic.bin"), [0u8, 9, 9, 0, 9]).unwrap();
    fs::write(r.join("gone.txt"), "changed there\n").unwrap();
    run(&r, &["mv", "old.txt", "renamed there.txt"]).unwrap();
    write_commit(&r, "added.txt", "there\n", "other");
    run(&r, &["commit", "-qam", "other edits"]).unwrap();
    run(&r, &["switch", "-q", "main"]).unwrap();
    fs::write(r.join("ünï code $x.txt"), "a\nMAIN\nc\n").unwrap();
    fs::write(r.join("pic.bin"), [0u8, 7, 7, 0, 7]).unwrap();
    run(&r, &["rm", "-q", "gone.txt"]).unwrap();
    run(&r, &["mv", "old.txt", "renamed here.txt"]).unwrap();
    write_commit(&r, "added.txt", "here\n", "main");
    run(&r, &["commit", "-qam", "main edits"]).unwrap();

    // A tree in the way: a change not staged, one staged, a new file.
    fs::write(r.join("shared.txt"), "dirty\n").unwrap();
    fs::write(r.join("renamed here.txt"), "dirty too\n").unwrap();
    run(&r, &["add", "renamed here.txt"]).unwrap();
    fs::write(r.join("untracked.txt"), "new\n").unwrap();
    let before = untouched(&r);

    let check = merge_check(&r, "refs/heads/other").unwrap();
    let mut found = check.conflicts.expect("git here can check a merge");
    found.sort();
    for expect in ["added.txt", "gone.txt", "pic.bin", "ünï code $x.txt"] {
        assert!(found.iter().any(|f| f == expect), "{expect} in {found:?}");
    }
    // Renamed to two names: the old path is the conflict.
    assert!(found.iter().any(|f| f == "old.txt"), "{found:?}");
    assert_eq!(untouched(&r), before);
    assert_eq!(fs::read_to_string(r.join("shared.txt")).unwrap(), "dirty\n");
    assert!(!r.join(".git/MERGE_HEAD").exists());
    assert!(!r.join(".git/MERGE_AUTOSTASH").exists());
}
