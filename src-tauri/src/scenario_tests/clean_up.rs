//! Clean up merged worktrees as real use pushes it: what changed between the dialog opening
//! and the click, worktrees inside worktrees, odd and huge ignored folders, and Undo.

use super::*;

fn item(path: &str, head: Option<&str>) -> CleanUp {
    CleanUp {
        path: path.to_string(),
        merged_head: head.map(str::to_string),
    }
}

/// A repo with `.gitignore` = `ignore`, and a worktree on `branch` with one commit merged into main.
fn merged_worktree(sb: &Sandbox, ignore: &str, branch: &str) -> (PathBuf, String) {
    let r = sb.path("r");
    init(&r);
    write_commit(&r, ".gitignore", ignore, "ignore");
    run(&r, &["branch", branch]).unwrap();
    let w = add_worktree(&r, branch, None, false, None).unwrap();
    write_commit(Path::new(&w), &format!("{branch}.txt"), "x\n", branch);
    run(&r, &["merge", "-q", branch]).unwrap();
    (r, w)
}

/// The dialog lists a worktree, then before the click: a file appears in one, a tracked file is
/// edited in another, a third gets a new commit. Each is left as it is, with its branch.
#[test]
fn clean_up_checks_again_what_changed_since_the_dialog_opened() {
    let sb = Sandbox::new("cu-race");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "a\n", "base");
    let made = |name: &str| {
        run(&r, &["branch", name]).unwrap();
        let w = add_worktree(&r, name, None, false, None).unwrap();
        write_commit(Path::new(&w), &format!("{name}.txt"), "x\n", name);
        w
    };
    let (new_file, edited, committed) = (made("new-file"), made("edited"), made("committed"));
    run(&r, &["merge", "-q", "new-file", "edited", "committed"]).unwrap();
    // The dialog showed all three as merged and clean. Then:
    fs::write(Path::new(&new_file).join("notes.md"), "draft\n").unwrap();
    fs::write(Path::new(&edited).join("a.txt"), "changed\n").unwrap();
    write_commit(Path::new(&committed), "more.txt", "x\n", "one more");

    let out = clean_up_worktrees(
        &r,
        &[
            item(&new_file, None),
            item(&edited, None),
            item(&committed, None),
        ],
    );
    assert!(out.removed.is_empty(), "removed {:?}", out.removed);
    assert_eq!(out.failed.len(), 3);
    for w in [&new_file, &edited, &committed] {
        assert!(Path::new(w).is_dir());
    }
    assert_eq!(
        fs::read_to_string(Path::new(&new_file).join("notes.md")).unwrap(),
        "draft\n"
    );
    for b in ["new-file", "edited", "committed"] {
        assert!(exists(&r, b));
    }
}

/// An agent working in a worktree makes its own worktrees inside it (`.claude/worktrees/`),
/// which projects ignore. `git worktree remove` of the outer one deletes the inner folder whole,
/// uncommitted work and all, so Clean up must leave the outer one. It's also offered after a
/// pull request merges in the app (offerWorktreeRemoval), which doesn't look for one inside.
#[test]
fn clean_up_leaves_a_worktree_that_holds_another_worktree() {
    let sb = Sandbox::new("cu-nested");
    let (r, outer) = merged_worktree(&sb, ".claude/worktrees/\n", "outer");
    let inner = Path::new(&outer).join(".claude/worktrees/agent-1");
    run(
        &r,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "agent-1",
            inner.to_str().unwrap(),
        ],
    )
    .unwrap();
    fs::write(inner.join("work.txt"), "not committed yet\n").unwrap();

    let head = rev(&r, "outer");
    let out = clean_up_worktrees(&r, &[item(&outer, Some(&head))]);
    assert!(
        inner.join("work.txt").exists(),
        "the inner worktree's uncommitted work was deleted (result: {out:?})"
    );
    assert_eq!(out.failed.len(), 1);
}

