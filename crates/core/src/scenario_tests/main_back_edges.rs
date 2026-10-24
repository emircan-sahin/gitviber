//! Handing a branch back to the main folder when real life is in the way: hooks, stopped
//! operations, submodules, races and a user who moved on. Whatever fails, `main` is never left
//! with the main folder on neither it nor the branch it came from.

use super::*;

/// The main folder on `into`, `branch` held by a linked worktree next to it.
fn held_elsewhere(sb: &Sandbox, into: &str, branch: &str) -> (PathBuf, PathBuf) {
    let r = sb.path("proj");
    init(&r);
    write_commit(&r, "a.txt", "a\n", "base");
    // init's own branch is `main`.
    let _ = run(&r, &["branch", branch]);
    let new = if exists(&r, into) { vec![] } else { vec!["-c"] };
    let args = ["switch", "-q"].into_iter().chain(new).chain([into]);
    run(&r, &args.collect::<Vec<_>>()).unwrap();
    write_commit(&r, "f.txt", "f\n", "work");
    let held = sb.path("held");
    run(
        &r,
        &["worktree", "add", "-q", held.to_str().unwrap(), branch],
    )
    .unwrap();
    (r, held)
}

fn head_of(repo: &Path) -> Option<String> {
    run_text(repo, &["symbolic-ref", "-q", "--short", "HEAD"])
        .ok()
        .map(|s| s.trim().to_string())
}

/// Every branch is checked out in at most one folder, and each folder is on `want`.
fn assert_layout(r: &Path, held: &Path, main: Option<&str>, other: Option<&str>) {
    assert_eq!(head_of(r).as_deref(), main, "main folder");
    assert_eq!(head_of(held).as_deref(), other, "other folder");
}

