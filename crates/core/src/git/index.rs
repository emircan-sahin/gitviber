//! Staging, unstaging and discarding whole files.

use super::{has_head, new_gitlink, run_text, run_with, untracked_nested_root, validate_rev};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub(super) fn with_paths<'a>(mut args: Vec<&'a str>, paths: &'a [String]) -> Vec<&'a str> {
    args.push("--");
    args.extend(paths.iter().map(String::as_str));
    args
}

/// Runs git on `paths` read from stdin: on argv, some 16k paths (an agent's unignored
/// node_modules) fail with "Argument list too long". GIT_LITERAL_PATHSPECS applies here too.
fn run_on(repo: &Path, args: &[&str], paths: &[String]) -> Result<(), String> {
    // An empty pathspec file would mean the whole tree.
    if paths.is_empty() {
        return Ok(());
    }
    let mut args = args.to_vec();
    args.extend(["--pathspec-from-file=-", "--pathspec-file-nul"]);
    run_with(repo, &args, &[], Some(paths.join("\0").as_bytes())).map(|_| ())
}

pub fn stage(repo: &Path, paths: &[String]) -> Result<(), String> {
    stage_with(repo, paths, false)
}

/// Refuses untracked nested repositories (an agent's worktree) unless `allow_nested`: git
/// would stage one as a gitlink, a pointer to its current commit, and none of its files.
pub fn stage_with(repo: &Path, paths: &[String], allow_nested: bool) -> Result<(), String> {
    if !allow_nested {
        if let Some(p) = nested_repos(repo, paths)?.first() {
            return Err(format!(
                "{p} is a separate git repository (a worktree or nested repo), so it was not staged. \
                 git would record only a pointer to its current commit, not its files. \
                 Commit inside it instead."
            ));
        }
    }
    run_on(repo, &["add", "-A"], paths)
}

/// Nested repositories at or under `paths` that `git add` would turn into new gitlinks (or
/// whose files it would take as ours). Only folders, and files inside a folder with a `.git`,
/// can be one or lie in one: plain files skip the status call.
fn nested_repos(repo: &Path, paths: &[String]) -> Result<Vec<String>, String> {
    // Whether a folder is or lies in one, as found once: the thousands of files of one
    // node_modules share their folders.
    let mut known: HashMap<&Path, bool> = HashMap::new();
    let mut suspect = vec![];
    for p in paths {
        let (mut walked, mut inside) = (vec![], false);
        for a in Path::new(p).ancestors().skip(1) {
            if a.as_os_str().is_empty() {
                break;
            }
            if let Some(&k) = known.get(a) {
                inside = k;
                break;
            }
            walked.push(a);
            if repo.join(a).join(".git").exists() {
                inside = true;
                break;
            }
        }
        known.extend(walked.into_iter().map(|a| (a, inside)));
        if inside || repo.join(p).is_dir() {
            suspect.push(p.clone());
        }
    }
    let mut found = vec![];
    // Few in practice; in chunks that fit on argv, which `status` needs (no pathspec file).
    for chunk in suspect.chunks(500) {
        let args = with_paths(
            vec!["status", "--porcelain=v2", "-z", "--untracked-files=all"],
            chunk,
        );
        let raw = run_text(repo, &args)?;
        let mut records = raw.split('\0');
        while let Some(rec) = records.next() {
            if let Some(p) = rec.strip_prefix("? ") {
                found.extend(untracked_nested_root(repo, p));
            } else if let Some(kind @ ('1' | '2')) = rec.chars().next() {
                let fields: Vec<&str> = rec.splitn(if kind == '1' { 9 } else { 10 }, ' ').collect();
                if kind == '2' {
                    records.next();
                }
                if new_gitlink(&fields) {
                    found.extend(fields.last().map(|p| p.to_string()));
                }
            }
        }
    }
    Ok(found)
}

pub fn unstage(repo: &Path, paths: &[String]) -> Result<(), String> {
    let base = if has_head(repo) {
        vec!["restore", "--staged"]
    } else {
        vec!["rm", "--cached", "-q", "-r"]
    };
    run_on(repo, &base, paths)
}

/// Reverts tracked files in the worktree to their index version. Untracked files are left alone.
pub fn discard(repo: &Path, paths: &[String]) -> Result<(), String> {
    run_on(repo, &["restore", "--worktree"], paths)
}

