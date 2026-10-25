//! Search in files, and Go to Definition / References.

use super::*;
use crate::grep;

// One search runs at a time (a newer one stops the older), so these take turns.
static GREP_TURN: Mutex<()> = Mutex::new(());

fn grep_repo(sb: &Sandbox) -> PathBuf {
    let r = sb.path("r");
    init(&r);
    write_commit(
        &r,
        "src/app.ts",
        "const Needle = 1;\nneedle();\nneedles\n",
        "a",
    );
    write_commit(&r, "docs/guide.md", "a needle here\n", "b");
    fs::write(r.join(".gitignore"), "build/\n").unwrap();
    fs::create_dir_all(r.join("build")).unwrap();
    fs::write(r.join("build/out.ts"), "needle\n").unwrap();
    // Untracked, not ignored: searched, as the explorer lists it.
    fs::write(r.join("notes.txt"), "needle in notes\n").unwrap();
    fs::write(r.join("blob.bin"), b"needle\0\x01\x02").unwrap();
    r
}

fn grep_query(text: &str) -> grep::Query {
    grep::Query {
        text: text.into(),
        ..Default::default()
    }
}

fn hits(found: &grep::Found) -> Vec<(String, u32)> {
    found
        .files
        .iter()
        .flat_map(|f| f.hits.iter().map(|h| (f.path.clone(), h.line)))
        .collect()
}

#[test]
fn grep_searches_what_the_explorer_lists() {
    let _turn = GREP_TURN.lock().unwrap_or_else(|e| e.into_inner());
    let sb = Sandbox::new("grep-files");
    let r = grep_repo(&sb);
    let found = grep::search(&r, &grep_query("needle")).unwrap();
    // Any case by default; ignored and binary files left out.
    assert_eq!(
        hits(&found),
        [
            ("docs/guide.md".into(), 1),
            ("notes.txt".into(), 1),
            ("src/app.ts".into(), 1),
            ("src/app.ts".into(), 2),
            ("src/app.ts".into(), 3),
        ]
    );
    assert_eq!(found.count, 5);
    assert!(!found.capped && !found.timed_out);
    assert_eq!(found.files[2].hits[0].text, "const Needle = 1;");
    assert_eq!(grep::search(&r, &grep_query("absent")).unwrap().count, 0);
}

#[test]
fn grep_options_case_word_regex() {
    let _turn = GREP_TURN.lock().unwrap_or_else(|e| e.into_inner());
    let sb = Sandbox::new("grep-options");
    let r = grep_repo(&sb);
    let only_app = |q: grep::Query| {
        let q = grep::Query {
            include: "src".into(),
            ..q
        };
        hits(&grep::search(&r, &q).unwrap())
            .into_iter()
            .map(|(_, l)| l)
            .collect::<Vec<_>>()
    };
    let q = grep_query("Needle");
    assert_eq!(
        only_app(grep::Query {
            match_case: true,
            ..q.clone()
        }),
        [1]
    );
    assert_eq!(
        only_app(grep::Query {
            whole_word: true,
            ..q.clone()
        }),
        [1, 2]
    );
    assert_eq!(
        only_app(grep::Query {
            text: r"needle\(\)|needles$".into(),
            regex: true,
            ..q.clone()
        }),
        [2, 3]
    );
    // Not a regex: its characters are literal.
    assert_eq!(only_app(grep_query("needle()")), [2]);
    // A regex that doesn't parse is git's error, not an empty result.
    let bad = grep::Query {
        text: "(".into(),
        regex: true,
        ..q
    };
    assert!(grep::search(&r, &bad).is_err());
}

#[test]
fn grep_include_and_exclude_globs() {
    let _turn = GREP_TURN.lock().unwrap_or_else(|e| e.into_inner());
    let sb = Sandbox::new("grep-globs");
    let r = grep_repo(&sb);
    let files = |include: &str, exclude: &str| {
        let q = grep::Query {
            include: include.into(),
            exclude: exclude.into(),
            ..grep_query("needle")
        };
        let found = grep::search(&r, &q).unwrap();
        found.files.into_iter().map(|f| f.path).collect::<Vec<_>>()
    };
    assert_eq!(files("*.ts", ""), ["src/app.ts"]);
    assert_eq!(files("*.md, *.txt", ""), ["docs/guide.md", "notes.txt"]);
    // A bare folder name, and a path.
    assert_eq!(files("docs", ""), ["docs/guide.md"]);
    assert_eq!(files("src/**", ""), ["src/app.ts"]);
    assert_eq!(files("", "src, *.md"), ["notes.txt"]);
    assert_eq!(files("*.ts,*.md", "docs"), ["src/app.ts"]);
    // Braces, which git's globs don't have.
    assert_eq!(files("*.{ts,md}", ""), ["docs/guide.md", "src/app.ts"]);
    assert_eq!(files("", "{docs,src}"), ["notes.txt"]);
}

#[test]
fn grep_stops_at_the_cap_and_cuts_long_lines() {
    let _turn = GREP_TURN.lock().unwrap_or_else(|e| e.into_inner());
    let sb = Sandbox::new("grep-cap");
    let r = sb.path("r");
    init(&r);
    let many = "hit\n".repeat(grep::MAX_HITS + 500);
    fs::write(r.join("many.txt"), &many).unwrap();
    let found = grep::search(&r, &grep_query("hit")).unwrap();
    assert!(found.capped);
    assert_eq!(found.count, grep::MAX_HITS);
    // A minified line: the match and a little around it, not megabytes.
    fs::remove_file(r.join("many.txt")).unwrap();
    let line = format!("{}target{}\n", "x".repeat(200_000), "y".repeat(200_000));
    fs::write(r.join("min.js"), line).unwrap();
    let found = grep::search(&r, &grep_query("target")).unwrap();
    let text = &found.files[0].hits[0].text;
    assert!(
        text.contains("target") && text.len() < 400,
        "{}",
        text.len()
    );
}

