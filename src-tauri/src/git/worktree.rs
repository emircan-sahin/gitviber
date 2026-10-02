//! Linked worktrees: listing, adding, renaming, locking and removing them.

use super::{
    default_branch, delete_branch_at, include_source, is_nested_repo, landed, run, run_text,
    validate_base, validate_branch, worktree_includes,
};
use serde::{Deserialize, Serialize};
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

/// Checks `branch` out in a new worktree and returns its path as listed, `<dir>/<branch>`;
/// `dir` is `<parent>/<project>.worktrees` unless given. With `base`, `branch` is a new branch
/// made there, tracking nothing like `create_branch`'s, or with `track` tracking `base`. Without, a branch only on a remote gets a
/// local tracking branch (git's own DWIM for `worktree add`). The ignored files the main
/// worktree's `.worktreeinclude` lists are copied in.
pub fn add_worktree(
    repo: &Path,
    branch: &str,
    base: Option<&str>,
    track: bool,
    dir: Option<&str>,
) -> Result<String, String> {
    let target = worktree_target(repo, branch, dir)?;
    match base {
        Some(base) => {
            validate_base(repo, base)?;
            let track = if track { "--track" } else { "--no-track" };
            let args = ["worktree", "add", track, "-b", branch, &target, base];
            run(repo, &args)?
        }
        None => run(repo, &["worktree", "add", &target, branch])?,
    };
    // The worktree is made either way; a file that didn't copy is left for the user.
    let from = include_source(repo);
    if let Ok(files) = worktree_includes(&from) {
        crate::fs::copy_into(&from, Path::new(&target), &files);
    }
    // As `worktree list` spells it, which callers compare with: git resolves a folder picked
    // through a symlink, yet keeps the letter case typed (canonicalize would take the disk's).
    let listed = worktrees(repo)
        .ok()
        .into_iter()
        .flatten()
        .map(|w| w.path)
        .find(|p| same_folder(Path::new(p), Path::new(&target)));
    Ok(listed.unwrap_or(target))
}

/// How many files `add_worktree` copies in: links and paths out of the worktree aren't copied, so
/// they aren't counted either.
pub fn include_count(repo: &Path) -> Result<usize, String> {
    let from = include_source(repo);
    let files = worktree_includes(&from)?;
    Ok(crate::fs::copyable(&from, &files).count())
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
    /// 0 for a branch squash- or rebase-merged upstream: their changes are all there.
    pub commits: u32,
    /// Committed on, then fully taken into the default branch, or squash- or rebase-merged
    /// upstream. A branch that never moved is in it too, but has nothing to call merged.
    pub merged: bool,
    /// Unix seconds of the last thing done in it: HEAD moving (made, committed, checked out)
    /// or an uncommitted file changing. Its branch's last commit says neither for a fresh
    /// worktree, nor while an agent edits without committing.
    pub updated: Option<u64>,
}

