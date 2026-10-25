//! Restore, revert and patches against the files real repositories have: binaries, renames,
//! modes, links, submodules, line endings, odd names, merges in progress and long undo chains.

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

fn commit_all(repo: &Path, msg: &str) {
    run(repo, &["add", "-A"]).unwrap();
    run(repo, &["commit", "-q", "-m", msg]).unwrap();
}

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

/// What Revert This File's Change runs (commands::patch::revert_file), journaled.
fn revert_file(
    j: &Journal,
    repo: &Path,
    sha: &str,
    path: &str,
    old: Option<&str>,
) -> Result<Vec<String>, String> {
    let named: Vec<String> = old
        .map(String::from)
        .into_iter()
        .chain([path.into()])
        .collect();
    let bytes = revert::commit_change(repo, sha, &named)?;
    let paths = patch::touched(&patch::files(repo, &bytes, true)?);
    let list = paths.clone();
    j.replace(
        repo,
        "Revert".into(),
        "before revert",
        &paths,
        &Mutex::new(()),
        move |r: &Path| patch::apply(r, &bytes, true, &list),
    )
}

/// What Apply Patch from Clipboard runs (commands::patch::apply_patch), journaled.
fn apply_patch(j: &Journal, repo: &Path, text: &str) -> Result<Vec<String>, String> {
    let bytes = text.as_bytes().to_vec();
    let paths = patch::touched(&patch::files(repo, &bytes, false)?);
    let list = paths.clone();
    j.replace(
        repo,
        "Apply".into(),
        "before patch",
        &paths,
        &Mutex::new(()),
        move |r: &Path| patch::apply(r, &bytes, false, &list),
    )
}

fn restore_file(j: &Journal, repo: &Path, sha: &str, path: &str) -> Result<Vec<String>, String> {
    let (sha, p) = (sha.to_string(), path.to_string());
    j.replace(
        repo,
        "Restore".into(),
        "before restore",
        &[path.to_string()],
        &Mutex::new(()),
        move |r: &Path| restore_from(r, &sha, &p).map(|_| vec![]),
    )
}

#[cfg(unix)]
fn executable(repo: &Path, path: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(repo.join(path)).unwrap().permissions().mode() & 0o111 != 0
}

// ---- a merge in progress -------------------------------------------------------------------

/// A merge stopped on c.txt, with a.txt free to change.
fn mid_merge(name: &str) -> (Sandbox, PathBuf) {
    let (sb, r) = repo(name);
    write_commit(&r, "c.txt", "1\n", "base");
    write_commit(&r, "a.txt", "a\n", "a");
    run(&r, &["switch", "-q", "-c", "other"]).unwrap();
    write_commit(&r, "c.txt", "other\n", "other");
    run(&r, &["switch", "-q", "main"]).unwrap();
    write_commit(&r, "c.txt", "main\n", "main");
    assert!(run(&r, &["merge", "-q", "other"]).is_err());
    assert!(!run_text(&r, &["ls-files", "-u"]).unwrap().is_empty());
    (sb, r)
}

#[test]
fn a_patch_applied_mid_merge_reports_no_conflicts_it_did_not_make() {
    let (_sb, r) = mid_merge("pe-midmerge-clean");
    let p = "diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -1 +1,2 @@\n a\n+b\n";
    let conflicts = patch::apply(&r, p.as_bytes(), false, &["a.txt".into()]).unwrap();
    assert_eq!(disk(&r, "a.txt"), "a\nb\n");
    assert!(
        conflicts.is_empty(),
        "c.txt was conflicted before the patch, which never touched it: {conflicts:?}"
    );
}

#[test]
fn a_patch_that_fails_mid_merge_is_an_error() {
    let (_sb, r) = mid_merge("pe-midmerge-fail");
    let p = "diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -1 +1,2 @@\n zzz\n+b\n";
    let out = patch::apply(&r, p.as_bytes(), false, &["a.txt".into()]);
    assert_eq!(disk(&r, "a.txt"), "a\n");
    assert!(
        out.is_err(),
        "nothing applied, yet it reads as applied with conflicts: {out:?}"
    );
}

// ---- the index stays as it was -------------------------------------------------------------

#[test]
fn reverting_keeps_a_staged_version_of_the_same_file() {
    let (_sb, r) = repo("pe-staged-same");
    write_commit(&r, "a.txt", "1\n2\n3\n4\n5\n6\n7\n8\n", "base");
    write_commit(&r, "a.txt", "1\nTWO\n3\n4\n5\n6\n7\n8\n", "two");
    let sha = rev(&r, "HEAD");
    fs::write(r.join("a.txt"), "1\nTWO\n3\n4\n5\n6\n7\nstaged\n").unwrap();
    stage(&r, &["a.txt".into()]).unwrap();
    fs::write(r.join("a.txt"), "1\nTWO\n3\n4\n5\n6\n7\nworking\n").unwrap();
    let before = run_text(&r, &["diff", "--cached"]).unwrap();

    let j = Journal::default();
    assert!(revert_file(&j, &r, &sha, "a.txt", None).unwrap().is_empty());
    assert_eq!(disk(&r, "a.txt"), "1\n2\n3\n4\n5\n6\n7\nworking\n");
    assert_eq!(run_text(&r, &["diff", "--cached"]).unwrap(), before);

    let req = RevertLines {
        sha: sha.clone(),
        path: "a.txt".into(),
        old_path: None,
        removed: vec![2],
        added: vec![2],
    };
    fs::write(r.join("a.txt"), "1\nTWO\n3\n4\n5\n6\n7\nworking\n").unwrap();
    assert!(revert::lines(&r, &req).unwrap().is_empty());
    assert_eq!(disk(&r, "a.txt"), "1\n2\n3\n4\n5\n6\n7\nworking\n");
    assert_eq!(run_text(&r, &["diff", "--cached"]).unwrap(), before);
    let none = run_text(&r, &["status", "--porcelain"]).unwrap();
    assert!(!none.contains("index.gitviber"), "no scratch index left");
    let left: Vec<_> = fs::read_dir(r.join(".git"))
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("index.gitviber")
        })
        .collect();
    assert!(left.is_empty(), "scratch indexes are removed");
}

