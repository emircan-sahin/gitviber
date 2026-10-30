//! Git LFS files in a diff. git stores only a small pointer for them; the file itself is in
//! `.git/lfs/objects` once downloaded. A diff never downloads one: that's a network transfer
//! the user didn't ask for. A checkout does, and downloads them first (prefetch).

use crate::git;
use crate::network::{self, Net};
use std::path::{Path, PathBuf};

const SPEC: &str = "version https://git-lfs.github.com/spec/v1";

#[derive(Debug, PartialEq)]
pub struct Pointer {
    /// sha256, 64 lowercase hex digits.
    pub oid: String,
    /// The real file's size in bytes.
    pub size: u64,
}

/// The pointer `bytes` are, if they are one. Pointers are under 1024 bytes by the spec.
pub fn pointer(bytes: &[u8]) -> Option<Pointer> {
    if bytes.len() >= 1024 {
        return None;
    }
    let mut lines = std::str::from_utf8(bytes).ok()?.lines();
    if lines.next()? != SPEC {
        return None;
    }
    let (mut oid, mut size) = (None, None);
    for line in lines {
        if let Some(h) = line.strip_prefix("oid sha256:") {
            oid = Some(h);
        } else if let Some(n) = line.strip_prefix("size ") {
            size = n.parse().ok();
        }
    }
    // Checked before it goes into a path.
    let oid = oid.filter(|h| {
        h.len() == 64
            && h.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })?;
    Some(Pointer {
        oid: oid.to_string(),
        size: size?,
    })
}

/// The downloaded object, if it is. Worktrees share the main repository's store.
pub fn object(repo: &Path, p: &Pointer) -> Option<PathBuf> {
    let path = objects(repo)?
        .join(&p.oid[..2])
        .join(&p.oid[2..4])
        .join(&p.oid);
    path.is_file().then_some(path)
}

fn objects(repo: &Path) -> Option<PathBuf> {
    git::git_dirs(repo).map(|d| d.common.join("lfs/objects"))
}

/// Where the checkout that follows a prefetch writes its files.
#[derive(Clone, Copy, PartialEq)]
pub enum Checkout {
    /// This worktree, over its uncommitted changes.
    Here,
    NewWorktree,
}

/// Downloads the LFS objects commit `rev` (a full ref or a sha) needs before a checkout of it.
/// Inside the checkout git-lfs downloads them file by file, with no progress, no askpass for
/// its ssh or https login, and no safe point to stop; here it is a fetch like any other. Does
/// nothing for a repository without LFS, or without git-lfs installed, or set to skip LFS
/// downloads; and nothing when git is about to refuse the checkout anyway, which then goes
/// ahead without it and fails at once.
pub fn prefetch(repo: &Path, rev: &str, into: Checkout, net: &Net) -> Result<(), String> {
    if std::env::var("GIT_LFS_SKIP_SMUDGE").is_ok_and(|v| truthy(&v)) {
        return Ok(());
    }
    let Ok(sha) = git::run_text(
        repo,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("{rev}^{{commit}}"),
        ],
    ) else {
        return Ok(());
    };
    let sha = sha.trim();
    if !in_use(repo, sha) || skips_smudge(repo) || refused(repo, rev, sha, into) {
        return Ok(());
    }
    let Some(remote) = remote_for(repo, rev) else {
        return Ok(());
    };
    let mut cmd = git::command(repo, &["lfs", "fetch", &remote, sha]);
    // Its progress is drawn for terminals only unless asked, and then on stdout.
    cmd.env("GIT_LFS_FORCE_PROGRESS", "1");
    match network::run(cmd, "git lfs fetch", net, None) {
        Err(e) if e.contains("'lfs' is not a git command") => Ok(()),
        r => r.map(|_| ()),
    }
}

/// The repository has LFS objects, or `sha` says some files are stored there.
fn in_use(repo: &Path, sha: &str) -> bool {
    let attrs = format!("{sha}:.gitattributes");
    git::run_text(repo, &["cat-file", "-p", &attrs]).is_ok_and(|a| a.contains("filter=lfs"))
        || objects(repo).is_some_and(|d| d.is_dir())
}

