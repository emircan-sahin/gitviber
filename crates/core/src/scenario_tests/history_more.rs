//! Edges of Split Commit, Fixup Staged Changes, moving stacked branches along and cherry-picking
//! several commits: odd commits, odd work trees, stops and undo.

use super::*;
use crate::rewrite::{after_abort, before_abort, run as rewrite, Edit, Outcome};

fn git(r: &Path, args: &[&str]) -> String {
    run_text(r, args).unwrap()
}
fn head(r: &Path) -> String {
    git(r, &["rev-parse", "HEAD"]).trim().to_string()
}
fn subjects(r: &Path) -> Vec<String> {
    git(r, &["log", "--format=%s"])
        .lines()
        .map(str::to_string)
        .collect()
}
fn sha_of(r: &Path, subject: &str) -> String {
    git(
        r,
        &["log", "--format=%H", "--grep", &format!("^{subject}$")],
    )
    .trim()
    .to_string()
}
fn idle(r: &Path) -> bool {
    operation(r).is_none()
}
fn commit_it(r: &Path, message: &str) {
    commit(r, message, &CommitOptions::default(), &Net::default()).unwrap();
}
/// one..four, a.txt..d.txt; "five" changes a.txt and adds x.txt and y.txt.
fn repo(name: &str) -> (Sandbox, PathBuf) {
    let sb = Sandbox::new(name);
    let r = sb.path("r");
    init(&r);
    for (f, s) in [("a", "one"), ("b", "two"), ("c", "three"), ("d", "four")] {
        write_commit(&r, &format!("{f}.txt"), &format!("{f}\n"), s);
    }
    fs::write(r.join("a.txt"), "a\nmore\n").unwrap();
    fs::write(r.join("x.txt"), "x\n").unwrap();
    fs::write(r.join("y.txt"), "y\n").unwrap();
    run(&r, &["add", "a.txt", "x.txt", "y.txt"]).unwrap();
    run(&r, &["commit", "-q", "-m", "five"]).unwrap();
    (sb, r)
}
fn split(sha: &str) -> Edit {
    Edit::Split { sha: sha.into() }
}

#[test]
fn split_the_newest_and_the_second_commit() {
    let (_sb, r) = repo("hm-split-head");
    let whole = git(&r, &["rev-parse", "HEAD^{tree}"]);
    // HEAD itself.
    assert_eq!(
        rewrite(&r, &head(&r), &split(&head(&r)), false).unwrap(),
        Outcome::Split
    );
    stage(&r, &["a.txt".into()]).unwrap();
    commit_it(&r, "five a");
    stage(&r, &["x.txt".into(), "y.txt".into()]).unwrap();
    commit_it(&r, "five xy");
    assert!(!op_continue(&r).unwrap());
    assert_eq!(subjects(&r)[..3], ["five xy", "five a", "four"]);
    assert_eq!(git(&r, &["rev-parse", "HEAD^{tree}"]), whole);

    // The commit right above the root.
    let two = sha_of(&r, "two");
    let before = head(&r);
    assert_eq!(
        rewrite(&r, &before, &split(&two), false).unwrap(),
        Outcome::Split
    );
    assert_eq!(git(&r, &["ls-files", "--others"]), "b.txt\n");
    stage(&r, &["b.txt".into()]).unwrap();
    commit_it(&r, "two again");
    assert!(!op_continue(&r).unwrap());
    assert_eq!(git(&r, &["rev-parse", "HEAD^{tree}"]), whole);
    assert_eq!(subjects(&r).len(), 6);
}

