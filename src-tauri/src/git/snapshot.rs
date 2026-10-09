//! The worktree's uncommitted changes as a tree object, for a guided review of them: staged,
//! unstaged and new files, written through a copy of the index so the real one is never touched.
//! The tree reads back like any commit's (`range_files`, `diff_pair("range")`, `tree:path`).

use super::{empty_tree, run, run_text};
use crate::patch::Scratch;
use std::path::Path;
use std::time::{Duration, SystemTime};

/// New files past these stay out: an un-ignored build folder or a video would be hashed into
/// the object store on every review.
pub(crate) const UNTRACKED_MAX_FILE: u64 = 5 * 1024 * 1024;
const UNTRACKED_MAX_TOTAL: u64 = 64 * 1024 * 1024;
const UNTRACKED_MAX_FILES: usize = 1000;

/// `git add` refuses a CRLF file under core.safecrlf=true; a split index would leave a new
/// sharedindex file in the git dir for every snapshot.
const PINS: [&str; 4] = ["-c", "core.safecrlf=false", "-c", "core.splitIndex=false"];

/// A copy of the index this old is one a killed run left behind: no run of ours lasts an hour.
const STALE: Duration = Duration::from_secs(3600);

pub struct Snapshot {
    /// HEAD's commit, or the empty tree before the first commit.
    pub base: String,
    pub tree: String,
    /// Whether HEAD is a commit (no first commit yet: everything is new).
    pub born: bool,
    /// New files left out by the caps (or nested repositories), sorted.
    pub left_out: Vec<String>,
}

impl Snapshot {
    /// Nothing differs from HEAD (what was left out aside).
    pub fn is_empty(&self, repo: &Path) -> Result<bool, String> {
        if !self.born {
            return Ok(self.tree == self.base);
        }
        let head = run_text(repo, &["rev-parse", &format!("{}^{{tree}}", self.base)])?;
        Ok(self.tree == head.trim())
    }
}

/// The worktree as it is now, against HEAD. Run inside `git::reading`, so a wedged filter or
/// fsmonitor gives up.
pub fn worktree_snapshot(repo: &Path) -> Result<Snapshot, String> {
    Scratch::sweep(repo, STALE);
    let head =
        run_text(repo, &["rev-parse", "--verify", "-q", "HEAD"]).map(|s| s.trim().to_string());
    let (base, born) = match head {
        Ok(h) if !h.is_empty() => (h, true),
        _ => (empty_tree(repo)?, false),
    };
    let index = Scratch::index(repo)?;
    let git =
        |args: &[&str], input: Option<&[u8]>| index.git(repo, &[&PINS[..], args].concat(), input);
    // Tracked edits and deletions; a conflicted file goes in as it is on disk, markers and all.
    git(&["add", "-u"], None)?;
    let (new, left_out) = untracked(repo)?;
    if !new.is_empty() {
        let list: Vec<u8> = new.iter().flat_map(|p| p.bytes().chain([0])).collect();
        // Plumbing: no ignore or sparse-checkout checks again (ls-files did them), clean filters
        // still run. --remove: a dev server's temp file gone since ls-files is skipped, not fatal.
        git(
            &["update-index", "--add", "--remove", "-z", "--stdin"],
            Some(&list),
        )?;
    }
    let tree = String::from_utf8_lossy(&git(&["write-tree"], None)?)
        .trim()
        .to_string();
    Ok(Snapshot {
        base,
        tree,
        born,
        left_out,
    })
}

/// New files not ignored, within the caps (in path order, so the cut is the same each time), and
/// those left out. A folder (a nested repository) is never added.
fn untracked(repo: &Path) -> Result<(Vec<String>, Vec<String>), String> {
    let raw = run(repo, &["ls-files", "-o", "--exclude-standard", "-z"])?;
    let mut paths: Vec<String> = raw
        .split(|b| *b == 0)
        .filter(|p| !p.is_empty())
        .map(|p| String::from_utf8_lossy(p).into_owned())
        .collect();
    paths.sort();
    let (mut kept, mut left) = (vec![], vec![]);
    let mut total = 0u64;
    for p in paths {
        let size = std::fs::symlink_metadata(repo.join(&p))
            .map(|m| m.len())
            .unwrap_or(0);
        if p.ends_with('/')
            || size > UNTRACKED_MAX_FILE
            || total + size > UNTRACKED_MAX_TOTAL
            || kept.len() >= UNTRACKED_MAX_FILES
        {
            left.push(p.trim_end_matches('/').to_string());
        } else {
            total += size;
            kept.push(p);
        }
    }
    Ok((kept, left))
}

/// What the uncommitted changes are, cheaply: HEAD, and each changed path with its size and time
/// on disk. Staging alone leaves it as it was; an edit, a new file or a commit changes it. Kept
/// with saved reviews, so a hash that stays the same across releases (FNV-1a), not std's.
pub fn changes_stamp(repo: &Path) -> Result<String, String> {
    let head = run_text(repo, &["rev-parse", "--verify", "-q", "HEAD"]).unwrap_or_default();
    let raw = run(
        repo,
        &[
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--no-renames",
        ],
    )?;
    let mut paths: Vec<&[u8]> = raw
        .split(|b| *b == 0)
        // "XY path": the codes say staged or not, which the stamp leaves out.
        .filter_map(|e| e.get(3..))
        .filter(|p| !p.is_empty())
        .collect();
    // A file staged as removed and new on disk (`git rm --cached`) is listed twice.
    paths.sort();
    paths.dedup();
    let mut h = Fnv::default();
    h.add(head.trim().as_bytes());
    for p in paths {
        h.add(p);
        if let Ok(m) = std::fs::symlink_metadata(repo.join(String::from_utf8_lossy(p).as_ref())) {
            h.add(&m.len().to_le_bytes());
            let at = m
                .modified()
                .ok()
                .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok());
            h.add(&at.map_or(0, |d| d.as_nanos()).to_le_bytes());
        }
    }
    Ok(format!("{:016x}", h.0))
}

/// FNV-1a, 64 bits; each part ends in a 0, so "ab" then "c" isn't "a" then "bc".
struct Fnv(u64);

impl Default for Fnv {
    fn default() -> Self {
        Fnv(0xcbf2_9ce4_8422_2325)
    }
}

impl Fnv {
    fn add(&mut self, bytes: &[u8]) {
        for b in bytes.iter().chain([&0u8]) {
            self.0 = (self.0 ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3);
        }
    }
}
