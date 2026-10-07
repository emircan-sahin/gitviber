//! Guided reviews (`suggest::run_guide`) against odd commits, odd branches and fake agent CLIs
//! that misbehave: they never read stdin, flood stdout, or leave a child holding the pipe.

use super::*;
use crate::suggest::{self, Target, CANCELLED};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn guide(repo: &Path, agent: &str, target: Target) -> Result<suggest::Guided, String> {
    suggest::run_guide(repo, agent, "PROMPT", &target, &AtomicBool::new(false))
}

fn commit_all(repo: &Path, msg: &str) {
    run(repo, &["add", "-A"]).unwrap();
    run(repo, &["commit", "-q", "-m", msg]).unwrap();
}

fn of(sha: String) -> Target {
    Target::Commit { sha }
}

/// Processes whose command line has `marker`, other than pgrep itself.
#[cfg(unix)]
fn alive(marker: &str) -> bool {
    std::process::Command::new("pgrep")
        .args(["-f", marker])
        .output()
        .map(|o| !o.stdout.is_empty())
        .unwrap_or(false)
}

#[test]
fn guide_of_merges_renames_binaries_and_paths_with_spaces() {
    let sb = Sandbox::new("guide-shapes");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "one\n", "root");
    run(&r, &["switch", "-q", "-c", "side"]).unwrap();
    write_commit(&r, "side.txt", "from side\n", "side work");
    run(&r, &["switch", "-q", "main"]).unwrap();
    write_commit(&r, "main.txt", "from main\n", "main work");
    run(&r, &["merge", "-q", "--no-ff", "side", "-m", "Merge side"]).unwrap();
    // A merge reads from its first parent: what the merge brought in, as History's file list has it.
    let g = guide(&r, "cat", of(rev(&r, "HEAD"))).unwrap();
    assert!(
        g.text.contains("+from side") && !g.text.contains("+from main"),
        "{}",
        g.text
    );
    let files = commit_files(&r, &rev(&r, "HEAD")).unwrap();
    assert_eq!(
        files.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(),
        ["side.txt"]
    );

    // A rename, a binary file and a path with spaces in one commit.
    fs::create_dir_all(r.join("my docs")).unwrap();
    run(&r, &["mv", "a.txt", "my docs/renamed a.txt"]).unwrap();
    fs::write(r.join("logo.bin"), [0u8, 159, 146, 150, 0, 1, 2]).unwrap();
    commit_all(&r, "Shapes");
    let g = guide(&r, "cat", of(rev(&r, "HEAD"))).unwrap();
    assert!(
        g.text.contains("rename to my docs/renamed a.txt"),
        "{}",
        g.text
    );
    assert!(g.text.contains("Binary files"), "{}", g.text);
    // Short ids are taken; names are not.
    assert!(guide(&r, "cat", of(rev(&r, "HEAD")[..7].into())).is_ok());
    assert!(guide(&r, "cat", of("main".into())).is_err());
    assert!(guide(&r, "cat", of("--output=/tmp/x".into())).is_err());
}

/// git quotes a non-ASCII path in a diff (`"caf\303\251.txt"`) unless core.quotePath is off,
/// while History's file list (diff-tree -z) has it as UTF-8: the model must see that form, or the
/// guide's file links for it never match.
#[test]
fn guide_names_non_ascii_paths_as_the_file_list_does() {
    let sb = Sandbox::new("guide-unicode");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "one\n", "root");
    write_commit(&r, "café/über.txt", "x\n", "Unicode");
    let g = guide(&r, "cat", of(rev(&r, "HEAD"))).unwrap();
    let files = commit_files(&r, &rev(&r, "HEAD")).unwrap();
    assert_eq!(files[0].path, "café/über.txt");
    assert!(g.text.contains("café/über.txt"), "{}", g.text);
}

#[test]
fn guide_of_a_huge_commit_is_cut_and_says_so() {
    let sb = Sandbox::new("guide-huge");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "one\n", "root");
    // 5 MB of diff in one file.
    let line = "abcdefghijklmnopqrstuvwxyz0123456789abcdefghijklmnopqrstuvwxyz0123456789\n";
    fs::write(r.join("big.txt"), line.repeat(5 * 1024 * 1024 / line.len())).unwrap();
    commit_all(&r, "Big");
    let t = Instant::now();
    let g = guide(&r, "cat", of(rev(&r, "HEAD"))).unwrap();
    assert!(t.elapsed() < Duration::from_secs(20), "{:?}", t.elapsed());
    assert!(g.text.len() <= suggest::MAX_DIFF + 2000, "{}", g.text.len());
    assert!(g.text.contains("cut off at 100 KB"));
    assert!(g.text.contains("The commit's message:\nBig\n"));
}

/// Past ~1,300 files the list alone is over MAX_DIFF: it's kept whole all the same, as the cut
/// note tells the model.
#[test]
fn guide_of_2000_files_keeps_the_whole_file_list_it_promises() {
    let sb = Sandbox::new("guide-many");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "one\n", "root");
    for i in 0..2000 {
        let dir = r.join(format!("packages/module-{:02}/src", i % 40));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(format!("component_file_{i:04}.ts")), "x\n").unwrap();
    }
    commit_all(&r, "Many");
    let t = Instant::now();
    let g = guide(&r, "cat", of(rev(&r, "HEAD"))).unwrap();
    assert!(t.elapsed() < Duration::from_secs(20), "{:?}", t.elapsed());
    assert!(g.text.contains("cut off at 100 KB"));
    assert!(
        g.text.contains("2000 files changed"),
        "the stat's last line"
    );
}

