//! Undo and redo for the app's own git actions. Each one records how it moved HEAD, the
//! local branches and the tags; undo moves them back, redo forward again. The record lives in memory
//! only: nothing is written to the repo, and commits an undo takes off a branch stay in the
//! object store, where redo (or the reflog) finds them.

use crate::git::{self, run, run_text};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

/// Entries kept per repository.
const KEEP: usize = 50;

/// How the checked-out branch's tip is moved back or forth.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mode {
    /// Keeps index and files: an undone commit's changes come back staged.
    Soft,
    /// Keeps the files, resets the index: what a mixed reset did is undone exactly.
    Mixed,
    /// Updates index and files, refusing to overwrite local changes (`reset --keep`).
    Keep,
}

pub struct Action {
    label: String,
    mode: Mode,
}

impl Action {
    pub fn new(label: impl Into<String>, mode: Mode) -> Self {
        Action {
            label: label.into(),
            mode,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Head {
    Branch(String),
    Detached(String),
}

struct Snapshot {
    head: Head,
    /// Local branch → commit.
    tips: BTreeMap<String, String>,
    /// Tag → what it points at (an annotated tag's own object, which outlives its deletion).
    tags: BTreeMap<String, String>,
    /// `branch.<name>.*` settings, which `git branch -D` deletes along with the branch.
    config: Vec<(String, String)>,
}

/// One branch an action moved. Index 0 is before the action, 1 after; None = no such branch.
#[derive(Clone)]
struct Change {
    branch: String,
    tips: [Option<String>; 2],
    /// Its settings from when it was last deleted, to restore with it.
    config: Vec<(String, String)>,
}

#[derive(Clone)]
struct Entry {
    id: u64,
    label: String,
    time: u64,
    mode: Mode,
    head: [Head; 2],
    changes: Vec<Change>,
    /// Tags it created, deleted or moved: before and after, None = no such tag.
    tags: Vec<(String, [Option<String>; 2])>,
    /// The push target's tip when the entry was last done or undone. Commits it had then
    /// (pulled ones, say) may come off the branch; ones pushed since may not.
    pushed: Option<String>,
}

#[derive(Default)]
struct Stack {
    done: Vec<Entry>,
    undone: Vec<Entry>,
    /// An action stopped on conflicts: its start, until continuing finishes it.
    pending: Option<(Action, Snapshot)>,
    next: u64,
}

#[derive(Default)]
pub struct Journal {
    /// Held for a whole action, so what one moved is never credited to another.
    acting: Mutex<()>,
    stacks: Mutex<HashMap<PathBuf, Stack>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryView {
    pub id: u64,
    pub label: String,
    /// Unix seconds.
    pub time: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    /// Newest first.
    pub undo: Vec<EntryView>,
    /// Next redo first.
    pub redo: Vec<EntryView>,
    /// Why the next undo or redo can't run now, if it can't.
    pub undo_blocked: Option<String>,
    pub redo_blocked: Option<String>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Entry {
    fn view(&self) -> EntryView {
        EntryView {
            id: self.id,
            label: self.label.clone(),
            time: self.time,
        }
    }
}

impl Journal {
    /// Runs an action that may move HEAD, local branches or tags, and records what it moved.
    /// Continuing or aborting an operation that stopped on conflicts completes the entry
    /// of the action that started it.
    pub fn record<T>(
        &self,
        repo: &Path,
        action: Action,
        f: impl FnOnce(&Path) -> Result<T, String>,
    ) -> Result<T, String> {
        let _one = lock(&self.acting);
        let busy = git::operation(repo).is_some();
        let before = snapshot(repo, true);
        let result = f(repo);
        let (Ok(before), Ok(after)) = (before, snapshot(repo, false)) else {
            return result;
        };
        let stopped = git::operation(repo).is_some();
        let pushed = git::pushed_tip(repo);
        let mut stacks = lock(&self.stacks);
        let s = stacks.entry(repo.to_path_buf()).or_default();
        if before.head != after.head || before.tips != after.tips || before.tags != after.tags {
            s.undone.clear();
        }
        // An operation started outside the app has no start to go back to. With none in
        // progress, a pending start is stale: that operation ended in a terminal.
        let pending = s.pending.take();
        let start = if busy {
            pending
        } else {
            Some((action, before))
        };
        match start {
            Some(start) if stopped => s.pending = Some(start),
            Some((action, before)) => {
                if let Some(e) = diff(s.next, action, &before, &after, pushed) {
                    s.next += 1;
                    s.done.push(e);
                    if s.done.len() > KEEP {
                        s.done.remove(0);
                    }
                }
            }
            None => {}
        }
        result
    }

    /// Undoes the newest entry, or redoes (`forward`) the last undone one. `id`, when given,
    /// must be that entry: a toast's Undo must not take back some other action.
    pub fn step(
        &self,
        repo: &Path,
        forward: bool,
        id: Option<u64>,
        index: &Mutex<()>,
    ) -> Result<EntryView, String> {
        let _one = lock(&self.acting);
        let _index = lock(index);
        let mut e = {
            let mut stacks = lock(&self.stacks);
            let s = stacks.entry(repo.to_path_buf()).or_default();
            let list = if forward { &s.undone } else { &s.done };
            let top = list.last().ok_or(if forward {
                "Nothing to redo."
            } else {
                "Nothing to undo."
            })?;
            if id.is_some_and(|id| id != top.id) {
                return Err(
                    "That is no longer the latest action. Open the undo history to pick it.".into(),
                );
            }
            top.clone()
        };
        let (from, to) = if forward { (0, 1) } else { (1, 0) };
        apply(repo, &mut e, from, to)?;
        e.pushed = git::pushed_tip(repo);
        let view = e.view();
        let mut stacks = lock(&self.stacks);
        let s = stacks.entry(repo.to_path_buf()).or_default();
        let (src, dst) = if forward {
            (&mut s.undone, &mut s.done)
        } else {
            (&mut s.done, &mut s.undone)
        };
        src.pop();
        dst.push(e);
        Ok(view)
    }

    /// The newest entry's id: comparing it before and after an action tells whether that
    /// action recorded one.
    pub fn last(&self, repo: &Path) -> Option<u64> {
        lock(&self.stacks)
            .get(repo)
            .and_then(|s| s.done.last())
            .map(|e| e.id)
    }

    pub fn view(&self, repo: &Path) -> View {
        let (undo_top, redo_top, undo, redo) = {
            let stacks = lock(&self.stacks);
            let Some(s) = stacks.get(repo) else {
                return View {
                    undo: vec![],
                    redo: vec![],
                    undo_blocked: None,
                    redo_blocked: None,
                };
            };
            (
                s.done.last().cloned(),
                s.undone.last().cloned(),
                s.done.iter().rev().map(Entry::view).collect(),
                s.undone.iter().rev().map(Entry::view).collect(),
            )
        };
        View {
            undo,
            redo,
            undo_blocked: undo_top.and_then(|e| blocked(repo, &e, 1, 0)),
            redo_blocked: redo_top.and_then(|e| blocked(repo, &e, 0, 1)),
        }
    }
}

fn snapshot(repo: &Path, with_config: bool) -> Result<Snapshot, String> {
    let head = match run_text(repo, &["symbolic-ref", "-q", "HEAD"]) {
        Ok(r) => Head::Branch(r.trim().trim_start_matches("refs/heads/").to_string()),
        Err(_) => Head::Detached(
            run_text(repo, &["rev-parse", "--verify", "-q", "HEAD"])?
                .trim()
                .to_string(),
        ),
    };
    let refs = run_text(
        repo,
        &[
            "for-each-ref",
            "--format=%(objectname) %(refname)",
            "refs/heads",
            "refs/tags",
        ],
    )?;
    let (mut tips, mut tags) = (BTreeMap::new(), BTreeMap::new());
    for (sha, name) in refs.lines().filter_map(|l| l.split_once(' ')) {
        if let Some(b) = name.strip_prefix("refs/heads/") {
            tips.insert(b.to_string(), sha.to_string());
        } else if let Some(t) = name.strip_prefix("refs/tags/") {
            tags.insert(t.to_string(), sha.to_string());
        }
    }
    let config = if with_config {
        branch_config(repo, None)
    } else {
        vec![]
    };
    Ok(Snapshot {
        head,
        tips,
        tags,
        config,
    })
}

/// The repo's `branch.*` settings, or only `name`'s. Exits 1 when there are none.
fn branch_config(repo: &Path, name: Option<&str>) -> Vec<(String, String)> {
    let all = run_text(
        repo,
        &["config", "--local", "-z", "--get-regexp", r"^branch\."],
    )
    .unwrap_or_default();
    let pairs = all.split('\0').filter_map(|rec| {
        let (k, v) = rec.split_once('\n')?;
        Some((k.to_string(), v.to_string()))
    });
    match name {
        Some(n) => pairs.filter(|(k, _)| owns(k, n)).collect(),
        None => pairs.collect(),
    }
}

/// `branch.feat.remote` is feat's; `branch.feat.x.remote` is feat.x's.
fn owns(key: &str, branch: &str) -> bool {
    key.strip_prefix("branch.")
        .and_then(|k| k.strip_prefix(branch))
        .and_then(|k| k.strip_prefix('.'))
        .is_some_and(|var| !var.contains('.'))
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// The entry for going from `before` to `after`, or None when nothing moved.
fn diff(
    id: u64,
    action: Action,
    before: &Snapshot,
    after: &Snapshot,
    pushed: Option<String>,
) -> Option<Entry> {
    let names: BTreeSet<&String> = before.tips.keys().chain(after.tips.keys()).collect();
    let changes: Vec<Change> = names
        .into_iter()
        .filter(|n| before.tips.get(*n) != after.tips.get(*n))
        .map(|n| {
            let gone = !after.tips.contains_key(n);
            Change {
                branch: n.clone(),
                tips: [before.tips.get(n).cloned(), after.tips.get(n).cloned()],
                config: if gone {
                    before
                        .config
                        .iter()
                        .filter(|(k, _)| owns(k, n))
                        .cloned()
                        .collect()
                } else {
                    vec![]
                },
            }
        })
        .collect();
    let names: BTreeSet<&String> = before.tags.keys().chain(after.tags.keys()).collect();
    let tags: Vec<_> = names
        .into_iter()
        .filter(|n| before.tags.get(*n) != after.tags.get(*n))
        .map(|n| {
            (
                n.clone(),
                [before.tags.get(n).cloned(), after.tags.get(n).cloned()],
            )
        })
        .collect();
    if changes.is_empty() && tags.is_empty() && before.head == after.head {
        return None;
    }
    Some(Entry {
        id,
        label: action.label,
        time: now(),
        mode: action.mode,
        head: [before.head.clone(), after.head.clone()],
        changes,
        tags,
        pushed,
    })
}

/// Why moving `e` from state `from` to state `to` (0 before, 1 after) isn't safe now.
fn blocked(repo: &Path, e: &Entry, from: usize, to: usize) -> Option<String> {
    if let Some(op) = git::operation(repo) {
        return Some(format!(
            "A {} is in progress. Continue or abort it first.",
            op.kind
        ));
    }
    let now = match snapshot(repo, false) {
        Ok(s) => s,
        Err(err) => return Some(err),
    };
    if now.head != e.head[from]
        || e.changes
            .iter()
            .any(|c| now.tips.get(&c.branch) != c.tips[from].as_ref())
        || e.tags
            .iter()
            .any(|(t, at)| now.tags.get(t) != at[from].as_ref())
    {
        return Some(
            "The repository changed outside GitViber since then (in a terminal, say), so this can't be done safely."
                .into(),
        );
    }
    // Taking commits off the checked-out branch that were pushed since would need a force push.
    let Head::Branch(b) = &e.head[to] else {
        return None;
    };
    let c = e.changes.iter().find(|c| &c.branch == b)?;
    if e.head[from] != e.head[to] {
        return None;
    }
    let current = c.tips[from].as_deref()?;
    let mut keep: Vec<&str> = c.tips[to].iter().map(String::as_str).collect();
    keep.extend(e.pushed.as_deref());
    git::drops_pushed_from(repo, current, &keep)
        .unwrap_or(false)
        .then(|| {
            format!("It would take pushed commits off {b}, which needs a force push. Use Reset in History if that's what you want.")
        })
}

fn apply(repo: &Path, e: &mut Entry, from: usize, to: usize) -> Result<(), String> {
    if let Some(why) = blocked(repo, e, from, to) {
        return Err(why);
    }
    let current = match &e.head[from] {
        Head::Branch(b) => Some(b.clone()),
        Head::Detached(_) => None,
    };
    // Tags, then other branches: a switch needs its target to exist. On failure, put them back.
    let mut tagged = 0;
    let mut result = Ok(());
    for (t, at) in &e.tags {
        result = set_tag(repo, t, at[to].as_deref());
        if result.is_err() {
            break;
        }
        tagged += 1;
    }
    let mut moved = vec![];
    for i in 0..e.changes.len() {
        if result.is_err() {
            break;
        }
        if Some(&e.changes[i].branch) == current.as_ref() {
            continue;
        }
        result = set_branch(repo, &mut e.changes[i], from, to);
        if result.is_ok() {
            moved.push(i);
        }
    }
    if result.is_ok() {
        result = move_head(repo, e, from, to);
    }
    if result.is_err() {
        for &i in moved.iter().rev() {
            let _ = set_branch(repo, &mut e.changes[i], to, from);
        }
        for (t, at) in e.tags[..tagged].iter().rev() {
            let _ = set_tag(repo, t, at[from].as_deref());
        }
    }
    result
}

fn move_head(repo: &Path, e: &mut Entry, from: usize, to: usize) -> Result<(), String> {
    let change = |b: &str| e.changes.iter().position(|c| c.branch == b);
    match (&e.head[from], &e.head[to]) {
        (Head::Branch(a), Head::Branch(b)) if a == b => match change(b) {
            Some(i) => match &e.changes[i].tips[to] {
                Some(sha) => reset(repo, e.mode, sha),
                // Back before the branch's first commit: unborn again, the index kept.
                None => run(repo, &["update-ref", "-d", "HEAD"]).map(|_| ()),
            },
            None => Ok(()),
        },
        (Head::Detached(a), Head::Detached(b)) if a == b => Ok(()),
        (Head::Detached(_), Head::Detached(b)) => reset(repo, e.mode, b),
        (_, target) => {
            match target {
                Head::Branch(b) => run(repo, &["switch", b]),
                Head::Detached(sha) => run(repo, &["switch", "--detach", sha]),
            }?;
            // The branch that was checked out is free to move now.
            let old = match &e.head[from] {
                Head::Branch(a) => change(a),
                Head::Detached(_) => None,
            };
            match old {
                Some(i) => set_branch(repo, &mut e.changes[i], from, to),
                None => Ok(()),
            }
        }
    }
}

fn reset(repo: &Path, mode: Mode, sha: &str) -> Result<(), String> {
    let flag = match mode {
        Mode::Soft => "--soft",
        Mode::Mixed => "--mixed",
        Mode::Keep => "--keep",
    };
    run(repo, &["reset", "-q", flag, sha]).map(|_| ())
}

/// Moves, creates or deletes a branch that isn't checked out here. git refuses one checked
/// out in another worktree.
fn set_branch(repo: &Path, c: &mut Change, from: usize, to: usize) -> Result<(), String> {
    let name = c.branch.as_str();
    match (&c.tips[from], &c.tips[to]) {
        (_, None) => {
            c.config = branch_config(repo, Some(name));
            run(repo, &["branch", "-D", name]).map(|_| ())
        }
        (None, Some(sha)) => {
            run(repo, &["branch", name, sha])?;
            for (k, v) in &c.config {
                run(repo, &["config", "--add", k, v])?;
            }
            Ok(())
        }
        (Some(_), Some(sha)) => run(repo, &["branch", "-f", name, sha]).map(|_| ()),
    }
}

/// Points a tag at `target`, or deletes it (None).
fn set_tag(repo: &Path, name: &str, target: Option<&str>) -> Result<(), String> {
    let full = format!("refs/tags/{name}");
    match target {
        Some(sha) => run(repo, &["update-ref", &full, sha]),
        None => run(repo, &["update-ref", "-d", &full]),
    }
    .map(|_| ())
}
