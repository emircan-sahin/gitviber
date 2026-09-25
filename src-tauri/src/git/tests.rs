use super::*;
use std::fs;
use std::path::{Path, PathBuf};

fn temp_repo(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("gitviber-test-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    for args in [
        vec!["init", "-q", "-b", "main"],
        vec!["config", "user.name", "Test"],
        vec!["config", "user.email", "test@example.com"],
        vec!["config", "commit.gpgsign", "false"],
    ] {
        run(&dir, &args).unwrap();
    }
    dir
}

#[test]
fn status_history_and_diffs() {
    let repo = temp_repo("status");
    fs::write(repo.join("a.txt"), "one\ntwo\n").unwrap();
    fs::write(repo.join("old name.txt"), "rename me\n").unwrap();
    stage(&repo, &["a.txt".into(), "old name.txt".into()]).unwrap();
    commit(&repo, "first\n\nbody line", &CommitOptions::default()).unwrap();

    fs::write(repo.join("a.txt"), "one\nTWO\nthree\n").unwrap();
    fs::rename(repo.join("old name.txt"), repo.join("new name.txt")).unwrap();
    stage(&repo, &["old name.txt".into(), "new name.txt".into()]).unwrap();
    fs::create_dir_all(repo.join("dir")).unwrap();
    fs::write(repo.join("dir/new.rs"), "fn main() {}\n").unwrap();

    let st = status(&repo).unwrap();
    assert_eq!(st.branch.as_deref(), Some("main"));
    let renamed = st
        .staged
        .iter()
        .find(|f| f.status == "R")
        .expect("rename staged");
    assert_eq!(renamed.path, "new name.txt");
    assert_eq!(renamed.old_path.as_deref(), Some("old name.txt"));
    let a = st.unstaged.iter().find(|f| f.path == "a.txt").unwrap();
    assert_eq!(
        (a.status.as_str(), a.additions, a.deletions),
        ("M", Some(2), Some(1))
    );
    let untracked = st.unstaged.iter().find(|f| f.path == "dir/new.rs").unwrap();
    assert_eq!(
        (untracked.status.as_str(), untracked.additions),
        ("?", Some(1))
    );

    let pair = diff_pair(&repo, "unstaged", "a.txt", None, None, None, None, |p| {
        to_file_text(fs::read(repo.join(p)).unwrap())
    })
    .unwrap();
    assert_eq!(pair.original.text, "one\ntwo\n");
    assert_eq!(pair.modified.text, "one\nTWO\nthree\n");
    let pair = diff_pair(
        &repo,
        "staged",
        "new name.txt",
        Some("old name.txt"),
        None,
        None,
        None,
        |_| FileText::default(),
    )
    .unwrap();
    assert_eq!(
        (pair.original.text.as_str(), pair.modified.text.as_str()),
        ("rename me\n", "rename me\n")
    );

    commit(&repo, "second", &CommitOptions::default()).unwrap();
    let commits = log(&repo, None, 0, 10).unwrap();
    assert_eq!(commits.len(), 2);
    assert_eq!(commits[1].body, "body line");
    assert!(commits[0].refs.iter().any(|r| r == "HEAD -> main"));

    let files = commit_files(&repo, &commits[0].sha).unwrap();
    assert!(files
        .iter()
        .any(|f| f.status == "R" && f.old_path.as_deref() == Some("old name.txt")));
    let root_files = commit_files(&repo, &commits[1].sha).unwrap();
    assert_eq!(root_files.len(), 2);

    assert_eq!(ignored(&repo, &["a.txt".into()]), Vec::<String>::new());
    fs::write(repo.join(".gitignore"), "target/\n").unwrap();
    assert_eq!(
        ignored(&repo, &["target/".into(), "a.txt".into()]),
        vec!["target".to_string()]
    );

    assert!(switch_branch(&repo, "--evil", true).is_err());
    switch_branch(&repo, "feat/x", true).unwrap();
    assert!(branches(&repo)
        .unwrap()
        .iter()
        .any(|b| b.name == "feat/x" && b.current));

    switch_branch(&repo, "main", false).unwrap();
    assert!(add_worktree(&repo, "--evil", None, None).is_err());
    let wt = add_worktree(&repo, "feat/x", None, None).unwrap();
    // git reports real paths: /var/folders is /private/var/folders on macOS.
    let real = repo.canonicalize().unwrap();
    let expected = real.with_file_name(format!(
        "{}.worktrees",
        real.file_name().unwrap().to_string_lossy()
    ));
    assert_eq!(Path::new(&wt), expected.join("feat-x"));
    assert!(worktrees(&repo)
        .unwrap()
        .iter()
        .any(|w| w.branch.as_deref() == Some("feat/x")));
    assert!(add_worktree(&repo, "feat/x", None, None).is_err());
    let _ = fs::remove_dir_all(&repo);
    let _ = fs::remove_dir_all(&expected);
}

