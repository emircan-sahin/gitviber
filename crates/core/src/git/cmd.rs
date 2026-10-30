//! Building and running git commands: the environment every call gets, and the repo root.

use crate::network::{self, Net};
use crate::process::{exec, git, search_path};
use std::cell::Cell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

/// The app reads git's messages ("not a git repository", "non-fast-forward", index.lock,
/// progress phases), so git must speak English whatever LANG says. UTF-8
/// keeps non-ASCII paths and messages intact. LANGUAGE would outrank both for gettext.
pub(crate) fn in_english(cmd: &mut Command) -> &mut Command {
    cmd.env("LC_ALL", "en_US.UTF-8")
        .env("LANG", "en_US.UTF-8")
        .env_remove("LANGUAGE")
}

/// What git reads to find the repo before looking at the working directory. Inherited (the app
/// started from a git hook, or from a terminal inside one), every repo resolved to that one.
const REPO_ENV: &[&str] = &[
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_COMMON_DIR",
    "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_NAMESPACE",
    "GIT_PREFIX",
];

/// For a patch read as text (patch-id, or `git apply` in patch.rs), none of the user's diff
/// settings may shape it: a submodule's log or hidden pointer, fewer context lines, blank ones cut,
/// other prefixes.
pub(crate) const PINS: [&str; 10] = [
    "-c",
    "diff.suppressBlankEmpty=false",
    "-c",
    "diff.noprefix=false",
    "-c",
    "diff.mnemonicPrefix=false",
    "-c",
    "diff.relative=false",
    "-c",
    "color.diff=false",
];
pub(crate) fn command(repo: &Path, args: &[&str]) -> Command {
    let mut cmd = Command::new(git());
    for var in REPO_ENV {
        cmd.env_remove(var);
    }
    in_english(&mut cmd)
        .current_dir(repo)
        .args(args)
        .env("PATH", search_path())
        // Never block on an interactive credential prompt; there is no terminal.
        .env("GIT_TERMINAL_PROMPT", "0")
        // Our background refreshes must not take index.lock, or they would race the
        // agent/terminal running git in the same repo.
        .env("GIT_OPTIONAL_LOCKS", "0")
        // merge/rebase --continue would otherwise open $EDITOR and hang with no terminal.
        .env("GIT_EDITOR", "true")
        // Paths are file names, never globs: `app/[id].tsx` must not also match `app/i.tsx`.
        .env("GIT_LITERAL_PATHSPECS", "1")
        // log.showSignature=true put gpg's verdict into every message we read (a squash commit
        // kept it) and ran a verify per signed commit in history. As env, not `-c`, so the
        // args still start with the subcommand; an explicit --show-signature still wins.
        .env("GIT_CONFIG_COUNT", "1")
        .env("GIT_CONFIG_KEY_0", "log.showSignature")
        .env("GIT_CONFIG_VALUE_0", "false")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cmd
}

/// `command` for reading a project's files: a folder outside any repository (a plain project)
/// is read as the work tree of an empty repository of the app's own, so listing, search and
/// .gitignore work there as in a repository with nothing committed. Nothing is written to it.
pub(crate) fn files_command(root: &Path, args: &[&str]) -> Command {
    let mut cmd = command(root, args);
    if outside_repo(root) {
        if let Some(empty) = empty_repo() {
            cmd.env("GIT_DIR", empty).env("GIT_WORK_TREE", root);
        }
    }
    cmd
}

/// Whether each opened root is a plain folder, as `open_repo` found it: every file read and
/// watcher batch asks, and asking git each time doubled the processes they ran.
fn opened_roots() -> &'static Mutex<HashMap<PathBuf, bool>> {
    static OPENED: OnceLock<Mutex<HashMap<PathBuf, bool>>> = OnceLock::new();
    OPENED.get_or_init(Mutex::default)
}

/// What opening `root` found; opening it again (a folder become a repository) replaces it.
pub fn opened_as(root: &Path, plain: bool) {
    let mut roots = opened_roots().lock().unwrap_or_else(|e| e.into_inner());
    roots.insert(root.to_path_buf(), plain);
}

/// `root` is a folder in no repository (a plain project). Git's answer, not whether `.git`
/// exists: an empty or broken `.git` is no repository to git.
pub fn outside_repo(root: &Path) -> bool {
    let known = opened_roots()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(root)
        .copied();
    known.unwrap_or_else(|| toplevel(root).is_err_and(|e| e == NOT_A_REPO))
}

/// The empty bare repository `files_command` reads plain folders through, made on first use
/// (again if a cleaner took it).
fn empty_repo() -> Option<std::path::PathBuf> {
    let dir = crate::paths::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("empty.git");
    if !dir.join("HEAD").is_file() {
        std::fs::create_dir_all(&dir).ok()?;
        run(&dir, &["init", "--bare", "-q"]).ok()?;
    }
    Some(dir)
}

/// Generous: a cold cache on a big repo is slow. But one wedged read (fsmonitor, a network
/// volume) used to leave the window's refresh waiting, and showing old state, for good.
const READ_TIMEOUT: Duration = Duration::from_secs(60);

thread_local! {
    static READING: Cell<bool> = const { Cell::new(false) };
}

/// Runs `f` with each git call in it given up after READ_TIMEOUT. Only for reads: a commit's
/// hooks or a rebase can rightly take longer. It covers `run_with` (and so `run`, `run_text`) on
/// this thread; a thread `f` spawns, or a direct `exec`, isn't covered.
pub(crate) fn reading<T>(f: impl FnOnce() -> T) -> T {
    // Reset even if `f` panics: a thread pool reuses its threads.
    struct Restore(bool);
    impl Drop for Restore {
        fn drop(&mut self) {
            READING.with(|r| r.set(self.0));
        }
    }
    let _restore = Restore(READING.with(|r| r.replace(true)));
    f()
}

