//! Patches copied and applied, and a past commit's file or change put back in the working tree.

use super::*;
use crate::journal::files_label;
use crate::patch::{self, LinesPatch};
use crate::revert::{self, RevertLines};

fn repo(name: &str) -> (Sandbox, PathBuf) {
    let sb = Sandbox::new(name);
    let r = sb.path("r");
    init(&r);
    (sb, r)
}

fn disk(repo: &Path, path: &str) -> String {
    fs::read_to_string(repo.join(path)).unwrap()
}

fn staged(repo: &Path) -> String {
    run_text(repo, &["diff", "--cached", "--name-only"]).unwrap()
}

/// `write` through the journal, as the commands run it.
fn journaled(
    j: &Journal,
    repo: &Path,
    paths: &[String],
    write: impl Fn(&Path) -> Result<Vec<String>, String> + Send + Sync + 'static,
) -> Vec<String> {
    j.replace(
        repo,
        files_label("Change", paths),
        "before test",
        paths,
        &Mutex::new(()),
        write,
    )
    .unwrap()
}

#[test]
fn restoring_a_past_version_leaves_the_index_and_undoes() {
    let (_sb, r) = repo("p-restore");
    write_commit(&r, "a.txt", "one\n", "base");
    let old = rev(&r, "HEAD");
    write_commit(&r, "a.txt", "two\n", "edit");
    fs::write(r.join("a.txt"), "agent's work\n").unwrap();
    let j = Journal::default();
    let paths = vec!["a.txt".to_string()];
    let sha = old.clone();
    journaled(&j, &r, &paths, move |r: &Path| {
        restore_from(r, &sha, "a.txt").map(|_| vec![])
    });
    assert_eq!(disk(&r, "a.txt"), "one\n");
    assert_eq!(staged(&r), "", "the index stays");
    step(&j, &r, false).unwrap();
    assert_eq!(disk(&r, "a.txt"), "agent's work\n");
    step(&j, &r, true).unwrap();
    assert_eq!(disk(&r, "a.txt"), "one\n");
    assert!(restore_from(&r, &old, "../x").is_err());
    assert!(restore_from(&r, &old, ".git/config").is_err());
}

#[test]
fn reverting_a_commits_change_to_a_file_keeps_later_edits() {
    let (_sb, r) = repo("p-revert");
    write_commit(&r, "a.txt", "1\n2\n3\n4\n5\n6\n7\n8\n", "base");
    write_commit(&r, "a.txt", "1\nTWO\n3\n4\n5\n6\n7\n8\n", "edit two");
    let sha = rev(&r, "HEAD");
    // Edited since, far from the commit's change, and with something staged elsewhere.
    fs::write(r.join("a.txt"), "1\nTWO\n3\n4\n5\n6\n7\nEIGHT\n").unwrap();
    write_commit(&r, "b.txt", "b\n", "b");
    fs::write(r.join("b.txt"), "b staged\n").unwrap();
    stage(&r, &["b.txt".into()]).unwrap();

    let paths = vec!["a.txt".to_string()];
    let change = revert::commit_change(&r, &sha, &paths).unwrap();
    let touched = patch::touched(&patch::files(&r, &change, true).unwrap());
    assert_eq!(touched, paths);
    let j = Journal::default();
    let list = touched.clone();
    let conflicts = journaled(&j, &r, &touched, move |r: &Path| {
        patch::apply(r, &change, true, &list)
    });
    assert!(conflicts.is_empty());
    assert_eq!(disk(&r, "a.txt"), "1\n2\n3\n4\n5\n6\n7\nEIGHT\n");
    assert_eq!(staged(&r).trim(), "b.txt", "the index is as it was");
    step(&j, &r, false).unwrap();
    assert_eq!(disk(&r, "a.txt"), "1\nTWO\n3\n4\n5\n6\n7\nEIGHT\n");

    // The line itself edited since: a conflict, marked in the file and not staged.
    fs::write(r.join("a.txt"), "1\nTwo!\n3\n4\n5\n6\n7\n8\n").unwrap();
    let change = revert::commit_change(&r, &sha, &paths).unwrap();
    let conflicts = patch::apply(&r, &change, true, &paths).unwrap();
    assert_eq!(conflicts, paths);
    assert!(disk(&r, "a.txt").contains("<<<<<<<"));
    assert_eq!(staged(&r).trim(), "b.txt");
    assert!(run_text(&r, &["ls-files", "-u"]).unwrap().is_empty());
}