// ---- binaries, renames, modes, links -------------------------------------------------------

#[test]
fn a_binary_file_copies_reverts_and_restores_byte_for_byte() {
    let (_sb, r) = repo("pe-binary");
    let v1: Vec<u8> = (0..=255u8).cycle().take(3000).collect();
    let mut v2 = v1.clone();
    v2[100] = 0;
    v2.extend_from_slice(b"\0tail");
    fs::write(r.join("img.bin"), &v1).unwrap();
    commit_all(&r, "v1");
    let old = rev(&r, "HEAD");
    fs::write(r.join("img.bin"), &v2).unwrap();
    commit_all(&r, "v2");
    let sha = rev(&r, "HEAD");

    // Copy as Patch of a working-tree change, discard it, apply it back.
    let mut v3 = v2.clone();
    v3.push(7);
    fs::write(r.join("img.bin"), &v3).unwrap();
    let p = patch::changes(&r, "unstaged", &["img.bin".into()], None).unwrap();
    assert!(p.contains("GIT binary patch"));
    discard(&r, &["img.bin".into()]).unwrap();
    let preview = patch::preview(&r, &p).unwrap();
    assert_eq!(preview.applies.as_deref(), Some("clean"));
    assert_eq!(preview.files[0].additions, None, "binary");
    let j = Journal::default();
    apply_patch(&j, &r, &p).unwrap();
    assert_eq!(fs::read(r.join("img.bin")).unwrap(), v3);
    step(&j, &r, false).unwrap();
    assert_eq!(fs::read(r.join("img.bin")).unwrap(), v2);

    revert_file(&j, &r, &sha, "img.bin", None).unwrap();
    assert_eq!(fs::read(r.join("img.bin")).unwrap(), v1);
    step(&j, &r, false).unwrap();
    assert_eq!(fs::read(r.join("img.bin")).unwrap(), v2);

    restore_file(&j, &r, &old, "img.bin").unwrap();
    assert_eq!(fs::read(r.join("img.bin")).unwrap(), v1);
    assert_eq!(staged(&r), "");
}

#[test]
fn reverting_a_rename_brings_the_old_name_back() {
    let (_sb, r) = repo("pe-rename");
    let body: String = (1..=20).map(|i| format!("line {i}\n")).collect();
    write_commit(&r, "old name.txt", &body, "base");
    run(&r, &["mv", "old name.txt", "new name.txt"]).unwrap();
    fs::write(r.join("new name.txt"), format!("{body}more\n")).unwrap();
    commit_all(&r, "rename");
    let sha = rev(&r, "HEAD");

    let j = Journal::default();
    revert_file(&j, &r, &sha, "new name.txt", Some("old name.txt")).unwrap();
    assert!(r.join("old name.txt").exists(), "the old name is back");
    assert_eq!(disk(&r, "old name.txt"), body);
    assert!(!r.join("new name.txt").exists());
    step(&j, &r, false).unwrap();
    assert!(
        r.join("new name.txt").exists(),
        "undo brings back the file the revert took away"
    );
    assert!(!r.join("old name.txt").exists());
    assert_eq!(disk(&r, "new name.txt"), format!("{body}more\n"));
    step(&j, &r, true).unwrap();
    assert_eq!(disk(&r, "old name.txt"), body);
    assert!(!r.join("new name.txt").exists());

    // The commit's patch, renamed file and all, applies to its parent.
    run(&r, &["checkout", "-q", "."]).unwrap();
    let mail = patch::commit(&r, &sha).unwrap();
    run(&r, &["reset", "-q", "--hard", "HEAD~1"]).unwrap();
    let preview = patch::preview(&r, &mail).unwrap();
    assert_eq!(preview.files[0].old_path.as_deref(), Some("old name.txt"));
    apply_patch(&j, &r, &mail).unwrap();
    assert_eq!(disk(&r, "new name.txt"), format!("{body}more\n"));
    assert!(!r.join("old name.txt").exists());
}

#[test]
fn a_rename_in_a_patch_lists_both_names() {
    let (_sb, r) = repo("pe-rename-names");
    let body: String = (1..=20).map(|i| format!("line {i}\n")).collect();
    write_commit(&r, "old.txt", &body, "base");
    run(&r, &["mv", "old.txt", "new.txt"]).unwrap();
    fs::write(r.join("new.txt"), format!("{body}more\n")).unwrap();
    commit_all(&r, "rename");
    let sha = rev(&r, "HEAD");
    let named = ["old.txt".to_string(), "new.txt".to_string()];
    let bytes = revert::commit_change(&r, &sha, &named).unwrap();
    assert!(String::from_utf8_lossy(&bytes).contains("rename from old.txt"));
    // Reverting writes both: undo must have both in the journal, and the scratch index both.
    let mut touched = patch::touched(&patch::files(&r, &bytes, true).unwrap());
    touched.sort();
    assert_eq!(touched, ["new.txt", "old.txt"]);
    let files = patch::files(&r, &bytes, false).unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(
        (files[0].status.as_str(), files[0].old_path.as_deref()),
        ("R", Some("old.txt"))
    );
}

