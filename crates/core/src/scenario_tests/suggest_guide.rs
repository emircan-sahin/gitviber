//! Guided reviews (`suggest::run_guide`) against odd commits, odd branches and fake agent CLIs
//! that misbehave: they never read stdin, flood stdout, or leave a child holding the pipe.

use super::*;
use crate::suggest::{self, Agent, Target, CANCELLED};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn guide(repo: &Path, agent: &str, target: Target) -> Result<suggest::Guided, String> {
    suggest::run_guide(
        repo,
        agent,
        "PROMPT",
        &target,
        &Agent::default(),
        &AtomicBool::new(false),
    )
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
    // A binary file is listed, and has no diff to send.
    assert!(g.text.contains("A binary logo.bin [binary]"), "{}", g.text);
    assert!(!g.text.contains("Binary files"), "{}", g.text);
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
    assert!(g.text.len() <= suggest::MAX_GUIDE_INPUT, "{}", g.text.len());
    // Too big to send: listed, and in the patch file only.
    assert!(
        g.text.contains("A +71820 -0 big.txt [file only]"),
        "{}",
        g.text
    );
    assert!(!g.text.contains("+abcdef") && g.text.contains("changes.patch"));
    assert!(g.text.contains("The commit's message:\nBig\n"));
}

/// 2,000 files: every one is listed, and as many diffs as fit are sent whole.
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
    assert!(g.text.len() <= suggest::MAX_GUIDE_INPUT, "{}", g.text.len());
    assert!(g.text.contains("Changed files (2000):"));
    for i in [0, 999, 1999] {
        let path = format!("packages/module-{:02}/src/component_file_{i:04}.ts", i % 40);
        assert!(g.text.contains(&format!("A +1 -0 {path}")), "{path}");
    }
    let whole = g.text.matches("diff --git ").count();
    let file_only = g
        .text
        .lines()
        .filter(|l| l.ends_with("[file only]"))
        .count();
    assert!(
        whole > 1000 && whole + file_only == 2000,
        "{whole} {file_only}"
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
        let out = suggest::run_guide(&r2, "cat", "P", &of(h2), &Agent::default(), &flag);
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
        &Agent::default(),
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
        &Agent::default(),
        &cancel,
    );
    let took = t.elapsed();
    assert!(took < Duration::from_secs(3), "{took:?} {out:?}");
}

/// A commit too big to send whole: a small file and a 2 MB one, whose diff goes only in the
/// patch file. `agent.sh` in the sandbox is the fake agent.
#[cfg(unix)]
fn big_commit(sb: &Sandbox, script: &str) -> (PathBuf, String, String) {
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "one\n", "root");
    fs::write(r.join("small.txt"), "tiny change\n").unwrap();
    fs::write(r.join("huge_only_in_file.txt"), "zz\n".repeat(700 * 1024)).unwrap();
    commit_all(&r, "Big");
    let agent = sb.path("agent.sh");
    fs::write(&agent, script).unwrap();
    let head = rev(&r, "HEAD");
    (r, format!("sh {}", agent.display()), head)
}

/// The patch file's path, as the prompt on stdin names it.
const FIND_PATCH: &str = "f=$(grep -o '/[^ `]*changes[.]patch' | head -1)\n";