#[test]
fn guide_of_a_branch_detached_empty_behind_or_without_its_base() {
    let sb = Sandbox::new("guide-branch");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "one\n", "root");
    let base = rev(&r, "HEAD");
    run(&r, &["switch", "-q", "-c", "feat/odd-name.v2"]).unwrap();
    write_commit(&r, "b.txt", "bee\n", "Add b");
    let branch = |b: &str| Target::Branch { base: b.into() };

    // Detached at the branch's tip: the same commits and range.
    run(&r, &["switch", "-q", "--detach", "HEAD"]).unwrap();
    let g = guide(&r, "cat", branch("refs/heads/main")).unwrap();
    assert!(g.text.contains("- Add b") && g.text.contains("+bee"));
    assert_eq!((g.base, g.head), (base.clone(), rev(&r, "HEAD")));

    // Behind its base: nothing of its own.
    run(&r, &["switch", "-q", "main"]).unwrap();
    write_commit(&r, "c.txt", "sea\n", "Main moves");
    run(&r, &["switch", "-q", "--detach", &base]).unwrap();
    let err = guide(&r, "cat", branch("refs/heads/main")).unwrap_err();
    assert!(err.contains("no commits"), "{err}");

    // The base deleted, or not a full ref.
    run(&r, &["switch", "-q", "feat/odd-name.v2"]).unwrap();
    run(&r, &["branch", "-q", "-D", "main"]).unwrap();
    let err = guide(&r, "cat", branch("refs/heads/main")).unwrap_err();
    assert!(err.contains("doesn't exist"), "{err}");
    assert!(guide(&r, "cat", branch("main")).is_err());
    assert!(guide(&r, "cat", branch("refs/heads/--help")).is_err());
}

#[cfg(unix)]
#[test]
fn guide_agents_that_ignore_stdin_flood_stdout_or_print_garbage() {
    let sb = Sandbox::new("guide-agents");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "one\n", "root");
    // Over a pipe's buffer, so the writer would block on an agent that never reads.
    fs::write(r.join("big.txt"), "y\n".repeat(60 * 1024)).unwrap();
    commit_all(&r, "Big");
    let head = rev(&r, "HEAD");

    let t = Instant::now();
    let g = guide(&r, "sh -c 'echo \"{}\"'", of(head.clone())).unwrap();
    assert_eq!(g.text, "{}\n");
    assert!(t.elapsed() < Duration::from_secs(5), "{:?}", t.elapsed());

    // 50 MB on stdout is read to its end, so the command doesn't block, and refused.
    let err = guide(
        &r,
        "sh -c 'cat >/dev/null; head -c 52428800 /dev/zero | tr \"\\0\" x'",
        of(head.clone()),
    )
    .unwrap_err();
    assert!(err.contains("over 1 MB"), "{err}");

    // Bytes that aren't UTF-8 read lossily rather than failing.
    let g = guide(&r, "sh -c 'printf \"\\377\\376{\\n\"'", of(head.clone())).unwrap();
    assert!(g.text.ends_with("{\n"), "{:?}", g.text);

    // A failure's stderr tail is the error.
    let err = guide(&r, "sh -c 'echo nope >&2; exit 7'", of(head)).unwrap_err();
    assert!(err.contains("code 7") && err.contains("nope"), "{err}");
}

#[cfg(unix)]
#[test]
fn starting_a_second_guide_cancels_the_first_and_kills_its_children() {
    use crate::suggest::{Kind, Suggester};
    let sb = Sandbox::new("guide-cancel");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "one\n", "root");
    let head = rev(&r, "HEAD");
    let s = Arc::new(Suggester::default());
    let first = s.start(Kind::Guide);
    let (s2, r2, h2) = (s.clone(), r.clone(), head.clone());
    let second = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        let flag = s2.start(Kind::Guide);
        let out = suggest::run_guide(&r2, "cat", "P", &of(h2), &flag);
        s2.finish(Kind::Guide, &flag);
        out
    });
    let t = Instant::now();
    let marker = "sleep 41.7";
    let err = suggest::run_guide(
        &r,
        &format!("sh -c '{marker} & {marker}'"),
        "P",
        &of(head),
        &first,
    )
    .unwrap_err();
    assert_eq!(err, CANCELLED);
    assert!(t.elapsed() < Duration::from_secs(5), "{:?}", t.elapsed());
    // The first's late finish leaves the second's slot alone.
    s.finish(Kind::Guide, &first);
    assert!(second.join().unwrap().unwrap().text.starts_with("P\n\n"));
    // Nothing of the first is left running.
    let gone = Instant::now() + Duration::from_secs(2);
    while alive(marker) && Instant::now() < gone {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(!alive(marker));
}

/// An agent that leaves a background child holding stdout and exits: the run doesn't wait for
/// stdout's end past a Cancel (the child here lives 6 s; a daemon would hang the guide for good).
#[cfg(unix)]
#[test]
fn cancel_ends_a_guide_whose_agent_left_a_child_on_the_pipe() {
    let sb = Sandbox::new("guide-straggler");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "one\n", "root");
    let cancel = Arc::new(AtomicBool::new(false));
    let c = cancel.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(500));
        c.store(true, std::sync::atomic::Ordering::Relaxed);
    });
    let t = Instant::now();
    let out = suggest::run_guide(
        &r,
        "sh -c '(sleep 6.3 &); echo \"{}\"'",
        "P",
        &of(rev(&r, "HEAD")),
        &cancel,
    );
    let took = t.elapsed();
    assert!(took < Duration::from_secs(3), "{took:?} {out:?}");
}