pub(crate) fn read_timeout() -> Option<Duration> {
    READING.with(Cell::get).then_some(READ_TIMEOUT)
}

/// Runs `a` here and `b` on a thread beside it, `b`'s git calls timed out as this thread's are.
pub(crate) fn both<A, B: Send>(a: impl FnOnce() -> A, b: impl FnOnce() -> B + Send) -> (A, B) {
    let timed = read_timeout().is_some();
    std::thread::scope(|s| {
        let b = s.spawn(move || if timed { reading(b) } else { b() });
        let a = a();
        (a, b.join().unwrap_or_else(|p| std::panic::resume_unwind(p)))
    })
}

/// Runs git and returns stdout. `ok_codes` lists exit codes that are not errors.
pub(crate) fn run_with(
    repo: &Path,
    args: &[&str],
    ok_codes: &[i32],
    input: Option<&[u8]>,
) -> Result<Vec<u8>, String> {
    exec(
        command(repo, args),
        &format!("git {}", args.first().unwrap_or(&"")),
        ok_codes,
        input,
        read_timeout(),
    )
}

/// Fetch, pull, push and clone: with progress, stoppable, and timed out only when silent.
pub(crate) fn run_network(repo: &Path, args: &[&str], net: &Net) -> Result<Vec<u8>, String> {
    let mut args = args.to_vec();
    // Without a terminal git reports no progress unless asked.
    args.insert(1, "--progress");
    // A pull's merge or rebase starts once its fetch has written FETCH_HEAD.
    let fetch_head = (args[0] == "pull")
        .then(|| run_text(repo, &["rev-parse", "--git-path", "FETCH_HEAD"]).ok())
        .flatten()
        .map(|p| repo.join(p.trim()));
    network::run(
        command(repo, &args),
        &format!("git {}", args[0]),
        net,
        fetch_head.as_deref(),
    )
}

pub fn run(repo: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    run_with(repo, args, &[], None)
}

/// `run` through `files_command`: a plain folder's files too.
pub(crate) fn run_files(root: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    exec(
        files_command(root, args),
        &format!("git {}", args.first().unwrap_or(&"")),
        &[],
        None,
        read_timeout(),
    )
}

pub(crate) fn run_text(repo: &Path, args: &[&str]) -> Result<String, String> {
    run(repo, args).map(|b| String::from_utf8_lossy(&b).into_owned())
}

pub(super) fn has_head(repo: &Path) -> bool {
    run(repo, &["rev-parse", "--verify", "-q", "HEAD"]).is_ok()
}

/// toplevel's error for an existing folder outside any repository; the page offers `git init`.
pub const NOT_A_REPO: &str = "git:not-a-repo";

/// git's error for a folder outside any repository. Not "fatal: not a git repository: <path>",
/// which is a .git file gone bad: a worktree whose repository moved.
pub fn outside_any_repository(error: &str) -> bool {
    error.contains("not a git repository (or any")
}

/// Only git's "not a git repository" means that; anything else (git missing, a
/// `safe.directory` refusal, a broken config) is shown in git's own words.
pub fn toplevel(path: &Path) -> Result<String, String> {
    if !path.is_dir() {
        return Err(format!("Folder not found: {}", path.display()));
    }
    run_text(path, &["rev-parse", "--show-toplevel"])
        .map(|s| s.trim().to_string())
        .map_err(|e| {
            if outside_any_repository(&e) {
                NOT_A_REPO.to_string()
            } else {
                e
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_reads_time_out() {
        assert_eq!(read_timeout(), None);
        assert_eq!(reading(read_timeout), Some(READ_TIMEOUT));
        // A nested scope ending doesn't end the outer one.
        let outer = reading(|| {
            reading(|| ());
            read_timeout()
        });
        assert_eq!(outer, Some(READ_TIMEOUT));
        let _ = std::panic::catch_unwind(|| reading(|| panic!("a read failed")));
        assert_eq!(read_timeout(), None, "reset after a panic");
        // A read run beside another is timed out as it is, and only then.
        assert_eq!(both(read_timeout, read_timeout), (None, None));
        let timeout = Some(READ_TIMEOUT);
        assert_eq!(
            reading(|| both(read_timeout, read_timeout)),
            (timeout, timeout)
        );
    }

    /// The exit used to be polled every 10 ms, so no git call came back sooner, however quick.
    #[test]
    fn a_short_git_call_returns_as_git_does() {
        let fastest = |f: &dyn Fn()| {
            (0..5)
                .map(|_| {
                    let t = std::time::Instant::now();
                    f();
                    t.elapsed()
                })
                .min()
                .unwrap()
        };
        let alone = fastest(&|| {
            Command::new(git()).arg("--version").output().unwrap();
        });
        // A refresh's read, with a timeout: the path every git call of a refresh takes.
        let called = fastest(&|| {
            reading(|| run(Path::new("."), &["--version"])).unwrap();
        });
        assert!(
            called < alone + Duration::from_millis(4),
            "{called:?} through run, {alone:?} for git alone"
        );
    }

    #[test]
    fn git_finds_the_repo_from_its_folder_only() {
        let cmd = command(Path::new("."), &["status"]);
        let removed: Vec<_> = cmd.get_envs().filter(|(_, v)| v.is_none()).collect();
        for var in [
            "GIT_DIR",
            "GIT_WORK_TREE",
            "GIT_INDEX_FILE",
            "GIT_COMMON_DIR",
        ] {
            assert!(removed.iter().any(|(k, _)| *k == var), "{var}");
        }
    }
}