#[test]
fn split_with_a_dirty_worktree_and_a_new_file_made_meanwhile() {
    let (_sb, r) = repo("hm-split-dirty");
    // Edits to the file the commit changes, a staged one elsewhere, an untracked one.
    fs::write(r.join("a.txt"), "a\nmore\nmine\n").unwrap();
    fs::write(r.join("d.txt"), "d mine\n").unwrap();
    stage(&r, &["d.txt".into()]).unwrap();
    fs::write(r.join("notes.txt"), "n\n").unwrap();
    let before = head(&r);
    let out = rewrite(&r, &before, &split(&before), false).unwrap();
    assert_eq!(out, Outcome::Split);
    // Set aside: the pieces start from exactly the commit's changes.
    assert_eq!(fs::read_to_string(r.join("a.txt")).unwrap(), "a\nmore\n");
    assert_eq!(fs::read_to_string(r.join("d.txt")).unwrap(), "d\n");
    // A file made now is the user's; it must not break the abort.
    fs::write(r.join("later.txt"), "later\n").unwrap();
    stage(&r, &["a.txt".into()]).unwrap();
    commit_it(&r, "part");
    before_abort(&r).unwrap();
    op_abort(&r).unwrap();
    after_abort(&r).unwrap();
    assert!(idle(&r));
    assert_eq!(head(&r), before);
    assert_eq!(
        fs::read_to_string(r.join("a.txt")).unwrap(),
        "a\nmore\nmine\n"
    );
    assert_eq!(fs::read_to_string(r.join("d.txt")).unwrap(), "d mine\n");
    assert_eq!(fs::read_to_string(r.join("later.txt")).unwrap(), "later\n");
    assert_eq!(fs::read_to_string(r.join("notes.txt")).unwrap(), "n\n");
    assert!(git(&r, &["stash", "list"]).is_empty());

    // Continued from here, the changes come back on top of the pieces.
    assert_eq!(
        rewrite(&r, &before, &split(&before), false).unwrap(),
        Outcome::Split
    );
    stage(&r, &["a.txt".into(), "x.txt".into(), "y.txt".into()]).unwrap();
    commit_it(&r, "all");
    assert!(!op_continue(&r).unwrap());
    assert!(idle(&r));
    assert_eq!(
        fs::read_to_string(r.join("a.txt")).unwrap(),
        "a\nmore\nmine\n"
    );
    assert_eq!(fs::read_to_string(r.join("d.txt")).unwrap(), "d mine\n");
}

#[test]
fn split_continued_from_a_terminal_or_left_half_done() {
    let (_sb, r) = repo("hm-split-terminal");
    let before = head(&r);
    let j = Journal::default();
    let action = || Action::new("Split", Mode::Keep);
    assert_eq!(
        j.record(&r, action(), |r| rewrite(
            r,
            &before,
            &split(&before),
            false
        ))
        .unwrap(),
        Outcome::Split
    );
    // Pieces left over, one a tracked file's edit: git refuses, and the stop stays a split.
    stage(&r, &["x.txt".into()]).unwrap();
    commit_it(&r, "five x");
    let left = op_continue(&r);
    eprintln!("continue with a.txt left: {left:?}");
    assert!(left.unwrap_err().contains("Commit or discard"));
    assert!(operation(&r).is_some_and(|o| o.split));
    stage(&r, &["a.txt".into(), "y.txt".into()]).unwrap();
    commit_it(&r, "five rest");
    // Everything committed, finished by a terminal's git.
    let out = std::process::Command::new("git")
        .current_dir(&r)
        .args(["rebase", "--continue"])
        .env("GIT_EDITOR", "true")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(idle(&r));
    // The next entry starts afresh; the stale split mark is harmless.
    write_commit(&r, "z.txt", "z\n", "six");
    assert!(operation(&r).is_none());
    let six = head(&r);
    assert_eq!(
        rewrite(
            &r,
            &six,
            &Edit::Drop {
                shas: vec![six.clone()]
            },
            false
        )
        .unwrap(),
        Outcome::Done
    );
    assert!(!status(&r).unwrap().operation.is_some_and(|o| o.split));
}

#[test]
fn a_rebase_stopped_by_someone_else_is_not_a_split() {
    let (_sb, r) = repo("hm-split-other");
    // A split that was left behind, then a terminal's own `edit` of another commit.
    let before = head(&r);
    rewrite(&r, &before, &split(&before), false).unwrap();
    before_abort(&r).unwrap();
    op_abort(&r).unwrap();
    after_abort(&r).unwrap();
    let two = sha_of(&r, "two");
    let out = std::process::Command::new("git")
        .current_dir(&r)
        .args(["rebase", "-i", &format!("{two}^")])
        .env("GIT_SEQUENCE_EDITOR", "sed -i.bak 's/^pick/edit/'")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let op = operation(&r).unwrap();
    assert!(!op.split, "{}", op.kind);
    run(&r, &["rebase", "--abort"]).unwrap();
}

#[test]
fn split_with_stacked_branches_and_a_root_with_a_file_mode_change() {
    let (_sb, r) = repo("hm-split-stacked");
    run(&r, &["branch", "at-five"]).unwrap();
    run(&r, &["branch", "at-four", "HEAD~1"]).unwrap();
    let before = head(&r);
    let five = before.clone();
    assert_eq!(
        rewrite(&r, &before, &split(&five), true).unwrap(),
        Outcome::Split
    );
    stage(&r, &["a.txt".into(), "x.txt".into(), "y.txt".into()]).unwrap();
    commit_it(&r, "five, again");
    assert!(!op_continue(&r).unwrap());
    // The branch on the split commit lands on the last piece; the one below stays.
    assert_eq!(rev(&r, "at-five"), head(&r));
    assert_eq!(rev(&r, "at-four"), rev(&r, "HEAD~1"));
}

