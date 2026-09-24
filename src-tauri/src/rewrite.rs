//! Editing the branch's own history, as GitHub Desktop and lazygit do: reword a commit, squash
//! or fixup one into its parent, drop one, or move one past its neighbour. An interactive rebase
//! does it from a todo written here, so no editor opens; conflicts stop it as any rebase's do.

use crate::git;
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Deserialize, Debug)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Edit {
    Reword {
        sha: String,
        message: String,
    },
    /// Into its parent: with a message the two become one commit with it (squash); without,
    /// the parent's message stays (fixup).
    Squash {
        sha: String,
        message: Option<String>,
    },
    Drop {
        sha: String,
    },
    /// Swaps it with the commit after it (`up`) or before it.
    Move {
        sha: String,
        up: bool,
    },
}

impl Edit {
    fn sha(&self) -> &str {
        match self {
            Edit::Reword { sha, .. }
            | Edit::Squash { sha, .. }
            | Edit::Drop { sha }
            | Edit::Move { sha, .. } => sha,
        }
    }
}

/// Makes the edit; true when the rebase stopped on conflicts. `head`: HEAD as the history the
/// edit was picked from showed it.
pub fn run(repo: &Path, head: &str, edit: &Edit) -> Result<bool, String> {
    git::ensure_idle(repo)?;
    git::ensure_head(repo, head)?;
    let sha = edit.sha();
    // The branch's own line, newest first: merged-in side branches aren't on it.
    let line: Vec<String> = git::run_text(repo, &["rev-list", "--first-parent", "HEAD"])?
        .lines()
        .map(str::to_string)
        .collect();
    let i = line
        .iter()
        .position(|c| c == sha)
        .ok_or("That commit isn't on this branch.")?;
    let parent = line.get(i + 1);
    let message = |m: &str| {
        let m = m.trim();
        if m.is_empty() {
            Err("The message can't be empty.".to_string())
        } else {
            Ok(m.to_string())
        }
    };

    // Rewording HEAD is an amend: the index stays as it is.
    if let (Edit::Reword { message: m, .. }, 0) = (edit, i) {
        let m = message(m)?;
        let args = [
            "commit",
            "--amend",
            "--only",
            "--allow-empty",
            "--no-verify",
            "-F",
            "-",
        ];
        git::run_with(repo, &args, &[], Some(m.as_bytes()))?;
        return Ok(false);
    }

    // The oldest commit the edit changes; the rebase replays from its parent.
    let oldest = match edit {
        Edit::Squash { .. } | Edit::Move { up: false, .. } => {
            parent.ok_or("The first commit has nothing before it.")?;
            i + 1
        }
        Edit::Move { up: true, .. } if i == 0 => {
            return Err("The newest commit has nothing after it.".into())
        }
        _ => i,
    };
    let base = line.get(oldest + 1);
    let range = base.map_or("HEAD".to_string(), |b| format!("{b}..HEAD"));
    if !git::run_text(repo, &["rev-list", "--merges", &range])?
        .trim()
        .is_empty()
    {
        return Err("There are merges in the way: rewriting past them would flatten them.".into());
    }

    let dir = work_dir(repo)?;
    let exec_message = |name: &str, m: &str| -> Result<String, String> {
        let file = dir.join(name);
        std::fs::write(&file, message(m)?).map_err(|e| e.to_string())?;
        Ok(format!(
            "exec git commit --amend --only --allow-empty --no-verify -F {}",
            quote(&file)
        ))
    };
    // Oldest first, as the todo lists them.
    let mut todo: Vec<String> = line[..=oldest]
        .iter()
        .rev()
        .map(|c| format!("pick {c}"))
        .collect();
    // Where `sha` sits in the todo.
    let at = oldest - i;
    match edit {
        Edit::Reword { message: m, .. } => todo.insert(at + 1, exec_message("message", m)?),
        Edit::Squash { message: m, .. } => {
            todo[at] = format!("fixup {sha}");
            if let Some(m) = m {
                todo.insert(at + 1, exec_message("message", m)?);
            }
        }
        Edit::Drop { .. } => todo[at] = format!("drop {sha}"),
        Edit::Move { up, .. } => {
            let other = if *up { at + 1 } else { at - 1 };
            todo.swap(at, other);
        }
    }
    let todo_file = dir.join("todo");
    std::fs::write(&todo_file, todo.join("\n") + "\n").map_err(|e| e.to_string())?;

    let mut args = vec!["rebase", "-i", "--autostash", "--empty=drop"];
    match base {
        Some(b) => args.push(b),
        None => args.push("--root"),
    }
    let mut cmd = git::command(repo, &args);
    // git hands the todo it wrote to this "editor", which puts ours in its place.
    cmd.env("GIT_SEQUENCE_EDITOR", format!("cp {}", quote(&todo_file)));
    git::stoppable(repo, git::exec(cmd, "git rebase", &[], None, None))
}

/// A folder in the git dir for the todo and the messages; they're read until the rebase ends,
/// which a conflict can put off.
fn work_dir(repo: &Path) -> Result<PathBuf, String> {
    let dir = git::git_dir(repo)
        .ok_or("Not a git repository.")?
        .join("gitviber-rewrite");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

/// A path as one word for the shell git runs the editor and exec lines with.
fn quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
}
