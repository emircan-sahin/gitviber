//! The gitviber-browser skill (Settings → Browser): how and when agents use `gitviber browser`,
//! written into Claude Code's and Codex's skills while agent control and the setting are both on.
//! Built from HELP at runtime, so its commands are the ones this build has.

use super::control::HELP;
use serde::Serialize;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const TEMPLATE: &str = include_str!("skill.md");
/// Only a file with this line is GitViber's to replace or delete.
const MARKER: &str = "<!-- managed by GitViber; edits are replaced -->";
const COMMANDS: &str = "{{COMMANDS}}";

/// An agent that reads skills: its own folder under home, there only where it's installed, and the
/// skill's file under home.
struct Agent {
    name: &'static str,
    home: &'static str,
    file: &'static str,
}

// Codex reads ~/.agents/skills; its own ~/.codex/skills is deprecated (codex-rs/ext/skills
// host_roots.rs, developers.openai.com/codex/skills).
const AGENTS: [Agent; 2] = [
    Agent {
        name: "Claude Code",
        home: ".claude",
        file: ".claude/skills/gitviber-browser/SKILL.md",
    },
    Agent {
        name: "Codex",
        home: ".codex",
        file: ".agents/skills/gitviber-browser/SKILL.md",
    },
];

/// The SKILL.md: the template with HELP in full.
pub fn text() -> String {
    // A longer fence should HELP ever hold one.
    let fence = if HELP.contains("```") { "````" } else { "```" };
    TEMPLATE.replace(COMMANDS, &format!("{fence}\n{}\n{fence}", HELP.trim_end()))
}

/// The same for an agent's instructions file (CLAUDE.md, AGENTS.md): no frontmatter, and a
/// pointer to `help` in place of HELP.
pub fn note() -> String {
    let body = TEMPLATE
        .split_once(MARKER)
        .map_or(TEMPLATE, |(_, body)| body);
    body.trim().replace(
        COMMANDS,
        "Run `gitviber browser help` for every command and its options.",
    )
}

fn ours(text: &str) -> bool {
    text.lines().any(|l| l.trim() == MARKER)
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    /// GitViber's file is there, as this build writes it.
    Written,
    /// No file there.
    None,
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

/// Each agent's skill written (`on`) or removed, then where it stands. The settings window and
/// the main one both ask: one at a time.
pub fn sync(on: bool) -> Vec<Target> {
    static SYNC: Mutex<()> = Mutex::new(());
    let _one = SYNC
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return Vec::new();
    };
    sync_in(&home, on, &text())
}

