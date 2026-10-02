//! The past conversations of the agents in agents.json that keep them (`history`), for Resume a
//! conversation: listed for one folder when asked, newest first. Their files' format isn't
//! documented, so this reads little and forgives much: the head and tail of each file only, and
//! any line that isn't the JSON it looks for is skipped.

use crate::agents::{self, Adapter, History};
use serde::Serialize;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// What's read of each file. The first prompt comes after a few small records, but one holding a
/// pasted file can be long.
const HEAD_BYTES: u64 = 64 * 1024;
/// The latest titles are near a long conversation's end: this much of it is read too.
const TAIL_BYTES: u64 = 64 * 1024;
/// The conversations listed, newest first: a much older one is rarely the one to go back to.
const MAX_LISTED: usize = 100;
/// The newest files read to find them, scripted runs among them.
const MAX_READ: usize = 1000;
/// A title past this is cut.
const TITLE_CHARS: usize = 160;
/// Claude Code cuts a project folder's name here and adds a hash of its own.
const CLAUDE_NAME_MAX: usize = 200;

#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Conversation {
    pub agent: String,
    pub id: String,
    /// The agent's own title for it, else its first prompt, on one line.
    pub title: String,
    /// The branch it started on.
    pub branch: Option<String>,
    /// Unix seconds of its last write.
    pub modified: u64,
    /// What resumes it, to run in a terminal in the folder.
    pub command: String,
}

/// The conversations agents had in `cwd`, newest first.
pub fn list(cwd: &Path, home: &Path) -> Vec<Conversation> {
    let mut out: Vec<Conversation> = agents::histories()
        .flat_map(|(adapter, history)| match history {
            History::ClaudeProjects { dir } => claude(adapter, &agents::from_home(dir, home), cwd),
        })
        .collect();
    out.sort_by(|a, b| b.modified.cmp(&a.modified));
    out
}

/// A folder's name under ~/.claude/projects: every UTF-16 unit but an ASCII letter or digit a
/// "-", as Claude Code's `replace(/[^a-zA-Z0-9]/g, "-")` makes it.
fn dashed(path: &str) -> String {
    path.chars()
        .flat_map(|c| {
            let keep = c.is_ascii_alphanumeric();
            std::iter::repeat_n(
                if keep { c } else { '-' },
                if keep { 1 } else { c.len_utf16() },
            )
        })
        .collect()
}

/// The project folders `cwd`'s conversations can be in: as given and with its links resolved
/// (macOS's /tmp is /private/tmp), a long name found by its first 200 characters. The hash Claude
/// Code adds after those isn't documented, so two folders whose names share their first 200
/// characters both match.
fn claude_folders(projects: &Path, cwd: &Path) -> Vec<PathBuf> {
    let mut paths = vec![cwd.to_path_buf()];
    if let Ok(real) = cwd.canonicalize() {
        if real != cwd {
            paths.push(real);
        }
    }
    let mut out = vec![];
    for p in paths {
        let Some(name) = p.to_str().map(dashed) else {
            continue;
        };
        if name.len() <= CLAUDE_NAME_MAX {
            out.push(projects.join(name));
            continue;
        }
        let prefix = format!("{}-", &name[..CLAUDE_NAME_MAX]);
        let Ok(read) = std::fs::read_dir(projects) else {
            continue;
        };
        out.extend(
            read.filter_map(Result::ok)
                .filter(|e| {
                    e.file_name()
                        .to_str()
                        .is_some_and(|n| n.starts_with(&prefix))
                })
                .map(|e| e.path()),
        );
    }
    out.dedup();
    out
}

fn claude(adapter: &Adapter, projects: &Path, cwd: &Path) -> Vec<Conversation> {
    let mut files: Vec<(u64, String, PathBuf)> = claude_folders(projects, cwd)
        .iter()
        .filter_map(|dir| std::fs::read_dir(dir).ok())
        .flatten()
        .filter_map(|e| {
            let e = e.ok()?;
            let path = e.path();
            let id = path
                .file_name()?
                .to_str()?
                .strip_suffix(".jsonl")?
                .to_string();
            let modified = e
                .metadata()
                .ok()?
                .modified()
                .ok()?
                .duration_since(UNIX_EPOCH)
                .ok()?
                .as_secs();
            Some((modified, id, path))
        })
        .collect();
    files.sort_by(|a, b| b.0.cmp(&a.0));
    // Scripted runs are skipped before the newest are counted: a tool running `claude -p` a
    // hundred times a day would hide every conversation typed.
    files
        .into_iter()
        .take(MAX_READ)
        .filter_map(|(modified, id, path)| {
            let command = adapter.resume_command(&id)?;
            let (title, branch) = claude_file(&path)?;
            Some(Conversation {
                agent: adapter.name.clone(),
                id,
                title,
                branch,
                modified,
                command,
            })
        })
        .take(MAX_LISTED)
        .collect()
}