#[test]
fn reverting_a_rename_whose_new_file_was_edited_since() {
    let (_sb, r) = repo("pe-rename-edited");
    let body: String = (1..=20).map(|i| format!("line {i}\n")).collect();
    write_commit(&r, "old.txt", &body, "base");
    run(&r, &["mv", "old.txt", "new.txt"]).unwrap();
    commit_all(&r, "rename");
    let sha = rev(&r, "HEAD");
    fs::write(r.join("new.txt"), format!("{body}edited since\n")).unwrap();
    let j = Journal::default();
    let out = revert_file(&j, &r, &sha, "new.txt", Some("old.txt"));
    assert!(out.is_ok(), "{out:?}");
    assert_eq!(disk(&r, "old.txt"), format!("{body}edited since\n"));
    assert!(!r.join("new.txt").exists());
    step(&j, &r, false).unwrap();
    assert_eq!(disk(&r, "new.txt"), format!("{body}edited since\n"));
    assert!(!r.join("old.txt").exists());
}

#[test]
fn copying_selected_lines_of_a_renamed_file_makes_a_patch_git_applies() {
    let (_sb, r) = repo("pe-lines-rename");
    write_commit(&r, "old.txt", "1\n2\n3\n", "base");
    run(&r, &["mv", "old.txt", "new.txt"]).unwrap();
    let req = LinesPatch {
        path: "new.txt".into(),
        old_path: Some("old.txt".into()),
        original: Some("1\n2\n3\n".into()),
        modified: Some("1\nTWO\n3\n".into()),
        removed: vec![2],
        added: vec![2],
    };
    let p = patch::lines(&req).unwrap();
    run(&r, &["reset", "-q", "--hard"]).unwrap();
    // On the original repo state (old.txt there), the patch renames and edits it.
    let preview = patch::preview(&r, &p);
    assert!(
        preview.is_ok(),
        "a lines patch of a rename doesn't parse: {preview:?}\n{p}"
    );
    let preview = preview.unwrap();
    assert!(preview.applies.is_some(), "{:?}\n{p}", preview.error);
}

#[cfg(unix)]
#[test]
fn reverting_a_mode_change_takes_the_executable_bit_back() {
    use std::os::unix::fs::PermissionsExt;
    let (_sb, r) = repo("pe-mode");
    write_commit(&r, "run.sh", "echo hi\n", "base");
    let base = rev(&r, "HEAD");
    fs::set_permissions(r.join("run.sh"), fs::Permissions::from_mode(0o755)).unwrap();
    commit_all(&r, "chmod");
    let sha = rev(&r, "HEAD");
    assert!(executable(&r, "run.sh"));
    let j = Journal::default();
    revert_file(&j, &r, &sha, "run.sh", None).unwrap();
    assert!(!executable(&r, "run.sh"));
    step(&j, &r, false).unwrap();
    assert!(executable(&r, "run.sh"), "undo puts the mode back too");
    restore_file(&j, &r, &base, "run.sh").unwrap();
    assert!(!executable(&r, "run.sh"));

    // A mode-only change copies as a patch and applies.
    run(&r, &["checkout", "-q", "."]).unwrap();
    fs::set_permissions(r.join("run.sh"), fs::Permissions::from_mode(0o644)).unwrap();
    let p = patch::changes(&r, "unstaged", &["run.sh".into()], None).unwrap();
    assert!(p.contains("new mode 100644"));
    run(&r, &["checkout", "-q", "."]).unwrap();
    assert!(executable(&r, "run.sh"));
    patch::apply(&r, p.as_bytes(), false, &["run.sh".into()]).unwrap();
    assert!(!executable(&r, "run.sh"));
}

#[cfg(unix)]
#[test]
fn symlinks_revert_and_restore_as_links() {
    let (_sb, r) = repo("pe-symlink");
    write_commit(&r, "a.txt", "a\n", "base");
    std::os::unix::fs::symlink("a.txt", r.join("link")).unwrap();
    commit_all(&r, "link");
    let sha = rev(&r, "HEAD");
    let j = Journal::default();
    revert_file(&j, &r, &sha, "link", None).unwrap();
    assert!(r.join("link").symlink_metadata().is_err());
    assert_eq!(disk(&r, "a.txt"), "a\n", "the target is left alone");
    step(&j, &r, false).unwrap();
    assert_eq!(fs::read_link(r.join("link")).unwrap(), Path::new("a.txt"));

    // Deleted since: restore brings the link back, not the target's text.
    fs::remove_file(r.join("link")).unwrap();
    restore_file(&j, &r, &sha, "link").unwrap();
    assert_eq!(fs::read_link(r.join("link")).unwrap(), Path::new("a.txt"));
}

#[cfg(unix)]
#[test]
fn a_tracked_link_that_points_out_of_the_repo_can_be_reverted() {
    let (sb, r) = repo("pe-symlink-out");
    fs::write(sb.path("outside.txt"), "secret\n").unwrap();
    std::os::unix::fs::symlink("../outside.txt", r.join("shared")).unwrap();
    commit_all(&r, "base");
    let base = rev(&r, "HEAD");
    fs::remove_file(r.join("shared")).unwrap();
    std::os::unix::fs::symlink("../elsewhere.txt", r.join("shared")).unwrap();
    commit_all(&r, "repoint");
    let sha = rev(&r, "HEAD");
    let j = Journal::default();
    // git writes the link itself, never through it, so the outside file is never at risk.
    let out = revert_file(&j, &r, &sha, "shared", None);
    assert!(out.is_ok(), "{out:?}");
    assert_eq!(
        fs::read_link(r.join("shared")).unwrap(),
        Path::new("../outside.txt")
    );
    assert_eq!(
        fs::read_to_string(sb.path("outside.txt")).unwrap(),
        "secret\n"
    );
    let out = restore_file(&j, &r, &base, "shared");
    assert!(out.is_ok(), "{out:?}");
}

