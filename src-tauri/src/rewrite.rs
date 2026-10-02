//! Editing the branch's own history, as GitHub Desktop and lazygit do: reword a commit, squash
//! commits into one, drop some, or move them. An interactive rebase does it from a todo written
//! here, so no editor opens; conflicts stop it as any rebase's do.

use crate::{git, process};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

#[derive(Deserialize, Debug)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Edit {
    Reword {
        sha: String,
        message: String,
    },
    /// `shas` and `onto` become one commit where `onto` is; the others move next to it first. With
    /// a message it's the commit's (squash); without, the oldest one's stays (fixup).
    Squash {
        shas: Vec<String>,
        onto: String,
        message: Option<String>,
    },
    Drop {
        shas: Vec<String>,
    },
    /// Swaps it with the commit after it (`up`) or before it.
    Move {
        sha: String,
        up: bool,
    },
    /// Puts `shas`, oldest first as they were, just before `before` (under it in the list), or on
    /// top without one.
    Reorder {
        shas: Vec<String>,
        before: Option<String>,
    },
}

/// How a rewrite ended: done, waiting on conflicts, or done with the uncommitted changes it set
/// aside conflicting as they came back (git keeps them in the stash too).
#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum Outcome {
    Done,
    Conflicts,
    StashConflicts,
}

impl Outcome {
    /// Conflicts with no rebase waiting are the autostash's.
    fn of(repo: &Path, stopped: bool) -> Self {
        match (stopped, git::operation(repo).is_some()) {
            (false, _) => Outcome::Done,
            (true, true) => Outcome::Conflicts,
            (true, false) => Outcome::StashConflicts,
        }
    }
}

