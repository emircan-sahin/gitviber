//! Partial stash, Take This File and stash rename on awkward files and timing.

use super::*;
use crate::lines::Request;

fn request(repo: &Path, path: &str, removed: &[u32], added: &[u32]) -> Request {
    let pair = diff_pair(repo, "unstaged", path, None, None, None, None, |p| {
        vfs::read_diff_side(repo, p)
    })
    .unwrap();
    let shown = |f: &FileText| f.exists.then(|| f.text.clone());
    Request {
        path: path.into(),
        old_path: None,
        kind: "unstaged".into(),
        action: "stash".into(),
        original: shown(&pair.original),
        modified: shown(&pair.modified),
        removed: removed.to_vec(),
        added: added.to_vec(),
    }
}

fn repo(name: &str) -> (Sandbox, PathBuf) {
    let sb = Sandbox::new(name);
    let r = sb.path("r");
    init(&r);
    (sb, r)
}

fn stash_all(r: &Path, msg: &str) {
    let what = StashWhat {
        untracked: true,
        staged: false,
        paths: &[],
    };
    stash_push(r, msg, what).unwrap();
}

/// CRLF files keep their line endings in the working tree and in the stash.
#[test]
fn partial_stash_keeps_crlf() {
    let (_sb, r) = repo("se-crlf");
    write_commit(&r, "a.txt", "1\r\n2\r\n3\r\n", "base");
    fs::write(r.join("a.txt"), "1\r\ntwo\r\nthree\r\n").unwrap();
    let req = request(&r, "a.txt", &[2], &[2]);
    crate::lines::stash(&r, "two", &req).unwrap();
    assert_eq!(
        fs::read(r.join("a.txt")).unwrap(),
        b"1\r\n2\r\nthree\r\n".to_vec()
    );
    let sha = stashes(&r).unwrap()[0].sha.clone();
    let stashed = run_text(&r, &["show", &format!("{sha}:a.txt")]).unwrap();
    assert_eq!(stashed, "1\r\ntwo\r\n3\r\n");
}

/// A file with no newline at its end stays that way on both sides.
#[test]
fn partial_stash_without_a_trailing_newline() {
    let (_sb, r) = repo("se-nonl");
    write_commit(&r, "a.txt", "1\n2\n3", "base");
    fs::write(r.join("a.txt"), "one\n2\nthree").unwrap();
    // The last line, whose own ending changed with it.
    let req = request(&r, "a.txt", &[3], &[3]);
    crate::lines::stash(&r, "three", &req).unwrap();
    assert_eq!(fs::read_to_string(r.join("a.txt")).unwrap(), "one\n2\n3");
    let sha = stashes(&r).unwrap()[0].sha.clone();
    assert_eq!(
        run_text(&r, &["show", &format!("{sha}:a.txt")]).unwrap(),
        "1\n2\nthree"
    );
}

/// Some lines of a new file: the stash holds those, the file keeps the rest.
#[test]
fn partial_stash_of_some_lines_of_a_new_file() {
    let (_sb, r) = repo("se-newpart");
    write_commit(&r, "a.txt", "a\n", "base");
    fs::write(r.join("new.txt"), "n1\nn2\nn3\n").unwrap();
    let req = request(&r, "new.txt", &[], &[2]);
    crate::lines::stash(&r, "middle", &req).unwrap();
    assert_eq!(fs::read_to_string(r.join("new.txt")).unwrap(), "n1\nn3\n");
    let sha = stashes(&r).unwrap()[0].sha.clone();
    assert_eq!(
        run_text(&r, &["show", &format!("{sha}:new.txt")]).unwrap(),
        "n2\n"
    );
}

/// A file renamed in the index and edited after: its lines stash against the renamed file.
#[test]
fn partial_stash_of_a_renamed_file() {
    let (_sb, r) = repo("se-rename");
    write_commit(&r, "old.txt", "1\n2\n3\n", "base");
    run(&r, &["mv", "old.txt", "new.txt"]).unwrap();
    fs::write(r.join("new.txt"), "1\ntwo\n3\n").unwrap();
    let req = request(&r, "new.txt", &[2], &[2]);
    crate::lines::stash(&r, "two", &req).unwrap();
    assert_eq!(fs::read_to_string(r.join("new.txt")).unwrap(), "1\n2\n3\n");
    assert!(run_text(&r, &["status", "--porcelain"])
        .unwrap()
        .contains("R  old.txt -> new.txt"));
}

