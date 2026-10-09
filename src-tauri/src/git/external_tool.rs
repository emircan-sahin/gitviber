//! The merge and diff tools set up for `git mergetool` / `git difftool`, run from the app.

use super::cmd::command;
use super::{config_value, run_text, run_with};
use crate::process;
use crate::scratch::ScratchDir;
use serde::Serialize;
use std::path::Path;
use std::process::Stdio;

/// The configured tools' names; None where nothing is set, or only a tool that needs a terminal.
#[derive(Serialize, Default, Debug, PartialEq)]
pub struct ExternalTools {
    pub merge: Option<String>,
    pub diff: Option<String>,
}

/// vimdiff and the like draw in a terminal, and the app runs tools without one: they'd fail, or
/// wait for a key no one can press.
fn needs_terminal(tool: &str) -> bool {
    tool.starts_with("vimdiff") || tool.starts_with("nvimdiff") || tool == "emerge"
}

/// As git's `--gui` mode reads them (git-mergetool--lib.sh): mergetool takes merge.guitool, then
/// merge.tool; difftool diff.guitool, merge.guitool, diff.tool, then merge.tool. `config` lists
/// them in the order git reads its files (system, global, includes, the repo's), and as for any
/// one-value key the last one read is the one git uses.
fn pick(config: &str) -> ExternalTools {
    let get = |key: &str| {
        config.lines().rev().find_map(|l| {
            let (k, v) = l.split_once(' ')?;
            k.eq_ignore_ascii_case(key).then(|| v.trim().to_string())
        })
    };
    // An empty value is no tool for that key: git goes on to the next one.
    let first = |keys: &[&str]| {
        keys.iter()
            .find_map(|k| get(k).filter(|t| !t.is_empty()))
            .filter(|t| !needs_terminal(t))
    };
    ExternalTools {
        merge: first(&["merge.guitool", "merge.tool"]),
        diff: first(&["diff.guitool", "merge.guitool", "diff.tool", "merge.tool"]),
    }
}

pub fn external_tools(repo: &Path) -> ExternalTools {
    // 1: none of them set.
    let out = run_with(
        repo,
        &["config", "--get-regexp", r"^(merge|diff)\.(gui)?tool$"],
        &[1],
        None,
    )
    .unwrap_or_default();
    pick(&String::from_utf8_lossy(&out))
}

/// Runs `git <args>` until the tool it opens is closed: no timeout, as a merge by hand takes what
/// it takes. Its output isn't piped: a GUI launcher that leaves a child holding a pipe open would
/// keep the read waiting for good (see process::exec). Errors go to a file, read once it's done.
fn run_tool(repo: &Path, label: &str, args: &[&str]) -> Result<(), String> {
    let scratch = ScratchDir::new("tool")?;
    let log = scratch.path().join("stderr");
    let file = std::fs::File::create(&log).map_err(|e| e.to_string())?;
    let mut cmd = command(repo, args);
    cmd.stdout(Stdio::null()).stderr(file);
    let status = process::spawn(&mut cmd)
        .and_then(|mut child| child.wait())
        .map_err(|e| format!("could not run git {label}: {e}"))?;
    if status.success() {
        return Ok(());
    }
    let mut err = std::fs::read(&log).unwrap_or_default();
    // A tool's own chatter can run long; git's reason comes last.
    if err.len() > 4096 {
        err.drain(..err.len() - 4096);
    }
    let err = String::from_utf8_lossy(&err).trim().to_string();
    Err(if err.is_empty() {
        format!("git {label} failed ({})", status.code().unwrap_or(-1))
    } else {
        err
    })
}

/// Opens the conflicted `path` in the merge tool; git stages it if the tool says it's merged.
/// Its BASE / LOCAL / REMOTE / BACKUP copies go to a temp folder, not beside the file, where a
/// tool that's missing or fails left them for an agent's `git add -A` to commit. So does the
/// `.orig` backup git keeps by default, unless the user asked for it.
pub fn open_merge_tool(repo: &Path, path: &str) -> Result<(), String> {
    let mut args = vec!["-c", "mergetool.writeToTemp=true"];
    if config_value(repo, None, "mergetool.keepBackup").is_none() {
        args.extend(["-c", "mergetool.keepBackup=false"]);
    }
    args.extend(["mergetool", "--gui", "--no-prompt", "--", path]);
    let started = std::time::SystemTime::now();
    let merged = run_tool(repo, "mergetool", &args);
    if merged.is_err() && !keeps_temporaries(repo) {
        // Off this thread, as reading a crowded temp folder takes a while (ScratchDir::fresh).
        let path = path.to_string();
        std::thread::spawn(move || remove_left_copies(&path, started));
    }
    merged
}