/// One line of the todo.
#[derive(Debug, PartialEq)]
enum Step<'a> {
    Pick(&'a str),
    Fixup(&'a str),
    Drop(&'a str),
    /// Gives the commit made just before this message.
    Message(&'a str),
    /// Marks the commit a squash starts on top of.
    Label,
    /// Makes everything picked since the label one commit, with this one's message and author.
    Gather(&'a str),
    /// Moves a local branch to the commit just made (rebase.updateRefs).
    UpdateRef(&'a str),
}

const LABEL: &str = "gitviber-squash";

/// Makes the edit. `head`: HEAD as the history the edit was picked from showed it. A failure
/// other than conflicts leaves no rebase behind.
pub fn run(repo: &Path, head: &str, edit: &Edit) -> Result<Outcome, String> {
    git::ensure_idle(repo)?;
    git::ensure_head(repo, head)?;
    // Else conflicts at the end couldn't tell the rewrite's from these.
    if git::has_conflicts(repo) {
        return Err("Resolve the conflicted files first.".into());
    }
    // The branch's own line, newest first: merged-in side branches aren't on it.
    let line: Vec<String> = git::run_text(repo, &["rev-list", "--first-parent", "HEAD"])?
        .lines()
        .map(str::to_string)
        .collect();

    // Rewording HEAD is an amend: the index stays as it is.
    if let Edit::Reword { sha, message: m } = edit {
        if line.first() == Some(sha) {
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
            return Ok(Outcome::Done);
        }
    }

    let (oldest, steps) = plan(&line, edit)?;
    let base = line.get(oldest + 1);
    let range = base.map_or("HEAD".to_string(), |b| format!("{b}..HEAD"));
    if !git::run_text(repo, &["rev-list", "--merges", &range])?
        .trim()
        .is_empty()
    {
        return Err("There are merges in the way: rewriting past them would flatten them.".into());
    }
    let stacked = stacked(repo, &line[..=oldest])?;
    let steps = with_refs(steps, &stacked);

    let dir = work_dir(repo)?;
    let mut todo = Vec::with_capacity(steps.len());
    for step in steps {
        todo.push(match step {
            Step::Pick(c) => format!("pick {c}"),
            Step::Fixup(c) => format!("fixup {c}"),
            Step::Drop(c) => format!("drop {c}"),
            Step::Message(m) => {
                let file = dir.join("message");
                std::fs::write(&file, message(m)?).map_err(|e| e.to_string())?;
                format!(
                    "exec git commit --amend --only --allow-empty --no-verify -F {}",
                    quote(&file)
                )
            }
            Step::Label => format!("label {LABEL}"),
            // Not fixup lines: git won't amend a fixup into an empty commit, as one undoing the
            // other makes it, and stops there. Messages stay byte for byte, as a fixup keeps them.
            Step::Gather(c) => format!(
                "exec git reset -q --soft refs/rewritten/{LABEL} && git commit -q --allow-empty --allow-empty-message --cleanup=verbatim --no-verify -C {c}"
            ),
            Step::UpdateRef(r) => format!("update-ref {r}"),
        });
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
    let result = git::stoppable(repo, process::exec(cmd, "git rebase", &[], None, None));
    // A hook, signing, an untracked file in the way: nothing to resolve, so nothing to wait on.
    if result.is_err() && git::operation(repo).is_some() {
        let _ = git::run(repo, &["rebase", "--abort"]);
    }
    // A rebase waiting on conflicts still reads them; the next rewrite starts afresh anyway.
    if git::operation(repo).is_none() {
        let _ = std::fs::remove_dir_all(&dir);
    }
    result.map(|stopped| Outcome::of(repo, stopped))
}

fn message(m: &str) -> Result<String, String> {
    let m = m.trim();
    if m.is_empty() {
        Err("The message can't be empty.".to_string())
    } else {
        Ok(m.to_string())
    }
}

fn position(line: &[String], sha: &str) -> Result<usize, String> {
    line.iter()
        .position(|c| c == sha)
        .ok_or_else(|| "That commit isn't on this branch.".to_string())
}

/// The commits picked, and the index of the oldest.
fn picked<'a>(line: &[String], shas: &'a [String]) -> Result<(HashSet<&'a str>, usize), String> {
    let mut oldest = None;
    for sha in shas {
        oldest = oldest.max(Some(position(line, sha)?));
    }
    let oldest = oldest.ok_or("No commits picked.")?;
    Ok((shas.iter().map(String::as_str).collect(), oldest))
}

/// The commits up to `oldest`, oldest first, as the todo lists them.
fn from(line: &[String], oldest: usize) -> impl Iterator<Item = &str> {
    line[..=oldest].iter().rev().map(String::as_str)
}

/// The todo for `edit` over `line` (newest first), oldest first, and the index in `line` of the
/// oldest commit it replays.
fn plan<'a>(line: &'a [String], edit: &'a Edit) -> Result<(usize, Vec<Step<'a>>), String> {
    match edit {
        Edit::Reword { sha, message } => {
            let i = position(line, sha)?;
            let mut todo: Vec<Step> = from(line, i).map(Step::Pick).collect();
            todo.insert(1, Step::Message(message));
            Ok((i, todo))
        }
        Edit::Drop { shas } => {
            let (picked, oldest) = picked(line, shas)?;
            if picked.len() == line.len() {
                return Err("That would leave the branch with no commits.".into());
            }
            let todo = from(line, oldest)
                .map(|c| match picked.contains(c) {
                    true => Step::Drop(c),
                    false => Step::Pick(c),
                })
                .collect();
            Ok((oldest, todo))
        }
        // As GitHub Desktop's squash.ts: picked commits older than `onto` wait for it, newer ones
        // follow it straight away, and the commits they pass over are replayed after the squash.
        Edit::Squash {
            shas,
            onto,
            message,
        } => {
            let (picked, oldest) = picked(line, shas)?;
            if picked.contains(onto.as_str()) {
                return Err("A commit can't be squashed into itself.".into());
            }
            let oldest = oldest.max(position(line, onto)?);
            let (mut todo, mut group, mut after, mut found) = (vec![], vec![], vec![], false);
            for c in from(line, oldest) {
                if picked.contains(c) || c == onto {
                    found |= c == onto;
                    group.push(c);
                } else if found {
                    after.push(Step::Pick(c));
                } else {
                    todo.push(Step::Pick(c));
                }
            }
            // Starting on the root, there's nothing to label under it: fixups, which an empty
            // result stops.
            if todo.is_empty() && oldest + 1 == line.len() {
                todo.push(Step::Pick(group[0]));
                todo.extend(group[1..].iter().map(|&c| Step::Fixup(c)));
            } else {
                todo.push(Step::Label);
                todo.extend(group.iter().map(|&c| Step::Pick(c)));
                todo.push(Step::Gather(group[0]));
            }
            if let Some(m) = message {
                todo.push(Step::Message(m));
            }
            todo.extend(after);
            Ok((oldest, todo))
        }
        Edit::Move { sha, up } => {
            let i = position(line, sha)?;
            let before = match up {
                true if i == 0 => return Err("The newest commit has nothing after it.".into()),
                true => i.checked_sub(2).map(|j| line[j].as_str()),
                false if i + 1 == line.len() => {
                    return Err("The first commit has nothing before it.".into())
                }
                false => Some(line[i + 1].as_str()),
            };
            reorder(line, std::slice::from_ref(sha), before)
        }
        Edit::Reorder { shas, before } => reorder(line, shas, before.as_deref()),
    }
}

/// As GitHub Desktop's reorder.ts: the moved commits keep their order, and land just before
/// `before` (on top without one).
fn reorder<'a>(
    line: &'a [String],
    shas: &'a [String],
    before: Option<&'a str>,
) -> Result<(usize, Vec<Step<'a>>), String> {
    let (picked, oldest) = picked(line, shas)?;
    if before.is_some_and(|b| picked.contains(b)) {
        return Err("Commits can't move next to themselves.".into());
    }
    let oldest = match before {
        Some(b) => oldest.max(position(line, b)?),
        None => oldest,
    };
    let (mut todo, mut held, mut after, mut found) = (vec![], vec![], vec![], false);
    for c in from(line, oldest) {
        if picked.contains(c) {
            if found {
                todo.push(Step::Pick(c));
            } else {
                held.push(Step::Pick(c));
            }
        } else if Some(c) == before {
            found = true;
            todo.append(&mut held);
            after.push(Step::Pick(c));
        } else if found {
            after.push(Step::Pick(c));
        } else {
            todo.push(Step::Pick(c));
        }
    }
    todo.extend(after);
    todo.extend(held);
    if todo
        .iter()
        .zip(from(line, oldest))
        .all(|(s, c)| *s == Step::Pick(c))
    {
        return Err("The commits are there already.".into());
    }
    Ok((oldest, todo))
}

/// With rebase.updateRefs, the local branches at the replayed commits, which git's own todo
/// would move along with them. Not one checked out anywhere, as git leaves those.
fn stacked(repo: &Path, replayed: &[String]) -> Result<HashMap<String, Vec<String>>, String> {
    let on = git::run_text(repo, &["config", "--type=bool", "rebase.updateRefs"]);
    let mut refs: HashMap<String, Vec<String>> = HashMap::new();
    if on.is_ok_and(|v| v.trim() == "true") {
        let format = "--format=%(objectname)%00%(worktreepath)%00%(refname)";
        for l in git::run_text(repo, &["for-each-ref", format, "refs/heads"])?.lines() {
            // The middle is empty unless the branch is checked out somewhere.
            if let [sha, "", name] = l.split('\0').collect::<Vec<_>>()[..] {
                if replayed.iter().any(|c| c == sha) {
                    refs.entry(sha.to_string())
                        .or_default()
                        .push(name.to_string());
                }
            }
        }
    }
    Ok(refs)
}

/// Puts an update-ref line for each branch in `refs` (by commit) after the commit it was on is
/// made: after its whole squash or message, or, dropped, where it would have been.
fn with_refs<'a>(steps: Vec<Step<'a>>, refs: &'a HashMap<String, Vec<String>>) -> Vec<Step<'a>> {
    let mut out = Vec::with_capacity(steps.len());
    let (mut pending, mut gathering): (Vec<&str>, bool) = (vec![], false);
    for step in steps {
        let next = matches!(step, Step::Label | Step::Drop(_))
            || matches!(step, Step::Pick(_) if !gathering);
        if next {
            out.extend(pending.drain(..).map(Step::UpdateRef));
        }
        match step {
            Step::Pick(c) | Step::Fixup(c) | Step::Drop(c) => {
                pending.extend(refs.get(c).into_iter().flatten().map(String::as_str))
            }
            Step::Label => gathering = true,
            Step::Gather(_) => gathering = false,
            _ => {}
        }
        out.push(step);
    }
    out.extend(pending.drain(..).map(Step::UpdateRef));
    out
}

/// A folder in the git dir for the todo and the messages; they're read until the rebase ends,
/// which a conflict can put off.
fn work_dir(repo: &Path) -> Result<PathBuf, String> {
    let dir = git::git_dir(repo)
        .ok_or("Not a git repository.")?
        .join("gitviber-rewrite");
    // What an earlier rewrite left, if it stopped on conflicts.
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

/// A path as one word for the shell git runs the editor and exec lines with.
fn quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A line of made-up commits, newest first: e d c b a.
    fn line() -> Vec<String> {
        ["e", "d", "c", "b", "a"].map(String::from).to_vec()
    }
    fn shas(s: &[&str]) -> Vec<String> {
        s.iter().map(|c| c.to_string()).collect()
    }
    /// The todo as text, e.g. "pick a, fixup b".
    fn todo(edit: Edit) -> Result<(usize, String), String> {
        let line = line();
        let (oldest, steps) = plan(&line, &edit)?;
        let text = steps
            .iter()
            .map(|s| match s {
                Step::Pick(c) => format!("pick {c}"),
                Step::Fixup(c) => format!("fixup {c}"),
                Step::Drop(c) => format!("drop {c}"),
                Step::Message(m) => format!("message {m}"),
                Step::Label => "label".into(),
                Step::Gather(c) => format!("gather {c}"),
                Step::UpdateRef(r) => format!("update-ref {r}"),
            })
            .collect::<Vec<_>>()
            .join(", ");
        Ok((oldest, text))
    }

    #[test]
    fn squash_gathers_commits_at_the_target() {
        // Into its parent: the single-commit squash.
        let edit = Edit::Squash {
            shas: shas(&["c"]),
            onto: "b".into(),
            message: Some("bc".into()),
        };
        assert_eq!(
            todo(edit).unwrap(),
            (
                3,
                "label, pick b, pick c, gather b, message bc, pick d, pick e".into()
            )
        );
        // Apart: the newer one comes down to the older, d after them.
        let edit = Edit::Squash {
            shas: shas(&["e"]),
            onto: "c".into(),
            message: None,
        };
        assert_eq!(
            todo(edit).unwrap(),
            (2, "label, pick c, pick e, gather c, pick d".into())
        );
        // From both sides: older ones wait for the target, the oldest picked first.
        let edit = Edit::Squash {
            shas: shas(&["e", "a"]),
            onto: "c".into(),
            message: Some("ace".into()),
        };
        assert_eq!(
            todo(edit).unwrap(),
            (
                4,
                "pick b, label, pick a, pick c, pick e, gather a, message ace, pick d".into()
            )
        );
        // Starting on the root: fixups.
        let edit = Edit::Squash {
            shas: shas(&["b"]),
            onto: "a".into(),
            message: None,
        };
        assert_eq!(
            todo(edit).unwrap(),
            (4, "pick a, fixup b, pick c, pick d, pick e".into())
        );
        let edit = Edit::Squash {
            shas: shas(&["c", "d"]),
            onto: "c".into(),
            message: None,
        };
        assert!(todo(edit).is_err());
        let edit = Edit::Squash {
            shas: shas(&["x"]),
            onto: "c".into(),
            message: None,
        };
        assert!(todo(edit).is_err());
    }

    #[test]
    fn drop_reword_and_move() {
        let edit = Edit::Drop {
            shas: shas(&["d", "b"]),
        };
        assert_eq!(
            todo(edit).unwrap(),
            (3, "drop b, pick c, drop d, pick e".into())
        );
        assert!(todo(Edit::Drop { shas: vec![] }).is_err());
        let all = Edit::Drop {
            shas: shas(&["a", "b", "c", "d", "e"]),
        };
        assert!(todo(all).is_err());
        let edit = Edit::Reword {
            sha: "c".into(),
            message: "new".into(),
        };
        assert_eq!(
            todo(edit).unwrap(),
            (2, "pick c, message new, pick d, pick e".into())
        );
        let up = |sha: &str| Edit::Move {
            sha: sha.into(),
            up: true,
        };
        let down = |sha: &str| Edit::Move {
            sha: sha.into(),
            up: false,
        };
        assert_eq!(todo(up("d")).unwrap(), (1, "pick e, pick d".into()));
        assert_eq!(todo(up("c")).unwrap(), (2, "pick d, pick c, pick e".into()));
        assert_eq!(
            todo(down("c")).unwrap(),
            (3, "pick c, pick b, pick d, pick e".into())
        );
        assert!(todo(up("e")).is_err());
        assert!(todo(down("a")).is_err());
    }

    #[test]
    fn reorder_keeps_the_moved_commits_in_order() {
        // To the top, from apart.
        let edit = Edit::Reorder {
            shas: shas(&["b", "d"]),
            before: None,
        };
        assert_eq!(
            todo(edit).unwrap(),
            (3, "pick c, pick e, pick b, pick d".into())
        );
        // Just under c, from both sides of it.
        let edit = Edit::Reorder {
            shas: shas(&["e", "a"]),
            before: Some("c".into()),
        };
        assert_eq!(
            todo(edit).unwrap(),
            (4, "pick b, pick a, pick e, pick c, pick d".into())
        );
        // Where they are already, or next to themselves.
        let edit = Edit::Reorder {
            shas: shas(&["e"]),
            before: None,
        };
        assert!(todo(edit).is_err());
        let edit = Edit::Reorder {
            shas: shas(&["c"]),
            before: Some("d".into()),
        };
        assert!(todo(edit).is_err());
        let edit = Edit::Reorder {
            shas: shas(&["c", "d"]),
            before: Some("d".into()),
        };
        assert!(todo(edit).is_err());
    }

    #[test]
    fn branches_follow_their_commits() {
        let line = line();
        let refs: HashMap<String, Vec<String>> =
            [("b", "refs/heads/at-b"), ("d", "refs/heads/at-d")]
                .map(|(c, r)| (c.to_string(), vec![r.to_string()]))
                .into();
        let text = |edit: Edit| {
            let (_, steps) = plan(&line, &edit).unwrap();
            with_refs(steps, &refs)
                .iter()
                .map(|s| match s {
                    Step::UpdateRef(r) => format!("ref {}", &r[11..]),
                    Step::Label => "label".into(),
                    Step::Pick(c) => format!("pick {c}"),
                    Step::Drop(c) => format!("drop {c}"),
                    Step::Gather(c) => format!("gather {c}"),
                    _ => format!("{s:?}"),
                })
                .collect::<Vec<_>>()
                .join(", ")
        };
        // After the whole squash a branch's commit went into; dropped, at what's under it.
        let squash = Edit::Squash {
            shas: shas(&["c"]),
            onto: "b".into(),
            message: None,
        };
        assert_eq!(
            text(squash),
            "label, pick b, pick c, gather b, ref at-b, pick d, ref at-d, pick e"
        );
        let drop = Edit::Drop { shas: shas(&["d"]) };
        assert_eq!(text(drop), "drop d, ref at-d, pick e");
    }
}
