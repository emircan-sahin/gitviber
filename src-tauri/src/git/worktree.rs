//! Linked worktrees: listing, adding, renaming, locking and removing them.

use super::{default_branch, is_nested_repo, run, run_text, validate_base, validate_branch};
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Worktree {
    pub path: String,
    pub head: Option<String>,
    /// Short branch name; None when detached (or bare).
    pub branch: Option<String>,
    pub detached: bool,
    pub bare: bool,
    pub locked: bool,
    /// Why it's locked, if the locker said; Claude Code writes "claude session … (pid N …)".
    pub lock_reason: Option<String>,
    /// The lock names a process that's still running: someone is working in it right now.
    /// Only the `worktrees` command fills this in (see `with_live_locks`).
    pub in_use: bool,
    /// Its directory is gone; `git worktree prune` would drop the entry.
    pub prunable: bool,
    /// The worktree this window has open.
    pub current: bool,
    /// The main worktree (git always lists it first).
    pub main: bool,
}

pub fn worktrees(repo: &Path) -> Result<Vec<Worktree>, String> {
    let raw = run_text(repo, &["worktree", "list", "--porcelain", "-z"])?;
    let here = repo.canonicalize().ok();
    let mut list: Vec<Worktree> = vec![];
    // NUL-separated fields; an empty field ends each worktree's record.
    let mut fields = raw.split('\0');
    while let Some(first) = fields.next() {
        let Some(path) = first.strip_prefix("worktree ") else {
            continue;
        };
        let mut w = Worktree {
            path: path.to_string(),
            head: None,
            branch: None,
            detached: false,
            bare: false,
            locked: false,
            lock_reason: None,
            in_use: false,
            prunable: false,
            current: here.is_some() && Path::new(path).canonicalize().ok() == here,
            main: list.is_empty(),
        };
        for field in fields.by_ref().take_while(|f| !f.is_empty()) {
            let (key, val) = field.split_once(' ').unwrap_or((field, ""));
            match key {
                // An unborn branch reports an all-zero HEAD.
                "HEAD" if val.chars().any(|c| c != '0') => {
                    w.head = Some(val.chars().take(7).collect())
                }
                "branch" => w.branch = Some(val.trim_start_matches("refs/heads/").to_string()),
                "detached" => w.detached = true,
                "bare" => w.bare = true,
                "locked" => {
                    w.locked = true;
                    w.lock_reason = (!val.is_empty()).then(|| val.to_string());
                }
                "prunable" => w.prunable = true,
                _ => {}
            }
        }
        list.push(w);
    }
    Ok(list)
}

/// Marks worktrees whose lock names a live pid; only the worktree list shows that.
pub fn with_live_locks(mut list: Vec<Worktree>) -> Vec<Worktree> {
    for w in &mut list {
        let pid = w.lock_reason.as_deref().and_then(|r| {
            let rest = &r[r.find("pid ")? + 4..];
            rest.split(|c: char| !c.is_ascii_digit())
                .next()?
                .parse::<u32>()
                .ok()
        });
        w.in_use = pid.is_some_and(process_alive);
    }
    list
}

/// `kill -0` without spawning `kill`: true when the process exists and is ours to signal.
#[cfg(unix)]
fn process_alive(pid: u32) -> bool {
    // 0 and anything past pid_t would name a whole process group, or every process.
    libc::pid_t::try_from(pid).is_ok_and(|pid| pid > 0 && unsafe { libc::kill(pid, 0) } == 0)
}

#[cfg(not(unix))]
fn process_alive(_pid: u32) -> bool {
    false
}

/// What the projects list keys this repo by: its main worktree, unless that is bare or gone.
pub fn main_worktree(repo: &Path) -> Option<String> {
    worktrees(repo)
        .ok()?
        .into_iter()
        .find(|w| w.main && !w.bare && Path::new(&w.path).is_dir())
        .map(|w| w.path)
}

/// Checks `branch` out in a new worktree and returns its path, `<dir>/<branch>`; `dir` is
/// `<parent>/<project>.worktrees` unless given. With `base`, `branch` is a new branch made
/// there, tracking nothing like `create_branch`'s. Without, a branch only on a remote gets a
/// local tracking branch (git's own DWIM for `worktree add`).
pub fn add_worktree(
    repo: &Path,
    branch: &str,
    base: Option<&str>,
    dir: Option<&str>,
) -> Result<String, String> {
    let target = worktree_target(repo, branch, dir)?;
    match base {
        Some(base) => {
            validate_base(repo, base)?;
            let args = ["worktree", "add", "--no-track", "-b", branch, &target, base];
            run(repo, &args)?
        }
        None => run(repo, &["worktree", "add", &target, branch])?,
    };
    Ok(target)
}

/// The folder `add_worktree` would make for `branch`, refused if it's taken; callers with
/// work to do first (a fetch) ask before it, so a refusal changes nothing.
pub(crate) fn worktree_target(
    repo: &Path,
    branch: &str,
    dir: Option<&str>,
) -> Result<String, String> {
    validate_branch(repo, branch)?;
    let dir = match dir {
        Some(d) if Path::new(d).is_absolute() => PathBuf::from(d),
        Some(d) => return Err(format!("not an absolute path: {d}")),
        None => worktrees_dir(repo)?,
    };
    let path = dir.join(branch.replace('/', "-"));
    if path.exists() {
        return Err(format!("{} already exists", path.display()));
    }
    Ok(path.to_string_lossy().into_owned())
}

