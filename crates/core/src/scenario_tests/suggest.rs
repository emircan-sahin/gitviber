//! Commit message suggestions from an agent CLI.

use super::*;

/// `cat` as the "agent": what it prints back is exactly what a real CLI would be sent.
#[test]
fn suggestion_input_follows_what_the_commit_takes() {
    use crate::suggest::{self, Scope};
    use std::sync::atomic::AtomicBool;
    let sb = Sandbox::new("suggest");
    let r = sb.path("r");
    init(&r);
    let go = |scope| suggest::run(&r, "cat", "PROMPT", scope, &AtomicBool::new(false));
    // A root commit amends from the empty tree.
    write_commit(&r, "a.txt", "one\n", "first");
    let sent = go(Scope::Amend).unwrap();
    assert!(sent.starts_with("PROMPT\n\n"), "{sent}");
    assert!(sent.contains("+one"), "{sent}");
    assert!(go(Scope::Staged).unwrap_err().contains("no changes"));

    fs::write(r.join("a.txt"), "two\n").unwrap();
    fs::write(r.join("new.txt"), "fresh\n").unwrap();
    // Commit all: tracked edits and untracked files both.
    let sent = go(Scope::All).unwrap();
    assert!(sent.contains("+two") && sent.contains("+fresh"), "{sent}");
    // Staged: only the index.
    stage(&r, &["new.txt".into()]).unwrap();
    let sent = go(Scope::Staged).unwrap();
    assert!(sent.contains("+fresh") && !sent.contains("+two"), "{sent}");

    // Past the cap the diff is cut and the prompt says so.
    fs::write(r.join("big.txt"), "x\n".repeat(80 * 1024)).unwrap();
    stage(&r, &["big.txt".into()]).unwrap();
    let sent = go(Scope::Staged).unwrap();
    assert!(sent.len() <= suggest::MAX_DIFF + 200, "{}", sent.len());
    assert!(sent.contains("cut off at 100 KB"));

    let err = suggest::run(
        &r,
        "no-such-agent-cli -p",
        "P",
        Scope::Staged,
        &AtomicBool::new(false),
    )
    .unwrap_err();
    assert!(err.contains("Couldn't find \"no-such-agent-cli\""), "{err}");
    let err = suggest::run(
        &r,
        "sh -c 'echo not logged in >&2; exit 3'",
        "P",
        Scope::Staged,
        &AtomicBool::new(false),
    )
    .unwrap_err();
    assert!(
        err.contains("code 3") && err.contains("not logged in"),
        "{err}"
    );
}

#[test]
fn pull_suggestion_gets_the_branch_commits_template_and_diff() {
    use crate::suggest;
    use std::sync::atomic::AtomicBool;
    let sb = Sandbox::new("suggest-pull");
    let c = sb.remote_with_clones(1);
    let a = &c[0];
    let base = "refs/remotes/origin/main";
    let go = || suggest::run_pull(a, "cat", "PROMPT", base, &AtomicBool::new(false));
    assert!(go().unwrap_err().contains("no commits"));
    run(a, &["switch", "-q", "-c", "feat"]).unwrap();
    write_commit(a, "b.txt", "bee\n", "Add b");
    write_commit(a, "c.txt", "sea\n", "Add c");
    // Uncommitted work isn't part of the pull request.
    fs::write(a.join("b.txt"), "local\n").unwrap();
    let sent = go().unwrap();
    assert!(
        sent.starts_with("PROMPT\n\nCommits, oldest first:\n- Add b\n- Add c\n"),
        "{sent}"
    );
    assert!(
        sent.contains("+bee") && sent.contains("+sea") && !sent.contains("local"),
        "{sent}"
    );
    assert!(!sent.contains("template"), "{sent}");

    // The template comes from the base, whatever its case.
    let seed = sb.path("seed");
    write_commit(
        &seed,
        ".github/PULL_REQUEST_TEMPLATE.md",
        "## Why\n",
        "template",
    );
    run(&seed, &["push", "-q"]).unwrap();
    run(a, &["fetch", "-q"]).unwrap();
    let sent = go().unwrap();
    assert!(sent.contains("follow its sections:\n## Why\n"), "{sent}");
    assert!(!sent.contains("- template"), "{sent}");
    assert!(suggest::run_pull(a, "cat", "P", "main", &AtomicBool::new(false)).is_err());

    // A long branch keeps its newest subjects, leaving room for the diff.
    for i in 0..200 {
        run(
            a,
            &["commit", "-q", "--allow-empty", "-m", &format!("empty {i}")],
        )
        .unwrap();
    }
    let sent = go().unwrap();
    assert!(
        sent.contains("The newest 200 commits, oldest first:\n- empty 0\n"),
        "{sent}"
    );
    assert!(!sent.contains("- Add c") && sent.contains("+sea"), "{sent}");
}