/// A binary file has no lines to pick: refused, nothing changes.
#[test]
fn partial_stash_of_a_binary_file_is_refused() {
    let (_sb, r) = repo("se-bin");
    fs::write(r.join("b.bin"), [0u8, 1, 2, 3]).unwrap();
    write_commit_files(&r, "base");
    fs::write(r.join("b.bin"), [0u8, 9, 2, 3]).unwrap();
    let pair = diff_pair(&r, "unstaged", "b.bin", None, None, None, None, |p| {
        vfs::read_diff_side(&r, p)
    })
    .unwrap();
    let req = Request {
        path: "b.bin".into(),
        old_path: None,
        kind: "unstaged".into(),
        action: "stash".into(),
        original: pair.original.exists.then(|| pair.original.text.clone()),
        modified: pair.modified.exists.then(|| pair.modified.text.clone()),
        removed: vec![1],
        added: vec![1],
    };
    let before = fs::read(r.join("b.bin")).unwrap();
    let _ = crate::lines::stash(&r, "x", &req);
    assert_eq!(fs::read(r.join("b.bin")).unwrap(), before);
    assert!(stashes(&r).unwrap().is_empty());
}

fn write_commit_files(r: &Path, msg: &str) {
    run(r, &["add", "-A"]).unwrap();
    run(r, &["commit", "-q", "-m", msg]).unwrap();
}

/// A rebase stopped halfway refuses too, and a detached HEAD still names its stash.
#[test]
fn partial_stash_during_a_rebase_and_on_a_detached_head() {
    let (_sb, r) = repo("se-op");
    write_commit(&r, "a.txt", "1\n2\n", "base");
    fs::write(r.join("a.txt"), "1\ntwo\n").unwrap();
    let req = request(&r, "a.txt", &[2], &[2]);
    fs::create_dir_all(r.join(".git/rebase-merge")).unwrap();
    assert!(crate::lines::stash(&r, "x", &req).is_err());
    assert!(stashes(&r).unwrap().is_empty());
    fs::remove_dir_all(r.join(".git/rebase-merge")).unwrap();
    run(&r, &["checkout", "-q", "--detach"]).unwrap();
    crate::lines::stash(&r, "d", &req).unwrap();
    assert_eq!(stashes(&r).unwrap().len(), 1);
}

/// A name that's already the stash's, or spans lines, changes nothing; a rename never leaves
/// two entries of the stash or loses one.
#[test]
fn renaming_to_the_same_name_or_a_multiline_one() {
    let (_sb, r) = repo("se-rename-edge");
    write_commit(&r, "a.txt", "a\n", "base");
    fs::write(r.join("a.txt"), "x\n").unwrap();
    stash_all(&r, "name");
    let mut sha = stashes(&r).unwrap()[0].sha.clone();
    stash_rename(&r, &sha, "name").unwrap();
    let list = stashes(&r).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].message, "On main: name");
    sha = list[0].sha.clone();
    assert!(stash_rename(&r, &sha, "a\nb").is_err());
    assert!(stash_rename(&r, &sha, "a\rb").is_err());
    assert_eq!(stashes(&r).unwrap().len(), 1);
}

/// A stash pushed between listing and renaming moves the indexes; the right stash is still the
/// one renamed, and every stash is still there once.
#[test]
fn renaming_while_stashes_are_added_never_loses_one() {
    let (_sb, r) = repo("se-rename-race");
    write_commit(&r, "a.txt", "a\n", "base");
    fs::write(r.join("a.txt"), "1\n").unwrap();
    stash_all(&r, "one");
    let target = stashes(&r).unwrap()[0].sha.clone();
    let pusher = {
        let r = r.clone();
        std::thread::spawn(move || {
            let mut made = 0;
            for i in 0..6 {
                fs::write(r.join("a.txt"), format!("{i}x\n")).unwrap();
                let what = StashWhat {
                    untracked: false,
                    staged: false,
                    paths: &[],
                };
                if stash_push(&r, &format!("other {i}"), what).is_ok() {
                    made += 1;
                }
            }
            made
        })
    };
    let renamed = stash_rename(&r, &target, "renamed");
    let made = pusher.join().unwrap();
    let list = stashes(&r).unwrap();
    let named = list
        .iter()
        .filter(|s| s.message == "On main: renamed")
        .count();
    if renamed.is_ok() {
        assert_eq!(list.len(), 1 + made, "no stash lost");
        assert_eq!(named, 1);
        assert!(list.iter().all(|s| s.sha != target), "the old one is gone");
    } else {
        // Whatever failed, the stash is still there.
        assert!(list.iter().any(|s| s.sha == target), "{renamed:?}");
    }
}

