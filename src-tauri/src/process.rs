//! Running the programs GitViber shells out to: with the user's PATH, drained and timed out.

use std::collections::HashSet;
use std::ffi::{OsStr, OsString};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::RwLock;
use std::time::{Duration, Instant};

/// Apps launched from Finder get a bare PATH, which hides Homebrew git, the credential
/// helpers / ssh next to it, and whatever hooks call (node from nvm and the like). The login
/// shell's PATH fills that in once it has answered (shell.rs); until then, Homebrew's.
/// Merged once per login-PATH change: every git call asks.
pub(crate) fn search_path() -> OsString {
    static CACHE: RwLock<Option<(u64, OsString)>> = RwLock::new(None);
    let generation = crate::shell::generation();
    if let Some((g, path)) = CACHE.read().unwrap_or_else(|e| e.into_inner()).as_ref() {
        if *g == generation {
            return path.clone();
        }
    }
    let current = std::env::var_os("PATH").unwrap_or_default();
    let path = merge_paths(crate::shell::login_path().as_deref(), &current);
    *CACHE.write().unwrap_or_else(|e| e.into_inner()) = Some((generation, path.clone()));
    path
}

/// The login shell's entries first (its order decides which node a hook gets), then
/// Homebrew's, then the app's own; each directory once.
pub(crate) fn merge_paths(login: Option<&OsStr>, current: &OsStr) -> OsString {
    let homebrew: &[&str] = if cfg!(target_os = "macos") {
        &["/opt/homebrew/bin", "/usr/local/bin"]
    } else {
        &[]
    };
    let mut seen = HashSet::new();
    let dirs: Vec<PathBuf> = login
        .into_iter()
        .flat_map(std::env::split_paths)
        .chain(homebrew.iter().map(PathBuf::from))
        .chain(std::env::split_paths(current))
        .filter(|d| !d.as_os_str().is_empty() && seen.insert(d.clone()))
        .collect();
    std::env::join_paths(dirs).unwrap_or_else(|_| current.to_os_string())
}

/// Runs a prepared command, killing it after `timeout`. Output is drained on threads so a
/// chatty process can't block on a full pipe while we wait.
pub(crate) fn exec(
    mut cmd: Command,
    label: &str,
    ok_codes: &[i32],
    input: Option<&[u8]>,
    timeout: Option<Duration>,
) -> Result<Vec<u8>, String> {
    if input.is_some() {
        cmd.stdin(Stdio::piped());
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("could not run {label}: {e}"))?;
    if let (Some(data), Some(mut stdin)) = (input, child.stdin.take()) {
        // A process can exit before reading its input (a failing pre-commit hook stops
        // `commit -F -`); its status and stderr say why, not the broken pipe.
        match stdin.write_all(data) {
            Err(e) if e.kind() != std::io::ErrorKind::BrokenPipe => return Err(e.to_string()),
            _ => {}
        }
    }
    let drain = |r: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(mut r) = r {
                let _ = r.read_to_end(&mut buf);
            }
            buf
        })
    };
    let out = drain(
        child
            .stdout
            .take()
            .map(|s| Box::new(s) as Box<dyn Read + Send>),
    );
    let err = drain(
        child
            .stderr
            .take()
            .map(|s| Box::new(s) as Box<dyn Read + Send>),
    );
    let deadline = timeout.map(|t| Instant::now() + t);
    let status = loop {
        if let Some(st) = child.try_wait().map_err(|e| e.to_string())? {
            break st;
        }
        if deadline.is_some_and(|d| Instant::now() > d) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("{label} timed out"));
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let (stdout, stderr) = (
        out.join().unwrap_or_default(),
        err.join().unwrap_or_default(),
    );
    let code = status.code().unwrap_or(-1);
    if status.success() || ok_codes.contains(&code) {
        Ok(stdout)
    } else {
        // Some failures (e.g. "nothing to commit") are explained only on stdout.
        let text = |b: &[u8]| String::from_utf8_lossy(b).trim().to_string();
        let e = Some(text(&stderr))
            .filter(|e| !e.is_empty())
            .unwrap_or_else(|| text(&stdout));
        Err(if e.is_empty() {
            format!("{label} failed ({code})")
        } else {
            e
        })
    }
}

/// Gives the command a process group of its own, so `kill_group` also stops what it started.
pub fn in_own_group(cmd: &mut Command) {
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(cmd, 0);
    #[cfg(not(unix))]
    let _ = cmd;
}