/// After a pull request merges in the app, its worktree is offered with the PR's head, which
/// skips the merged check. If HEAD was detached there and committed on since (an agent bisecting,
/// a quick experiment), those commits are on no branch: removing the worktree takes its HEAD
/// and reflog along, and they're gone.
#[test]
fn clean_up_by_pull_request_leaves_commits_made_on_a_detached_head() {
    let sb = Sandbox::new("cu-detached");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "a\n", "base");
    run(&r, &["branch", "pr"]).unwrap();
    let w = add_worktree(&r, "pr", None, false, None).unwrap();
    write_commit(Path::new(&w), "pr.txt", "x\n", "the PR");
    let pr_head = rev(&r, "pr");
    run(Path::new(&w), &["checkout", "-q", "--detach"]).unwrap();
    write_commit(Path::new(&w), "experiment.txt", "x\n", "an experiment");
    let experiment = rev(Path::new(&w), "HEAD");

    let out = clean_up_worktrees(&r, &[item(&w, Some(&pr_head))]);
    let kept = run(&r, &["cat-file", "-e", &format!("{experiment}^{{commit}}")]).is_ok()
        && Path::new(&w).is_dir();
    assert!(
        kept,
        "the worktree with a commit on no branch was removed (result: {out:?})"
    );
}

/// `git worktree add --force` can put a branch in two worktrees. Cleaning up one keeps the
/// branch: deleting it would leave the other worktree on an unborn HEAD.
#[test]
fn clean_up_keeps_a_branch_another_worktree_has_out() {
    let sb = Sandbox::new("cu-held");
    let (r, w1) = merged_worktree(&sb, "", "shared");
    let w2 = sb.path("second");
    run(
        &r,
        &[
            "worktree",
            "add",
            "-q",
            "--force",
            w2.to_str().unwrap(),
            "shared",
        ],
    )
    .unwrap();
    let tip = rev(&r, "shared");

    let out = clean_up_worktrees(&r, &[item(&w1, None)]);
    assert_eq!(out.removed, [w1]);
    assert!(out.deleted.is_empty());
    assert_eq!(out.kept, ["shared"]);
    assert_eq!(rev(&r, "shared"), tip);
    assert_eq!(rev(&w2, "HEAD"), tip);
}

/// A worktree locked since the dialog opened (on a removable disk), and one whose folder was
/// deleted in Finder: both are left, with their branches, and nothing panics.
#[test]
fn clean_up_leaves_locked_and_missing_worktrees() {
    let sb = Sandbox::new("cu-locked");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "a\n", "base");
    let made = |name: &str| {
        run(&r, &["branch", name]).unwrap();
        let w = add_worktree(&r, name, None, false, None).unwrap();
        write_commit(Path::new(&w), &format!("{name}.txt"), "x\n", name);
        w
    };
    let (locked, gone) = (made("locked"), made("gone"));
    run(&r, &["merge", "-q", "locked", "gone"]).unwrap();
    lock_worktree(&r, &locked, Some("on an external disk")).unwrap();
    fs::remove_dir_all(&gone).unwrap();
    assert!(worktree_ignored(&r, &gone).is_err());

    let out = clean_up_worktrees(&r, &[item(&locked, None), item(&gone, None)]);
    let failed: Vec<_> = out.failed.iter().map(|(p, _)| p.as_str()).collect();
    assert!(failed.contains(&locked.as_str()), "{out:?}");
    assert!(Path::new(&locked).is_dir());
    assert!(exists(&r, "locked"));
    // The missing one: either left, or removed with its branch only if git counts it merged.
    if !failed.contains(&gone.as_str()) {
        assert!(!exists(&r, "gone") || out.kept.contains(&"gone".to_string()));
    }
}

/// Undo after Clean up brings every deleted branch back at its commit; the folders stay gone.
#[test]
fn undo_after_clean_up_brings_the_branches_back() {
    let sb = Sandbox::new("cu-undo");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "a\n", "base");
    let mut list = vec![];
    let mut tips = vec![];
    for name in ["one", "two", "fix/three"] {
        run(&r, &["branch", name]).unwrap();
        let w = add_worktree(&r, name, None, false, None).unwrap();
        write_commit(
            Path::new(&w),
            &format!("{}.txt", name.replace('/', "-")),
            "x\n",
            name,
        );
        tips.push((name, rev(&r, name)));
        list.push(item(&w, None));
    }
    run(&r, &["merge", "-q", "one", "two", "fix/three"]).unwrap();
    let j = Journal::default();
    let out = j
        .record(
            &r,
            Action::new("Delete merged worktrees' branches", Mode::Keep),
            |r| Ok(clean_up_worktrees(r, &list)),
        )
        .unwrap();
    assert_eq!(out.deleted.len(), 3, "{out:?}");
    step(&j, &r, false).unwrap();
    for (name, tip) in &tips {
        assert_eq!(&rev(&r, name), tip, "{name} came back");
    }
    assert_eq!(on_branch(&r), "main");
    // Redo deletes them again.
    step(&j, &r, true).unwrap();
    assert!(tips.iter().all(|(n, _)| !exists(&r, n)));
}

