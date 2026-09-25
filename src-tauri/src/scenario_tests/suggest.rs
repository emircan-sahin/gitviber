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
fn cancelling_a_suggestion_stops_the_command_and_its_children() {
    use crate::suggest::{self, Scope, Suggester};
    let sb = Sandbox::new("suggest-cancel");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "one\n", "first");
    let s = Suggester::default();
    let flag = s.start();
    let started = std::time::Instant::now();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(200));
        s.cancel();
    });
    // The grandchild `sleep` keeps stdout open; only killing the group ends it.
    let err =
        suggest::run(&r, "sh -c 'sleep 30 & sleep 30'", "P", Scope::Amend, &flag).unwrap_err();
    assert_eq!(err, suggest::CANCELLED);
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
}