#[cfg(unix)]
#[test]
fn an_agent_reads_what_isnt_inline_from_the_patch_file_which_then_goes() {
    let sb = Sandbox::new("guide-patch-file");
    let script = format!(
        "{FIND_PATCH}echo \"$f\"\nls -l \"$f\" | cut -c1-10\necho \"{{\\\"sections\\\": [{{\\\"files\\\": [\\\"$(grep -o 'b/huge_only_in_file.txt' \"$f\" | head -1 | cut -c3-)\\\"]}}]}}\"\n"
    );
    let (r, agent, head) = big_commit(&sb, &script);
    // What a tool-less agent gets: the small file's diff, not the big one's.
    let sent = guide(&r, "cat", of(head.clone())).unwrap().text;
    assert!(
        sent.contains("+tiny change"),
        "{}",
        &sent[..sent.len().min(3000)]
    );
    assert!(sent.contains("huge_only_in_file.txt [file only]") && !sent.contains("+zz"));

    let g = guide(&r, &agent, of(head)).unwrap();
    let mut lines = g.text.lines();
    let file = PathBuf::from(lines.next().unwrap());
    assert!(file.ends_with("changes.patch"), "{}", g.text);
    // Only the user can read it, and it's gone with its folder once the run is over.
    assert_eq!(lines.next(), Some("-rw-------"), "{}", g.text);
    assert_eq!(
        lines.next(),
        Some(r#"{"sections": [{"files": ["huge_only_in_file.txt"]}]}"#),
        "{}",
        g.text
    );
    assert!(!file.exists() && !file.parent().unwrap().exists());
}

#[cfg(unix)]
#[test]
fn the_patch_file_goes_when_the_agent_fails_is_cancelled_or_times_out() {
    let sb = Sandbox::new("guide-patch-gone");
    let record = sb.path("record");
    let script = format!(
        "{FIND_PATCH}echo \"$f\" > {}\n[ \"$1\" = fail ] && exit 3\nsleep 30\n",
        record.display()
    );
    let (r, agent, head) = big_commit(&sb, &script);
    let recorded = || {
        let file = PathBuf::from(fs::read_to_string(&record).unwrap().trim());
        let _ = fs::remove_file(&record);
        assert!(file.ends_with("changes.patch"), "{}", file.display());
        file
    };
    let run = |agent: &str, cancel: &AtomicBool, timeout| {
        suggest::guide_within(
            &r,
            agent,
            "P",
            &of(head.clone()),
            &Agent::default(),
            cancel,
            timeout,
        )
    };

    let err = run(&format!("{agent} fail"), &AtomicBool::new(false), None).unwrap_err();
    assert!(err.contains("code 3"), "{err}");
    assert!(!recorded().exists());

    let cancel = Arc::new(AtomicBool::new(false));
    let c = cancel.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(800));
        c.store(true, std::sync::atomic::Ordering::Relaxed);
    });
    assert_eq!(run(&agent, &cancel, None).unwrap_err(), CANCELLED);
    assert!(!recorded().exists());

    let err = run(
        &agent,
        &AtomicBool::new(false),
        Some(Duration::from_secs(1)),
    )
    .unwrap_err();
    assert!(err.contains("within 1 seconds"), "{err}");
    assert!(!recorded().exists());
}

/// Claude Code is told of the patch's folder with --add-dir, opencode with its permission
/// config; a change sent whole has no patch file, so neither.
#[cfg(unix)]
#[test]
fn the_agent_is_let_read_the_patch_folder_only_when_there_is_one() {
    use crate::suggest::Reads;
    let sb = Sandbox::new("guide-patch-reads");
    let script = "cat >/dev/null\nprintf '%s\\n' \"$@\"\necho \"env:$OPENCODE_PERMISSION\"\n";
    let (r, agent, head) = big_commit(&sb, script);
    let go = |reads, target: Target| {
        let agent_args = Agent {
            args: vec!["--json-schema".into(), "{\"type\": \"object\"}".into()],
            reads,
        };
        suggest::run_guide(
            &r,
            &agent,
            "P",
            &target,
            &agent_args,
            &AtomicBool::new(false),
        )
        .unwrap()
        .text
    };
    let out = go(Some(Reads::Claude), of(head.clone()));
    let mut lines = out.lines();
    assert_eq!(lines.next(), Some("--json-schema"));
    assert_eq!(lines.next(), Some("{\"type\": \"object\"}"));
    let dir = lines.next().unwrap().strip_prefix("--add-dir=").unwrap();
    assert!(
        dir.contains("gitviber-guide-") && !Path::new(dir).exists(),
        "{out}"
    );
    assert_eq!(lines.next(), Some("env:"));

    let out = go(Some(Reads::Opencode), of(head.clone()));
    let env = out.lines().find_map(|l| l.strip_prefix("env:")).unwrap();
    let v: serde_json::Value = serde_json::from_str(env).unwrap();
    let rules = v["external_directory"].as_object().unwrap();
    assert!(
        rules
            .keys()
            .all(|k| k.contains("gitviber-guide-") && k.ends_with("/*"))
            && rules.len() == 1
    );
    assert_eq!(rules.values().next().unwrap(), "allow");

    // A small commit goes whole: no file, no grant.
    write_commit(&r, "small.txt", "another\n", "Small");
    let out = go(Some(Reads::Claude), of(rev(&r, "HEAD")));
    assert!(!out.contains("--add-dir"), "{out}");
}