#[test]
fn reverting_an_added_file_deletes_it_and_undo_brings_it_back() {
    let (_sb, r) = repo("p-revert-add");
    write_commit(&r, "a.txt", "a\n", "base");
    write_commit(&r, "new.txt", "new\n", "add");
    let sha = rev(&r, "HEAD");
    let paths = vec!["new.txt".to_string()];
    let change = revert::commit_change(&r, &sha, &paths).unwrap();
    let j = Journal::default();
    let list = paths.clone();
    journaled(&j, &r, &paths, move |r: &Path| {
        patch::apply(r, &change, true, &list)
    });
    assert!(!r.join("new.txt").exists());
    step(&j, &r, false).unwrap();
    assert_eq!(disk(&r, "new.txt"), "new\n");
    // A root commit has no parent to diff with: the empty tree stands in.
    let root = rev(&r, "HEAD~1");
    assert!(revert::commit_change(&r, &root, &["a.txt".into()]).is_ok());
}

#[test]
fn reverting_some_lines_of_a_commit_merges_into_the_working_tree() {
    let (_sb, r) = repo("p-revert-lines");
    write_commit(&r, "a.txt", "1\n2\n3\n4\n5\n6\n7\n8\n9\n", "base");
    write_commit(&r, "a.txt", "1\nTWO\n3\n4\n5\n6\n7\nEIGHT\n9\n", "edit");
    let sha = rev(&r, "HEAD");
    // Edited since, apart from both changes.
    fs::write(r.join("a.txt"), "0\n1\nTWO\n3\n4\n5\n6\n7\nEIGHT\n9\n").unwrap();
    // Only 2 → TWO: removed line 2 of the parent, added line 2 of the commit.
    let req = RevertLines {
        sha: sha.clone(),
        path: "a.txt".into(),
        old_path: None,
        removed: vec![2],
        added: vec![2],
    };
    let j = Journal::default();
    let ask = req.clone();
    let conflicts = journaled(&j, &r, &["a.txt".into()], move |r: &Path| {
        revert::lines(r, &ask)
    });
    assert!(conflicts.is_empty());
    assert_eq!(disk(&r, "a.txt"), "0\n1\n2\n3\n4\n5\n6\n7\nEIGHT\n9\n");
    step(&j, &r, false).unwrap();
    assert_eq!(disk(&r, "a.txt"), "0\n1\nTWO\n3\n4\n5\n6\n7\nEIGHT\n9\n");
    step(&j, &r, true).unwrap();
    assert_eq!(disk(&r, "a.txt"), "0\n1\n2\n3\n4\n5\n6\n7\nEIGHT\n9\n");

    // The same line changed again since: conflict markers, labelled with the file.
    fs::write(r.join("a.txt"), "1\nTwo!\n3\n4\n5\n6\n7\nEIGHT\n9\n").unwrap();
    assert_eq!(revert::lines(&r, &req).unwrap(), ["a.txt"]);
    assert!(disk(&r, "a.txt").contains("<<<<<<< a.txt"));
    let none = RevertLines {
        removed: vec![],
        added: vec![],
        ..req
    };
    assert!(revert::lines(&r, &none).is_err());
}