/// As git-lfs reads GIT_LFS_SKIP_SMUDGE.
fn truthy(v: &str) -> bool {
    matches!(
        v.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

/// `git lfs install --skip-smudge` (or by hand): checkouts keep the pointers, and so does this.
fn skips_smudge(repo: &Path) -> bool {
    git::ConfigList::read(repo).is_ok_and(|c| {
        ["filter.lfs.smudge", "filter.lfs.process"].iter().any(|k| {
            c.value(k)
                .is_some_and(|v| v.split_whitespace().any(|w| w == "--skip"))
        })
    })
}

/// git will refuse the checkout of `rev` (at `sha`) for a reason seen cheaply: its branch is
/// out in a worktree already, or an uncommitted change here is in a file the checkout changes.
/// Untracked files in the way aren't looked for.
fn refused(repo: &Path, rev: &str, sha: &str, into: Checkout) -> bool {
    let held = |branch| {
        git::worktrees(repo)
            .is_ok_and(|all| all.iter().any(|w| w.branch.as_deref() == Some(branch)))
    };
    if rev.strip_prefix("refs/heads/").is_some_and(held) {
        return true;
    }
    if into == Checkout::NewWorktree {
        return false;
    }
    let names = |args: &[&str]| {
        git::run(repo, args).map(|out| {
            out.split(|b| *b == 0)
                .filter(|p| !p.is_empty())
                .map(<[u8]>::to_vec)
                .collect::<std::collections::HashSet<_>>()
        })
    };
    let (Ok(changes), Ok(dirty)) = (
        names(&["diff", "--name-only", "-z", "--no-renames", "HEAD", sha]),
        names(&["diff", "--name-only", "-z", "--no-renames", "HEAD"]),
    ) else {
        return false;
    };
    !changes.is_disjoint(&dirty)
}

/// Where `rev`'s objects are: its remote for a remote branch, a local branch's upstream, else
/// origin or the only remote.
fn remote_for(repo: &Path, rev: &str) -> Option<String> {
    let listed = git::run_text(repo, &["remote"]).ok()?;
    let remotes: Vec<&str> = listed.lines().collect();
    let tracked = match rev.strip_prefix("refs/heads/") {
        Some(b) => git::config_value(repo, None, &format!("branch.{b}.remote")),
        None => rev
            .strip_prefix("refs/remotes/")
            .and_then(|r| git::split_remote(remotes.iter().copied(), r))
            .map(|(remote, _)| remote.to_string()),
    };
    tracked
        .filter(|r| remotes.contains(&r.as_str()))
        .or_else(|| {
            remotes
                .iter()
                .find(|r| **r == "origin")
                .map(|r| r.to_string())
        })
        .or_else(|| (remotes.len() == 1).then(|| remotes[0].to_string()))
}

pub fn not_downloaded(p: &Pointer) -> String {
    format!("LFS object not downloaded ({})", size_label(p.size))
}

pub fn size_label(n: u64) -> String {
    if n < 1024 {
        return format!("{n} B");
    }
    let mut v = n as f64 / 1024.0;
    for unit in ["KB", "MB", "GB"] {
        if v < 1024.0 || unit == "GB" {
            return format!("{v:.1} {unit}");
        }
        v /= 1024.0;
    }
    unreachable!()
}

#[cfg(test)]
mod tests {
    use super::*;

    const OID: &str = "4d7a214614ab2935c943f9e0ff69d22eadbb8f32b1258daaa5e2ca24d17e2393";

    #[test]
    fn parses_pointers() {
        let text = format!("{SPEC}\noid sha256:{OID}\nsize 12345\n");
        assert_eq!(
            pointer(text.as_bytes()),
            Some(Pointer {
                oid: OID.into(),
                size: 12345
            })
        );
        // Extension lines are allowed between version and oid.
        let ext = format!("{SPEC}\next-0-foo sha256:{OID}\noid sha256:{OID}\nsize 1\n");
        assert!(pointer(ext.as_bytes()).is_some());
        assert_eq!(pointer(b"hello\n"), None);
        assert_eq!(
            pointer(format!("{SPEC}\noid sha256:{OID}\n").as_bytes()),
            None
        );
        let bad = format!("{SPEC}\noid sha256:../../{}\nsize 1\n", &OID[6..]);
        assert_eq!(pointer(bad.as_bytes()), None);
        let long = format!("{SPEC}\noid sha256:{OID}\nsize 1\n{}", "x".repeat(1024));
        assert_eq!(pointer(long.as_bytes()), None);
    }

    #[test]
    fn labels_sizes() {
        assert_eq!(size_label(900), "900 B");
        assert_eq!(size_label(13_002_342), "12.4 MB");
        assert_eq!(size_label(5 << 40), "5120.0 GB");
    }
}