/// A conversation file's head, and its tail where it's longer: a title is written as the
/// conversation goes, so the latest is near the end. Claude Code's own picker reads both.
fn claude_file(path: &Path) -> Option<(String, Option<String>)> {
    use std::io::{Seek, SeekFrom};
    let mut file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let mut read = |from: u64, bytes: u64| -> Option<String> {
        let mut buf = Vec::new();
        file.seek(SeekFrom::Start(from)).ok()?;
        (&mut file).take(bytes).read_to_end(&mut buf).ok()?;
        Some(String::from_utf8_lossy(&buf).into_owned())
    };
    let head = read(0, HEAD_BYTES)?;
    // A program's run (claude -p): skipped, so its tail isn't read.
    if head.contains(r#""entrypoint":"sdk-"#) {
        return None;
    }
    let tail = if len > HEAD_BYTES {
        Some(read(
            len.saturating_sub(TAIL_BYTES).max(HEAD_BYTES),
            TAIL_BYTES,
        )?)
    } else {
        None
    };
    claude_text(&head, tail.as_deref())
}

/// What the lines of a conversation file say of it.
#[derive(Default)]
struct Seen {
    /// A program's run (`claude -p`, the SDKs: an "sdk-…" entrypoint), not a conversation typed.
    scripted: bool,
    /// The first prompt typed, and the branch it was typed on.
    prompt: Option<(String, Option<String>)>,
    /// The latest name `/rename` gave it, and the agent's latest title.
    custom: Option<String>,
    ai: Option<String>,
}

fn scan(text: &str, seen: &mut Seen) {
    // A line cut off by the read fails to parse like any broken one.
    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        match v["type"].as_str() {
            Some("custom-title") => {
                if let Some(t) = v["customTitle"].as_str().and_then(one_line) {
                    seen.custom = Some(t);
                }
            }
            Some("ai-title") => {
                if let Some(t) = v["aiTitle"].as_str().and_then(one_line) {
                    seen.ai = Some(t);
                }
            }
            Some("user") => {
                if v["entrypoint"]
                    .as_str()
                    .is_some_and(|e| e.starts_with("sdk-"))
                {
                    seen.scripted = true;
                }
                if seen.prompt.is_some() || v["isMeta"] == true || v["isSidechain"] == true {
                    continue;
                }
                let text = match &v["message"]["content"] {
                    serde_json::Value::String(s) => Some(s.as_str()),
                    // Tool results come back as user records too, with no text part.
                    serde_json::Value::Array(parts) => parts
                        .iter()
                        .find(|p| p["type"] == "text")
                        .and_then(|p| p["text"].as_str()),
                    _ => None,
                };
                if let Some(title) = text.and_then(prompt_title) {
                    let branch = v["gitBranch"]
                        .as_str()
                        .filter(|b| !b.is_empty())
                        .map(str::to_string);
                    seen.prompt = Some((title, branch));
                }
            }
            _ => {}
        }
    }
}

/// A conversation's title and the branch it started on: the name `/rename` gave it, else the
/// agent's own title, else the first prompt typed. None for one with nothing typed, or a run a
/// program made.
fn claude_text(head: &str, tail: Option<&str>) -> Option<(String, Option<String>)> {
    let mut seen = Seen::default();
    scan(head, &mut seen);
    // A first prompt with a pasted log can be longer than the head: its start is the title.
    // Only where the read cut it: a file that ends mid-line (a crash) has no such prompt.
    let first = match tail {
        Some(_) => seen.prompt.take().or_else(|| cut_prompt(head, &mut seen)),
        None => seen.prompt.take(),
    };
    if let Some(tail) = tail {
        scan(tail, &mut seen);
    }
    if seen.scripted {
        return None;
    }
    let (prompt, branch) = first.or(seen.prompt)?;
    Some((seen.custom.or(seen.ai).unwrap_or(prompt), branch))
}

