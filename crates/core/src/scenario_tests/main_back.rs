//! Handing a branch a linked worktree holds back to the main folder.

use super::*;

/// The layout this is for: the main folder on `feat`, `main` held by a linked worktree whose
/// folder name says nothing about it. Returns the repo and that worktree's path.
fn misleading(sb: &Sandbox) -> (PathBuf, PathBuf) {
    let r = sb.path("proj");
    init(&r);
    write_commit(&r, "a.txt", "a\n", "base");
    run(&r, &["switch", "-q", "-c", "feat"]).unwrap();
    write_commit(&r, "f.txt", "f\n", "feature work");
    let vault = sb.path("proj-vault-tiers");
    run(
        &r,
        &["worktree", "add", "-q", vault.to_str().unwrap(), "main"],
    )
    .unwrap();
    (r, vault)
}

#[test]
fn main_goes_back_to_the_main_folder_and_undo_returns_it() {
    let sb = Sandbox::new("mbmove");
    let (r, vault) = misleading(&sb);
    let tip = rev(&r, "main");

    let plan = move_main_back(&vault, "main").unwrap();
    assert!(same_dir(&plan.main, &r) && same_dir(&plan.holder, &vault));
    assert_eq!(plan.main_branch, "feat");
    assert_eq!(on_branch(&r), "main");
    assert!(
        run_text(&vault, &["symbolic-ref", "-q", "HEAD"]).is_err(),
        "the other folder is detached"
    );
    assert_eq!(
        rev(&vault, "HEAD"),
        tip,
        "at main's tip, files as they were"
    );
    assert!(exists(&r, "feat"), "the branch the main folder had stays");
    let list = worktrees(&r).unwrap();
    assert!(list.iter().find(|w| w.main).unwrap().branch.as_deref() == Some("main"));
    assert!(list.iter().any(|w| same_dir(&w.path, &vault) && w.detached));

    undo_main_back(&r, &plan).unwrap();
    assert_eq!(on_branch(&r), "feat");
    assert_eq!(on_branch(&vault), "main");
}

#[test]
fn it_is_refused_while_either_folder_has_changes() {
    let sb = Sandbox::new("mbdirty");
    let (r, vault) = misleading(&sb);

    fs::write(r.join("new.txt"), "x\n").unwrap();
    let e = main_back_plan(&r, "main").unwrap_err();
    assert!(e.starts_with("proj has 1 uncommitted change"), "{e}");
    fs::remove_file(r.join("new.txt")).unwrap();

    fs::write(vault.join("a.txt"), "edited\n").unwrap();
    let e = move_main_back(&r, "main").unwrap_err();
    assert!(
        e.starts_with("proj-vault-tiers has 1 uncommitted change"),
        "{e}"
    );
    assert_eq!(on_branch(&r), "feat", "nothing moved");
    assert_eq!(on_branch(&vault), "main");
}

#[test]
fn it_is_refused_when_the_main_folder_is_detached_or_stopped() {
    let sb = Sandbox::new("mbdetached");
    let (r, _vault) = misleading(&sb);
    fs::write(git_dir(&r).unwrap().join("MERGE_HEAD"), rev(&r, "HEAD")).unwrap();
    let e = main_back_plan(&r, "main").unwrap_err();
    assert!(e.contains("merge is in progress"), "{e}");
    fs::remove_file(git_dir(&r).unwrap().join("MERGE_HEAD")).unwrap();

    run(&r, &["switch", "-q", "--detach"]).unwrap();
    let e = main_back_plan(&r, "main").unwrap_err();
    assert!(e.contains("detached HEAD"), "{e}");
}

#[test]
fn it_is_refused_when_the_branch_is_not_held_elsewhere() {
    let sb = Sandbox::new("mbheld");
    let (r, vault) = misleading(&sb);
    run(&r, &["branch", "free"]).unwrap();
    let e = main_back_plan(&r, "free").unwrap_err();
    assert!(e.contains("isn't checked out in another worktree"), "{e}");
    // Held by the main folder itself: nothing to hand back.
    let e = main_back_plan(&vault, "feat").unwrap_err();
    assert!(e.contains("already checked out in proj"), "{e}");
    assert!(main_back_plan(&r, "../x").is_err());
}

#[test]
fn it_is_refused_while_something_works_in_the_other_folder_or_it_is_gone() {
    let sb = Sandbox::new("mbbusy");
    let (r, vault) = misleading(&sb);
    let live = format!("claude session x (pid {} start now)", std::process::id());
    run(
        &r,
        &[
            "worktree",
            "lock",
            "--reason",
            &live,
            vault.to_str().unwrap(),
        ],
    )
    .unwrap();
    let e = main_back_plan(&r, "main").unwrap_err();
    assert!(
        e.contains("Something is working in proj-vault-tiers"),
        "{e}"
    );
    run(&r, &["worktree", "unlock", vault.to_str().unwrap()]).unwrap();

    fs::remove_dir_all(&vault).unwrap();
    let e = main_back_plan(&r, "main").unwrap_err();
    assert!(e.contains("folder is gone"), "{e}");
}

#[test]
fn a_bare_main_repo_has_no_folder_to_move_to() {
    let sb = Sandbox::new("mbbare");
    sb.remote_with_clones(0);
    let origin = sb.path("origin.git");
    let wt = sb.path("wt");
    run(
        &origin,
        &["worktree", "add", "-q", "-b", "feat", wt.to_str().unwrap()],
    )
    .unwrap();
    let e = main_back_plan(&wt, "feat").unwrap_err();
    assert!(e.contains("bare repository"), "{e}");
}

#[test]
fn undo_stops_when_either_folder_moved_on() {
    let sb = Sandbox::new("mbundo");
    let (r, vault) = misleading(&sb);
    let plan = move_main_back(&r, "main").unwrap();

    write_commit(&r, "m.txt", "m\n", "committed on main since");
    let e = undo_main_back(&r, &plan).unwrap_err();
    assert!(e.contains("new commits"), "{e}");
    assert_eq!(on_branch(&r), "main");

    run(&r, &["switch", "-q", "feat"]).unwrap();
    let e = undo_main_back(&r, &plan).unwrap_err();
    assert!(e.contains("isn't on main any more"), "{e}");
    run(&vault, &["switch", "-q", "main"]).unwrap();
}