/// The dialog's sizes: links aren't followed (a loop, a link to a big folder elsewhere), odd
/// names come back as they are, and a folder being deleted meanwhile doesn't fail it.
#[cfg(unix)]
#[test]
fn ignored_files_are_sized_without_following_links() {
    let sb = Sandbox::new("cu-sizes");
    let (r, w) = merged_worktree(&sb, "node_modules/\n*.log\n.env*\n", "sized");
    let w = Path::new(&w);
    let nm = w.join("node_modules");
    fs::create_dir_all(nm.join("pkg/lib")).unwrap();
    fs::write(nm.join("pkg/lib/index.js"), "x".repeat(1000)).unwrap();
    // pnpm-style links back up the tree, and one to a big folder outside.
    std::os::unix::fs::symlink("..", nm.join("pkg/lib/loop")).unwrap();
    let outside = sb.path("outside");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("big.bin"), vec![0u8; 1 << 20]).unwrap();
    std::os::unix::fs::symlink(&outside, nm.join("linked")).unwrap();
    let odd = "it's \"$HOME\" ünïcode ü.log";
    fs::write(w.join(odd), "12345").unwrap();
    fs::write(w.join(".env.local"), "TOKEN=made-up\n").unwrap();

    let got = worktree_ignored(&r, w.to_str().unwrap()).unwrap();
    assert!(got.complete);
    let nm_entry = got
        .entries
        .iter()
        .find(|e| e.path == "node_modules/")
        .unwrap();
    assert!(
        nm_entry.bytes < 10_000,
        "followed a link: {} bytes",
        nm_entry.bytes
    );
    assert!(
        got.entries.iter().any(|e| e.path == odd && e.bytes == 5),
        "{got:?}"
    );
    assert!(got.entries.iter().any(|e| e.path == ".env.local"));
}

/// A folder the walk can't read (a Docker volume owned by root, a `chmod 000` cache): its size
/// isn't known, so the dialog must say "at least", and `git worktree remove` can't delete it.
/// Git then fails halfway: the worktree's admin folder is gone but the folder stays, a broken
/// checkout no longer listed. Clean up should leave such a worktree whole instead.
#[cfg(unix)]
#[test]
fn clean_up_leaves_a_worktree_with_a_folder_it_cant_delete() {
    use std::os::unix::fs::PermissionsExt;
    let sb = Sandbox::new("cu-denied");
    let (r, w) = merged_worktree(&sb, "data/\n", "denied");
    let locked = Path::new(&w).join("data/pg");
    fs::create_dir_all(locked.join("base")).unwrap();
    fs::write(locked.join("base/1"), vec![0u8; 4096]).unwrap();
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
    let sized = worktree_ignored(&r, &w);
    let out = clean_up_worktrees(&r, &[item(&w, None)]);
    let listed = worktrees(&r).unwrap().iter().any(|x| x.path == w);
    let folder = Path::new(&w).is_dir();
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();

    let sized = sized.unwrap();
    let orphan = !listed && folder;
    assert!(
        !sized.complete && !orphan,
        "sizes complete: {} (should be false: {sized:?}); left a folder that's no longer \
         a worktree: {orphan} (result: {out:?})",
        sized.complete
    );
}

/// A big node_modules (tens of thousands of files) is sized in well under the time a user
/// waits on a dialog, while its files are being deleted by another process.
#[test]
fn a_big_node_modules_is_sized_quickly_while_it_changes() {
    let sb = Sandbox::new("cu-big");
    let (r, w) = merged_worktree(&sb, "node_modules/\n", "big");
    let nm = Path::new(&w).join("node_modules");
    for d in 0..400 {
        let dir = nm.join(format!("pkg-{d}/dist"));
        fs::create_dir_all(&dir).unwrap();
        for f in 0..50 {
            fs::write(dir.join(format!("f{f}.js")), "export {}\n").unwrap();
        }
    }
    let gone = nm.clone();
    let deleter = std::thread::spawn(move || {
        for d in (0..400).step_by(2) {
            let _ = fs::remove_dir_all(gone.join(format!("pkg-{d}")));
        }
    });
    let t = std::time::Instant::now();
    let got = worktree_ignored(&r, &w).unwrap();
    let took = t.elapsed();
    deleter.join().unwrap();
    assert!(took.as_secs() < 5, "took {took:?}");
    let e = &got.entries[0];
    assert_eq!(e.path, "node_modules/");
    assert!(e.files >= 10_000 && e.files <= 20_000, "{e:?}");
}
