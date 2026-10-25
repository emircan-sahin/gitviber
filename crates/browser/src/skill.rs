//! The gitviber-browser skill (Settings → Browser): how and when agents use `gitviber browser`,
//! written into Claude Code's and Codex's skills while agent control and the setting are both on.
//! Built from HELP at runtime, so its commands are the ones this build has.

use super::control::HELP;
use serde::Serialize;
use std::io::{Error, ErrorKind};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const TEMPLATE: &str = include_str!("skill.md");
const VERSION: &str = env!("CARGO_PKG_VERSION");
const COMMANDS: &str = "{{COMMANDS}}";
/// What a skill of GitViber's starts with; the marker is the line after its frontmatter.
const HEAD: &str = "---\nname: gitviber-browser\n";
const MARKER_START: &str = "<!-- managed by GitViber ";
const MARKER_END: &str = "; edits are replaced -->";

/// An agent that reads skills: its own folder under home, there only where it's installed, and the
/// skill's file under home.
struct Agent {
    name: &'static str,
    dir: &'static str,
    file: &'static str,
}

// Codex reads ~/.agents/skills; its own ~/.codex/skills is deprecated (codex-rs/ext/skills
// host_roots.rs, developers.openai.com/codex/skills).
const AGENTS: [Agent; 2] = [
    Agent {
        name: "Claude Code",
        dir: ".claude",
        file: ".claude/skills/gitviber-browser/SKILL.md",
    },
    Agent {
        name: "Codex",
        dir: ".codex",
        file: ".agents/skills/gitviber-browser/SKILL.md",
    },
];

/// The SKILL.md: the template with this version in its marker and HELP in full.
pub fn text() -> String {
    text_at(VERSION)
}

fn text_at(version: &str) -> String {
    // A longer fence should HELP ever hold one.
    let fence = if HELP.contains("```") { "````" } else { "```" };
    TEMPLATE
        .replace("{{VERSION}}", version)
        .replace(COMMANDS, &format!("{fence}\n{}\n{fence}", HELP.trim_end()))
}

/// The same for an agent's instructions file (CLAUDE.md, AGENTS.md): a section of it, with no
/// frontmatter or marker, and a pointer to `help` in place of HELP.
pub fn note() -> String {
    let body = TEMPLATE
        .split_once(MARKER_END)
        .map_or(TEMPLATE, |(_, body)| body);
    let body = body.trim();
    let body = body.strip_prefix("# GitViber browser").unwrap_or(body);
    format!("## Browser{}", body.replace("\n## ", "\n### ")).replace(
        COMMANDS,
        "Run `gitviber browser help` for every command and its options.",
    )
}

type Version = (u64, u64, u64);

/// 0.1.13 as (0, 1, 13); a pre-release counts as its release.
fn version(text: &str) -> Option<Version> {
    let core = text.split(['-', '+']).next()?;
    let parts: Vec<u64> = core
        .split('.')
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    match parts[..] {
        [major, minor, patch] => Some((major, minor, patch)),
        _ => None,
    }
}