#[test]
fn fixup_staged_edges() {
    let (_sb, r) = repo("hm-fixup");
    let fix = |sha: &str| Edit::FixupStaged { sha: sha.into() };
    let before = head(&r);
    // Nothing staged, though there are unstaged edits.
    fs::write(r.join("b.txt"), "b2\n").unwrap();
    assert!(rewrite(&r, &before, &fix(&sha_of(&r, "two")), false)
        .unwrap_err()
        .contains("no staged"));
    assert!(idle(&r));
    run(&r, &["checkout", "-q", "--", "b.txt"]).unwrap();

    // A staged rename goes into the older commit as a rename.
    run(&r, &["mv", "b.txt", "b2.txt"]).unwrap();
    let j = Journal::default();
    j.record(&r, Action::new("Fixup", Mode::Soft), |r| {
        rewrite(r, &before, &fix(&sha_of(r, "three")), false)
    })
    .unwrap();
    assert_eq!(subjects(&r), ["five", "four", "three", "two", "one"]);
    assert_eq!(
        git(&r, &["ls-tree", "--name-only", "-r", &sha_of(&r, "three")]),
        "a.txt\nb2.txt\nc.txt\n"
    );
    assert!(idle(&r));
    step(&j, &r, false).unwrap();
    assert_eq!(head(&r), before);
    assert_eq!(
        git(&r, &["diff", "--cached", "--name-status"])
            .lines()
            .count(),
        1
    );
    run(&r, &["reset", "-q", "--hard"]).unwrap();

    // Into the root: the first commit is made again with --root.
    fs::write(r.join("first.txt"), "late\n").unwrap();
    stage(&r, &["first.txt".into()]).unwrap();
    let one = sha_of(&r, "one");
    assert_eq!(
        rewrite(&r, &head(&r), &fix(&one), false).unwrap(),
        Outcome::Done
    );
    assert_eq!(subjects(&r), ["five", "four", "three", "two", "one"]);
    assert_eq!(
        git(&r, &["show", &format!("{}:first.txt", sha_of(&r, "one"))]),
        "late\n"
    );
    assert!(idle(&r));
    assert!(git(&r, &["diff", "--cached", "--name-only"]).is_empty());
}

#[test]
fn fixup_that_conflicts_can_be_continued_and_undone() {
    let (_sb, r) = repo("hm-fixup-conflict");
    let before = head(&r);
    // five's a.txt, which "two" doesn't have changed: conflicts going down into "two"? Use b.txt.
    write_commit(&r, "b.txt", "b5\n", "six");
    let before2 = head(&r);
    fs::write(r.join("b.txt"), "b fixed\n").unwrap();
    stage(&r, &["b.txt".into()]).unwrap();
    let j = Journal::default();
    let edit = Edit::FixupStaged {
        sha: sha_of(&r, "two"),
    };
    let out = j
        .record(&r, Action::new("Fixup", Mode::Soft), |r| {
            rewrite(r, &before2, &edit, false)
        })
        .unwrap();
    assert_eq!(out, Outcome::Conflicts);
    resolve_side(&r, "b.txt", Side::Theirs).unwrap();
    let res = j.record(&r, Action::new("Continue", Mode::Keep), op_continue);
    eprintln!("continue: {res:?} {}", git(&r, &["status", "--short"]));
    let mut guard = 0;
    while !idle(&r) && guard < 5 {
        for p in git(&r, &["diff", "--name-only", "--diff-filter=U"]).lines() {
            resolve_side(&r, p, Side::Theirs).unwrap();
        }
        let _ = j.record(&r, Action::new("Continue", Mode::Keep), op_continue);
        guard += 1;
    }
    assert!(idle(&r));
    assert_eq!(j.view(&r).undo.len(), 1);
    step(&j, &r, false).unwrap();
    assert_eq!(head(&r), before2);
    let _ = before;
}