/// Stops a child started `in_own_group`, and everything in its group. With a `grace`, SIGTERM
/// first so it can clean up, then SIGKILL for whatever is still there after it.
pub fn kill_group(child: &mut Child, grace: Duration) {
    // Not the `kill` command: procps's (Ubuntu) reads `-KILL -12345` as `-1`, every process
    // of the user, which took down the CI runner.
    #[cfg(unix)]
    let group = child.id() as libc::pid_t;
    if !grace.is_zero() {
        #[cfg(unix)]
        unsafe {
            libc::killpg(group, libc::SIGTERM);
        }
        let deadline = Instant::now() + grace;
        while Instant::now() < deadline && matches!(child.try_wait(), Ok(None)) {
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    #[cfg(unix)]
    unsafe {
        libc::killpg(group, libc::SIGKILL);
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Splits a command line into argv with shell-style quoting and nothing else a shell does: no
/// variables, globs or pipes, so a user's template can't do more than run one program.
/// '…' is literal; in "…" a `\` escapes only `"`, `\`, `$` and `` ` `` (as in sh), elsewhere
/// the next character. Nothing to run is an empty list; each caller says what to set up.
pub fn split_command(line: &str) -> Result<Vec<String>, String> {
    let mut args = Vec::new();
    // None between arguments, so `""` still makes an (empty) argument.
    let mut arg: Option<String> = None;
    let mut quote: Option<char> = None;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (None, c) if c.is_whitespace() => args.extend(arg.take()),
            (None, '\'' | '"') => {
                quote = Some(c);
                arg.get_or_insert_with(String::new);
            }
            (Some(q), c) if c == q => quote = None,
            (Some('\''), c) => arg.get_or_insert_with(String::new).push(c),
            (Some('"'), '\\') if !matches!(chars.peek(), Some('"' | '\\' | '$' | '`')) => {
                arg.get_or_insert_with(String::new).push('\\')
            }
            (_, '\\') => {
                let next = chars.next().ok_or("The command ends with a backslash.")?;
                arg.get_or_insert_with(String::new).push(next);
            }
            (_, c) => arg.get_or_insert_with(String::new).push(c),
        }
    }
    if let Some(q) = quote {
        return Err(format!("The command has an unclosed {q} quote."));
    }
    args.extend(arg);
    Ok(args)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn split(s: &str) -> Vec<String> {
        split_command(s).unwrap()
    }

    #[test]
    fn splits_like_a_shell_without_one() {
        assert_eq!(split("claude -p"), ["claude", "-p"]);
        assert_eq!(split("  nvim-qt   {path} "), ["nvim-qt", "{path}"]);
        assert_eq!(
            split(r#"/Users/me/My\ Tools/llm --system 'be brief' -m "gpt 4""#),
            [
                "/Users/me/My Tools/llm",
                "--system",
                "be brief",
                "-m",
                "gpt 4"
            ]
        );
        assert_eq!(
            split(r#"'/Apps/My Editor' --opt="a b" c\ d '$HOME' """#),
            ["/Apps/My Editor", "--opt=a b", "c d", "$HOME", ""]
        );
        assert_eq!(split(r#"say "it's""#), ["say", "it's"]);
        assert_eq!(split(r#"a "" b''c"#), ["a", "", "bc"]);
        // In "…" a backslash escapes only what sh lets it; before anything else it stays.
        assert_eq!(
            split(r#"a "x \"y\" \n \\ \$HOME" 'no \ $HOME'"#),
            ["a", r#"x "y" \n \ $HOME"#, r"no \ $HOME"]
        );
        // Shell syntax is just text: nothing is piped or substituted.
        assert_eq!(
            split("a | b $(rm -rf x)"),
            ["a", "|", "b", "$(rm", "-rf", "x)"]
        );
        assert!(split("   ").is_empty());
    }

    #[test]
    fn rejects_what_a_shell_would_not_run() {
        assert!(split_command("code 'oops").is_err());
        assert!(split_command("claude -p \"oops").is_err());
        assert!(split_command("claude -p \"oops\\\"").is_err());
        assert!(split_command("code \\").is_err());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn login_path_goes_first_and_each_dir_once() {
        let merged = merge_paths(
            Some(OsStr::new("/nvm/bin:/opt/homebrew/bin:/usr/bin")),
            OsStr::new("/usr/bin:/bin::/usr/bin"),
        );
        assert_eq!(
            merged,
            "/nvm/bin:/opt/homebrew/bin:/usr/bin:/usr/local/bin:/bin"
        );
        assert_eq!(
            merge_paths(None, OsStr::new("/usr/bin:/bin")),
            "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin"
        );
    }
}