#[test]
fn a_copied_patch_applies_back_after_a_discard() {
    let (_sb, r) = repo("p-roundtrip");
    write_commit(&r, "a.txt", "a\n", "base");
    write_commit(&r, "b.txt", "b\n", "base");
    fs::write(r.join("a.txt"), "a\nmore\n").unwrap();
    fs::write(r.join("new file.txt"), "new\n").unwrap();
    fs::write(r.join("b.txt"), "b staged\n").unwrap();
    stage(&r, &["b.txt".into()]).unwrap();

    let unstaged = patch::changes(
        &r,
        "unstaged",
        &["a.txt".into(), "new file.txt".into()],
        None,
    )
    .unwrap();
    assert!(unstaged.contains("+more"));
    assert!(unstaged.contains("new file mode"));
    let cached = patch::changes(&r, "staged", &["b.txt".into()], None).unwrap();
    assert!(cached.contains("+b staged"));
    assert!(
        patch::changes(&r, "staged", &["a.txt".into()], None).is_err(),
        "nothing to copy"
    );

    discard(&r, &["a.txt".into()]).unwrap();
    fs::remove_file(r.join("new file.txt")).unwrap();
    let preview = patch::preview(&r, &unstaged).unwrap();
    assert_eq!(preview.applies.as_deref(), Some("clean"));
    let statuses: Vec<(&str, &str)> = preview
        .files
        .iter()
        .map(|f| (f.path.as_str(), f.status.as_str()))
        .collect();
    assert_eq!(statuses, [("a.txt", "M"), ("new file.txt", "A")]);
    assert_eq!(preview.files[0].additions, Some(1));
    let paths = patch::touched(&preview.files);
    patch::apply(&r, unstaged.as_bytes(), false, &paths).unwrap();
    assert_eq!(disk(&r, "a.txt"), "a\nmore\n");
    assert_eq!(disk(&r, "new file.txt"), "new\n");
    assert_eq!(staged(&r).trim(), "b.txt");
}

#[test]
fn a_patch_whose_context_moved_needs_a_merge() {
    let (_sb, r) = repo("p-merge");
    write_commit(&r, "a.txt", "1\n2\n3\n4\n5\n6\n7\n8\n9\n", "base");
    write_commit(&r, "a.txt", "1\n2\n3\n4\nFIVE\n6\n7\n8\n9\n", "edit");
    let p = patch::commit(&r, &rev(&r, "HEAD")).unwrap();
    assert!(p.starts_with("From "), "format-patch's mbox");
    run(&r, &["reset", "-q", "--hard", "HEAD~1"]).unwrap();
    // Moved down a line: git finds the context there.
    fs::write(r.join("a.txt"), "0\n1\n2\n3\n4\n5\n6\n7\n8\n9\n").unwrap();
    assert_eq!(
        patch::preview(&r, &p).unwrap().applies.as_deref(),
        Some("clean")
    );
    fs::write(r.join("a.txt"), "1\n2\n3\n4\n5!\n6\n7\n8\n9\n").unwrap();
    let preview = patch::preview(&r, &p).unwrap();
    assert_eq!(preview.applies.as_deref(), Some("merge"));
    assert_eq!(
        patch::apply(&r, p.as_bytes(), false, &["a.txt".into()]).unwrap(),
        ["a.txt"]
    );
    // Nothing applies to a file that isn't there, and the clipboard's junk is said to be junk.
    fs::remove_file(r.join("a.txt")).unwrap();
    run(&r, &["rm", "-q", "--cached", "a.txt"]).unwrap();
    let preview = patch::preview(&r, &p).unwrap();
    assert!(preview.applies.is_none() && preview.error.is_some());
    assert!(patch::preview(&r, "hello")
        .unwrap_err()
        .contains("no patch"));
    assert!(patch::preview(&r, " \n").is_err());
}

#[test]
fn a_patch_into_the_git_dir_or_outside_is_refused() {
    let (_sb, r) = repo("p-unsafe");
    write_commit(&r, "a.txt", "a\n", "base");
    let into_git = "diff --git a/.git/hooks/pre-commit b/.git/hooks/pre-commit\nnew file mode 100755\n--- /dev/null\n+++ b/.git/hooks/pre-commit\n@@ -0,0 +1 @@\n+echo pwned\n";
    assert!(patch::preview(&r, into_git).is_err());
    assert!(!r.join(".git/hooks/pre-commit").exists());
    let outside = "--- /dev/null\n+++ b/../escape.txt\n@@ -0,0 +1 @@\n+x\n";
    assert!(patch::preview(&r, outside).is_err());
    let huge = "x".repeat(patch::MAX_BYTES + 1);
    assert!(patch::preview(&r, &huge).unwrap_err().contains("10 MB"));
}