#[test]
fn worktree_new_branch_rename_and_prune() {
    let repo = temp_repo("worktrees");
    commit_file(&repo, "a.txt", "a\n", "first");
    // Canonical, as git reports it: /var/folders is /private/var/folders on macOS.
    let tmp = std::env::temp_dir().canonicalize().unwrap();
    let dir = tmp.join(format!("gitviber-test-wtdir-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let d = dir.to_str().unwrap();
    let listed = |branch: &str| {
        worktrees(&repo)
            .unwrap()
            .into_iter()
            .find(|w| w.branch.as_deref() == Some(branch))
    };

    // A new branch, made in a chosen folder, tracking nothing.
    assert!(add_worktree(&repo, "x", Some("main"), Some(d)).is_err());
    assert!(add_worktree(&repo, "x", Some("refs/heads/main"), Some("rel")).is_err());
    let wt = add_worktree(&repo, "feat/new", Some("refs/heads/main"), Some(d)).unwrap();
    assert_eq!(Path::new(&wt), dir.join("feat-new"));
    assert_eq!(listed("feat/new").unwrap().path, wt);
    let upstream = run_text(
        &repo,
        &[
            "for-each-ref",
            "--format=%(upstream)",
            "refs/heads/feat/new",
        ],
    );
    assert_eq!(upstream.unwrap().trim(), "");
    // From a commit, by its full id only.
    let first = run_text(&repo, &["rev-parse", "HEAD"]).unwrap();
    let first = first.trim();
    assert!(add_worktree(&repo, "at", Some(&first[..12]), Some(d)).is_err());
    assert!(add_worktree(&repo, "at", Some(&"0".repeat(40)), Some(d)).is_err());
    let at = add_worktree(&repo, "at/first", Some(first), Some(d)).unwrap();
    let head = run_text(Path::new(&at), &["rev-parse", "HEAD"]).unwrap();
    assert_eq!(head.trim(), first);
    remove_worktree(&repo, &listed("at/first").unwrap().path, false).unwrap();

    // Branch and folder together.
    let moved = rename_worktree(&repo, &wt, "feat/renamed", true).unwrap();
    assert_eq!(Path::new(&moved), dir.join("feat-renamed"));
    assert!(!Path::new(&wt).exists());
    assert_eq!(listed("feat/renamed").unwrap().path, moved);
    // A taken name changes nothing.
    assert!(rename_worktree(&repo, &moved, "main", true).is_err());
    assert!(Path::new(&moved).exists() && listed("feat/renamed").is_some());
    // Locked with a reason, it keeps its folder and its branch; unlocked, it moves again.
    assert!(lock_worktree(&repo, &moved, Some("usb\ndrive")).is_err());
    lock_worktree(&repo, &moved, Some("  --on the usb drive ")).unwrap();
    let w = listed("feat/renamed").unwrap();
    assert!(w.locked);
    assert_eq!(w.lock_reason.as_deref(), Some("--on the usb drive"));
    assert!(rename_worktree(&repo, &moved, "feat/locked", true).is_err());
    assert!(listed("feat/renamed").is_some());
    unlock_worktree(&repo, &moved).unwrap();
    assert!(!listed("feat/renamed").unwrap().locked);
    lock_worktree(&repo, &moved, None).unwrap();
    let w = listed("feat/renamed").unwrap();
    assert!(w.locked && w.lock_reason.is_none());
    unlock_worktree(&repo, &moved).unwrap();
    assert!(unlock_worktree(&repo, "/not/a/worktree").is_err());

    // Only the case changes: on a case-insensitive disk the new folder name is "taken" by the old.
    let upper = add_worktree(&repo, "Case", Some("refs/heads/main"), Some(d)).unwrap();
    let lower = rename_worktree(&repo, &upper, "case", true).unwrap();
    assert_eq!(Path::new(&lower), dir.join("case"));
    assert_eq!(listed("case").unwrap().path, lower);
    let names: Vec<_> = fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(names.contains(&"case".to_string()) && !names.contains(&"Case".to_string()));
    // The branch renamed alone, the folder catches up later under the same name.
    assert_eq!(
        rename_worktree(&repo, &lower, "later", false).unwrap(),
        lower
    );
    let caught_up = rename_worktree(&repo, &lower, "later", true).unwrap();
    assert_eq!(Path::new(&caught_up), dir.join("later"));
    assert_eq!(listed("later").unwrap().path, caught_up);

    // The main worktree's folder stays; its branch can still be renamed. It can't be locked.
    let main = listed("main").unwrap().path;
    assert!(lock_worktree(&repo, &main, None).is_err());
    assert!(rename_worktree(&repo, &main, "trunk", true).is_err());
    assert_eq!(rename_worktree(&repo, &main, "trunk", false).unwrap(), main);
    assert!(listed("trunk").is_some_and(|w| w.main));

    // A worktree whose folder is gone is pruned by removing it.
    let gone = add_worktree(&repo, "gone", Some("refs/heads/trunk"), Some(d)).unwrap();
    fs::remove_dir_all(&gone).unwrap();
    assert!(listed("gone").unwrap().prunable);
    remove_worktree(&repo, &gone, false).unwrap();
    assert!(listed("gone").is_none());

    let _ = fs::remove_dir_all(&repo);
    let _ = fs::remove_dir_all(&dir);
}

fn commit_file(repo: &Path, path: &str, content: &str, msg: &str) {
    fs::write(repo.join(path), content).unwrap();
    stage(repo, &[path.into()]).unwrap();
    commit(repo, msg, &CommitOptions::default()).unwrap();
}

#[test]
fn merge_conflict_resolve_and_continue() {
    let repo = temp_repo("merge");
    commit_file(&repo, "a.txt", "base\n", "base");
    commit_file(&repo, "gone.txt", "keep?\n", "add gone");
    switch_branch(&repo, "feature", true).unwrap();
    commit_file(&repo, "a.txt", "feature\n", "feature edit");
    run(&repo, &["rm", "-q", "gone.txt"]).unwrap();
    commit(&repo, "feature deletes gone", &CommitOptions::default()).unwrap();
    switch_branch(&repo, "main", false).unwrap();
    commit_file(&repo, "a.txt", "main\n", "main edit");
    commit_file(&repo, "gone.txt", "edited on main\n", "main edits gone");

    assert!(
        merge(&repo, "feature", MergeKind::Ff).unwrap(),
        "merge should stop on conflicts"
    );
    let st = status(&repo).unwrap();
    assert_eq!(
        st.operation.as_ref().map(|o| o.kind.as_str()),
        Some("merge")
    );
    let code = |p: &str| {
        st.conflicted
            .iter()
            .find(|f| f.path == p)
            .and_then(|f| f.conflict.clone())
    };
    assert_eq!(code("a.txt").as_deref(), Some("UU"));
    assert_eq!(code("gone.txt").as_deref(), Some("UD"));
    assert!(fs::read_to_string(repo.join("a.txt"))
        .unwrap()
        .contains("<<<<<<<"));

    // Starting another operation now must be refused, not reported as conflicts.
    assert!(rebase(&repo, "feature").is_err());

    resolve_side(&repo, "a.txt", Side::Theirs).unwrap();
    resolve_side(&repo, "gone.txt", Side::Theirs).unwrap(); // theirs deleted it
    assert_eq!(fs::read_to_string(repo.join("a.txt")).unwrap(), "feature\n");
    assert!(!repo.join("gone.txt").exists());
    assert!(!op_continue(&repo).unwrap());
    assert!(operation(&repo).is_none());
    assert_eq!(log(&repo, None, 0, 1).unwrap()[0].parents.len(), 2);
    let _ = fs::remove_dir_all(&repo);
}

#[test]
fn rebase_conflict_abort() {
    let repo = temp_repo("rebase");
    commit_file(&repo, "a.txt", "base\n", "base");
    switch_branch(&repo, "feature", true).unwrap();
    commit_file(&repo, "a.txt", "feature\n", "feature edit");
    switch_branch(&repo, "main", false).unwrap();
    commit_file(&repo, "a.txt", "main\n", "main edit");
    switch_branch(&repo, "feature", false).unwrap();

    assert!(rebase(&repo, "main").unwrap());
    let op = operation(&repo).unwrap();
    assert_eq!(
        (op.kind.as_str(), op.subject.as_deref(), op.step, op.total),
        ("rebase", Some("feature"), Some(1), Some(1))
    );
    op_abort(&repo).unwrap();
    assert!(operation(&repo).is_none());
    assert_eq!(fs::read_to_string(repo.join("a.txt")).unwrap(), "feature\n");
    assert!(validate_ref(&repo, "--help").is_err());
    let _ = fs::remove_dir_all(&repo);
}

#[test]
fn unborn_branch() {
    let repo = temp_repo("unborn");
    fs::write(repo.join("x"), "x\n").unwrap();
    stage(&repo, &["x".into()]).unwrap();
    assert_eq!(status(&repo).unwrap().staged.len(), 1);
    assert!(log(&repo, None, 0, 10).unwrap().is_empty());
    unstage(&repo, &["x".into()]).unwrap();
    assert_eq!(status(&repo).unwrap().staged.len(), 0);
    let _ = fs::remove_dir_all(&repo);
}

#[test]
fn toplevel_tells_no_repo_from_git_failing() {
    let dir = std::env::temp_dir().join(format!("gitviber-test-norepo-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let err = toplevel(&dir.join("missing")).unwrap_err();
    assert!(err.starts_with("Folder not found"), "{err}");
    assert_eq!(toplevel(&dir).unwrap_err(), NOT_A_REPO);
    // A .git file pointing nowhere is a broken repo, so git's own message comes through.
    // Not checked for the path: git 2.55 prints "not a git repository: (null)".
    fs::write(dir.join(".git"), "gitdir: /nowhere/at/all\n").unwrap();
    let err = toplevel(&dir).unwrap_err();
    assert_ne!(err, NOT_A_REPO);
    assert!(err.contains("not a git repository"), "{err}");
    let _ = fs::remove_dir_all(&dir);
}

/// The page sends these (src/lib/api/git.ts); anything else fails before git runs.
#[test]
fn modes_are_the_strings_the_page_sends() {
    fn parse<T: serde::de::DeserializeOwned>(s: &str) -> Option<T> {
        serde_json::from_value(serde_json::json!(s)).ok()
    }
    assert_eq!(parse("ff"), Some(PullMode::Ff));
    assert_eq!(parse("rebase"), Some(PullMode::Rebase));
    assert_eq!(parse("no-ff"), Some(MergeKind::NoFf));
    assert_eq!(parse("squash"), Some(MergeKind::Squash));
    assert_eq!(parse::<MergeKind>("rebase"), None);
    assert_eq!(parse("theirs"), Some(Side::Theirs));
    assert_eq!(parse("mixed"), Some(ResetMode::Mixed));
    assert_eq!(parse::<ResetMode>("--hard"), None);
}