/// Where one of this repo's worktrees stands: uncommitted files, and commits found nowhere else.
/// `upstream`: also whether a squash or rebase merge upstream took them, which reads diffs.
pub fn worktree_state(repo: &Path, path: &str, upstream: bool) -> Result<WorktreeState, String> {
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
    // Not following symlinks: a link made now to an old file is new work.
    let mtime = |p: &Path| {
        let t = std::fs::symlink_metadata(p)
            .and_then(|m| m.modified())
            .ok()?;
        t.duration_since(std::time::UNIX_EPOCH)
            .ok()
            .map(|d| d.as_secs())
    };
    // The newest entry's own time, not the log file's: gc rewrites every worktree's reflog.
    let mut updated = reflog_time(dir);
    let mut uncommitted = 0;
    let mut touched = |p: Option<&[u8]>| {
        uncommitted += 1;
        // A deleted file has no time; its folder changed when it went.
        let Some(mut path) = p.map(|p| dir.join(os_path(p))) else {
            return;
        };
        while !path.exists() && path.pop() && path.starts_with(dir) {}
        updated = updated.max(mtime(&path));
    };
    let mut records = raw.split(|b| *b == 0);
    while let Some(rec) = records.next() {
        match rec.first() {
            Some(b'?') => {
                let p = String::from_utf8_lossy(&rec[2..]);
                let root = real(&dir.join(p.as_ref()));
                if !(is_nested_repo(dir, &p) && root.is_some_and(|r| others.contains(&r))) {
                    touched(rec.get(2..));
                }
            }
            Some(b'1') => touched(path_after(rec, 8)),
            Some(b'u') => touched(path_after(rec, 10)),
            // A rename's original path follows as its own record.
            Some(b'2') => {
                touched(path_after(rec, 9));
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
    // Squash- or rebase-merged on the remote, which deleted the branch: its commits aren't in
    // the default branch, but all they changed is.
    let squashed = upstream
        && commits > 0
        && !bases.is_empty()
        && w.branch
            .as_deref()
            .is_some_and(|b| !landed(repo, Some(b)).is_empty());
    Ok(WorktreeState {
        uncommitted,
        commits: if squashed { 0 } else { commits },
        merged: squashed
            || !bases.is_empty() && commits == 0 && w.branch.as_deref().is_some_and(moved),
        updated,
    })
}

/// When HEAD last moved, from its newest reflog entry; works for files and reftable alike.
fn reflog_time(dir: &Path) -> Option<u64> {
    let out = run_text(
        dir,
        &["log", "-g", "-1", "--date=unix", "--format=%gd", "HEAD"],
    )
    .ok()?;
    // "HEAD@{1790936093}"
    let (_, t) = out.trim().split_once("@{")?;
    t.strip_suffix('}')?.parse().ok()
}

#[cfg(unix)]
fn os_path(p: &[u8]) -> PathBuf {
    use std::os::unix::ffi::OsStrExt;
    std::ffi::OsStr::from_bytes(p).into()
}

#[cfg(not(unix))]
fn os_path(p: &[u8]) -> PathBuf {
    String::from_utf8_lossy(p).into_owned().into()
}

/// A porcelain v2 status record's path: the field after `n` space-separated ones.
fn path_after(rec: &[u8], n: usize) -> Option<&[u8]> {
    rec.splitn(n + 1, |b| *b == b' ').nth(n)
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

/// An ignored file or folder in a worktree, which removing the worktree deletes for good.
#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Ignored {
    /// From the worktree's folder; a folder git ignores whole is one entry, ending in "/".
    pub path: String,
    pub bytes: u64,
    pub files: u64,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct IgnoredFiles {
    /// Largest first.
    pub entries: Vec<Ignored>,
    /// False when the count stopped at IGNORED_WALK_LIMIT, or a folder couldn't be read: the
    /// sizes are then at least these.
    pub complete: bool,
    /// Folders in them this user can't read or delete (a Docker volume root owns, a `chmod
    /// 000` cache), from the worktree's folder; removing it would stop partway.
    pub denied: Vec<String>,
}

/// Entries the size count looks at per worktree: a node_modules is ~100k; a few would be a
/// second's walk each, and the dialog only needs the order of magnitude.
const IGNORED_WALK_LIMIT: u64 = 300_000;
/// Denied folders named; one is reason enough to leave the worktree.
const DENIED_NAMED: usize = 5;

/// What removing a worktree deletes that git doesn't call a change: its ignored files (`.env`,
/// `node_modules/`). `git worktree remove` refuses untracked and modified files, never these.
pub fn worktree_ignored(repo: &Path, path: &str) -> Result<IgnoredFiles, String> {
    let w = listed_worktree(repo, path)?;
    if w.bare || w.prunable {
        return Err(format!("this worktree has no files on disk: {path}"));
    }
    let dir = Path::new(&w.path);
    let raw = run(
        dir,
        &[
            "status",
            "--porcelain",
            "-z",
            "--ignored",
            "--untracked-files=normal",
        ],
    )?;
    let mut walk = Walk {
        budget: IGNORED_WALK_LIMIT,
        ..Walk::default()
    };
    let mut entries: Vec<Ignored> = raw
        .split(|b| *b == 0)
        .filter_map(|rec| rec.strip_prefix(b"!! "))
        .map(|p| {
            let (bytes, files) = walk.size(&dir.join(os_path(p)));
            Ignored {
                path: String::from_utf8_lossy(p).into_owned(),
                bytes,
                files,
            }
        })
        .collect();
    entries.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.path.cmp(&b.path)));
    // git lists no folder it can't read into: the whole worktree is looked at for one.
    if walk.denied.is_empty() {
        walk.denied.extend(undeletable(dir));
    }
    let denied: Vec<String> = walk
        .denied
        .iter()
        .map(|p| {
            let rel = p.strip_prefix(dir).unwrap_or(p).to_string_lossy();
            if rel.is_empty() {
                "./".into()
            } else {
                format!("{rel}/")
            }
        })
        .collect();
    Ok(IgnoredFiles {
        entries,
        complete: walk.budget > 0 && denied.is_empty(),
        denied,
    })
}

/// A size count over ignored folders: links not followed, a hard-linked file (pnpm's store
/// links every package file) counted once, each entry taken from `budget`.
#[derive(Default)]
struct Walk {
    budget: u64,
    linked: std::collections::HashSet<(u64, u64)>,
    denied: Vec<PathBuf>,
}

impl Walk {
    /// Bytes and files under `path`.
    fn size(&mut self, path: &Path) -> (u64, u64) {
        let (mut bytes, mut files) = (0, 0);
        let mut todo = vec![path.to_path_buf()];
        while let Some(p) = todo.pop() {
            if self.budget == 0 {
                break;
            }
            self.budget -= 1;
            // Gone meanwhile (a package manager at work): nothing to count.
            let Ok(meta) = std::fs::symlink_metadata(&p) else {
                continue;
            };
            if meta.is_dir() {
                match std::fs::read_dir(&p) {
                    Ok(read) if deletable(&p) => {
                        todo.extend(read.filter_map(|e| e.ok().map(|e| e.path())))
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    _ => {
                        if self.denied.len() < DENIED_NAMED {
                            self.denied.push(p);
                        }
                    }
                }
            } else if self.first_link(&meta) {
                bytes += meta.len();
                files += 1;
            }
        }
        (bytes, files)
    }

    #[cfg(unix)]
    fn first_link(&mut self, meta: &std::fs::Metadata) -> bool {
        use std::os::unix::fs::MetadataExt;
        meta.nlink() < 2 || self.linked.insert((meta.dev(), meta.ino()))
    }

    #[cfg(not(unix))]
    fn first_link(&mut self, _meta: &std::fs::Metadata) -> bool {
        true
    }
}

/// Whether this user can empty folder `dir` and so delete it: list, write and enter it.
#[cfg(unix)]
fn deletable(dir: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let Ok(c) = std::ffi::CString::new(dir.as_os_str().as_bytes()) else {
        return false;
    };
    unsafe { libc::access(c.as_ptr(), libc::R_OK | libc::W_OK | libc::X_OK) == 0 }
}

#[cfg(not(unix))]
fn deletable(_dir: &Path) -> bool {
    true
}

/// The first folder in `dir` (or `dir` itself) this user can't empty: `git worktree remove`
/// would delete what it can, then stop with the worktree's entry gone and a broken folder left.
fn undeletable(dir: &Path) -> Option<PathBuf> {
    let mut todo = vec![dir.to_path_buf()];
    while let Some(p) = todo.pop() {
        let read = std::fs::read_dir(&p).ok().filter(|_| deletable(&p));
        let Some(read) = read else {
            return Some(p);
        };
        // The entry's own type: no stat per file, and links aren't followed.
        todo.extend(
            read.filter_map(Result::ok)
                .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
                .map(|e| e.path()),
        );
    }
    None
}

/// A worktree to clean up, as the page found it: merged by git's count, or by a merged pull
/// request whose head is `merged_head` (a squash merge git can't see until the branch is gone).
#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CleanUp {
    pub path: String,
    pub merged_head: Option<String>,
}

#[derive(Serialize, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CleanedUp {
    /// The worktrees removed.
    pub removed: Vec<String>,
    /// Their branches deleted along with them.
    pub deleted: Vec<String>,
    /// Their branches kept: what they hold isn't known to be merged.
    pub kept: Vec<String>,
    /// The worktrees left, and why.
    pub failed: Vec<(String, String)>,
}

/// A branch whose worktree went, and the commit it was checked merged at.
pub type MergedBranch = (String, String);

/// Removes merged worktrees, each checked again first, and the branches whose work is known to
/// be merged. A failure leaves that one and goes on with the rest. The command runs the two
/// halves apart, the second journaled.
#[cfg(test)]
pub fn clean_up_worktrees(repo: &Path, list: &[CleanUp]) -> CleanedUp {
    let (mut out, branches) = remove_merged_worktrees(repo, list);
    delete_merged_branches(repo, &mut out, branches);
    out
}

/// Clean up's first half: the worktrees, each checked again and removed, and the branches that
/// may go with them. Apart from the second so the journal isn't held through a big folder's delete.
pub fn remove_merged_worktrees(repo: &Path, list: &[CleanUp]) -> (CleanedUp, Vec<MergedBranch>) {
    let mut out = CleanedUp::default();
    let mut branches = vec![];
    for item in list {
        match clean_up(repo, item) {
            Ok(branch) => {
                out.removed.push(item.path.clone());
                match branch {
                    Some((b, Some(tip))) => branches.push((b, tip)),
                    Some((b, None)) => out.kept.push(b),
                    None => {}
                }
            }
            Err(e) => out.failed.push((item.path.clone(), e)),
        }
    }
    (out, branches)
}

/// Clean up's second half, for the journal to record: each branch deleted at the commit that was
/// checked. One that moved meanwhile, or that another worktree has out (`worktree add --force`,
/// whose HEAD would go unborn), is kept.
pub fn delete_merged_branches(repo: &Path, out: &mut CleanedUp, branches: Vec<MergedBranch>) {
    for (branch, tip) in branches {
        let held = run_text(
            repo,
            &[
                "for-each-ref",
                "--format=%(worktreepath)",
                &format!("refs/heads/{branch}"),
            ],
        )
        .map_or(true, |s| !s.trim().is_empty());
        if !held && delete_branch_at(repo, &branch, &tip).is_ok() {
            out.deleted.push(branch);
        } else {
            out.kept.push(branch);
        }
    }
}

/// One worktree, if it's still unlocked, holds no other worktree, has nothing uncommitted, every
/// folder in it can be deleted, and is merged: by git's count, or with its branch at the merged
/// pull request's head (a detached HEAD's commits are on no branch, and would be lost). Then
/// its branch, with the commit to delete it at when that's known merged.
fn clean_up(repo: &Path, item: &CleanUp) -> Result<Option<(String, Option<String>)>, String> {
    let all = worktrees(repo)?;
    let w = all
        .iter()
        .find(|w| w.path == item.path)
        .ok_or_else(|| format!("not a worktree of this repository: {}", item.path))?;
    if w.locked {
        return Err("it's locked".into());
    }
    // An agent's own worktrees sit inside the one it works in, ignored: removing the outer one
    // would delete them, uncommitted work and all.
    let real = |p: &str| {
        Path::new(p)
            .canonicalize()
            .unwrap_or_else(|_| PathBuf::from(p))
    };
    let outer = real(&w.path);
    if all
        .iter()
        .any(|o| o.path != w.path && real(&o.path).starts_with(&outer))
    {
        return Err("another worktree is inside it".into());
    }
    let state = worktree_state(repo, &item.path, true)?;
    if state.uncommitted > 0 {
        return Err(format!(
            "it has {} uncommitted {}",
            state.uncommitted,
            if state.uncommitted == 1 {
                "change"
            } else {
                "changes"
            }
        ));
    }
    let tip = w.branch.as_deref().and_then(|b| {
        run_text(
            repo,
            &["rev-parse", "--verify", "-q", &format!("refs/heads/{b}")],
        )
        .ok()
        .map(|s| s.trim().to_string())
    });
    let at_head = item
        .merged_head
        .as_deref()
        .zip(tip.as_deref())
        .is_some_and(|(h, t)| h.eq_ignore_ascii_case(t));
    if !state.merged && !at_head {
        return Err(match (&item.merged_head, &w.branch) {
            (Some(_), None) => "its HEAD is detached, off the merged pull request's branch".into(),
            (Some(_), Some(_)) => "it has commits the merged pull request doesn't".into(),
            (None, _) => "it isn't merged anymore".into(),
        });
    }
    if outer.parent().is_some_and(|p| !deletable(p)) {
        return Err("its folder can't be deleted (permission denied)".into());
    }
    if let Some(p) = undeletable(&outer) {
        return Err(match p.strip_prefix(&outer) {
            Ok(rel) if !rel.as_os_str().is_empty() => format!(
                "{}/ in it can't be deleted (permission denied)",
                rel.to_string_lossy()
            ),
            _ => "its folder can't be deleted (permission denied)".into(),
        });
    }
    remove_worktree(repo, &item.path, false)?;
    Ok(w.branch.clone().map(|b| (b, tip)))
}