#[test]
fn selected_lines_copy_as_a_patch_git_applies() {
    let (_sb, r) = repo("p-lines");
    write_commit(&r, "a.txt", "1\n2\n3\n4\n5\n6\n7\n8\n9\n", "base");
    let req = LinesPatch {
        path: "a.txt".into(),
        old_path: None,
        original: Some("1\n2\n3\n4\n5\n6\n7\n8\n9\n".into()),
        modified: Some("1\nTWO\n3\n4\n5\n6\n7\nEIGHT\n9\n".into()),
        removed: vec![2],
        added: vec![2],
    };
    let p = patch::lines(&req).unwrap();
    assert!(p.starts_with("diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n"));
    patch::apply(&r, p.as_bytes(), false, &["a.txt".into()]).unwrap();
    assert_eq!(disk(&r, "a.txt"), "1\nTWO\n3\n4\n5\n6\n7\n8\n9\n");
    let none = LinesPatch {
        removed: vec![],
        added: vec![],
        ..req
    };
    assert!(patch::lines(&none).is_err());
}

#[test]
fn a_stash_copies_with_its_untracked_files() {
    let (_sb, r) = repo("p-stash");
    write_commit(&r, "a.txt", "a\n", "base");
    fs::write(r.join("a.txt"), "a\nb\n").unwrap();
    fs::write(r.join("u.txt"), "u\n").unwrap();
    run(&r, &["stash", "push", "-q", "-u"]).unwrap();
    let p = patch::stash(&r, &rev(&r, "stash@{0}")).unwrap();
    assert!(p.contains("+b") && p.contains("+++ b/u.txt"));
}

#[test]
fn no_copy_of_the_index_or_merge_side_stays_in_the_git_dir() {
    let (_sb, r) = repo("p-scratch");
    write_commit(&r, "a.txt", "1\n2\n3\n4\n5\n6\n7\n8\n9\n", "base");
    write_commit(&r, "a.txt", "1\n2\n3\n4\nFIVE\n6\n7\n8\n9\n", "edit");
    let sha = rev(&r, "HEAD");
    let p = patch::commit(&r, &sha).unwrap();
    // Context moved: the preview and the apply both need the copy, and a merge.
    fs::write(r.join("a.txt"), "1\n2\n3\n4\n5!\n6\n7\n8\n9\n").unwrap();
    fs::write(r.join("new.txt"), "n\n").unwrap();
    patch::preview(&r, &p).unwrap();
    patch::apply(&r, p.as_bytes(), false, &["a.txt".into()]).unwrap();
    // Fails: nothing to apply to.
    fs::remove_file(r.join("a.txt")).unwrap();
    assert!(patch::apply(&r, p.as_bytes(), false, &["a.txt".into()]).is_err());
    assert!(patch::preview(&r, &p).unwrap().applies.is_none());
    patch::changes(&r, "unstaged", &["new.txt".into()], None).unwrap();
    // Revert some lines into a file edited since: merge-file's sides.
    run(&r, &["checkout", "-q", "a.txt"]).unwrap();
    fs::write(r.join("a.txt"), "0\n1\n2\n3\n4\nFIVE\n6\n7\n8\n9\n").unwrap();
    let req = RevertLines {
        sha,
        path: "a.txt".into(),
        old_path: None,
        removed: vec![5],
        added: vec![5],
    };
    revert::lines(&r, &req).unwrap();
    assert_eq!(disk(&r, "a.txt"), "0\n1\n2\n3\n4\n5\n6\n7\n8\n9\n");
    let left: Vec<_> = fs::read_dir(r.join(".git"))
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains(".gitviber."))
        .collect();
    assert!(left.is_empty(), "{left:?}");
}