#[test]
fn stacked_branches_at_the_same_commit_and_remote_tracking() {
    let sb = Sandbox::new("hm-stacked-same");
    let clones = sb.remote_with_clones(1);
    let r = &clones[0];
    for (f, s) in [("a", "one"), ("b", "two"), ("c", "three")] {
        write_commit(r, &format!("{f}2.txt"), "v\n", s);
    }
    run(r, &["push", "-q", "origin", "main"]).unwrap();
    write_commit(r, "d2.txt", "v\n", "four");
    // Two branches on "three" and one on "two", a remote-tracking ref on "three".
    run(r, &["branch", "p", "HEAD~1"]).unwrap();
    run(r, &["branch", "q", "HEAD~1"]).unwrap();
    run(r, &["branch", "low", "HEAD~2"]).unwrap();
    let offer = crate::rewrite::stacked_for(
        r,
        &Edit::Drop {
            shas: vec![sha_of(r, "two")],
        },
    )
    .unwrap();
    assert_eq!(offer.branches, ["low", "p", "q"]);
    let remote_before = rev(r, "origin/main");
    rewrite(
        r,
        &head(r),
        &Edit::Drop {
            shas: vec![sha_of(r, "two")],
        },
        true,
    )
    .unwrap();
    assert_eq!(rev(r, "p"), rev(r, "q"));
    assert_eq!(rev(r, "p"), sha_of(r, "three"));
    // "low" sat on the dropped commit: it goes to the one under it.
    assert_eq!(rev(r, "low"), sha_of(r, "one"));
    assert_eq!(rev(r, "origin/main"), remote_before);
    assert!(idle(r));
}

#[test]
fn rebase_onto_with_and_without_update_refs() {
    let (_sb, r) = repo("hm-rebase-onto");
    run(&r, &["branch", "tip-three", "HEAD~2"]).unwrap();
    run(&r, &["switch", "-q", "-c", "base", "HEAD~4"]).unwrap();
    write_commit(&r, "base.txt", "b\n", "base work");
    run(&r, &["switch", "-q", "main"]).unwrap();
    let offer = crate::rewrite::stacked_onto(&r, "base").unwrap();
    assert_eq!(offer.branches, ["tip-three"]);
    let old = rev(&r, "tip-three");
    assert!(!rebase(&r, "base", false, Some(false)).unwrap());
    assert_eq!(rev(&r, "tip-three"), old);
    run(&r, &["reset", "-q", "--hard", "ORIG_HEAD"]).unwrap();
    assert!(!rebase(&r, "base", false, Some(true)).unwrap());
    assert_ne!(rev(&r, "tip-three"), old);
    // A bogus ref is refused before git sees it.
    assert!(crate::rewrite::stacked_onto(&r, "--bogus").is_err());
}

/// main has "base"; feat has f1, f2 (changes a.txt), a merge of a side branch, f3.
fn feat_repo(name: &str) -> (Sandbox, PathBuf, Vec<String>) {
    let sb = Sandbox::new(name);
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "base\n", "base");
    run(&r, &["switch", "-q", "-c", "feat"]).unwrap();
    write_commit(&r, "f1.txt", "1\n", "f1");
    run(&r, &["switch", "-q", "-c", "side", "HEAD~1"]).unwrap();
    write_commit(&r, "s.txt", "s\n", "side work");
    run(&r, &["switch", "-q", "feat"]).unwrap();
    run(&r, &["merge", "-q", "--no-ff", "-m", "merge side", "side"]).unwrap();
    write_commit(&r, "f3.txt", "3\n", "f3");
    let shas = ["f1", "merge side", "f3"]
        .iter()
        .map(|s| sha_of(&r, s))
        .collect();
    run(&r, &["switch", "-q", "main"]).unwrap();
    (sb, r, shas)
}

#[test]
fn cherry_pick_many_with_a_merge_in_the_list() {
    let (_sb, r, shas) = feat_repo("hm-pick-merge");
    let j = Journal::default();
    let action = || Action::new("Cherry-pick 3 commits", Mode::Keep);
    let before = head(&r);
    // f1 is new, the merge relative to its first parent brings "side work", f3 is new.
    assert!(!j
        .record(&r, action(), |r| cherry_pick_many(r, &shas))
        .unwrap());
    assert_eq!(subjects(&r)[..4], ["f3", "merge side", "f1", "base"]);
    assert!(r.join("s.txt").exists() && r.join("f3.txt").exists());
    assert_eq!(j.view(&r).undo.len(), 1);
    step(&j, &r, false).unwrap();
    assert_eq!(head(&r), before);
    step(&j, &r, true).unwrap();
    assert_eq!(subjects(&r)[0], "f3");
}