/// A tool that fails leaves git mergetool's temp folder of copies behind, and on macOS its
/// `mktemp -t` won't put that folder anywhere but the system's: the one for `path` made since
/// `started` goes, once the mergetool that made it (its copies carry its pid) is gone. Another
/// mergetool of the same file name may be open in another worktree.
fn remove_left_copies(path: &str, started: std::time::SystemTime) {
    let Some(local) = local_copy(path) else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) else {
        return;
    };
    let since = started - std::time::Duration::from_secs(1);
    for entry in entries.flatten() {
        if !entry
            .file_name()
            .to_string_lossy()
            .starts_with("git-mergetool-")
        {
            continue;
        }
        let new = entry
            .metadata()
            .and_then(|m| m.modified())
            .is_ok_and(|t| t >= since);
        let Some(pid) = new.then(|| maker(&entry.path(), &local)).flatten() else {
            continue;
        };
        // Elsewhere there's no telling whether it still runs.
        if cfg!(unix) && !super::worktree::process_alive(pid) {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

/// mergetool.keepTemporaries as git reads a bool (yes, on, 1, a key with no value…).
fn keeps_temporaries(repo: &Path) -> bool {
    run_text(
        repo,
        &[
            "config",
            "--type=bool",
            "--get",
            "mergetool.keepTemporaries",
        ],
    )
    .is_ok_and(|v| v.trim() == "true")
}

/// How git-mergetool's LOCAL copy of `path` begins: its name less its last extension, which
/// leaves nothing of a dotfile (`.env`'s is `_LOCAL_<pid>.env`).
fn local_copy(path: &str) -> Option<String> {
    let name = Path::new(path).file_name()?.to_string_lossy().into_owned();
    let stem = name
        .rsplit_once('.')
        .map_or(name.as_str(), |(stem, _)| stem);
    Some(format!("{stem}_LOCAL_"))
}

/// The pid in a copy's name, `a_LOCAL_<pid>.txt`.
fn maker(dir: &Path, local: &str) -> Option<u32> {
    std::fs::read_dir(dir).ok()?.flatten().find_map(|f| {
        let name = f.file_name().to_string_lossy().into_owned();
        let rest = name.strip_prefix(local)?;
        rest.split('.').next()?.parse().ok()
    })
}

/// Opens `path`'s unstaged changes, or with `staged` its staged ones, in the diff tool.
pub fn open_diff_tool(repo: &Path, path: &str, staged: bool) -> Result<(), String> {
    let mut args = vec!["difftool", "--gui", "--no-prompt"];
    if staged {
        args.push("--cached");
    }
    args.extend(["--", path]);
    run_tool(repo, "difftool", &args)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tools(merge: Option<&str>, diff: Option<&str>) -> ExternalTools {
        ExternalTools {
            merge: merge.map(str::to_string),
            diff: diff.map(str::to_string),
        }
    }

    #[test]
    fn keep_temporaries_reads_as_git_reads_a_bool() {
        let repo = std::env::temp_dir().join(format!("gitviber-keeptemp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&repo);
        std::fs::create_dir_all(&repo).unwrap();
        let git = |args: &[&str]| crate::git::run(&repo, args).unwrap();
        git(&["init", "-q"]);
        assert!(!keeps_temporaries(&repo));
        for (value, kept) in [
            ("true", true),
            ("Yes", true),
            ("on", true),
            ("1", true),
            ("false", false),
            ("off", false),
            ("0", false),
        ] {
            git(&["config", "mergetool.keepTemporaries", value]);
            assert_eq!(keeps_temporaries(&repo), kept, "{value}");
        }
        let _ = std::fs::remove_dir_all(&repo);
    }

    #[test]
    fn a_merge_tools_copies_are_named_as_git_mergetool_names_them() {
        assert_eq!(local_copy("src/a.txt").as_deref(), Some("a_LOCAL_"));
        assert_eq!(local_copy(".env").as_deref(), Some("_LOCAL_"));
        assert_eq!(local_copy("Makefile").as_deref(), Some("Makefile_LOCAL_"));
        assert_eq!(local_copy("a.test.ts").as_deref(), Some("a.test_LOCAL_"));
    }

    #[test]
    fn picks_tools_as_git_gui_mode_does() {
        assert_eq!(pick(""), tools(None, None));
        assert_eq!(
            pick("merge.tool kdiff3\n"),
            tools(Some("kdiff3"), Some("kdiff3"))
        );
        assert_eq!(
            pick("merge.tool opendiff\ndiff.tool meld\n"),
            tools(Some("opendiff"), Some("meld"))
        );
        assert_eq!(
            pick("diff.tool meld\ndiff.guitool bc\nmerge.guitool smerge\nmerge.tool p4merge\n"),
            tools(Some("smerge"), Some("bc"))
        );
        // A terminal tool is left out, and doesn't stand in for a GUI one.
        assert_eq!(
            pick("merge.tool vimdiff\ndiff.tool nvimdiff2\n"),
            tools(None, None)
        );
        assert_eq!(
            pick("merge.tool vimdiff\ndiff.guitool meld\n"),
            tools(None, Some("meld"))
        );
        assert_eq!(pick("merge.tool \n"), tools(None, None));
        // The last value read wins (the repo's over the global one), and empty means none.
        assert_eq!(
            pick("merge.tool meld\nmerge.tool kdiff3\n"),
            tools(Some("kdiff3"), Some("kdiff3"))
        );
        assert_eq!(
            pick("merge.tool meld\nmerge.tool vimdiff\n"),
            tools(None, None)
        );
        assert_eq!(pick("merge.tool meld\nmerge.tool \n"), tools(None, None));
    }
}
