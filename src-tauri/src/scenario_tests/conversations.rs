//! Resume a conversation against Claude Code project folders the way real ones look: long
//! sessions, renamed ones, folders with odd names, broken and growing files. Every test uses
//! its own temp HOME, never the real ~/.claude.

use crate::conversations::list;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

struct Home(PathBuf);

impl Home {
    fn new(name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("gitviber-convs-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Home(dir)
    }
    /// A folder to run in, and the project folder Claude Code keeps its conversations in.
    fn project(&self, rel: &str) -> (PathBuf, PathBuf) {
        let cwd = self.0.join(rel);
        fs::create_dir_all(&cwd).unwrap();
        let dir = self
            .0
            .join(".claude/projects")
            .join(js_dashed(cwd.to_str().unwrap()));
        fs::create_dir_all(&dir).unwrap();
        (cwd, dir)
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Claude Code's own rule, `path.replace(/[^a-zA-Z0-9]/g, "-")`, over UTF-16 units.
fn js_dashed(path: &str) -> String {
    path.encode_utf16()
        .map(|u| match char::from_u32(u as u32) {
            Some(c) if c.is_ascii_alphanumeric() => c,
            _ => '-',
        })
        .collect()
}

fn id(n: u32) -> String {
    format!("{n:08x}-1111-4222-8333-{:012x}", n as u64 * 7)
}

fn user(text: &str) -> String {
    serde_json::json!({"type": "user", "entrypoint": "cli", "isSidechain": false, "gitBranch": "fix-login", "message": {"role": "user", "content": text}}).to_string()
}

fn assistant(text: &str) -> String {
    serde_json::json!({"type": "assistant", "isSidechain": false, "message": {"role": "assistant", "content": [{"type": "text", "text": text}]}}).to_string()
}

fn write(dir: &Path, name: &str, lines: &[String], secs: u64) {
    let f = dir.join(name);
    fs::write(&f, lines.join("\n") + "\n").unwrap();
    fs::File::options()
        .write(true)
        .open(&f)
        .unwrap()
        .set_modified(UNIX_EPOCH + Duration::from_secs(secs))
        .unwrap();
}

fn titles(cwd: &Path, home: &Path) -> Vec<String> {
    list(cwd, home).into_iter().map(|c| c.title).collect()
}

/// `/rename Login fix` writes a custom-title record. Claude Code's own /resume shows that name;
/// so should this list.
#[test]
fn a_renamed_conversation_shows_the_name_it_was_given() {
    let home = Home::new("renamed");
    let (cwd, dir) = home.project("code/app");
    let lines = [
        user("fix the login redirect loop"),
        assistant("Looking."),
        format!(
            r#"{{"type":"custom-title","customTitle":"Login fix","sessionId":"{}"}}"#,
            id(1)
        ),
    ];
    write(&dir, &format!("{}.jsonl", id(1)), &lines, 1_000);
    assert_eq!(titles(&cwd, &home.0), ["Login fix"]);
}

/// The agent's title is written once the conversation is under way, usually past the first
/// 64 KB (measured: in 213 of 314 real sessions that had one). Claude Code's picker reads the
/// file's tail as well as its head for that reason.
#[test]
fn the_agents_title_written_late_in_a_long_conversation_is_shown() {
    let home = Home::new("late-title");
    let (cwd, dir) = home.project("code/app");
    let mut lines = vec![user("look at the flaky test in ci")];
    for i in 0..60 {
        lines.push(assistant(&format!("step {i}: {}", "output ".repeat(300))));
    }
    lines.push(r#"{"type":"ai-title","aiTitle":"Fix flaky CI test"}"#.into());
    write(&dir, &format!("{}.jsonl", id(2)), &lines, 1_000);
    assert_eq!(titles(&cwd, &home.0), ["Fix flaky CI test"]);
}

/// A first prompt with a pasted log or file is one JSON line longer than the 64 KB read: cut,
/// it doesn't parse, and with no later prompt in the head the conversation isn't listed at all
/// (33 of 400 real sessions here). It should still be there, under some title.
#[test]
fn a_conversation_whose_first_prompt_is_longer_than_the_read_is_still_listed() {
    let home = Home::new("long-prompt");
    let (cwd, dir) = home.project("code/app");
    let pasted = format!(
        "why does this fail?\n{}",
        "at frame (file.ts:1:1)\n".repeat(4000)
    );
    let lines = [
        r#"{"type":"mode","mode":"normal"}"#.to_string(),
        user(&pasted),
        assistant("The stack shows a loop."),
        user("ok fix it"),
    ];
    write(&dir, &format!("{}.jsonl", id(3)), &lines, 1_000);
    assert_eq!(list(&cwd, &home.0).len(), 1, "the conversation is missing");
}

/// GitViber-like tools and scripts run `claude -p` in a project many times a day. Those are
/// skipped, but only after the newest 100 files are taken: 100 of them hide every conversation
/// the user typed.
#[test]
fn many_scripted_runs_dont_hide_the_conversations_typed() {
    let home = Home::new("sdk");
    let (cwd, dir) = home.project("code/app");
    write(
        &dir,
        &format!("{}.jsonl", id(9999)),
        &[user("the one I typed")],
        1_000,
    );
    let sdk = r#"{"type":"user","entrypoint":"sdk-cli","message":{"role":"user","content":"Write a commit message"}}"#;
    for n in 0..120 {
        write(
            &dir,
            &format!("{}.jsonl", id(n)),
            &[sdk.to_string()],
            2_000 + n as u64,
        );
    }
    assert_eq!(titles(&cwd, &home.0), ["the one I typed"]);
}

/// Folders named with spaces, dots, `$`, quotes, accents and emoji map to Claude Code's folder.
#[test]
fn folders_with_odd_names_find_their_conversations() {
    let home = Home::new("odd");
    for (n, rel) in [
        "my app/v1.2",
        "it's \"$HOME\" & co",
        "Masaüstü/çalışma",
        "rocket 🚀/x",
        "dots.../..hidden",
    ]
    .iter()
    .enumerate()
    {
        let (cwd, dir) = home.project(rel);
        write(
            &dir,
            &format!("{}.jsonl", id(n as u32)),
            &[user(&format!("work in {rel}"))],
            1_000,
        );
        assert_eq!(titles(&cwd, &home.0), [format!("work in {rel}")], "{rel}");
    }
}

/// macOS keeps a name as it was typed: decomposed (NFD, as some tools and archives write it) or
/// composed. Claude Code names the folder from its working directory as the system returns it;
/// GitViber may hold the other form of the same path.
#[cfg(target_os = "macos")]
#[test]
fn a_folder_named_in_decomposed_unicode_is_found_from_its_composed_path() {
    let home = Home::new("nfd");
    let nfd = "cafe\u{301}";
    let nfc = "caf\u{e9}";
    let on_disk = home.0.join(nfd);
    fs::create_dir_all(&on_disk).unwrap();
    let real = on_disk.canonicalize().unwrap();
    let dir = home
        .0
        .join(".claude/projects")
        .join(js_dashed(real.to_str().unwrap()));
    fs::create_dir_all(&dir).unwrap();
    write(
        &dir,
        &format!("{}.jsonl", id(1)),
        &[user("accented folder")],
        1_000,
    );
    assert_eq!(titles(&home.0.join(nfc), &home.0), ["accented folder"]);
    assert_eq!(titles(&on_disk, &home.0), ["accented folder"]);
}

/// A file name is typed into a shell as `claude --resume <id>`: anything but a conversation id
/// is never offered, whatever it holds.
#[test]
fn file_names_that_arent_conversation_ids_are_never_offered() {
    let home = Home::new("ids");
    let (cwd, dir) = home.project("code/app");
    for name in [
        "$(touch pwned)",
        "`id`",
        "a;rm -rf ~",
        "0b5f6c1e-2d3a-4e5f-8a9b-1c2d3e4f5a6b; echo hi",
        "0b5f6c1e-2d3a-4e5f-8a9b-1c2d3e4f5a6b\nls",
        "-c",
        "../escape",
        "0b5f6c1e-2d3a-4e5f-8a9b-1c2d3e4f5a6",
        "agent-0b5f6c1e",
    ] {
        let _ = fs::write(dir.join(format!("{name}.jsonl")), user("x") + "\n");
    }
    write(&dir, &format!("{}.jsonl", id(5)), &[user("real")], 1_000);
    let got = list(&cwd, &home.0);
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0].command, format!("claude --resume {}", id(5)));
}

/// Empty, meta-only, sidechain-only, tool-results-only, garbage and cut-off files, and a folder
/// named like one: none listed, none fails the rest.
#[test]
fn broken_and_empty_conversation_files_are_skipped() {
    let home = Home::new("broken");
    let (cwd, dir) = home.project("code/app");
    let bodies: [&[u8]; 8] = [
        b"",
        b"\n\n\n",
        br#"{"type":"mode","mode":"normal"}
{"type":"user","isMeta":true,"message":{"role":"user","content":"<local-command-caveat>x</local-command-caveat>"}}"#,
        br#"{"type":"user","isSidechain":true,"message":{"role":"user","content":"a subagent's task"}}"#,
        br#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","content":"ok"}]}}"#,
        b"\xff\xfe\x00garbage\x00\x01{{{{",
        br#"{"type":"user","message":{"role":"user","content":"cut of"#,
        br#"{"type":"user","message":{"role":"user","content":42}}"#,
    ];
    for (n, body) in bodies.iter().enumerate() {
        fs::write(dir.join(format!("{}.jsonl", id(n as u32))), body).unwrap();
    }
    fs::create_dir_all(dir.join(format!("{}.jsonl", id(50)))).unwrap();
    write(
        &dir,
        &format!("{}.jsonl", id(60)),
        &[user("the good one")],
        1,
    );
    assert_eq!(titles(&cwd, &home.0), ["the good one"]);
}

/// The list is read while an agent is still appending to the file, a line cut mid-write.
#[test]
fn a_conversation_being_written_is_listed() {
    let home = Home::new("growing");
    let (cwd, dir) = home.project("code/app");
    let path = dir.join(format!("{}.jsonl", id(7)));
    fs::write(&path, user("keep going") + "\n").unwrap();
    let writer = {
        let path = path.clone();
        std::thread::spawn(move || {
            let mut f = fs::OpenOptions::new().append(true).open(&path).unwrap();
            for i in 0..2000 {
                let line = assistant(&format!("chunk {i}")) + "\n";
                let (a, b) = line.split_at(line.len() / 2);
                f.write_all(a.as_bytes()).unwrap();
                f.write_all(b.as_bytes()).unwrap();
            }
        })
    };
    for _ in 0..50 {
        assert_eq!(titles(&cwd, &home.0), ["keep going"]);
    }
    writer.join().unwrap();
}

/// A project with a thousand conversations and a 20 MB one lists in a blink: only the newest
/// files are read, and only their head.
#[test]
fn a_project_with_many_and_huge_conversations_lists_quickly() {
    let home = Home::new("many");
    let (cwd, dir) = home.project("code/app");
    for n in 0..1000 {
        write(
            &dir,
            &format!("{}.jsonl", id(n)),
            &[user(&format!("task {n}"))],
            1_000 + n as u64,
        );
    }
    let mut huge = vec![user("the huge one")];
    huge.extend((0..2000).map(|i| assistant(&format!("{i} {}", "y".repeat(10_000)))));
    write(&dir, &format!("{}.jsonl", id(5000)), &huge, 9_000);
    let t = std::time::Instant::now();
    let got = list(&cwd, &home.0);
    assert!(
        t.elapsed() < Duration::from_secs(2),
        "took {:?}",
        t.elapsed()
    );
    assert_eq!(got.len(), 100);
    assert_eq!(got[0].title, "the huge one");
    assert_eq!(got[1].title, "task 999");
}