/// The version of GitViber that wrote `text`, if it's a skill of GitViber's: its name first, and
/// the marker the first line after its frontmatter. A file that only quotes the marker is theirs.
fn owner(text: &str) -> Option<Version> {
    let rest = text.strip_prefix(HEAD)?;
    let body = rest
        .strip_prefix("---\n")
        .or_else(|| rest.split_once("\n---\n").map(|(_, body)| body))?;
    let line = body.lines().next()?;
    version(line.strip_prefix(MARKER_START)?.strip_suffix(MARKER_END)?)
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    /// GitViber's file is there.
    Written,
    /// No file there.
    Absent,
    /// A newer GitViber's file, kept: this one's HELP may be older.
    Newer,
    /// The agent isn't installed: its folder isn't there.
    NoAgent,
    /// A file without the marker: the user's, left alone.
    Theirs,
    Failed,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Target {
    pub agent: &'static str,
    /// Under `~/`, as Settings shows it.
    pub path: String,
    pub status: Status,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Each agent's skill, and why nothing is listed or nothing changes, when that's so.
#[derive(Serialize, Clone, Debug)]
pub struct Skill {
    pub targets: Vec<Target>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Act {
    Report,
    Write,
    Remove,
}

/// Each agent's skill written (`Some(true)`) or removed (`Some(false)`), or only looked at (None),
/// then where it stands.
pub fn sync(on: Option<bool>) -> Skill {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return Skill {
            targets: Vec::new(),
            why: Some("HOME isn't set, so the agents' folders can't be found.".into()),
        };
    };
    // A dev build keeps settings apart from the installed app's, and its HELP may not be released:
    // it would delete or replace the installed app's skill.
    let dev = cfg!(debug_assertions) && std::env::var("GITVIBER_DEV_SKILL").as_deref() != Ok("1");
    let act = match on {
        _ if dev => Act::Report,
        None => Act::Report,
        Some(true) => Act::Write,
        Some(false) => Act::Remove,
    };
    Skill {
        targets: sync_in(&home, act, &text()),
        why: dev.then(|| {
            "A development build only shows the skill; GITVIBER_DEV_SKILL=1 lets it write.".into()
        }),
    }
}

fn sync_in(home: &Path, act: Act, text: &str) -> Vec<Target> {
    // One at a time: a sync sweeps the part files a crash left, which another's write would be.
    static SYNC: Mutex<()> = Mutex::new(());
    let _one = SYNC
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    AGENTS
        .iter()
        .map(|a| {
            let (status, error) = match place(&home.join(a.dir), &home.join(a.file), act, text) {
                Ok(status) => (status, None),
                Err(e) => (Status::Failed, Some(e.to_string())),
            };
            Target {
                agent: a.name,
                path: format!("~/{}", a.file),
                status,
                error,
            }
        })
        .collect()
}

enum Found {
    Absent,
    Ours(Version, String),
    Theirs,
}

fn found(file: &Path) -> std::io::Result<Found> {
    let meta = match std::fs::metadata(file) {
        Ok(meta) => meta,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Found::Absent),
        Err(e) => return Err(e),
    };
    // Never read a FIFO: it waits for a writer, with the lock held.
    if !meta.is_file() {
        return Err(Error::other(format!("{} isn't a file", file.display())));
    }
    let Ok(text) = String::from_utf8(std::fs::read(file)?) else {
        return Ok(Found::Theirs);
    };
    Ok(match owner(&text) {
        Some(v) => Found::Ours(v, text),
        None => Found::Theirs,
    })
}

fn place(dir: &Path, file: &Path, act: Act, text: &str) -> std::io::Result<Status> {
    let installed = dir.is_dir();
    let had = found(file)?;
    let mine = owner(text).ok_or_else(|| Error::other("the skill's text has no marker"))?;
    let folder = file
        .parent()
        .ok_or_else(|| Error::other(format!("{} has no folder", file.display())))?;
    if let Found::Theirs = had {
        return Ok(Status::Theirs);
    }
    if act == Act::Report {
        return Ok(match had {
            _ if !installed => Status::NoAgent,
            Found::Ours(v, _) if v > mine => Status::Newer,
            Found::Ours(..) => Status::Written,
            _ => Status::Absent,
        });
    }
    sweep(folder);
    // Off, or the agent uninstalled: ours goes, whichever version wrote it.
    if act == Act::Remove || !installed {
        if let Found::Ours(..) = had {
            remove(file)?;
        }
        // Only when empty: anything the user put beside it stays.
        let _ = std::fs::remove_dir(folder);
        return Ok(if installed {
            Status::Absent
        } else {
            Status::NoAgent
        });
    }
    match had {
        Found::Ours(v, _) if v > mine => Ok(Status::Newer),
        Found::Ours(_, had) if had == text => Ok(Status::Written),
        _ => {
            std::fs::create_dir_all(folder)?;
            // Whole or not at all: Claude Code reloads a skill as it changes.
            crate::files::place_private(file, text, true)?;
            Ok(Status::Written)
        }
    }
}

/// Gone already counts as removed: another GitViber turned it off at the same time.
fn remove(file: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(file) {
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        removed => removed,
    }
}

/// The part files a write left when the app quit or crashed in the middle of it.
fn sweep(folder: &Path) {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(".SKILL.md.") && name.ends_with(".part") {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::ScratchDir;

    fn home() -> ScratchDir {
        ScratchDir::new("skill-test").unwrap()
    }

    fn claude(home: &Path) -> PathBuf {
        home.join(".claude/skills/gitviber-browser/SKILL.md")
    }

    fn codex(home: &Path) -> PathBuf {
        home.join(".agents/skills/gitviber-browser/SKILL.md")
    }

    fn agents(home: &Path) {
        std::fs::create_dir(home.join(".claude")).unwrap();
        std::fs::create_dir(home.join(".codex")).unwrap();
    }

    fn marker(version: &str) -> String {
        format!("{MARKER_START}{version}{MARKER_END}")
    }

    /// A skill GitViber `version` wrote, with `body` after its marker.
    fn ours_at(version: &str, body: &str) -> String {
        format!("{HEAD}description: x\n---\n{}\n{body}", marker(version))
    }

    fn sync_in(home: &Path, on: bool, text: &str) -> Vec<Target> {
        super::sync_in(home, if on { Act::Write } else { Act::Remove }, text)
    }

    fn report(home: &Path) -> Vec<Status> {
        statuses(&super::sync_in(home, Act::Report, &text()))
    }

    fn statuses(targets: &[Target]) -> Vec<Status> {
        targets.iter().map(|t| t.status).collect()
    }

    #[test]
    fn the_skill_holds_help_whole_after_its_frontmatter_and_versioned_marker() {
        let text = text();
        assert!(text.starts_with(&format!("{HEAD}description: Only in a GitViber terminal")));
        let (frontmatter, body) = text[4..].split_once("\n---\n").unwrap();
        // A colon and a space would end the plain YAML value early.
        let description = frontmatter.lines().nth(1).unwrap();
        assert!(!description["description: ".len()..].contains(": "));
        assert!(body.starts_with(&format!("{}\n", marker(VERSION))));
        assert!(text.contains(&format!("```\n{}\n```", HELP.trim_end())));
        assert!(!text.contains(COMMANDS) && !text.contains("{{"));
        assert_eq!(owner(&text), version(VERSION));
    }

    #[test]
    fn the_note_is_a_section_with_the_rules_but_no_frontmatter_marker_or_help() {
        let note = note();
        assert!(note.starts_with("## Browser\n"));
        assert!(note.contains("GITVIBER_BROWSER_SOCKET") && note.contains("\n### Rules\n"));
        assert!(!note.contains("\n## ") && !note.contains("\n# "));
        assert!(note.contains("`gitviber browser help`"));
        assert!(!note.contains(MARKER_START) && !note.contains("name: gitviber-browser"));
        assert!(!note.contains(COMMANDS) && !note.contains(HELP.lines().next().unwrap()));
    }

    #[test]
    fn versions_compare_as_numbers_and_a_pre_release_as_its_release() {
        assert_eq!(version("0.1.13"), Some((0, 1, 13)));
        assert!(version("0.1.13") > version("0.1.9"));
        assert_eq!(version("0.2.0-beta.1"), Some((0, 2, 0)));
        assert_eq!(version("0.2"), None);
        assert_eq!(version("x.1.2"), None);
    }

    #[test]
    fn only_its_name_then_the_marker_right_after_the_frontmatter_marks_a_file_ours() {
        assert_eq!(owner(&ours_at("0.1.2", "body")), Some((0, 1, 2)));
        assert_eq!(
            owner(&format!("{HEAD}---\n{}\n", marker("1.0.0"))),
            Some((1, 0, 0))
        );
        // Another name, the marker later, indented, or with no version: theirs.
        assert_eq!(
            owner(&format!("---\nname: mine\n---\n{}\n", marker("1.0.0"))),
            None
        );
        assert_eq!(owner(&format!("{HEAD}---\n\n{}\n", marker("1.0.0"))), None);
        assert_eq!(owner(&format!("{HEAD}---\n  {}\n", marker("1.0.0"))), None);
        assert_eq!(owner(&format!("{HEAD}---\n{}\n", marker("dev"))), None);
        assert_eq!(owner(&format!("see `{}`", marker("1.0.0"))), None);
    }

    #[test]
    fn written_for_the_agents_there_and_rewritten_when_the_text_changes() {
        let home = home();
        let h = home.path();
        agents(h);
        let old = ours_at(VERSION, "old help");
        let targets = sync_in(h, true, &old);
        assert_eq!(statuses(&targets), [Status::Written, Status::Written]);
        assert_eq!(
            targets[0].path,
            "~/.claude/skills/gitviber-browser/SKILL.md"
        );
        assert_eq!(std::fs::read_to_string(claude(h)).unwrap(), old);

        let new = text();
        assert_eq!(
            statuses(&sync_in(h, true, &new)),
            [Status::Written, Status::Written]
        );
        assert_eq!(std::fs::read_to_string(claude(h)).unwrap(), new);
        assert_eq!(std::fs::read_to_string(codex(h)).unwrap(), new);
        // No part file left behind.
        let left = std::fs::read_dir(claude(h).parent().unwrap())
            .unwrap()
            .count();
        assert_eq!(left, 1);
    }

    #[test]
    fn an_older_version_is_replaced_and_a_newer_one_kept_until_turned_off() {
        let home = home();
        let h = home.path();
        agents(h);
        let file = claude(h);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, ours_at("0.0.1", "old")).unwrap();
        assert_eq!(
            sync_in(h, true, &text_at("0.1.0"))[0].status,
            Status::Written
        );
        assert_eq!(std::fs::read_to_string(&file).unwrap(), text_at("0.1.0"));

        let newer = ours_at("99.0.0", "newer help");
        std::fs::write(&file, &newer).unwrap();
        assert_eq!(sync_in(h, true, &text_at("0.1.0"))[0].status, Status::Newer);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), newer);
        assert_eq!(
            sync_in(h, false, &text_at("0.1.0"))[0].status,
            Status::Absent
        );
        assert!(!file.exists());
    }

    #[test]
    fn a_report_changes_nothing() {
        let home = home();
        let h = home.path();
        assert_eq!(report(h), [Status::NoAgent, Status::NoAgent]);
        agents(h);
        assert_eq!(report(h), [Status::Absent, Status::Absent]);
        assert!(!claude(h).exists() && !h.join(".agents").exists());

        let old = ours_at("0.0.1", "old");
        let newer = ours_at("99.0.0", "newer");
        for (path, text) in [(claude(h), &old), (codex(h), &newer)] {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, text).unwrap();
        }
        assert_eq!(report(h), [Status::Written, Status::Newer]);
        assert_eq!(std::fs::read_to_string(claude(h)).unwrap(), old);
        assert_eq!(std::fs::read_to_string(codex(h)).unwrap(), newer);
    }

    #[test]
    fn turned_off_removes_its_own_file_and_folder_but_not_what_sits_beside_it() {
        let home = home();
        let h = home.path();
        agents(h);
        sync_in(h, true, &text());
        let beside = codex(h).with_file_name("notes.md");
        std::fs::write(&beside, "mine").unwrap();

        assert_eq!(
            statuses(&sync_in(h, false, &text())),
            [Status::Absent, Status::Absent]
        );
        assert!(!claude(h).parent().unwrap().exists());
        assert!(h.join(".claude/skills").is_dir());
        assert!(!codex(h).exists() && beside.exists());
        assert_eq!(
            statuses(&sync_in(h, false, &text())),
            [Status::Absent, Status::Absent]
        );
    }

    #[test]
    fn codex_uninstalled_takes_our_skill_with_it() {
        let home = home();
        let h = home.path();
        agents(h);
        sync_in(h, true, &text());
        std::fs::remove_dir(h.join(".codex")).unwrap();
        assert_eq!(report(h)[1], Status::NoAgent);
        assert_eq!(sync_in(h, true, &text())[1].status, Status::NoAgent);
        assert!(!codex(h).exists() && h.join(".agents/skills").is_dir());
        assert!(claude(h).exists());
    }

    #[test]
    fn a_skill_of_the_users_own_is_left_alone_on_or_off() {
        let home = home();
        let h = home.path();
        let file = claude(h);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        let mine = "---\nname: gitviber-browser\n---\nmine";
        std::fs::write(&file, mine).unwrap();
        for on in [true, false] {
            assert_eq!(sync_in(h, on, &text())[0].status, Status::Theirs);
            assert_eq!(std::fs::read_to_string(&file).unwrap(), mine);
        }
    }

    #[test]
    fn no_agent_folder_no_skill_and_none_made() {
        let home = home();
        let h = home.path();
        assert_eq!(
            statuses(&sync_in(h, true, &text())),
            [Status::NoAgent, Status::NoAgent]
        );
        assert!(!h.join(".claude").exists() && !h.join(".agents").exists());
        // Codex's skills live outside its folder: with ~/.agents there but no ~/.codex, still none.
        std::fs::create_dir(h.join(".agents")).unwrap();
        assert_eq!(sync_in(h, true, &text())[1].status, Status::NoAgent);
        assert!(!h.join(".agents/skills").exists());
    }

    #[cfg(unix)]
    #[test]
    fn an_unwritable_folder_fails_with_why() {
        use std::os::unix::fs::PermissionsExt;
        let home = home();
        let h = home.path();
        std::fs::create_dir(h.join(".claude")).unwrap();
        std::fs::set_permissions(h.join(".claude"), std::fs::Permissions::from_mode(0o500))
            .unwrap();
        let targets = sync_in(h, true, &text());
        std::fs::set_permissions(h.join(".claude"), std::fs::Permissions::from_mode(0o700))
            .unwrap();
        assert_eq!(targets[0].status, Status::Failed);
        assert!(targets[0].error.is_some());
    }

    #[cfg(unix)]
    #[test]
    fn a_linked_skill_is_replaced_by_a_file_never_written_through() {
        let home = home();
        let h = home.path();
        std::fs::create_dir(h.join(".claude")).unwrap();
        let file = claude(h);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        // Ours, but kept elsewhere (a dotfiles repo): the link goes, what it points at stays.
        let elsewhere = h.join("dotfiles-skill.md");
        let old = ours_at("0.0.1", "old help");
        std::fs::write(&elsewhere, &old).unwrap();
        std::os::unix::fs::symlink(&elsewhere, &file).unwrap();
        assert_eq!(sync_in(h, true, &text())[0].status, Status::Written);
        assert!(!std::fs::symlink_metadata(&file).unwrap().is_symlink());
        assert_eq!(std::fs::read_to_string(&elsewhere).unwrap(), old);

        // Theirs behind a link: neither the link nor its file touched, on or off.
        std::fs::remove_file(&file).unwrap();
        std::fs::write(&elsewhere, "mine").unwrap();
        std::os::unix::fs::symlink(&elsewhere, &file).unwrap();
        for on in [true, false] {
            assert_eq!(sync_in(h, on, &text())[0].status, Status::Theirs);
            assert!(std::fs::symlink_metadata(&file).unwrap().is_symlink());
            assert_eq!(std::fs::read_to_string(&elsewhere).unwrap(), "mine");
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_linked_skills_folder_gets_the_skill_where_it_points() {
        let home = home();
        let h = home.path();
        let shared = h.join("dotfiles/skills");
        std::fs::create_dir_all(&shared).unwrap();
        std::fs::create_dir(h.join(".claude")).unwrap();
        std::os::unix::fs::symlink(&shared, h.join(".claude/skills")).unwrap();
        assert_eq!(sync_in(h, true, &text())[0].status, Status::Written);
        assert!(shared.join("gitviber-browser/SKILL.md").is_file());
        assert_eq!(sync_in(h, false, &text())[0].status, Status::Absent);
        assert!(!shared.join("gitviber-browser").exists() && shared.is_dir());
    }

    #[test]
    fn a_folder_named_skill_md_fails_and_stays() {
        let home = home();
        let h = home.path();
        let file = claude(h);
        std::fs::create_dir_all(file.join("inside")).unwrap();
        for on in [true, false] {
            let target = &sync_in(h, on, &text())[0];
            assert_eq!(target.status, Status::Failed);
            assert!(target.error.is_some());
            assert!(file.join("inside").is_dir());
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_fifo_named_skill_md_fails_without_being_read() {
        let home = home();
        let h = home.path();
        let file = claude(h);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        let path = std::ffi::CString::new(file.to_string_lossy().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
        // Reading it would wait for a writer forever.
        let target = &sync_in(h, true, &text())[0];
        assert_eq!(target.status, Status::Failed);
        assert!(target.error.as_deref().unwrap().ends_with("isn't a file"));
    }

    #[test]
    fn a_file_not_utf8_is_theirs_and_kept() {
        let home = home();
        let h = home.path();
        let file = claude(h);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        let bytes = [ours_at(VERSION, "").as_bytes(), b"\xff\xfe"].concat();
        std::fs::write(&file, &bytes).unwrap();
        for on in [true, false] {
            assert_eq!(sync_in(h, on, &text())[0].status, Status::Theirs);
            assert_eq!(std::fs::read(&file).unwrap(), bytes);
        }
    }

    #[test]
    fn two_writers_at_once_leave_one_whole_skill_and_no_part_files() {
        let home = home();
        let h = home.path().to_path_buf();
        std::fs::create_dir(h.join(".claude")).unwrap();
        let texts = [
            ours_at(VERSION, &"a".repeat(64 * 1024)),
            ours_at(VERSION, &"b".repeat(64 * 1024)),
        ];
        std::thread::scope(|s| {
            for text in &texts {
                let h = &h;
                s.spawn(move || {
                    for _ in 0..200 {
                        let status = sync_in(h, true, text)[0].status;
                        assert_eq!(status, Status::Written);
                    }
                });
            }
        });
        let left = std::fs::read_to_string(claude(&h)).unwrap();
        assert!(texts.contains(&left));
        let entries = std::fs::read_dir(claude(&h).parent().unwrap())
            .unwrap()
            .count();
        assert_eq!(entries, 1);
    }

    #[test]
    fn a_skill_that_only_quotes_the_marker_is_theirs() {
        let home = home();
        let h = home.path();
        let file = claude(h);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        let mine = format!(
            "---\nname: gitviber-browser\ndescription: my notes\n---\n# Mine\n\nGitViber marks its own with:\n\n```\n{}\n```\n",
            marker(VERSION)
        );
        std::fs::write(&file, &mine).unwrap();
        assert_eq!(sync_in(h, false, &text())[0].status, Status::Theirs);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), mine);
    }

    #[test]
    fn a_part_file_left_by_a_crash_is_swept_with_the_skill() {
        let home = home();
        let h = home.path();
        std::fs::create_dir(h.join(".claude")).unwrap();
        sync_in(h, true, &text());
        let folder = claude(h).parent().unwrap().to_path_buf();
        std::fs::write(folder.join(".SKILL.md.0123456789abcdef.part"), "half").unwrap();
        assert_eq!(sync_in(h, false, &text())[0].status, Status::Absent);
        assert!(!folder.exists());
    }

    #[test]
    fn removed_by_another_at_the_same_time_is_no_failure() {
        let home = home();
        assert!(remove(&home.path().join("SKILL.md")).is_ok());
        assert!(remove(home.path()).is_err());
    }
}