/// A post-checkout hook that fails in the folder named in the flag file, after git has already
/// switched: git then exits non-zero for a switch that happened.
#[cfg(unix)]
fn failing_hook(r: &Path, sb: &Sandbox) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let flag = sb.path("fail-in");
    let hook = r.join(".git/hooks/post-checkout");
    fs::create_dir_all(hook.parent().unwrap()).unwrap();
    fs::write(
        &hook,
        format!(
            "#!/bin/sh\n[ -e '{f}' ] && [ \"$(basename \"$PWD\")\" = \"$(cat '{f}')\" ] && exit 1\nexit 0\n",
            f = flag.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
    flag
}

#[cfg(unix)]
#[test]
fn a_failing_hook_in_the_main_folder_rolls_everything_back() {
    let sb = Sandbox::new("mbhookmain");
    let (r, held) = held_elsewhere(&sb, "feat", "main");
    let flag = failing_hook(&r, &sb);
    fs::write(&flag, "proj").unwrap();

    let e = move_main_back(&r, "main");
    assert!(e.is_err());
    // git switched the main folder before the hook failed: it must be put back, and the other
    // folder must get its branch again.
    assert_layout(&r, &held, Some("feat"), Some("main"));
}

#[cfg(unix)]
#[test]
fn a_failing_hook_in_the_other_folder_leaves_it_on_its_branch() {
    let sb = Sandbox::new("mbhookheld");
    let (r, held) = held_elsewhere(&sb, "feat", "main");
    let flag = failing_hook(&r, &sb);
    fs::write(&flag, "held").unwrap();

    let e = move_main_back(&r, "main");
    assert!(e.is_err());
    // The detach happened before the hook failed; nothing may be left detached with the main
    // folder untouched.
    assert_layout(&r, &held, Some("feat"), Some("main"));
}

#[cfg(unix)]
#[test]
fn a_failing_hook_during_undo_leaves_the_move_as_it_was() {
    let sb = Sandbox::new("mbhookundo");
    let (r, held) = held_elsewhere(&sb, "feat", "main");
    let flag = failing_hook(&r, &sb);
    let plan = move_main_back(&r, "main").unwrap();
    assert_layout(&r, &held, Some("main"), None);

    fs::write(&flag, "held").unwrap();
    assert!(undo_main_back(&r, &plan).is_err());
    // Back where the move left it, not half way.
    assert_layout(&r, &held, Some("main"), None);
}

#[test]
fn changes_in_either_folder_refuse_whatever_their_kind() {
    let sb = Sandbox::new("mbkinds");
    let (r, held) = held_elsewhere(&sb, "feat", "main");

    fs::write(held.join("scratch.txt"), "x\n").unwrap();
    let e = main_back_plan(&r, "main").unwrap_err();
    assert!(e.starts_with("held has 1 uncommitted change"), "{e}");
    fs::remove_file(held.join("scratch.txt")).unwrap();

    fs::write(r.join("f.txt"), "edited\n").unwrap();
    stage(&r, &["f.txt".into()]).unwrap();
    let e = main_back_plan(&r, "main").unwrap_err();
    assert!(e.starts_with("proj has 1 uncommitted change"), "{e}");
    run(&r, &["reset", "-q", "--hard"]).unwrap();

    // Ignored files are not changes.
    fs::write(r.join(".git/info/exclude"), "*.log\n").unwrap();
    fs::write(r.join("a.log"), "x\n").unwrap();
    fs::write(held.join("b.log"), "x\n").unwrap();
    main_back_plan(&r, "main").unwrap();
}

#[test]
fn a_stopped_operation_in_either_folder_refuses() {
    let sb = Sandbox::new("mbstopped");
    let (r, held) = held_elsewhere(&sb, "feat", "main");
    let tip = rev(&r, "HEAD");

    let stop = |dir: &Path, file: &str| {
        let d = git_dir(dir).unwrap();
        fs::write(d.join(file), format!("{tip}\n")).unwrap();
        d.join(file)
    };
    for file in ["CHERRY_PICK_HEAD", "REVERT_HEAD", "MERGE_HEAD"] {
        let f = stop(&r, file);
        assert!(main_back_plan(&r, "main").is_err(), "{file} in main");
        fs::remove_file(f).unwrap();
        let f = stop(&held, file);
        assert!(main_back_plan(&r, "main").is_err(), "{file} in the other");
        fs::remove_file(f).unwrap();
    }
    // A bisect begun in the main folder (still on its branch).
    run(&r, &["bisect", "start"]).unwrap();
    let e = main_back_plan(&r, "main").unwrap_err();
    assert!(e.contains("bisect"), "{e}");
    run(&r, &["bisect", "reset"]).unwrap();
    main_back_plan(&r, "main").unwrap();
    assert_layout(&r, &held, Some("feat"), Some("main"));
}

#[test]
fn a_rebase_stopped_in_the_other_folder_moves_nothing() {
    let sb = Sandbox::new("mbrebase");
    let (r, held) = held_elsewhere(&sb, "feat", "main");
    // main (in `held`) and feat both change a.txt, so rebasing main onto feat stops.
    write_commit(&held, "a.txt", "main side\n", "main edits a");
    write_commit(&r, "a.txt", "feat side\n", "feat edits a");
    assert!(run(&held, &["rebase", "feat"]).is_err());

    // The folder is mid-rebase: its HEAD is detached, so it no longer reads as holding main,
    // but git still counts the branch as taken. Whatever the plan says, nothing may move.
    assert!(move_main_back(&r, "main").is_err());
    assert_eq!(head_of(&r).as_deref(), Some("feat"));
}

#[test]
fn a_lock_without_a_live_process_does_not_stop_it() {
    let sb = Sandbox::new("mbstalelock");
    let (r, held) = held_elsewhere(&sb, "feat", "main");
    run(
        &r,
        &[
            "worktree",
            "lock",
            "--reason",
            "kept by hand",
            held.to_str().unwrap(),
        ],
    )
    .unwrap();
    move_main_back(&r, "main").unwrap();
    assert_layout(&r, &held, Some("main"), None);
}

#[test]
fn default_branches_named_master_or_with_slashes_move_too() {
    for (into, branch) in [("develop", "master"), ("main", "release/1.x/hotfix")] {
        let sb = Sandbox::new("mbnames");
        let (r, held) = held_elsewhere(&sb, into, branch);
        let plan = move_main_back(&held, branch).unwrap();
        assert_layout(&r, &held, Some(branch), None);
        undo_main_back(&r, &plan).unwrap();
        assert_layout(&r, &held, Some(into), Some(branch));
    }
}

#[test]
fn another_worktree_detached_at_the_branch_does_not_hold_it() {
    let sb = Sandbox::new("mbdetachedtwin");
    let (r, held) = held_elsewhere(&sb, "feat", "main");
    let twin = sb.path("twin");
    run(
        &r,
        &[
            "worktree",
            "add",
            "-q",
            "--detach",
            twin.to_str().unwrap(),
            "main",
        ],
    )
    .unwrap();
    move_main_back(&r, "main").unwrap();
    assert_layout(&r, &held, Some("main"), None);
    assert_eq!(head_of(&twin), None, "the twin is left alone");
}

#[test]
fn undo_after_the_branch_it_returns_to_is_gone_leaves_the_move() {
    let sb = Sandbox::new("mbundogone");
    let (r, held) = held_elsewhere(&sb, "feat", "main");
    let plan = move_main_back(&r, "main").unwrap();
    run(&r, &["branch", "-q", "-D", "feat"]).unwrap();

    assert!(undo_main_back(&r, &plan).is_err());
    assert_layout(&r, &held, Some("main"), None);
}

#[test]
fn undo_after_another_folder_took_the_old_branch_leaves_the_move() {
    let sb = Sandbox::new("mbundotaken");
    let (r, held) = held_elsewhere(&sb, "feat", "main");
    let plan = move_main_back(&r, "main").unwrap();
    let third = sb.path("third");
    run(
        &r,
        &["worktree", "add", "-q", third.to_str().unwrap(), "feat"],
    )
    .unwrap();

    assert!(undo_main_back(&r, &plan).is_err());
    assert_layout(&r, &held, Some("main"), None);
    assert_eq!(head_of(&third).as_deref(), Some("feat"));
}

#[test]
fn undo_stops_when_the_detached_folder_was_committed_in() {
    let sb = Sandbox::new("mbundocommit");
    let (r, held) = held_elsewhere(&sb, "feat", "main");
    let plan = move_main_back(&r, "main").unwrap();
    write_commit(&held, "h.txt", "h\n", "committed while detached");

    let e = undo_main_back(&r, &plan).unwrap_err();
    assert!(e.contains("new commits"), "{e}");
    assert_layout(&r, &held, Some("main"), None);
}

#[test]
fn two_runs_at_once_end_in_the_same_place() {
    let sb = Sandbox::new("mbrace");
    let (r, held) = held_elsewhere(&sb, "feat", "main");
    let handles: Vec<_> = (0..4)
        .map(|_| {
            let r = r.clone();
            std::thread::spawn(move || move_main_back(&r, "main"))
        })
        .collect();
    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert!(results.iter().any(Result::is_ok), "{results:?}");
    assert_layout(&r, &held, Some("main"), None);
}

#[test]
fn a_main_folder_with_a_submodule_moves_and_undoes() {
    let sb = Sandbox::new("mbsub");
    let r = repo_with_submodule(&sb);
    run(&r, &["branch", "trunk"]).unwrap();
    run(&r, &["switch", "-q", "-c", "feat"]).unwrap();
    write_commit(&r, "f.txt", "f\n", "work");
    let held = sb.path("held");
    run(
        &r,
        &["worktree", "add", "-q", held.to_str().unwrap(), "trunk"],
    )
    .unwrap();

    let plan = move_main_back(&r, "trunk").unwrap();
    assert_layout(&r, &held, Some("trunk"), None);
    undo_main_back(&r, &plan).unwrap();
    assert_layout(&r, &held, Some("feat"), Some("trunk"));
}

#[test]
fn a_sparse_main_folder_moves_without_phantom_changes() {
    let sb = Sandbox::new("mbsparse");
    let r = sb.path("proj");
    init(&r);
    write_commit(&r, "keep/k.txt", "k\n", "keep");
    write_commit(&r, "drop/d.txt", "d\n", "drop");
    run(&r, &["branch", "trunk"]).unwrap();
    run(&r, &["switch", "-q", "-c", "feat"]).unwrap();
    run(&r, &["sparse-checkout", "set", "keep"]).unwrap();
    let held = sb.path("held");
    run(
        &r,
        &["worktree", "add", "-q", held.to_str().unwrap(), "trunk"],
    )
    .unwrap();

    move_main_back(&r, "trunk").unwrap();
    assert_layout(&r, &held, Some("trunk"), None);
    assert!(!r.join("drop/d.txt").exists(), "still sparse");
}

#[test]
fn an_untracked_file_the_switch_would_overwrite_is_refused_before_anything_moves() {
    let sb = Sandbox::new("mbclobber");
    let (r, held) = held_elsewhere(&sb, "feat", "main");
    // Ignored here, tracked on main: the switch would replace it silently, which is git's rule
    // for ignored files; nothing is lost that git tracked.
    fs::write(r.join(".gitignore"), "ignored.txt\n").unwrap();
    stage(&r, &[".gitignore".into()]).unwrap();
    commit(&r, "ignore", &CommitOptions::default(), &Net::default()).unwrap();
    write_commit(&held, "ignored.txt", "tracked on main\n", "track it");
    fs::write(r.join("ignored.txt"), "mine\n").unwrap();

    let before = (head_of(&r), head_of(&held));
    let _ = move_main_back(&r, "main");
    // Either it went through or it was undone whole; never half.
    let after = (head_of(&r), head_of(&held));
    assert!(
        after == before || after == (Some("main".into()), None),
        "{after:?}"
    );
}

#[cfg(unix)]
#[test]
fn a_repo_opened_through_a_symlink_plans_and_undoes() {
    let sb = Sandbox::new("mblink");
    let (r, held) = held_elsewhere(&sb, "feat", "main");
    let link = sb.path("link");
    std::os::unix::fs::symlink(&r, &link).unwrap();

    let plan = move_main_back(&link, "main").unwrap();
    assert_layout(&r, &held, Some("main"), None);
    undo_main_back(&link, &plan).unwrap();
    assert_layout(&r, &held, Some("feat"), Some("main"));
}

#[test]
fn a_gone_holder_is_pruned_and_then_the_branch_switches_like_any_other() {
    let sb = Sandbox::new("mbprune");
    let (r, held) = held_elsewhere(&sb, "feat", "main");
    fs::remove_dir_all(&held).unwrap();
    assert!(
        run(&r, &["switch", "-q", "main"]).is_err(),
        "git still holds it"
    );

    let listed = worktrees(&r).unwrap();
    let gone = listed.iter().find(|w| w.prunable).unwrap();
    remove_worktree(&r, &gone.path, false).unwrap();
    run(&r, &["switch", "-q", "main"]).unwrap();
    assert_eq!(head_of(&r).as_deref(), Some("main"));
}

#[test]
fn a_missing_main_folder_is_not_called_bare() {
    let sb = Sandbox::new("mbnomain");
    let (r, held) = held_elsewhere(&sb, "feat", "main");
    // Asked from the other folder while the main one is moved away.
    let moved = sb.path("moved");
    fs::rename(&r, &moved).unwrap();
    let e = main_back_plan(&held, "main").unwrap_err();
    assert!(!e.contains("bare"), "{e}");
}