#[test]
fn cancelling_a_suggestion_stops_the_command_and_its_children() {
    use crate::suggest::{self, Kind, Scope, Suggester};
    let sb = Sandbox::new("suggest-cancel");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "one\n", "first");
    let s = Suggester::default();
    let flag = s.start(Kind::Message);
    let started = std::time::Instant::now();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(200));
        s.cancel(Kind::Message);
    });
    // The grandchild `sleep` keeps stdout open; only killing the group ends it.
    let err =
        suggest::run(&r, "sh -c 'sleep 30 & sleep 30'", "P", Scope::Amend, &flag).unwrap_err();
    assert_eq!(err, suggest::CANCELLED);
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
}

#[test]
fn guide_gets_a_commit_or_the_branch_and_names_the_range() {
    use crate::suggest::{self, Agent, Target};
    use std::sync::atomic::AtomicBool;
    let sb = Sandbox::new("suggest-guide");
    let r = sb.path("r");
    init(&r);
    let go = |target: Target| {
        let none = Agent::default();
        suggest::run_guide(&r, "cat", "PROMPT", &target, &none, &AtomicBool::new(false))
    };
    let sha = |rev: &str| {
        run_text(&r, &["rev-parse", rev])
            .unwrap()
            .trim()
            .to_string()
    };
    // A root commit is described from the empty tree.
    write_commit(&r, "a.txt", "one\n", "Add a\n\nWhy it's here.");
    let root = sha("HEAD");
    let g = go(Target::Commit { sha: root.clone() }).unwrap();
    assert!(
        g.text.starts_with("PROMPT\n\nBelow is what the change is (a commit's message")
            && g.text.contains(
                "\n\nThe commit's message:\nAdd a\n\nWhy it's here.\n\nChanged files (1):\nA +1 -0 a.txt\n\n"
            ),
        "{}",
        g.text
    );
    assert!(g.text.contains("+one") && g.head == root, "{}", g.text);

    run(&r, &["switch", "-q", "-c", "feat"]).unwrap();
    write_commit(&r, "b.txt", "bee\n", "Add b");
    write_commit(&r, "a.txt", "two\n", "Change a");
    // One commit: its own diff only, from its parent.
    let g = go(Target::Commit { sha: sha("HEAD") }).unwrap();
    assert!(
        g.text.contains("+two") && !g.text.contains("+bee"),
        "{}",
        g.text
    );
    assert!(go(Target::Commit { sha: "HEAD".into() }).is_err());

    // The branch: its commits and their diff, not uncommitted work; the range read comes back.
    fs::write(r.join("b.txt"), "local\n").unwrap();
    let g = go(Target::Branch {
        base: "refs/heads/main".into(),
    })
    .unwrap();
    assert!(
        g.text
            .contains("\n\nCommits, oldest first:\n- Add b\n- Change a\n\nChanged files (2):\n"),
        "{}",
        g.text
    );
    assert!(
        g.text.contains("+bee") && g.text.contains("+two") && !g.text.contains("local"),
        "{}",
        g.text
    );
    assert_eq!((g.base, g.head), (root.clone(), sha("HEAD")));
    let err = go(Target::Branch {
        base: "refs/heads/feat".into(),
    })
    .unwrap_err();
    assert!(err.contains("no commits"), "{err}");

    // A pull request: its head since it left its base, titled, whatever HEAD is.
    let feat = sha("feat");
    run(&r, &["checkout", "-q", "--", "b.txt"]).unwrap();
    run(&r, &["switch", "-q", "main"]).unwrap();
    write_commit(&r, "c.txt", "sea\n", "Add c");
    let g = go(Target::Pull {
        base: sha("main"),
        head: feat.clone(),
        title: "Add b, change a".into(),
    })
    .unwrap();
    assert!(
        g.text.contains("\n\nThe pull request's title: Add b, change a\n\nCommits, oldest first:\n- Add b\n- Change a\n\n")
            && g.text.contains("+bee")
            && !g.text.contains("+sea"),
        "{}",
        g.text
    );
    assert_eq!((g.base, g.head), (root, feat));
}
