//! A guided review of the worktree's uncommitted changes (`Target::Changes`): what the snapshot
//! takes in and leaves out, odd index states, and the stamp that tells it went stale.

use super::*;
use crate::suggest::{self, Agent, Target};
use std::sync::atomic::AtomicBool;

fn guide(repo: &Path) -> Result<suggest::Guided, String> {
    suggest::run_guide(
        repo,
        "cat",
        "PROMPT",
        &Target::Changes,
        &Agent::default(),
        &AtomicBool::new(false),
    )
}

fn index_bytes(repo: &Path) -> Vec<u8> {
    fs::read(repo.join(".git/index")).unwrap()
}

fn leftovers(repo: &Path) -> Vec<String> {
    fs::read_dir(repo.join(".git"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains("gitviber") || n.starts_with("sharedindex"))
        .collect()
}

#[test]
fn staged_unstaged_deleted_and_new_files_go_in_and_the_real_index_stays_as_it_was() {
    let sb = Sandbox::new("changes-guide");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, ".gitignore", "ignored.log\n", "ignore");
    write_commit(&r, "a.txt", "one\n", "a");
    write_commit(&r, "b.txt", "bee\n", "b");
    write_commit(&r, "c.txt", "sea\n", "c");
    fs::write(r.join("a.txt"), "one\nTWO\n").unwrap();
    fs::write(r.join("b.txt"), "BEE\n").unwrap();
    run(&r, &["add", "b.txt"]).unwrap();
    fs::remove_file(r.join("c.txt")).unwrap();
    fs::write(r.join("new.txt"), "fresh\n").unwrap();
    fs::write(r.join("ignored.log"), "noise\n").unwrap();
    let before = index_bytes(&r);

    let g = guide(&r).unwrap();
    assert!(
        g.text
            .contains("Uncommitted changes in the worktree on the branch main"),
        "{}",
        g.text
    );
    for want in ["+TWO", "+BEE", "-sea", "+fresh"] {
        assert!(g.text.contains(want), "{want} missing:\n{}", g.text);
    }
    assert!(!g.text.contains("noise"), "{}", g.text);
    assert_eq!(g.base, rev(&r, "HEAD"));
    assert!(g.stamp.is_some());
    assert_eq!(index_bytes(&r), before, "the real index changed");
    assert!(leftovers(&r).is_empty(), "{:?}", leftovers(&r));

    // The tree reads back as a pull request's head does.
    let files = range_files(&r, &g.base, &g.head).unwrap();
    let mut paths: Vec<_> = files
        .iter()
        .map(|f| format!("{} {}", f.status, f.path))
        .collect();
    paths.sort();
    assert_eq!(paths, ["A new.txt", "D c.txt", "M a.txt", "M b.txt"]);
    let wt = |p: &str| vfs::read_file(&r, p);
    let pair = diff_pair(
        &r,
        "range",
        "new.txt",
        None,
        Some(&g.head),
        Some(&g.base),
        None,
        wt,
    )
    .unwrap();
    assert_eq!(pair.modified.text, "fresh\n");
}

#[test]
fn before_the_first_commit_everything_is_new_against_the_empty_tree() {
    let sb = Sandbox::new("changes-unborn");
    let r = sb.path("r");
    init(&r);
    fs::write(r.join("a.txt"), "first\n").unwrap();
    let g = guide(&r).unwrap();
    assert_eq!(g.base, empty_tree(&r).unwrap());
    assert!(
        g.text.contains("no commit yet, so every file is new") && g.text.contains("+first"),
        "{}",
        g.text
    );
}

#[test]
fn nothing_to_review_says_so_and_what_can_t_be_read_is_named() {
    let sb = Sandbox::new("changes-none");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "one\n", "a");
    assert_eq!(guide(&r).unwrap_err(), "There are no uncommitted changes.");
    // Only a file past the size cap: nothing the review could read.
    let big = vec![b'x'; (UNTRACKED_MAX_FILE + 1) as usize];
    fs::write(r.join("big.bin"), &big).unwrap();
    assert!(guide(&r)
        .unwrap_err()
        .starts_with("The only changes are new files"));
    // With a real change, the big file and a nested repository are left out by name.
    init(&r.join("nested"));
    fs::write(r.join("a.txt"), "two\n").unwrap();
    let g = guide(&r).unwrap();
    assert!(g.text.contains("Left out, as new files too big or too many to read, or nested repositories: big.bin, nested."), "{}", g.text);
    assert!(!g.text.contains("xxxx"));
}

#[test]
fn a_conflicted_merge_goes_in_as_it_is_on_disk() {
    let sb = Sandbox::new("changes-merge");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "base\n", "base");
    run(&r, &["switch", "-q", "-c", "other"]).unwrap();
    write_commit(&r, "a.txt", "theirs\n", "theirs");
    run(&r, &["switch", "-q", "main"]).unwrap();
    write_commit(&r, "a.txt", "ours\n", "ours");
    assert!(run(&r, &["merge", "-q", "other"]).is_err());
    let g = guide(&r).unwrap();
    assert!(
        g.text.contains("A merge is in progress") && g.text.contains("+<<<<<<<"),
        "{}",
        g.text
    );
}