/// Take This File of a path the stash doesn't know: an error, and the working tree is as it was.
#[test]
fn taking_a_path_the_stash_lacks_is_an_error() {
    let (_sb, r) = repo("se-take");
    write_commit(&r, "a.txt", "a\n", "base");
    write_commit(&r, "b.txt", "b\n", "b");
    fs::write(r.join("a.txt"), "a stashed\n").unwrap();
    stash_all(&r, "s");
    let sha = stashes(&r).unwrap()[0].sha.clone();
    fs::write(r.join("b.txt"), "b mine\n").unwrap();
    assert!(stash_restore_file(&r, &sha, "nothere.txt", false).is_err());
    assert_eq!(fs::read_to_string(r.join("b.txt")).unwrap(), "b mine\n");
}

/// A file the stash deleted: taking it removes the file from the working tree.
#[test]
fn taking_a_file_the_stash_deleted_removes_it() {
    let (_sb, r) = repo("se-take-del");
    write_commit(&r, "a.txt", "a\n", "base");
    write_commit(&r, "b.txt", "b\n", "b");
    fs::remove_file(r.join("a.txt")).unwrap();
    stash_all(&r, "s");
    let sha = stashes(&r).unwrap()[0].sha.clone();
    assert!(r.join("a.txt").exists());
    stash_restore_file(&r, &sha, "a.txt", false).unwrap();
    assert!(!r.join("a.txt").exists());
    assert!(r.join("b.txt").exists());
}

/// Discarding the files of a folder: tracked ones come back, a symlink the folder holds is
/// a symlink the folder holds is listed as itself (its target stays), and an ignored file is none of the list's business.
#[test]
fn discarding_a_folder_leaves_ignored_files_and_link_targets_alone() {
    let (_sb, r) = repo("se-folder");
    write_commit(&r, ".gitignore", "*.log\n", "ignore");
    write_commit(&r, "d/a.txt", "a\n", "a");
    fs::write(r.join("d/a.txt"), "changed\n").unwrap();
    fs::write(r.join("d/x.log"), "log\n").unwrap();
    let outside = _sb.path("outside");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("keep.txt"), "k\n").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, r.join("d/link")).unwrap();
    let listed: Vec<String> = status(&r)
        .unwrap()
        .unstaged
        .iter()
        .map(|f| f.path.clone())
        .filter(|p| p.starts_with("d/"))
        .collect();
    assert!(!listed.contains(&"d/x.log".to_string()), "{listed:?}");
    discard(&r, &["d/a.txt".to_string()]).unwrap();
    assert_eq!(fs::read_to_string(r.join("d/a.txt")).unwrap(), "a\n");
    assert!(r.join("d/x.log").exists());
    #[cfg(unix)]
    {
        assert!(listed.contains(&"d/link".to_string()), "{listed:?}");
    }
    assert!(outside.join("keep.txt").exists());
}

/// A repository with a split index keeps its shared part next to the index: the scratch copy
/// has to sit there too, or git can't read it.
#[test]
fn partial_stash_with_a_split_index() {
    let (_sb, r) = repo("se-split");
    write_commit(&r, "a.txt", "1\n2\n3\n", "base");
    write_commit(&r, "b.txt", "b\n", "b");
    run(&r, &["update-index", "--split-index"]).unwrap();
    fs::write(r.join("b.txt"), "b2\n").unwrap();
    stage(&r, &["b.txt".into()]).unwrap();
    fs::write(r.join("a.txt"), "1\ntwo\n3\n").unwrap();
    let req = request(&r, "a.txt", &[2], &[2]);
    crate::lines::stash(&r, "two", &req).unwrap();
    assert_eq!(fs::read_to_string(r.join("a.txt")).unwrap(), "1\n2\n3\n");
    assert_eq!(stashes(&r).unwrap().len(), 1);
}
