use super::*;
use crate::lines::{run as change, Request};

/// A request as the view makes it: the texts it showed, and the chosen lines.
fn request(
    repo: &Path,
    kind: &str,
    action: &str,
    path: &str,
    removed: &[u32],
    added: &[u32],
) -> Request {
    let pair = diff_pair(repo, kind, path, None, None, None, None, |p| {
        vfs::read_file(repo, p)
    })
    .unwrap();
    let shown = |f: &FileText| f.exists.then(|| f.text.clone());
    Request {
        path: path.into(),
        kind: kind.into(),
        action: action.into(),
        original: shown(&pair.original),
        modified: shown(&pair.modified),
        removed: removed.to_vec(),
        added: added.to_vec(),
    }
}
fn act(repo: &Path, kind: &str, action: &str, path: &str, removed: &[u32], added: &[u32]) {
    change(repo, &request(repo, kind, action, path, removed, added)).unwrap();
}
fn index(repo: &Path, path: &str) -> String {
    run_text(repo, &["show", &format!(":{path}")]).unwrap()
}
fn disk(repo: &Path, path: &str) -> String {
    fs::read_to_string(repo.join(path)).unwrap()
}
fn repo(name: &str) -> (Sandbox, PathBuf) {
    let sb = Sandbox::new(name);
    let r = sb.path("r");
    init(&r);
    (sb, r)
}

#[test]
fn staging_one_change_of_two_leaves_the_other() {
    let (_sb, r) = repo("lines-stage");
    write_commit(&r, "a.txt", "1\n2\n3\n4\n5\n6\n7\n8\n9\n", "base");
    fs::write(r.join("a.txt"), "1\nTWO\n3\n4\n5\n6\n7\n8\n9\nten\n").unwrap();
    // The first change: 2 → TWO (removed line 2, added line 2).
    act(&r, "unstaged", "stage", "a.txt", &[2], &[2]);
    assert_eq!(index(&r, "a.txt"), "1\nTWO\n3\n4\n5\n6\n7\n8\n9\n");
    assert_eq!(disk(&r, "a.txt"), "1\nTWO\n3\n4\n5\n6\n7\n8\n9\nten\n");
    // What's left unstaged is the other change alone.
    let rest = request(&r, "unstaged", "stage", "a.txt", &[], &[]);
    assert_eq!(
        rest.original.as_deref(),
        Some("1\nTWO\n3\n4\n5\n6\n7\n8\n9\n")
    );
    // And back: unstaging it.
    act(&r, "staged", "unstage", "a.txt", &[2], &[2]);
    assert_eq!(index(&r, "a.txt"), "1\n2\n3\n4\n5\n6\n7\n8\n9\n");
}

#[test]
fn some_added_lines_of_a_block() {
    let (_sb, r) = repo("lines-some");
    write_commit(&r, "a.txt", "a\nz\n", "base");
    fs::write(r.join("a.txt"), "a\nb\nc\nd\nz\n").unwrap();
    act(&r, "unstaged", "stage", "a.txt", &[], &[3]);
    assert_eq!(index(&r, "a.txt"), "a\nc\nz\n");
    // Unstaging from the staged side, where c is line 2 of the index.
    act(&r, "staged", "unstage", "a.txt", &[], &[2]);
    assert_eq!(index(&r, "a.txt"), "a\nz\n");
}

#[test]
fn discarding_a_change_keeps_the_rest_and_the_index() {
    let (_sb, r) = repo("lines-discard");
    write_commit(&r, "a.txt", "1\n2\n3\n4\n5\n6\n7\n8\n9\n", "base");
    fs::write(r.join("a.txt"), "1\nTWO\n3\n4\n5\n6\n7\n8\n9\nten\n").unwrap();
    act(&r, "unstaged", "discard", "a.txt", &[], &[10]);
    assert_eq!(disk(&r, "a.txt"), "1\nTWO\n3\n4\n5\n6\n7\n8\n9\n");
    assert_eq!(index(&r, "a.txt"), "1\n2\n3\n4\n5\n6\n7\n8\n9\n");
    // A removed line comes back.
    fs::write(r.join("a.txt"), "1\n3\n4\n5\n6\n7\n8\n9\n").unwrap();
    act(&r, "unstaged", "discard", "a.txt", &[2], &[]);
    assert_eq!(disk(&r, "a.txt"), "1\n2\n3\n4\n5\n6\n7\n8\n9\n");
}