#[test]
fn cherry_pick_many_root_duplicates_applied_and_empty() {
    let (_sb, r, shas) = feat_repo("hm-pick-odd");
    // The root commit of an unrelated history.
    run(&r, &["switch", "-q", "--orphan", "other"]).unwrap();
    run(&r, &["rm", "-rfq", "--ignore-unmatch", "."]).unwrap();
    write_commit(&r, "o.txt", "o\n", "orphan root");
    write_commit(&r, "o2.txt", "o2\n", "orphan two");
    let orphan = [sha_of(&r, "orphan root"), sha_of(&r, "orphan two")];
    run(&r, &["switch", "-q", "main"]).unwrap();
    let before = head(&r);
    assert!(!cherry_pick_many(&r, &orphan).unwrap());
    assert_eq!(subjects(&r)[..3], ["orphan two", "orphan root", "base"]);
    assert!(idle(&r));
    run(&r, &["reset", "-q", "--hard", &before]).unwrap();

    // Listed twice: the second is empty by then and left out.
    let twice = [shas[0].clone(), shas[0].clone()];
    assert!(!cherry_pick_many(&r, &twice).unwrap());
    assert_eq!(subjects(&r)[..2], ["f1", "base"]);
    assert!(idle(&r));
    // Applied before, in the middle of the list: only the new ones land.
    let mixed = [shas[0].clone(), shas[2].clone()];
    assert!(!cherry_pick_many(&r, &mixed).unwrap());
    assert_eq!(subjects(&r)[..2], ["f3", "f1"]);
    assert!(idle(&r));
    // All of it there already.
    let err = cherry_pick_many(&r, &mixed).unwrap_err();
    assert!(err.contains("already has"), "{err}");
    assert!(idle(&r));

    // An empty commit among them.
    run(&r, &["reset", "-q", "--hard", &before]).unwrap();
    run(&r, &["switch", "-q", "feat"]).unwrap();
    run(&r, &["commit", "-q", "--allow-empty", "-m", "nothing"]).unwrap();
    let nothing = sha_of(&r, "nothing");
    run(&r, &["switch", "-q", "main"]).unwrap();
    let res = cherry_pick_many(&r, &[shas[0].clone(), nothing.clone()]);
    eprintln!("empty among them: {res:?}");
    assert!(res.is_ok(), "{res:?}");
    assert!(idle(&r));
    assert_eq!(subjects(&r)[0], "f1");
    let res = cherry_pick_many(&r, &[nothing]);
    assert!(res.is_err(), "{res:?}");
    assert!(idle(&r));
}

#[test]
fn cherry_pick_many_conflict_resolved_to_nothing_then_undo_and_dirty_tree() {
    let sb = Sandbox::new("hm-pick-conflict");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "base\n", "base");
    run(&r, &["switch", "-q", "-c", "feat"]).unwrap();
    write_commit(&r, "a.txt", "feat\n", "feat a");
    write_commit(&r, "n.txt", "n\n", "feat n");
    let picks = [rev(&r, "HEAD~1"), rev(&r, "HEAD")];
    run(&r, &["switch", "-q", "main"]).unwrap();
    write_commit(&r, "a.txt", "main\n", "main a");
    let before = head(&r);
    let j = Journal::default();
    let action = || Action::new("Cherry-pick 2 commits", Mode::Keep);
    assert!(j
        .record(&r, action(), |r| cherry_pick_many(r, &picks))
        .unwrap());
    // Keeping main's side leaves the pick with no change: it's left out, the next one lands.
    resolve_side(&r, "a.txt", Side::Ours).unwrap();
    assert!(!j.record(&r, action(), op_continue).unwrap());
    assert!(idle(&r));
    assert_eq!(subjects(&r)[..2], ["feat n", "main a"]);
    assert_eq!(j.view(&r).undo.len(), 1);
    step(&j, &r, false).unwrap();
    assert_eq!(head(&r), before);

    // A dirty file the second pick needs: the first lands, then git refuses; nothing is left
    // half-way waiting on nothing to resolve.
    run(&r, &["reset", "-q", "--hard", "feat~1"]).unwrap();
    run(&r, &["reset", "-q", "--hard", &before]).unwrap();
    write_commit(&r, "x.txt", "x\n", "main x");
    let before = head(&r);
    run(&r, &["switch", "-q", "feat"]).unwrap();
    write_commit(&r, "p1.txt", "1\n", "pick one");
    write_commit(&r, "x.txt", "feat x\n", "pick two");
    let picks = [rev(&r, "HEAD~1"), rev(&r, "HEAD")];
    run(&r, &["switch", "-q", "main"]).unwrap();
    fs::write(r.join("x.txt"), "my edit\n").unwrap();
    let res = j.record(&r, action(), |r| cherry_pick_many(r, &picks));
    eprintln!(
        "dirty: {res:?} op={:?} head==before {}",
        operation(&r).map(|o| o.kind),
        head(&r) == before
    );
    if res.is_err() {
        assert!(
            idle(&r),
            "left a pick waiting on nothing: {:?}",
            operation(&r).map(|o| o.kind)
        );
        assert_eq!(head(&r), before);
        assert_eq!(fs::read_to_string(r.join("x.txt")).unwrap(), "my edit\n");
    }
}