fn sync_in(home: &Path, on: bool, text: &str) -> Vec<Target> {
    AGENTS
        .iter()
        .map(|a| {
            let (status, error) = match place(&home.join(a.home), &home.join(a.file), on, text) {
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

fn place(agent_home: &Path, file: &Path, on: bool, text: &str) -> std::io::Result<Status> {
    let had = match std::fs::read_to_string(file) {
        Ok(had) => Some(had),
        Err(e) if e.kind() == ErrorKind::NotFound => None,
        // Not UTF-8: not ours either.
        Err(e) if e.kind() == ErrorKind::InvalidData => return Ok(Status::Theirs),
        Err(e) => return Err(e),
    };
    let folder = file.parent().ok_or(ErrorKind::InvalidInput)?;
    match had {
        Some(had) if !ours(&had) => Ok(Status::Theirs),
        Some(_) if !on => {
            std::fs::remove_file(file)?;
            // Only when empty: anything the user put beside it stays.
            let _ = std::fs::remove_dir(folder);
            Ok(Status::None)
        }
        Some(had) if had == text => Ok(Status::Written),
        // Never makes the agent's own folder: without it, there's no agent to read the skill.
        None if !agent_home.is_dir() => Ok(Status::NoAgent),
        None if !on => Ok(Status::None),
        _ => {
            std::fs::create_dir_all(folder)?;
            write(folder, file, text)?;
            Ok(Status::Written)
        }
    }
}

/// Whole or not at all: Claude Code reloads a skill as it changes, and must never read half of one.
fn write(folder: &Path, file: &Path, text: &str) -> std::io::Result<()> {
    let hex = crate::local_socket::random_hex(8).ok_or(ErrorKind::Other)?;
    let part = folder.join(format!(".SKILL.md.{hex}.part"));
    let placed =
        crate::scratch::write_private(&part, text).and_then(|()| std::fs::rename(&part, file));
    if placed.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    placed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scratch::ScratchDir;

    fn home() -> ScratchDir {
        ScratchDir::new("skill-test").unwrap()
    }

    fn claude(home: &Path) -> PathBuf {
        home.join(".claude/skills/gitviber-browser/SKILL.md")
    }

    fn codex(home: &Path) -> PathBuf {
        home.join(".agents/skills/gitviber-browser/SKILL.md")
    }

    fn statuses(targets: &[Target]) -> Vec<Status> {
        targets.iter().map(|t| t.status).collect()
    }

    #[test]
    fn the_skill_holds_help_whole_after_its_frontmatter_and_marker() {
        let text = text();
        assert!(text.starts_with("---\nname: gitviber-browser\ndescription: "));
        let (frontmatter, body) = text[4..].split_once("\n---\n").unwrap();
        // A colon and a space would end the plain YAML value early.
        let description = frontmatter.lines().nth(1).unwrap();
        assert!(!description["description: ".len()..].contains(": "));
        assert!(body.starts_with(MARKER));
        assert!(text.contains(&format!("```\n{}\n```", HELP.trim_end())));
        assert!(!text.contains(COMMANDS));
        assert!(ours(&text));
    }

    #[test]
    fn the_note_has_the_rules_but_no_frontmatter_marker_or_help() {
        let note = note();
        assert!(note.starts_with("# GitViber browser"));
        assert!(note.contains("GITVIBER_BROWSER_SOCKET") && note.contains("## Rules"));
        assert!(note.contains("`gitviber browser help`"));
        assert!(!note.contains(MARKER) && !note.contains("name: gitviber-browser"));
        assert!(!note.contains(COMMANDS) && !note.contains(HELP.lines().next().unwrap()));
    }

    #[test]
    fn only_a_line_of_its_own_marks_a_file_ours() {
        assert!(ours(&format!("---\nname: x\n---\n{MARKER}\n\nbody")));
        assert!(ours(&format!("  {MARKER}  \n")));
        assert!(!ours("---\nname: gitviber-browser\n---\nmy own"));
        assert!(!ours(&format!("see `{MARKER}` in theirs")));
    }

    #[test]
    fn written_for_the_agents_there_and_rewritten_when_the_text_changes() {
        let home = home();
        let h = home.path();
        std::fs::create_dir(h.join(".claude")).unwrap();
        std::fs::create_dir(h.join(".codex")).unwrap();
        let old = format!("---\n---\n{MARKER}\nold help");
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
    fn turned_off_removes_its_own_file_and_folder_but_not_what_sits_beside_it() {
        let home = home();
        let h = home.path();
        std::fs::create_dir(h.join(".claude")).unwrap();
        std::fs::create_dir(h.join(".codex")).unwrap();
        sync_in(h, true, &text());
        let beside = codex(h).with_file_name("notes.md");
        std::fs::write(&beside, "mine").unwrap();

        assert_eq!(
            statuses(&sync_in(h, false, &text())),
            [Status::None, Status::None]
        );
        assert!(!claude(h).parent().unwrap().exists());
        assert!(h.join(".claude/skills").is_dir());
        assert!(!codex(h).exists() && beside.exists());
        assert_eq!(
            statuses(&sync_in(h, false, &text())),
            [Status::None, Status::None]
        );
    }

    #[test]
    fn a_skill_of_the_users_own_is_left_alone_on_or_off() {
        let home = home();
        let h = home.path();
        let file = claude(h);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "---\nname: gitviber-browser\n---\nmine").unwrap();
        for on in [true, false] {
            assert_eq!(sync_in(h, on, &text())[0].status, Status::Theirs);
            assert_eq!(
                std::fs::read_to_string(&file).unwrap(),
                "---\nname: gitviber-browser\n---\nmine"
            );
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
}