#[test]
fn new_untracked_and_deleted_files() {
    let (_sb, r) = repo("lines-files");
    write_commit(&r, "keep.txt", "k\n", "base");
    // Part of an untracked file: it's added with only that.
    fs::write(r.join("new.txt"), "one\ntwo\n").unwrap();
    act(&r, "unstaged", "stage", "new.txt", &[], &[1]);
    assert_eq!(index(&r, "new.txt"), "one\n");
    // All of a new file unstaged: out of the index, untracked again.
    act(&r, "staged", "unstage", "new.txt", &[], &[1]);
    assert!(run_text(&r, &["ls-files", "--", "new.txt"])
        .unwrap()
        .is_empty());
    assert_eq!(disk(&r, "new.txt"), "one\ntwo\n");
    // Every line of a deleted file: the deletion is staged.
    fs::remove_file(r.join("keep.txt")).unwrap();
    act(&r, "unstaged", "stage", "keep.txt", &[1], &[]);
    assert_eq!(
        run_text(&r, &["diff", "--cached", "--name-status"]).unwrap(),
        "D\tkeep.txt\n"
    );
    // Unstaging it puts the file back in the index; discarding brings it back to disk.
    act(&r, "staged", "unstage", "keep.txt", &[1], &[]);
    assert_eq!(index(&r, "keep.txt"), "k\n");
    act(&r, "unstaged", "discard", "keep.txt", &[1], &[]);
    assert_eq!(disk(&r, "keep.txt"), "k\n");
    // An untracked file isn't discarded line by line to nothing.
    let all = request(&r, "unstaged", "discard", "new.txt", &[], &[1, 2]);
    assert!(change(&r, &all).is_err());
}

#[test]
fn line_endings_filters_and_modes() {
    let (_sb, r) = repo("lines-eol");
    run(&r, &["config", "core.autocrlf", "true"]).unwrap();
    write_commit(&r, "w.txt", "a\r\nb\r\n", "base");
    fs::write(r.join("w.txt"), "a\r\nx\r\nb\r\ny\r\n").unwrap();
    act(&r, "unstaged", "stage", "w.txt", &[], &[2]);
    // Stored as git would: through the clean filter, LF.
    assert_eq!(
        run_text(&r, &["cat-file", "-p", ":w.txt"]).unwrap(),
        "a\nx\nb\n"
    );
    assert_eq!(disk(&r, "w.txt"), "a\r\nx\r\nb\r\ny\r\n");
    act(&r, "unstaged", "discard", "w.txt", &[], &[4]);
    assert_eq!(disk(&r, "w.txt"), "a\r\nx\r\nb\r\n");

    // No newline at the end, then lines after it.
    run(&r, &["config", "core.autocrlf", "false"]).unwrap();
    write_commit(&r, "n.txt", "a\nb", "no newline");
    fs::write(r.join("n.txt"), "a\nb\nc\n").unwrap();
    let shown = request(&r, "unstaged", "stage", "n.txt", &[], &[]);
    assert_eq!(shown.original.as_deref(), Some("a\nb"));
    act(&r, "unstaged", "stage", "n.txt", &[], &[3]);
    assert_eq!(index(&r, "n.txt"), "a\nb\nc\n");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        write_commit(&r, "run.sh", "echo 1\n", "script");
        fs::set_permissions(r.join("run.sh"), fs::Permissions::from_mode(0o755)).unwrap();
        stage(&r, &["run.sh".into()]).unwrap();
        commit(&r, "exec", &CommitOptions::default()).unwrap();
        fs::write(r.join("run.sh"), "echo 1\necho 2\n").unwrap();
        act(&r, "unstaged", "stage", "run.sh", &[], &[2]);
        assert!(run_text(&r, &["ls-files", "-s", "run.sh"])
            .unwrap()
            .starts_with("100755 "));
    }
}

#[test]
fn a_discarded_change_comes_back_with_undo() {
    let (_sb, r) = repo("lines-undo");
    write_commit(&r, "a.txt", "a\nb\n", "base");
    fs::write(r.join("a.txt"), "a\nkeep\nb\ngone\n").unwrap();
    let j = crate::journal::Journal::default();
    let req = request(&r, "unstaged", "discard", "a.txt", &[], &[4]);
    j.discard(&r, &["a.txt".into()], || change(&r, &req))
        .unwrap();
    assert_eq!(disk(&r, "a.txt"), "a\nkeep\nb\n");
    j.step(&r, false, None, &std::sync::Mutex::new(())).unwrap();
    assert_eq!(disk(&r, "a.txt"), "a\nkeep\nb\ngone\n");
}

#[test]
fn a_file_changed_since_it_was_shown_is_left_alone() {
    let (_sb, r) = repo("lines-stale");
    write_commit(&r, "a.txt", "a\n", "base");
    fs::write(r.join("a.txt"), "a\nb\n").unwrap();
    let req = request(&r, "unstaged", "stage", "a.txt", &[], &[2]);
    fs::write(r.join("a.txt"), "a\nb\nc\n").unwrap();
    assert!(change(&r, &req).unwrap_err().contains("changed since"));
    assert_eq!(index(&r, "a.txt"), "a\n");
    let wrong = request(&r, "staged", "stage", "a.txt", &[], &[]);
    assert!(change(&r, &wrong).is_err());
}
