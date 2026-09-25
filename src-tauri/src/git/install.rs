//! Whether a usable git is installed, and installing Apple's command line tools when not.

use super::in_english;
use crate::process::{exec, search_path};
use serde::Serialize;
use std::process::{Command, Stdio};
use std::time::Duration;

/// The oldest git that works: `worktree list --porcelain -z`, which every repo open runs
/// (main_worktree), arrived in 2.36. switch/restore need 2.23.
pub const MIN_VERSION: (u32, u32) = (2, 36);

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GitInfo {
    /// "ok", "old", "missing" (git can't run), or "tools": macOS's /usr/bin/git stub
    /// without the Command Line Tools behind it.
    pub state: &'static str,
    /// `git --version` without its prefix, e.g. "2.39.5 (Apple Git-154)".
    pub version: Option<String>,
    /// Why it can't run, in the OS's or xcrun's words.
    pub detail: Option<String>,
    pub minimum: String,
}

pub fn check_install() -> GitInfo {
    let out = in_english(&mut Command::new("git"))
        .arg("--version")
        .env("PATH", search_path())
        .stdin(Stdio::null())
        .output();
    classify_install(out.map_err(|e| format!("could not run git: {e}")).map(|o| {
        (
            o.status.success(),
            String::from_utf8_lossy(&o.stdout).trim().to_string(),
            String::from_utf8_lossy(&o.stderr).trim().to_string(),
        )
    }))
}

/// `ran`: whether `git --version` succeeded, with its stdout and stderr.
fn classify_install(ran: Result<(bool, String, String), String>) -> GitInfo {
    let info = |state, version, detail| GitInfo {
        state,
        version,
        detail,
        minimum: format!("{}.{}", MIN_VERSION.0, MIN_VERSION.1),
    };
    match ran {
        Err(e) => info("missing", None, Some(e)),
        // The stub asks xcode-select to install the tools and fails until they're there.
        Ok((false, _, err)) if err.contains("xcrun: error") || err.contains("xcode-select") => {
            info("tools", None, Some(err))
        }
        Ok((false, out, err)) => info(
            "missing",
            None,
            Some(if err.is_empty() { out } else { err }),
        ),
        Ok((true, out, _)) => {
            let version = out.trim_start_matches("git version ").to_string();
            let old = parse_version(&version).is_some_and(|v| v < MIN_VERSION);
            info(if old { "old" } else { "ok" }, Some(version), None)
        }
    }
}

/// Major and minor of "2.39.5 (Apple Git-154)" or "2.45.1.windows.1".
fn parse_version(v: &str) -> Option<(u32, u32)> {
    let mut parts = v.split(|c: char| !c.is_ascii_digit());
    Some((parts.next()?.parse().ok()?, parts.next()?.parse().ok()?))
}

/// Opens macOS's installer for the Command Line Tools, which bring git.
pub fn install_tools() -> Result<(), String> {
    if !cfg!(target_os = "macos") {
        return Err("Install git with your system's package manager.".into());
    }
    let mut cmd = Command::new("xcode-select");
    cmd.arg("--install")
        .env("PATH", search_path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    exec(
        cmd,
        "xcode-select",
        &[],
        None,
        Some(Duration::from_secs(30)),
    )
    .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_states() {
        let ran = |ok, out: &str, err: &str| classify_install(Ok((ok, out.into(), err.into())));
        let ok = ran(true, "git version 2.39.5 (Apple Git-154)", "");
        assert_eq!(
            (ok.state, ok.version.as_deref()),
            ("ok", Some("2.39.5 (Apple Git-154)"))
        );
        assert_eq!(ran(true, "git version 2.45.1.windows.1", "").state, "ok");
        assert_eq!(ran(true, "git version 2.35.8", "").state, "old");
        assert_eq!(ran(true, "git version 1.9.5", "").state, "old");
        let stub = ran(
            false,
            "",
            "xcrun: error: invalid active developer path (/Library/Developer/CommandLineTools), missing xcrun at: /Library/Developer/CommandLineTools/usr/bin/xcrun",
        );
        assert_eq!(stub.state, "tools");
        assert_eq!(ran(false, "", "boom").state, "missing");
        assert_eq!(
            classify_install(Err("could not run git".into())).state,
            "missing"
        );
    }
}