/// Puts `path` in the working tree as commit `sha` has it; the index stays as it is.
pub fn restore_from(repo: &Path, sha: &str, path: &str) -> Result<(), String> {
    validate_rev(sha)?;
    let source = format!("--source={sha}");
    run_with(
        repo,
        &["restore", &source, "--worktree", "--", path],
        &[],
        None,
    )
    .map(|_| ())
}

/// git holds index.lock for as long as it writes the index, well under this; an older one is
/// a killed git's (an agent stopped mid-command), or one that waits on an editor or a hook.
const STALE_LOCK: Duration = Duration::from_secs(5);

/// Removes the index.lock git named in an error, the user having said no git command is
/// running. Only this repository's own (git's path for it: a linked worktree has its own),
/// only once it's too old to be a command's that is still writing, and on macOS only while no
/// git process works in the repository.
pub fn remove_index_lock(repo: &Path, named: &str) -> Result<(), String> {
    let lock = repo.join(run_text(repo, &["rev-parse", "--git-path", "index.lock"])?.trim());
    // The folders may be spelled differently (/tmp and /private/tmp on macOS).
    let real = |p: &Path| -> Option<PathBuf> {
        Some(p.parent()?.canonicalize().ok()?.join(p.file_name()?))
    };
    if real(&lock).is_none() || real(&lock) != real(Path::new(named)) {
        return Err(format!(
            "{named} isn't this repository's index lock. Open its repository to remove it."
        ));
    }
    let modified = match std::fs::metadata(&lock).and_then(|m| m.modified()) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        m => m.map_err(|e| e.to_string())?,
    };
    if modified.elapsed().unwrap_or_default() < STALE_LOCK {
        return Err("The lock was taken a moment ago: a git command is still using it. Let it finish, then try again.".into());
    }
    if let Some(pid) = git_working_in(repo) {
        return Err(format!("git is still running in this repository (process {pid}), maybe a commit waiting on its hooks or an editor, or a `git log`/`git diff` open in a pager. Let it finish or quit it, then try again."));
    }
    match std::fs::remove_file(&lock) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
        _ => Ok(()),
    }
}

/// A git process working in `dir`, by its current folder: git moves to the worktree's top
/// before it takes index.lock and stays there while a hook or an editor runs. `git commit -a`
/// holds the lock that long without keeping it open, so the lock file itself can't tell. The
/// app's own reads there take no lock; one running now only makes the user try again.
#[cfg(target_os = "macos")]
fn git_working_in(dir: &Path) -> Option<libc::pid_t> {
    use std::ffi::{CStr, OsStr};
    use std::os::unix::ffi::OsStrExt;
    let dir = dir.canonicalize().ok()?;
    // With no buffer it says how many there are; a few more may start before the second call.
    let count = unsafe { libc::proc_listallpids(std::ptr::null_mut(), 0) };
    let mut pids = vec![0 as libc::pid_t; count.max(0) as usize + 64];
    let bytes = std::mem::size_of_val(pids.as_slice()) as libc::c_int;
    let count = unsafe { libc::proc_listallpids(pids.as_mut_ptr().cast(), bytes) };
    pids.truncate(count.max(0) as usize);
    pids.into_iter().find(|&pid| {
        let mut name = [0u8; 64];
        let len = unsafe { libc::proc_name(pid, name.as_mut_ptr().cast(), name.len() as u32) };
        if name.get(..len.max(0) as usize) != Some(&b"git"[..]) {
            return false;
        }
        let mut info: libc::proc_vnodepathinfo = unsafe { std::mem::zeroed() };
        let size = std::mem::size_of_val(&info) as libc::c_int;
        let ptr = (&mut info as *mut libc::proc_vnodepathinfo).cast();
        if unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDVNODEPATHINFO, 0, ptr, size) } != size {
            return false;
        }
        let cwd = unsafe { CStr::from_ptr(info.pvi_cdir.vip_path.as_ptr().cast()) };
        Path::new(OsStr::from_bytes(cwd.to_bytes())).starts_with(&dir)
    })
}

/// Elsewhere only the lock's age and the user's word guard it.
#[cfg(not(target_os = "macos"))]
fn git_working_in(_dir: &Path) -> Option<i32> {
    None
}