/// The start of the prompt in the head's last line, which the read cut off.
fn cut_prompt(head: &str, seen: &mut Seen) -> Option<(String, Option<String>)> {
    if head.ends_with('\n') {
        return None;
    }
    let line = head.lines().last()?;
    if line.contains(r#""entrypoint":"sdk-"#) {
        seen.scripted = true;
    }
    if [
        r#""isMeta":true"#,
        r#""isSidechain":true"#,
        r#""type":"assistant""#,
    ]
    .iter()
    .any(|k| line.contains(k))
    {
        return None;
    }
    // A string, or (with an image pasted) a list of parts, the text one among them.
    let content = &line[line.find(r#""content":"#)? + r#""content":"#.len()..];
    let text = match content.strip_prefix('"') {
        Some(text) => text,
        None => &content[content.find(r#""text":""#)? + r#""text":""#.len()..],
    };
    let mut text: String = text.chars().take(TITLE_CHARS * 4).collect();
    // Cut mid-escape (`\`, `\u00`), the string doesn't parse: shorten it until it does.
    for _ in 0..8 {
        if let Ok(s) = serde_json::from_str::<String>(&format!("\"{text}\"")) {
            return prompt_title(&s).map(|t| (t, None));
        }
        text.pop();
    }
    None
}

/// What a typed prompt shows as: a slash command as typed (`/review 12`); the notes Claude Code
/// writes around local commands aren't one.
fn prompt_title(text: &str) -> Option<String> {
    let text = text.trim();
    if text.starts_with("<local-command-") || text.starts_with("<system-reminder>") {
        return None;
    }
    let tag = |name: &str| {
        let open = format!("<{name}>");
        let at = text.find(&open)? + open.len();
        let len = text[at..].find(&format!("</{name}>"))?;
        Some(text[at..at + len].trim())
    };
    if let Some(command) = tag("command-name") {
        // /clear starts over: nothing was asked yet.
        if command == "/clear" {
            return None;
        }
        return one_line(&format!("{command} {}", tag("command-args").unwrap_or("")));
    }
    one_line(text)
}

/// `text` on one line, cut to TITLE_CHARS; None when there's nothing to show.
fn one_line(text: &str) -> Option<String> {
    let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.is_empty() {
        return None;
    }
    Some(match line.char_indices().nth(TITLE_CHARS) {
        Some((at, _)) => format!("{}…", &line[..at]),
        None => line,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "0b5f6c1e-2d3a-4e5f-8a9b-1c2d3e4f5a6b";
    const B: &str = "9e8d7c6b-5a4f-4e3d-9c2b-1a0f9e8d7c6b";

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("gitviber-conv-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn user(text: &str) -> String {
        serde_json::json!({"type": "user", "entrypoint": "cli", "isSidechain": false, "gitBranch": "fix-login", "message": {"role": "user", "content": text}}).to_string()
    }

    #[test]
    fn a_folder_is_named_as_claude_code_names_it() {
        assert_eq!(
            dashed("/Users/dev/my_app.worktrees/fix-1"),
            "-Users-dev-my-app-worktrees-fix-1"
        );
        // An emoji is two UTF-16 units in JavaScript, so two dashes.
        assert_eq!(dashed("/a/ü😀"), "-a----");
    }

    #[test]
    fn the_title_is_the_agents_own_else_the_first_prompt_typed() {
        let head = [
            r#"{"type":"mode","mode":"normal"}"#.to_string(),
            r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"<local-command-caveat>Caveat</local-command-caveat>"}}"#.into(),
            "not json at all".into(),
            user("<command-name>/clear</command-name>\n<command-message>clear</command-message>\n<command-args></command-args>"),
            r#"{"type":"user","isSidechain":true,"message":{"role":"user","content":"a subagent's task"}}"#.into(),
            user("  Fix the login\n  redirect loop  "),
            user("a later prompt"),
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","content":"ok"}]}}"#.into(),
            r#"{"type":"user","message":"#.into(),
        ]
        .join("\n");
        assert_eq!(
            claude_text(&head, None),
            Some((
                "Fix the login redirect loop".into(),
                Some("fix-login".into())
            ))
        );
        let titled = format!(
            "{head}\n{}",
            r#"{"type":"ai-title","aiTitle":"Login redirect fix"}"#
        );
        assert_eq!(claude_text(&titled, None).unwrap().0, "Login redirect fix");
        let parts = r#"{"type":"user","message":{"role":"user","content":[{"type":"image"},{"type":"text","text":"What's this?"}]}}"#;
        assert_eq!(claude_text(parts, None).unwrap().0, "What's this?");
        assert_eq!(
            claude_text(
                &user("<command-name>/review</command-name><command-args>12</command-args>"),
                None
            )
            .unwrap()
            .0,
            "/review 12"
        );
        // Nothing typed, or a program's run: not offered.
        assert_eq!(claude_text(r#"{"type":"mode"}"#, None), None);
        let sdk = r#"{"type":"user","entrypoint":"sdk-cli","message":{"role":"user","content":"Write a commit message"}}"#;
        assert_eq!(claude_text(sdk, None), None);
        let long = "word ".repeat(100);
        assert_eq!(
            claude_text(&user(&long), None).unwrap().0.chars().count(),
            TITLE_CHARS + 1
        );
    }

    #[test]
    fn a_first_prompt_the_read_cut_off_still_gives_the_title() {
        // Claude Code writes "type" before "message"; the read stopped mid-escape.
        let head = format!(
            "{}\n{}",
            r#"{"type":"mode"}"#,
            r#"{"type":"user","entrypoint":"cli","message":{"role":"user","content":"why does this fail?\nat frame \u00"#
        );
        let tail = user("ok fix it");
        assert_eq!(
            claude_text(&head, Some(&tail)).unwrap().0,
            "why does this fail? at frame"
        );
        // Not cut by the read (the file itself ends there): nothing to show.
        assert_eq!(claude_text(&head, None), None);
        let parts = format!(
            "{}\n{}",
            r#"{"type":"mode"}"#,
            r#"{"type":"user","message":{"role":"user","content":[{"type":"image","source":{}},{"type":"text","text":"what's in this log? \"ERR"#
        );
        assert_eq!(
            claude_text(&parts, Some(&tail)).unwrap().0,
            r#"what's in this log? "ERR"#
        );
        let scripted = head.replace(r#""entrypoint":"cli""#, r#""entrypoint":"sdk-ts""#);
        assert_eq!(claude_text(&scripted, Some(&tail)), None);
    }

    #[test]
    fn a_folders_conversations_are_listed_newest_first_with_their_resume() {
        let home = temp("list");
        let cwd = home.join("code/app.worktrees/fix-login");
        std::fs::create_dir_all(&cwd).unwrap();
        let dir = home
            .join(".claude/projects")
            .join(dashed(cwd.to_str().unwrap()));
        std::fs::create_dir_all(&dir).unwrap();
        let write = |name: &str, body: &str, secs: u64| {
            let f = dir.join(name);
            std::fs::write(&f, body).unwrap();
            let t = UNIX_EPOCH + std::time::Duration::from_secs(secs);
            std::fs::File::options()
                .write(true)
                .open(&f)
                .unwrap()
                .set_modified(t)
                .unwrap();
        };
        write(&format!("{A}.jsonl"), &user("older work"), 1_000);
        write(&format!("{B}.jsonl"), &user("newer work"), 2_000);
        // Not a conversation id: never typed into a shell.
        write("--help.jsonl", &user("x"), 3_000);
        write(&format!("{A}.txt"), &user("x"), 3_000);
        let list = list(&cwd, &home);
        let got: Vec<_> = list
            .iter()
            .map(|c| (c.title.as_str(), c.modified, c.command.as_str()))
            .collect();
        assert_eq!(
            got,
            [
                ("newer work", 2_000, format!("claude --resume {B}").as_str()),
                ("older work", 1_000, format!("claude --resume {A}").as_str()),
            ]
        );
        assert_eq!(list[0].agent, "Claude Code");
        assert!(super::list(&home.join("elsewhere"), &home).is_empty());
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn a_long_folder_name_is_found_by_its_start() {
        let home = temp("long");
        let cwd = home.join("x".repeat(220));
        std::fs::create_dir_all(&cwd).unwrap();
        let name = dashed(cwd.to_str().unwrap());
        let dir = home
            .join(".claude/projects")
            .join(format!("{}-1a2b3c", &name[..CLAUDE_NAME_MAX]));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("{A}.jsonl")), user("deep folder")).unwrap();
        assert_eq!(list(&cwd, &home)[0].title, "deep folder");
        let _ = std::fs::remove_dir_all(&home);
    }
}