#[cfg(unix)]
#[test]
fn a_patch_through_a_link_out_of_the_repo_is_refused() {
    let (sb, r) = repo("pe-link-out");
    write_commit(&r, "a.txt", "a\n", "base");
    fs::create_dir_all(sb.path("outside")).unwrap();
    std::os::unix::fs::symlink(sb.path("outside"), r.join("ln")).unwrap();
    let p = "diff --git a/ln/x.txt b/ln/x.txt\nnew file mode 100644\n--- /dev/null\n+++ b/ln/x.txt\n@@ -0,0 +1 @@\n+x\n";
    assert!(patch::preview(&r, p).is_err());
    let j = Journal::default();
    assert!(apply_patch(&j, &r, p).is_err());
    assert!(!sb.path("outside/x.txt").exists());
}

// ---- submodules, LFS, line endings ---------------------------------------------------------

#[test]
fn reverting_a_submodule_bump_moves_it_back_or_says_it_cannot() {
    let sb = Sandbox::new("pe-submodule");
    let r = repo_with_submodule(&sb);
    let sub = r.join("sub");
    let old = rev(&sub, "HEAD");
    write_commit(&sub, "l.txt", "l2\n", "lib 2");
    commit_all(&r, "bump");
    let sha = rev(&r, "HEAD");
    let j = Journal::default();
    let out = revert_file(&j, &r, &sha, "sub", None);
    // Either the submodule's checkout went back, or the user hears that it didn't.
    let moved = rev(&sub, "HEAD") == old;
    assert!(
        moved || out.is_err(),
        "reported as reverted, yet the submodule is still at the bump: {out:?}"
    );
}

#[test]
fn line_endings_survive_revert_restore_and_revert_lines() {
    let (_sb, r) = repo("pe-crlf");
    write_commit(&r, ".gitattributes", "*.txt text eol=crlf\n", "attrs");
    write_commit(
        &r,
        "a.txt",
        "1\r\n2\r\n3\r\n4\r\n5\r\n6\r\n7\r\n8\r\n",
        "base",
    );
    let base = rev(&r, "HEAD");
    write_commit(
        &r,
        "a.txt",
        "1\r\nTWO\r\n3\r\n4\r\n5\r\n6\r\n7\r\nEIGHT\r\n",
        "edit",
    );
    let sha = rev(&r, "HEAD");
    assert_eq!(
        disk(&r, "a.txt"),
        "1\r\nTWO\r\n3\r\n4\r\n5\r\n6\r\n7\r\nEIGHT\r\n"
    );
    let j = Journal::default();
    revert_file(&j, &r, &sha, "a.txt", None).unwrap();
    assert_eq!(
        disk(&r, "a.txt"),
        "1\r\n2\r\n3\r\n4\r\n5\r\n6\r\n7\r\n8\r\n"
    );
    step(&j, &r, false).unwrap();

    fs::write(
        r.join("a.txt"),
        "0\r\n1\r\nTWO\r\n3\r\n4\r\n5\r\n6\r\n7\r\nEIGHT\r\n",
    )
    .unwrap();
    let req = RevertLines {
        sha: sha.clone(),
        path: "a.txt".into(),
        old_path: None,
        removed: vec![2],
        added: vec![2],
    };
    assert!(revert::lines(&r, &req).unwrap().is_empty());
    assert_eq!(
        disk(&r, "a.txt"),
        "0\r\n1\r\n2\r\n3\r\n4\r\n5\r\n6\r\n7\r\nEIGHT\r\n"
    );
    // Unedited since: the reverted text keeps its endings too.
    run(&r, &["checkout", "-q", "."]).unwrap();
    assert!(revert::lines(&r, &req).unwrap().is_empty());
    assert_eq!(
        disk(&r, "a.txt"),
        "1\r\n2\r\n3\r\n4\r\n5\r\n6\r\n7\r\nEIGHT\r\n"
    );

    restore_file(&j, &r, &base, "a.txt").unwrap();
    assert_eq!(
        disk(&r, "a.txt"),
        "1\r\n2\r\n3\r\n4\r\n5\r\n6\r\n7\r\n8\r\n"
    );
}

#[test]
fn a_patch_with_crlf_line_endings_says_why_it_does_not_apply() {
    let (_sb, r) = repo("pe-crlf-patch");
    write_commit(&r, "a.txt", "x\n", "base");
    let p =
        "diff --git a/a.txt b/a.txt\r\n--- a/a.txt\r\n+++ b/a.txt\r\n@@ -1 +1,2 @@\r\n x\r\n+y\r\n";
    let preview = patch::preview(&r, p).unwrap();
    assert_eq!(preview.files[0].path, "a.txt", "no \\r in the name");
    if preview.applies.is_none() {
        assert!(preview.error.is_some());
    }
}

#[test]
fn a_file_without_a_final_newline_reverts_by_line_and_copies_lines() {
    let (_sb, r) = repo("pe-no-eol");
    write_commit(&r, "a.txt", "1\n2\n3", "base");
    write_commit(&r, "a.txt", "1\n2\nTHREE", "edit");
    let sha = rev(&r, "HEAD");
    let req = RevertLines {
        sha,
        path: "a.txt".into(),
        old_path: None,
        removed: vec![3],
        added: vec![3],
    };
    assert!(revert::lines(&r, &req).unwrap().is_empty());
    assert_eq!(disk(&r, "a.txt"), "1\n2\n3");

    let lp = LinesPatch {
        path: "a.txt".into(),
        old_path: None,
        original: Some("1\n2\n3".into()),
        modified: Some("1\n2\n3\n4".into()),
        removed: vec![3],
        added: vec![3, 4],
    };
    let p = patch::lines(&lp).unwrap();
    patch::apply(&r, p.as_bytes(), false, &["a.txt".into()]).unwrap();
    assert_eq!(disk(&r, "a.txt"), "1\n2\n3\n4");
}