#[test]
fn grep_a_newer_search_stops_an_older_one() {
    let _turn = GREP_TURN.lock().unwrap_or_else(|e| e.into_inner());
    let sb = Sandbox::new("grep-cancel");
    let r = grep_repo(&sb);
    // Overtaken before it starts: it doesn't run.
    let id = grep::LATEST.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
    grep::LATEST.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    assert_eq!(
        grep::run(&r, &grep_query("needle"), id, true).unwrap_err(),
        grep::CANCELLED
    );
    // The latest one does.
    assert_eq!(grep::search(&r, &grep_query("needle")).unwrap().count, 5);
}

#[test]
fn definitions_are_found_in_other_files_of_the_worktree_and_a_commit() {
    use crate::definitions::{find, Location, Request};
    let sb = Sandbox::new("definitions");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "src/git.rs", "pub fn run() {}\n", "git");
    write_commit(
        &r,
        "src/other.rs",
        "pub fn run() {}\nfn unrelated() {}\n",
        "other",
    );
    let head = log(&r, None, 0, 1).unwrap()[0].sha.clone();
    // Not committed: the worktree has it, the commit doesn't.
    fs::write(r.join("src/new.rs"), "pub struct Fresh;\n").unwrap();
    let at = |text: &str, line, column, rev: Option<&str>| {
        let req = Request {
            path: "src/lib.rs".into(),
            text: text.into(),
            line,
            column,
            rev: rev.map(String::from),
        };
        find(&r, &req).unwrap()
    };
    let loc = |path: &str, line, column, end_column| Location {
        path: path.into(),
        line,
        column,
        end_column,
    };

    // `git::run` names the file: that one alone.
    let src = "mod git;\nfn f() { git::run(); }\n";
    assert_eq!(at(src, 2, 14, None), [loc("src/git.rs", 1, 7, 10)]);
    assert_eq!(at(src, 2, 14, Some(&head)), [loc("src/git.rs", 1, 7, 10)]);
    // Nothing to go by: every file that defines it.
    let src = "fn f() { run(); }\n";
    assert_eq!(
        at(src, 1, 9, None),
        [loc("src/git.rs", 1, 7, 10), loc("src/other.rs", 1, 7, 10)]
    );
    let src = "fn f() -> Fresh { Fresh }\n";
    assert_eq!(at(src, 1, 10, None), [loc("src/new.rs", 1, 11, 16)]);
    assert!(at(src, 1, 10, Some(&head)).is_empty());
    // An import no file defines: the import itself.
    let src = "use outside::Thing;\nfn f(t: Thing) {}\n";
    assert_eq!(at(src, 2, 8, None), [loc("src/lib.rs", 1, 13, 18)]);
}

#[test]
fn references_are_the_uses_that_mean_the_same_definition() {
    use crate::definitions::{references, Request};
    let sb = Sandbox::new("references");
    let r = sb.path("r");
    init(&r);
    let files = [
        ("src/util.ts", "export function helper() {}\n"),
        ("src/other.ts", "export function helper() {}\n"),
        (
            "src/a.ts",
            "import { helper } from \"./util\";\nhelper(); // helper\n",
        ),
        (
            "src/b.ts",
            "import { helper } from \"./other\";\nhelper();\n",
        ),
        (
            "src/c.ts",
            "function f() { const helper = 1; return helper; }\n",
        ),
        ("src/git.rs", "pub fn run() {}\n"),
        ("src/other.rs", "pub fn run() {}\n"),
        ("src/lib.rs", "mod git;\nfn f() { git::run(); }\n"),
        ("src/uses.rs", "use crate::other::run;\nfn g() { run(); }\n"),
    ];
    for (path, text) in files {
        write_commit(&r, path, text, path);
    }
    let head = log(&r, None, 0, 1).unwrap()[0].sha.clone();
    let at = |path: &str, line, column, rev: Option<&str>| {
        let text = fs::read_to_string(r.join(path)).unwrap();
        let req = Request {
            path: path.into(),
            text,
            line,
            column,
            rev: rev.map(String::from),
        };
        let mut found: Vec<String> = references(&r, &req)
            .unwrap()
            .iter()
            .map(|l| format!("{}:{}:{}", l.path, l.line, l.column))
            .collect();
        found.sort();
        found
    };

    // From the definition or a use, the same: its imports and uses, not the other helper's, a
    // local of the same name or the comment.
    let util = ["src/a.ts:1:9", "src/a.ts:2:0", "src/util.ts:1:16"];
    assert_eq!(at("src/util.ts", 1, 16, None), util);
    assert_eq!(at("src/a.ts", 2, 1, None), util);
    assert_eq!(at("src/a.ts", 2, 1, Some(&head)), util);
    assert_eq!(
        at("src/c.ts", 1, 43, None),
        ["src/c.ts:1:21", "src/c.ts:1:40"]
    );
    assert_eq!(
        at("src/git.rs", 1, 7, None),
        ["src/git.rs:1:7", "src/lib.rs:2:14"]
    );
    assert_eq!(
        at("src/other.rs", 1, 7, None),
        ["src/other.rs:1:7", "src/uses.rs:1:18", "src/uses.rs:2:9"]
    );
}