/// Two paths name one folder: on a case-insensitive disk "Feat" and "feat" do.
fn same_folder(a: &Path, b: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        matches!((a.metadata(), b.metadata()), (Ok(x), Ok(y)) if x.dev() == y.dev() && x.ino() == y.ino())
    }
    #[cfg(not(unix))]
    {
        b.exists()
            && a.to_string_lossy()
                .eq_ignore_ascii_case(&b.to_string_lossy())
    }
}

/// Renames a worktree's branch to `branch` and, with `move_folder`, its folder to match,
/// beside where it is now. Returns the worktree's path afterwards.
pub fn rename_worktree(
    repo: &Path,
    path: &str,
    branch: &str,
    move_folder: bool,
) -> Result<String, String> {
    let w = listed_worktree(repo, path)?;
    let old = w.branch.ok_or("this worktree has no branch to rename")?;
    validate_branch(repo, branch)?;
    let from = Path::new(path);
    let to = from
        .parent()
        .ok_or_else(|| format!("no folder above {path}"))?
        .join(branch.replace('/', "-"));
    let moving = move_folder && to != from;
    // Checked before anything changes, so a refusal leaves everything as it was.
    if moving {
        if w.main {
            return Err("the main worktree's folder can't be moved".into());
        }
        if w.current {
            return Err("this window has that worktree open; switch to another one first".into());
        }
        if w.locked {
            return Err(
                "this worktree is locked; unlock it (its row's lock) before moving it".into(),
            );
        }
        if w.prunable {
            return Err(format!("this worktree has no files on disk: {path}"));
        }
        if to.exists() && !same_folder(from, &to) {
            return Err(format!("{} already exists", to.display()));
        }
    }
    let renaming = branch != old;
    // A case-only rename: on a case-insensitive disk git reads "case" as the existing "Case"
    // and refuses. -M is safe once no branch has exactly the new name.
    let flag = if renaming && branch.eq_ignore_ascii_case(&old) {
        let names = run_text(
            repo,
            &["for-each-ref", "--format=%(refname)", "refs/heads/"],
        )?;
        let exact = format!("refs/heads/{branch}");
        if names.lines().any(|n| n == exact) {
            return Err(format!("a branch named '{branch}' already exists"));
        }
        "-M"
    } else {
        "-m"
    };
    if renaming {
        run(repo, &["branch", flag, &old, branch])?;
    }
    if !moving {
        return Ok(path.to_string());
    }
    let target = to.to_string_lossy().into_owned();
    if let Err(e) = move_worktree(repo, path, &target) {
        if renaming && run(repo, &["branch", flag, branch, &old]).is_err() {
            return Err(format!(
                "{e}\nThe folder stayed, and the branch is still named {branch}."
            ));
        }
        return Err(e);
    }
    Ok(target)
}

/// `git worktree move`. A case-only rename goes by way of a third name: git sees "feat"
/// as taken by "Feat" on a case-insensitive disk.
fn move_worktree(repo: &Path, from: &str, to: &str) -> Result<(), String> {
    if !same_folder(Path::new(from), Path::new(to)) {
        return run(repo, &["worktree", "move", from, to]).map(|_| ());
    }
    let step = format!("{to}.gitviber-rename");
    run(repo, &["worktree", "move", from, &step])?;
    run(repo, &["worktree", "move", &step, to])
        .map(|_| ())
        .map_err(|e| match run(repo, &["worktree", "move", &step, from]) {
            Ok(_) => e,
            Err(_) => format!("{e}\nIts folder is left at {step}."),
        })
}

/// `git worktree lock`: kept from prune, move and remove until unlocked, as for a folder on
/// a drive that isn't always plugged in. `reason` shows on its row.
pub fn lock_worktree(repo: &Path, path: &str, reason: Option<&str>) -> Result<(), String> {
    let w = listed_worktree(repo, path)?;
    if w.main {
        return Err("the main worktree can't be locked".into());
    }
    let reason = reason.map(str::trim).filter(|r| !r.is_empty());
    if reason.is_some_and(|r| r.contains(['\n', '\r'])) {
        return Err("the reason must be one line of text".into());
    }
    let flag = reason.map(|r| format!("--reason={r}"));
    let mut args = vec!["worktree", "lock"];
    args.extend(flag.as_deref());
    args.push(path);
    run(repo, &args).map(|_| ())
}

pub fn unlock_worktree(repo: &Path, path: &str) -> Result<(), String> {
    listed_worktree(repo, path)?;
    run(repo, &["worktree", "unlock", path]).map(|_| ())
}