// ---- deleted files, the root commit, merge commits -----------------------------------------

#[test]
fn a_file_deleted_since_restores_and_a_deletion_reverts() {
    let (_sb, r) = repo("pe-deleted");
    write_commit(&r, "keep.txt", "k\n", "base");
    write_commit(&r, "gone.txt", "g\n", "add");
    let had = rev(&r, "HEAD");
    run(&r, &["rm", "-q", "gone.txt"]).unwrap();
    commit_all(&r, "delete");
    let deletion = rev(&r, "HEAD");

    let j = Journal::default();
    restore_file(&j, &r, &had, "gone.txt").unwrap();
    assert_eq!(disk(&r, "gone.txt"), "g\n");
    step(&j, &r, false).unwrap();
    assert!(!r.join("gone.txt").exists(), "undo takes it away again");

    revert_file(&j, &r, &deletion, "gone.txt", None).unwrap();
    assert_eq!(disk(&r, "gone.txt"), "g\n");
    assert_eq!(staged(&r), "", "back on disk, not staged");
    step(&j, &r, false).unwrap();
    assert!(!r.join("gone.txt").exists());

    // The commit that deleted it has no version to restore: an error, nothing written.
    assert!(restore_file(&j, &r, &deletion, "gone.txt").is_err());
    assert!(!r.join("gone.txt").exists());
    // Reverting a deletion over a file that's back since (untracked): conflict markers, and
    // undo brings the file back as it was.
    fs::write(r.join("gone.txt"), "mine\n").unwrap();
    let conflicts = revert_file(&j, &r, &deletion, "gone.txt", None).unwrap();
    assert_eq!(conflicts, ["gone.txt"]);
    assert!(disk(&r, "gone.txt").contains("mine") && disk(&r, "gone.txt").contains("<<<<<<<"));
    step(&j, &r, false).unwrap();
    assert_eq!(disk(&r, "gone.txt"), "mine\n");
}

#[test]
fn the_root_commit_reverts_restores_and_copies() {
    let (_sb, r) = repo("pe-root");
    write_commit(&r, "a.txt", "1\n2\n", "root");
    let root = rev(&r, "HEAD");
    write_commit(&r, "a.txt", "1\n2\n3\n", "more");
    let j = Journal::default();
    // The root added the file: reverting its change deletes it, conflicting with the edit since.
    let out = revert_file(&j, &r, &root, "a.txt", None);
    assert!(out.is_err() || r.join("a.txt").exists());
    restore_file(&j, &r, &root, "a.txt").unwrap();
    assert_eq!(disk(&r, "a.txt"), "1\n2\n");
    let p = patch::changes(&r, "commit", &["a.txt".into()], Some(&root)).unwrap();
    assert!(p.contains("new file mode"));
    let mail = patch::commit(&r, &root).unwrap();
    assert!(mail.contains("+1"));
    let req = RevertLines {
        sha: root,
        path: "a.txt".into(),
        old_path: None,
        removed: vec![],
        added: vec![2],
    };
    assert!(revert::lines(&r, &req).unwrap().is_empty());
    assert_eq!(disk(&r, "a.txt"), "1\n");
}

#[test]
fn a_merge_commit_reverts_against_its_first_parent() {
    let (_sb, r) = repo("pe-merge-commit");
    write_commit(&r, "a.txt", "a\n", "base");
    run(&r, &["switch", "-q", "-c", "side"]).unwrap();
    write_commit(&r, "s.txt", "side\n", "side");
    run(&r, &["switch", "-q", "main"]).unwrap();
    write_commit(&r, "m.txt", "main\n", "main");
    run(&r, &["merge", "-q", "--no-ff", "-m", "merge side", "side"]).unwrap();
    let merge = rev(&r, "HEAD");
    // The merge brought s.txt in (from main's view): reverting its change removes it.
    let j = Journal::default();
    revert_file(&j, &r, &merge, "s.txt", None).unwrap();
    assert!(!r.join("s.txt").exists());
    step(&j, &r, false).unwrap();
    // main's own file is no change of the merge's.
    assert!(revert_file(&j, &r, &merge, "m.txt", None).is_err());
    let p = patch::changes(&r, "commit", &["s.txt".into()], Some(&merge)).unwrap();
    assert!(p.contains("+side"));
}

#[test]
fn copying_a_merge_commit_as_a_patch_never_copies_another_commit() {
    let (_sb, r) = repo("pe-merge-mail");
    write_commit(&r, "a.txt", "a\n", "base");
    run(&r, &["switch", "-q", "-c", "side"]).unwrap();
    write_commit(&r, "s.txt", "side\n", "side commit");
    run(&r, &["switch", "-q", "main"]).unwrap();
    write_commit(&r, "m.txt", "main\n", "main commit");
    run(&r, &["merge", "-q", "--no-ff", "-m", "merge side", "side"]).unwrap();
    let out = patch::commit(&r, &rev(&r, "HEAD"));
    assert!(
        out.is_err(),
        "a merge commit copied as some other commit: {}",
        out.unwrap_or_default().lines().nth(3).unwrap_or_default()
    );
}

// ---- odd paths -----------------------------------------------------------------------------

const ODD: &[&str] = &[
    "with space.txt",
    "ünïcødé.txt",
    "-leading-dash.txt",
    "$HOME.txt",
    "quo\"te.txt",
    "back\\slash.txt",
    "dir with space/inner $x.txt",
];