/// The user's diff settings can't change the patch's shape: without a/ b/ prefixes or in an
/// order file's order, its diffs still pair with the file list and go whole, with hunk names.
#[test]
fn user_diff_settings_dont_turn_a_small_guide_into_the_patch_start() {
    let sb = Sandbox::new("guide-diff-config");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.rs", "fn alpha() {\n    1;\n}\n", "root");
    fs::write(r.join("a.rs"), "fn alpha() {\n    2;\n}\n").unwrap();
    fs::write(r.join("z.txt"), "zed\n").unwrap();
    commit_all(&r, "Two files");
    let head = rev(&r, "HEAD");
    fs::write(sb.path("order"), "z.txt\na.rs\n").unwrap();
    for (key, value) in [
        ("diff.noprefix", "true".to_string()),
        ("diff.orderFile", sb.path("order").display().to_string()),
    ] {
        run(&r, &["config", key, &value]).unwrap();
        let sent = guide(&r, "cat", of(head.clone())).unwrap().text;
        assert!(!sent.contains("the start of the patch"), "{key}: {sent}");
        assert!(!sent.contains("is in the file"), "{key}: {sent}");
        run(&r, &["config", "--unset", key]).unwrap();
    }
}

/// Two guides running at once, each with a patch file: their own folders, both gone after.
#[cfg(unix)]
#[test]
fn two_guides_at_once_each_have_their_own_patch_folder() {
    let sb = Sandbox::new("guide-two-at-once");
    let script = format!("{FIND_PATCH}sleep 1\necho \"$f\"\n");
    let (r, agent, head) = big_commit(&sb, &script);
    let one = {
        let (r, agent, head) = (r.clone(), agent.clone(), head.clone());
        std::thread::spawn(move || guide(&r, &agent, of(head)).unwrap().text)
    };
    let two = guide(&r, &agent, of(head)).unwrap().text;
    let one = one.join().unwrap();
    let (one, two) = (PathBuf::from(one.trim()), PathBuf::from(two.trim()));
    assert!(one.ends_with("changes.patch") && two.ends_with("changes.patch"));
    assert_ne!(one.parent(), two.parent());
    assert!(!one.parent().unwrap().exists() && !two.parent().unwrap().exists());
}

/// Arguments reach the agent as they are: `--tools ""` from a command line is one empty
/// argument, and a schema with spaces and quotes is one argument, never split again.
#[cfg(unix)]
#[test]
fn an_empty_tools_value_and_a_quoted_schema_each_stay_one_argument() {
    let sb = Sandbox::new("guide-argv");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "one\n", "root");
    let agent = sb.path("agent.sh");
    fs::write(&agent, "cat >/dev/null\nprintf '[%s]\\n' \"$@\"\n").unwrap();
    let schema = r#"{"description": "a \"quoted\" word's value", "x": "{prompt}"}"#;
    let out = suggest::run_guide(
        &r,
        &format!("sh {} --tools \"\" --model m", agent.display()),
        "P",
        &of(rev(&r, "HEAD")),
        &Agent {
            args: vec!["--json-schema".into(), schema.into()],
            reads: None,
        },
        &AtomicBool::new(false),
    )
    .unwrap()
    .text;
    assert_eq!(
        out.lines().collect::<Vec<_>>(),
        [
            "[--tools]".to_string(),
            "[]".into(),
            "[--model]".into(),
            "[m]".into(),
            "[--json-schema]".into(),
            format!("[{schema}]"),
        ],
        "{out}"
    );
}