/// `<parent>/<project>.worktrees`, the folder `add_worktree` puts new worktrees in.
fn worktrees_dir(repo: &Path) -> Result<PathBuf, String> {
    let main = main_worktree(repo).ok_or("this repository has no main worktree")?;
    let main = Path::new(&main);
    let (Some(parent), Some(name)) = (main.parent(), main.file_name()) else {
        return Err(format!("no folder beside {}", main.display()));
    };
    Ok(parent.join(format!("{}.worktrees", name.to_string_lossy())))
}

/// One of this repo's worktrees by path. Only paths `git worktree list` reports are
/// accepted, so the frontend can't point git at an arbitrary folder.
pub(super) fn listed_worktree(repo: &Path, path: &str) -> Result<Worktree, String> {
    worktrees(repo)?
        .into_iter()
        .find(|w| w.path == path)
        .ok_or_else(|| format!("not a worktree of this repository: {path}"))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeState {
    /// Files `git status` lists: gone for good if the folder is deleted.
    pub uncommitted: u32,
    /// Commits the default branch lacks; on the default branch itself, commits no remote has.
    pub commits: u32,
    /// Committed on, then fully taken into the default branch. A branch that never moved
    /// is in it too, but has nothing to call merged.
    pub merged: bool,
}

/// Where one of this repo's worktrees stands: uncommitted files, and commits found nowhere else.
pub fn worktree_state(repo: &Path, path: &str) -> Result<WorktreeState, String> {
    let all = worktrees(repo)?;
    let w = all
        .iter()
        .find(|w| w.path == path)
        .ok_or_else(|| format!("not a worktree of this repository: {path}"))?;
    if w.bare || w.prunable {
        return Err(format!("this worktree has no files on disk: {path}"));
    }
    let dir = Path::new(&w.path);
    // Worktrees kept inside this one (.claude/worktrees/*) show as untracked folders.
    let real = |p: &Path| p.canonicalize().ok();
    let others: Vec<_> = all
        .iter()
        .filter_map(|o| real(Path::new(&o.path)))
        .collect();
    let raw = run(
        dir,
        &["status", "--porcelain=v2", "-z", "--untracked-files=all"],
    )?;
    let mut uncommitted = 0;
    let mut records = raw.split(|b| *b == 0);
    while let Some(rec) = records.next() {
        match rec.first() {
            Some(b'?') => {
                let p = String::from_utf8_lossy(&rec[2..]);
                let root = real(&dir.join(p.as_ref()));
                if !(is_nested_repo(dir, &p) && root.is_some_and(|r| others.contains(&r))) {
                    uncommitted += 1;
                }
            }
            Some(b'1' | b'u') => uncommitted += 1,
            // A rename's original path follows as its own record.
            Some(b'2') => {
                uncommitted += 1;
                records.next();
            }
            _ => {}
        }
    }
    let count = |args: &[&str]| -> u32 {
        run_text(dir, args)
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0)
    };
    // The default branch anywhere counts: locally (merged, not pushed yet), on origin, or on
    // another remote, like a fork's upstream, where its PRs land while origin's copy lags.
    let default = default_branch(repo);
    let mut bases = vec![];
    let local = format!("refs/heads/{default}");
    if run(dir, &["rev-parse", "--verify", "-q", &local]).is_ok() {
        bases.push(local);
    }
    let remote = format!("refs/remotes/*/{default}");
    if run_text(dir, &["for-each-ref", "--count=1", &remote]).is_ok_and(|s| !s.trim().is_empty()) {
        bases.push(format!("--glob={remote}"));
    }
    // The default branch itself can only be measured against the remotes.
    if w.branch.as_deref() == Some(default.as_str()) {
        bases.clear();
    }
    let mut args = vec!["rev-list", "--count", "HEAD", "--not"];
    if bases.is_empty() {
        args.push("--remotes");
    }
    args.extend(bases.iter().map(String::as_str));
    let commits = count(&args);
    // One reflog entry is the branch's creation: it never moved.
    let moved = |b: &str| {
        run_text(
            dir,
            &["reflog", "show", "--format=%H", &format!("refs/heads/{b}")],
        )
        .is_ok_and(|log| log.lines().count() > 1)
    };
    Ok(WorktreeState {
        uncommitted,
        commits,
        merged: !bases.is_empty() && commits == 0 && w.branch.as_deref().is_some_and(moved),
    })
}

/// Deletes a linked worktree's folder and entry; its branch stays. `force` also drops
/// uncommitted files and overrides a lock (git wants `-f` twice for that).
pub fn remove_worktree(repo: &Path, path: &str, force: bool) -> Result<(), String> {
    let w = listed_worktree(repo, path)?;
    if w.main {
        return Err("the main worktree can't be removed".into());
    }
    if w.current {
        return Err("this window has that worktree open; switch to another one first".into());
    }
    let mut args = vec!["worktree", "remove"];
    if force {
        args.extend(["--force", "--force"]);
    }
    args.push(path);
    run(repo, &args)?;
    // The last one out takes the .worktrees folder GitViber made; remove_dir leaves it
    // while anything is still inside.
    if let Ok(dir) = worktrees_dir(repo) {
        if Path::new(path).parent() == Some(dir.as_path()) {
            let _ = std::fs::remove_dir(&dir);
        }
    }
    Ok(())
}