#[test]
fn odd_paths_copy_apply_restore_and_revert() {
    for (i, name) in ODD.iter().enumerate() {
        let (_sb, r) = repo(&format!("pe-odd-{i}"));
        write_commit(&r, name, "1\n2\n3\n", "base");
        let base = rev(&r, "HEAD");
        write_commit(&r, name, "1\nTWO\n3\n", "edit");
        let sha = rev(&r, "HEAD");
        let j = Journal::default();

        revert_file(&j, &r, &sha, name, None).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(disk(&r, name), "1\n2\n3\n", "{name}");
        step(&j, &r, false).unwrap();
        restore_file(&j, &r, &base, name).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(disk(&r, name), "1\n2\n3\n", "{name}");
        step(&j, &r, false).unwrap();

        fs::write(r.join(name), "1\nTWO\n3\n4\n").unwrap();
        let p = patch::changes(&r, "unstaged", &[name.to_string()], None)
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        discard(&r, &[name.to_string()]).unwrap();
        let preview = patch::preview(&r, &p).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(preview.files[0].path, *name);
        apply_patch(&j, &r, &p).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(disk(&r, name), "1\nTWO\n3\n4\n", "{name}");

        let lp = LinesPatch {
            path: name.to_string(),
            old_path: None,
            original: Some("1\nTWO\n3\n4\n".into()),
            modified: Some("1\nTWO\n3\n4\n5\n".into()),
            removed: vec![],
            added: vec![5],
        };
        let p = patch::lines(&lp).unwrap();
        let preview = patch::preview(&r, &p);
        assert!(preview.is_ok(), "{name}: {preview:?}\n{p}");
        assert_eq!(preview.unwrap().files[0].path, *name, "{name}\n{p}");
        apply_patch(&j, &r, &p).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(disk(&r, name), "1\nTWO\n3\n4\n5\n", "{name}");

        let req = RevertLines {
            sha: sha.clone(),
            path: name.to_string(),
            old_path: None,
            removed: vec![2],
            added: vec![2],
        };
        revert::lines(&r, &req).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(disk(&r, name), "1\n2\n3\n4\n5\n", "{name}");
    }
}

#[test]
fn a_new_file_with_an_odd_name_shows_as_added() {
    let (_sb, r) = repo("pe-odd-added");
    write_commit(&r, "a.txt", "a\n", "base");
    fs::write(r.join("ünï with space.txt"), "n\n").unwrap();
    fs::write(r.join("quo\"te.txt"), "q\n").unwrap();
    let p = patch::changes(
        &r,
        "unstaged",
        &["ünï with space.txt".into(), "quo\"te.txt".into()],
        None,
    )
    .unwrap();
    fs::remove_file(r.join("ünï with space.txt")).unwrap();
    fs::remove_file(r.join("quo\"te.txt")).unwrap();
    let preview = patch::preview(&r, &p).unwrap();
    for f in &preview.files {
        assert_eq!(f.status, "A", "{} reads as {}", f.path, f.status);
    }
}

// ---- unsafe patches ------------------------------------------------------------------------

fn new_file(path: &str) -> String {
    format!(
        "diff --git a/{path} b/{path}\nnew file mode 100644\n--- /dev/null\n+++ b/{path}\n@@ -0,0 +1 @@\n+x\n"
    )
}

#[test]
fn patches_into_git_internals_or_out_of_the_repo_are_refused() {
    let (sb, r) = repo("pe-unsafe");
    write_commit(&r, "a.txt", "a\n", "base");
    fs::create_dir_all(r.join("sub")).unwrap();
    let j = Journal::default();
    for p in [
        new_file(".GIT/hooks/pre-commit"),
        new_file(".Git/config2"),
        new_file("sub/.git/hooks/x"),
        new_file("sub/../../escape.txt"),
        new_file("sub/../.git/x"),
    ] {
        assert!(patch::preview(&r, &p).is_err(), "previewed:\n{p}");
        assert!(apply_patch(&j, &r, &p).is_err(), "applied:\n{p}");
    }
    // A rename whose old side is in .git: git itself refuses it.
    let config = fs::read(r.join(".git/config")).unwrap();
    let steal = "diff --git a/.git/config b/stolen\nsimilarity index 100%\nrename from .git/config\nrename to stolen\n";
    assert!(apply_patch(&j, &r, steal).is_err());
    assert_eq!(fs::read(r.join(".git/config")).unwrap(), config);
    assert!(!sb.path("escape.txt").exists());
    assert!(!r.join("stolen").exists());
    assert!(!r.join(".git/hooks/pre-commit").exists());
    // An absolute name is taken relative to the repo, as git does: inside it, harmless.
    let abs = "--- /dev/null\n+++ /tmp/abs-gv.txt\n@@ -0,0 +1 @@\n+x\n";
    if let Ok(pv) = patch::preview(&r, abs) {
        assert!(!pv.files[0].path.starts_with('/'));
    }
}

#[test]
fn empty_junk_and_huge_clipboards_are_refused_plainly() {
    let (_sb, r) = repo("pe-junk");
    write_commit(&r, "a.txt", "a\n", "base");
    assert!(patch::preview(&r, "").unwrap_err().contains("empty"));
    assert!(patch::preview(&r, "\n\t \r\n")
        .unwrap_err()
        .contains("empty"));
    assert!(patch::preview(&r, "just some text\nnot a patch\n")
        .unwrap_err()
        .contains("no patch"));
    let mut big = new_file("big.txt");
    big.push_str(&"+y\n".repeat(patch::MAX_BYTES / 3 + 10));
    assert!(patch::preview(&r, &big).unwrap_err().contains("10 MB"));
    // Copying a change over 10 MB is refused the same way.
    fs::write(r.join("big.txt"), "z\n".repeat(patch::MAX_BYTES / 2 + 10)).unwrap();
    assert!(patch::changes(&r, "unstaged", &["big.txt".into()], None)
        .unwrap_err()
        .contains("10 MB"));
}

