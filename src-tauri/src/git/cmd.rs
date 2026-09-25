//! Building and running git commands: the environment every call gets, and the repo root.

use crate::network::{self, Net};
use crate::process::{exec, search_path};
use std::path::Path;
use std::process::{Command, Stdio};

/// The app reads git's messages ("not a git repository", "non-fast-forward", index.lock,
/// progress phases), so git must speak English whatever LANG says, as in VS Code. UTF-8
/// keeps non-ASCII paths and messages intact. LANGUAGE would outrank both for gettext.
pub(crate) fn in_english(cmd: &mut Command) -> &mut Command {
    cmd.env("LC_ALL", "en_US.UTF-8")
        .env("LANG", "en_US.UTF-8")
        .env_remove("LANGUAGE")
}

pub(crate) fn command(repo: &Path, args: &[&str]) -> Command {
    let mut cmd = Command::new("git");
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
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cmd
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
        None,
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

pub(crate) fn run_text(repo: &Path, args: &[&str]) -> Result<String, String> {
    run(repo, args).map(|b| String::from_utf8_lossy(&b).into_owned())
}

pub(super) fn has_head(repo: &Path) -> bool {
    run(repo, &["rev-parse", "--verify", "-q", "HEAD"]).is_ok()
}

/// toplevel's error for an existing folder outside any repository; the page offers `git init`.
pub const NOT_A_REPO: &str = "git:not-a-repo";

/// Only git's "not a git repository" means that; anything else (git missing, a
/// `safe.directory` refusal, a broken config) is shown in git's own words.
pub fn toplevel(path: &Path) -> Result<String, String> {
    if !path.is_dir() {
        return Err(format!("Folder not found: {}", path.display()));
    }
    run_text(path, &["rev-parse", "--show-toplevel"])
        .map(|s| s.trim().to_string())
        .map_err(|e| {
            // Not "fatal: not a git repository: <path>", which is a .git file gone bad.
            if e.contains("not a git repository (or any") {
                NOT_A_REPO.to_string()
            } else {
                e
            }
        })
}