#[test]
fn intent_to_add_crlf_and_split_index_repos_work() {
    let sb = Sandbox::new("changes-odd-index");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "one\n", "a");
    run(&r, &["config", "core.autocrlf", "true"]).unwrap();
    run(&r, &["config", "core.safecrlf", "true"]).unwrap();
    run(&r, &["config", "core.splitIndex", "true"]).unwrap();
    run(&r, &["update-index", "--split-index"]).unwrap();
    fs::write(r.join("later.txt"), "planned\n").unwrap();
    run(&r, &["add", "-N", "later.txt"]).unwrap();
    fs::write(r.join("dos.txt"), "a\r\nb\r\n").unwrap();
    let shared_before = leftovers(&r).len();
    let g = guide(&r).unwrap();
    assert!(
        g.text.contains("+planned") && g.text.contains("dos.txt"),
        "{}",
        g.text
    );
    assert_eq!(leftovers(&r).len(), shared_before, "{:?}", leftovers(&r));
}

#[test]
fn the_stamp_moves_with_an_edit_a_new_file_or_a_commit_not_with_staging() {
    let sb = Sandbox::new("changes-stamp");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "one\n", "a");
    fs::write(r.join("a.txt"), "two\n").unwrap();
    let first = changes_stamp(&r).unwrap();
    assert_eq!(changes_stamp(&r).unwrap(), first);
    run(&r, &["add", "a.txt"]).unwrap();
    assert_eq!(changes_stamp(&r).unwrap(), first, "staging moved it");
    fs::write(r.join("a.txt"), "three, longer\n").unwrap();
    let edited = changes_stamp(&r).unwrap();
    assert_ne!(edited, first);
    fs::write(r.join("b.txt"), "new\n").unwrap();
    let added = changes_stamp(&r).unwrap();
    assert_ne!(added, edited);
    run(&r, &["add", "-A"]).unwrap();
    run(&r, &["commit", "-q", "-m", "all"]).unwrap();
    assert_ne!(changes_stamp(&r).unwrap(), added);
}

#[test]
fn a_copy_of_an_old_index_is_never_taken_for_a_killed_run_s() {
    let sb = Sandbox::new("changes-copy-time");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "one\n", "a");
    let day_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(86_400);
    fs::File::options()
        .write(true)
        .open(r.join(".git/index"))
        .unwrap()
        .set_modified(day_ago)
        .unwrap();
    let copy = crate::patch::Scratch::index(&r).unwrap();
    crate::patch::Scratch::sweep(&r, std::time::Duration::from_secs(3600));
    assert!(Path::new(copy.path().unwrap()).exists());
}

#[test]
fn a_killed_run_s_index_copy_is_swept_once_old_and_a_fresh_one_kept() {
    let sb = Sandbox::new("changes-sweep");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "one\n", "a");
    fs::write(r.join("a.txt"), "two\n").unwrap();
    let (old, fresh) = (
        r.join(".git/index.gitviber.1.0"),
        r.join(".git/index.gitviber.2.0"),
    );
    fs::write(&old, "x").unwrap();
    fs::write(&fresh, "x").unwrap();
    let two_hours_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(7200);
    fs::File::options()
        .write(true)
        .open(&old)
        .unwrap()
        .set_modified(two_hours_ago)
        .unwrap();
    guide(&r).unwrap();
    assert!(!old.exists() && fresh.exists());
}

#[test]
fn past_the_file_count_cap_the_same_new_files_are_left_out_each_time() {
    let sb = Sandbox::new("changes-caps");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "one\n", "a");
    fs::create_dir_all(r.join("many")).unwrap();
    for i in 0..1002 {
        fs::write(r.join(format!("many/{i:04}.txt")), "x\n").unwrap();
    }
    let first = guide(&r).unwrap();
    assert!(first.text.contains("Left out, as new files too big or too many to read, or nested repositories: many/1000.txt, many/1001.txt."), "{}", first.text);
    assert_eq!(guide(&r).unwrap().head, first.head);
}

#[test]
fn unstaging_a_file_kept_on_disk_leaves_the_stamp_as_it_was() {
    let sb = Sandbox::new("changes-rm-cached");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "one\n", "a");
    write_commit(&r, "b.txt", "bee\n", "b");
    fs::write(r.join("a.txt"), "two\n").unwrap();
    let before = changes_stamp(&r).unwrap();
    run(&r, &["rm", "-q", "--cached", "a.txt"]).unwrap();
    let after = changes_stamp(&r).unwrap();
    // `a.txt` is now staged as removed and new on disk: one path, the same content.
    assert_eq!(after, before);
}