#[test]
fn an_mbox_of_several_commits_applies_them_all() {
    let (_sb, r) = repo("pe-mbox");
    write_commit(&r, "a.txt", "1\n2\n3\n4\n5\n6\n7\n8\n9\n", "base");
    write_commit(&r, "a.txt", "ONE\n2\n3\n4\n5\n6\n7\n8\n9\n", "first");
    write_commit(&r, "a.txt", "ONE\n2\n3\n4\n5\n6\n7\n8\nNINE\n", "second");
    write_commit(&r, "b.txt", "b\n", "third");
    let mbox = run_text(&r, &["format-patch", "-3", "--stdout"]).unwrap();
    run(&r, &["reset", "-q", "--hard", "HEAD~3"]).unwrap();
    let preview = patch::preview(&r, &mbox).unwrap();
    assert_eq!(preview.applies.as_deref(), Some("clean"));
    let j = Journal::default();
    apply_patch(&j, &r, &mbox).unwrap();
    assert_eq!(disk(&r, "a.txt"), "ONE\n2\n3\n4\n5\n6\n7\n8\nNINE\n");
    assert_eq!(disk(&r, "b.txt"), "b\n");
    step(&j, &r, false).unwrap();
    assert_eq!(disk(&r, "a.txt"), "1\n2\n3\n4\n5\n6\n7\n8\n9\n");
    assert!(!r.join("b.txt").exists());
    // One row per file, though two of its patches touch a.txt.
    let paths: Vec<&str> = preview.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(paths, ["a.txt", "b.txt"], "one row per file");
}

#[test]
fn non_utf8_files_revert_and_say_why_they_do_not_copy() {
    let (_sb, r) = repo("pe-latin1");
    fs::write(r.join("l.txt"), b"caf\xe9\n1\n2\n").unwrap();
    commit_all(&r, "base");
    fs::write(r.join("l.txt"), b"caf\xe9\nONE\n2\n").unwrap();
    commit_all(&r, "edit");
    let sha = rev(&r, "HEAD");
    let j = Journal::default();
    revert_file(&j, &r, &sha, "l.txt", None).unwrap();
    assert_eq!(fs::read(r.join("l.txt")).unwrap(), b"caf\xe9\n1\n2\n");
    assert!(patch::changes(&r, "commit", &["l.txt".into()], Some(&sha))
        .unwrap_err()
        .contains("UTF-8"));
    let req = RevertLines {
        sha,
        path: "l.txt".into(),
        old_path: None,
        removed: vec![2],
        added: vec![2],
    };
    assert!(
        revert::lines(&r, &req).is_err(),
        "lossy text is never written back"
    );
    assert_eq!(fs::read(r.join("l.txt")).unwrap(), b"caf\xe9\n1\n2\n");
}

// ---- odd repositories ----------------------------------------------------------------------

#[test]
fn a_patch_applies_in_a_repo_with_no_commits_and_in_a_linked_worktree() {
    let (sb, r) = repo("pe-unborn");
    let p = new_file("first.txt");
    let j = Journal::default();
    apply_patch(&j, &r, &p).unwrap();
    assert_eq!(disk(&r, "first.txt"), "x\n");
    step(&j, &r, false).unwrap();
    assert!(!r.join("first.txt").exists());

    write_commit(&r, "a.txt", "a\n", "base");
    let wt = sb.path("wt");
    run(
        &r,
        &["worktree", "add", "-q", "-b", "wt", wt.to_str().unwrap()],
    )
    .unwrap();
    let p = "diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -1 +1,2 @@\n a\n+b\n";
    assert_eq!(
        patch::preview(&wt, p).unwrap().applies.as_deref(),
        Some("clean")
    );
    apply_patch(&j, &wt, p).unwrap();
    assert_eq!(disk(&wt, "a.txt"), "a\nb\n");
    assert_eq!(disk(&r, "a.txt"), "a\n", "the main checkout is untouched");
    assert_eq!(staged(&wt), "");
}

// ---- undo and redo chains ------------------------------------------------------------------

#[test]
fn undo_and_redo_walk_a_chain_of_discard_restore_apply_and_revert() {
    let (_sb, r) = repo("pe-chain");
    write_commit(&r, "a.txt", "a1\n", "a");
    write_commit(&r, "b.txt", "b1\n", "b");
    let b1 = rev(&r, "HEAD");
    write_commit(&r, "b.txt", "b2\n", "b2");
    write_commit(&r, "d.txt", "1\n2\n3\n", "d");
    write_commit(&r, "d.txt", "1\nTWO\n3\n", "d2");
    let d2 = rev(&r, "HEAD");
    fs::write(r.join("a.txt"), "a dirty\n").unwrap();
    let j = Journal::default();

    let list = vec!["a.txt".to_string()];
    let write = move |r: &Path| discard(r, &list).map(|_| vec![]);
    journaled(&j, &r, &["a.txt".into()], write);
    restore_file(&j, &r, &b1, "b.txt").unwrap();
    apply_patch(&j, &r, &new_file("c.txt")).unwrap();
    revert_file(&j, &r, &d2, "d.txt", None).unwrap();

    let state = |r: &Path| {
        (
            disk(r, "a.txt"),
            disk(r, "b.txt"),
            r.join("c.txt").exists(),
            disk(r, "d.txt"),
        )
    };
    let done = state(&r);
    assert_eq!(
        done,
        ("a1\n".into(), "b1\n".into(), true, "1\n2\n3\n".into())
    );
    for _ in 0..4 {
        step(&j, &r, false).unwrap();
    }
    assert_eq!(
        state(&r),
        (
            "a dirty\n".into(),
            "b2\n".into(),
            false,
            "1\nTWO\n3\n".into()
        )
    );
    for _ in 0..4 {
        step(&j, &r, true).unwrap();
    }
    assert_eq!(state(&r), done);
    assert_eq!(staged(&r), "");
}

#[test]
fn undoing_a_conflicted_revert_takes_the_markers_away() {
    let (_sb, r) = repo("pe-undo-conflict");
    write_commit(&r, "a.txt", "1\n2\n3\n", "base");
    write_commit(&r, "a.txt", "1\nTWO\n3\n", "edit");
    let sha = rev(&r, "HEAD");
    fs::write(r.join("a.txt"), "1\nTwo!\n3\n").unwrap();
    let j = Journal::default();
    assert_eq!(revert_file(&j, &r, &sha, "a.txt", None).unwrap(), ["a.txt"]);
    assert!(disk(&r, "a.txt").contains("<<<<<<<"));
    assert!(run_text(&r, &["ls-files", "-u"]).unwrap().is_empty());
    step(&j, &r, false).unwrap();
    assert_eq!(disk(&r, "a.txt"), "1\nTwo!\n3\n");
    step(&j, &r, true).unwrap();
    assert!(disk(&r, "a.txt").contains("<<<<<<<"));
}

#[test]
fn a_large_file_copies_selected_lines_and_reverts_by_line() {
    let (_sb, r) = repo("pe-large");
    let old: String = (0..30_000).map(|i| format!("line {i}\n")).collect();
    let new = old
        .replace("line 29990\n", "LINE 29990\n")
        .replace("line 5\n", "LINE 5\n");
    write_commit(&r, "big.txt", &old, "base");
    write_commit(&r, "big.txt", &new, "edit");
    let sha = rev(&r, "HEAD");
    let req = RevertLines {
        sha,
        path: "big.txt".into(),
        old_path: None,
        removed: vec![29991],
        added: vec![29991],
    };
    let started = std::time::Instant::now();
    assert!(revert::lines(&r, &req).unwrap().is_empty());
    assert_eq!(disk(&r, "big.txt"), old.replace("line 5\n", "LINE 5\n"));
    let lp = LinesPatch {
        path: "big.txt".into(),
        old_path: None,
        original: Some(old.clone()),
        modified: Some(new.clone()),
        removed: vec![6],
        added: vec![6],
    };
    let p = patch::lines(&lp).unwrap();
    assert!(p.len() < 1000, "one small hunk, not the whole file");
    assert!(started.elapsed().as_secs() < 10);
}

// ---- comparing -----------------------------------------------------------------------------

#[test]
fn the_working_tree_compared_with_a_commit_leaves_ignored_files_out() {
    let (_sb, r) = repo("pe-compare-wt");
    write_commit(&r, ".gitignore", "*.log\n", "ignore");
    write_commit(&r, "a.txt", "a\n", "base");
    let base = rev(&r, "HEAD");
    fs::write(r.join("debug.log"), "noise\n").unwrap();
    fs::write(r.join("new file.txt"), "n\n").unwrap();
    fs::write(r.join("a.txt"), "a\nb\n").unwrap();
    let review = worktree_review(&r, &base).unwrap();
    let rows: Vec<(&str, &str)> = review
        .files
        .iter()
        .map(|f| (f.path.as_str(), f.status.as_str()))
        .collect();
    assert_eq!(rows, [("a.txt", "M"), ("new file.txt", "?")]);
}

#[cfg(unix)]
#[test]
fn two_files_compare_the_same_a_binary_a_folder_and_a_link_out() {
    let (sb, r) = repo("pe-two-files");
    fs::write(r.join("a.txt"), "1\n2\n").unwrap();
    fs::write(r.join("bin"), b"\0\x01\x02").unwrap();
    fs::create_dir_all(r.join("dir")).unwrap();
    fs::write(sb.path("outside.txt"), "private words\n").unwrap();
    std::os::unix::fs::symlink(sb.path("outside.txt"), r.join("out")).unwrap();
    let pair = |old: &str, new: &str| {
        diff_pair(&r, "files", new, Some(old), None, None, None, |p| {
            vfs::read_diff_side(&r, p)
        })
        .unwrap()
    };
    let same = pair("a.txt", "a.txt");
    assert!(same.rows.iter().all(|row| row.k == 0));
    assert!(pair("a.txt", "bin").modified.binary);
    let folder = pair("a.txt", "dir");
    assert!(!folder.modified.exists || folder.modified.text.is_empty());
    let out = pair("a.txt", "out");
    assert!(
        !out.modified.text.contains("private words"),
        "a link's target path at most, never the outside file's text"
    );
}

#[test]
fn a_patch_over_a_thousand_files_applies_and_undoes() {
    let (_sb, r) = repo("pe-many");
    for i in 0..1000 {
        let dir = format!("d{}", i % 20);
        fs::create_dir_all(r.join(&dir)).unwrap();
        fs::write(r.join(format!("{dir}/f{i}.txt")), format!("{i}\n")).unwrap();
    }
    commit_all(&r, "many");
    for i in 0..1000 {
        fs::write(
            r.join(format!("d{}/f{i}.txt", i % 20)),
            format!("{i}\nmore\n"),
        )
        .unwrap();
    }
    let paths: Vec<String> = (0..1000).map(|i| format!("d{}/f{i}.txt", i % 20)).collect();
    let p = patch::changes(&r, "unstaged", &paths, None).unwrap();
    run(&r, &["checkout", "-q", "."]).unwrap();
    let started = std::time::Instant::now();
    let preview = patch::preview(&r, &p).unwrap();
    assert_eq!(preview.files.len(), 1000);
    let j = Journal::default();
    apply_patch(&j, &r, &p).unwrap();
    assert_eq!(disk(&r, "d7/f987.txt"), "987\nmore\n");
    step(&j, &r, false).unwrap();
    assert_eq!(disk(&r, "d7/f987.txt"), "987\n");
    assert!(started.elapsed().as_secs() < 30, "{:?}", started.elapsed());
}
