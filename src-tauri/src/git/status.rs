//! The working tree's status: changed files, line counts, nested repos, the operation under way.

use super::{
    command, is_binary, operation, publish_remote_among, push_target, read_regular, remotes, run,
    worktrees, PushTarget, MAX_TEXT_BYTES,
};
use crate::process::exec;
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FileChange {
    pub path: String,
    pub old_path: Option<String>,
    /// One of M A D R C T U ? (U = conflicted, ? = untracked)
    pub status: String,
    pub additions: Option<u32>,
    pub deletions: Option<u32>,
    /// Content identity for "viewed" marks: the index blob for staged entries, size+mtime
    /// for the working tree (cheap, and changes on every write).
    pub oid: Option<String>,
    /// For conflicts, git's two-letter code: UU both modified, AA both added,
    /// UD deleted by them, DU deleted by us, AU/UA added by one side, DD both deleted.
    pub conflict: Option<String>,
    /// Untracked entries that are another repository's root. This repo's own linked
    /// worktrees are left out of status: the worktree picker reaches them.
    pub nested: Option<Nested>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Nested {
    /// Absolute path of the nested repository.
    pub path: String,
}

/// A merge, rebase, cherry-pick or revert that stopped and waits for the user.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Operation {
    /// "merge" | "rebase" | "cherry-pick" | "revert"
    pub kind: String,
    /// Branch being rebased, or what is being merged in (from MERGE_MSG).
    pub subject: Option<String>,
    /// Rebase progress (1-based step of total).
    pub step: Option<u32>,
    pub total: Option<u32>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoStatus {
    pub root: String,
    pub branch: Option<String>,
    pub head: Option<String>,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    /// Where `git push` sends this branch, which a fork can set apart from where it pulls
    /// (`remote.pushDefault`): pull from upstream/dev, push to origin/dev.
    pub push: Option<PushTarget>,
    /// Configured remotes, to pick where an unpublished branch goes.
    pub remotes: Vec<String>,
    /// Where Publish sends a branch with no upstream; None when that's the user's choice.
    pub publish: Option<String>,
    pub staged: Vec<FileChange>,
    pub unstaged: Vec<FileChange>,
    pub conflicted: Vec<FileChange>,
    pub operation: Option<Operation>,
}

pub(super) fn change(path: &str, old_path: Option<&str>, status: char) -> FileChange {
    FileChange {
        path: path.to_string(),
        old_path: old_path.map(str::to_string),
        status: status.to_string(),
        additions: None,
        deletions: None,
        oid: None,
        conflict: None,
        nested: None,
    }
}

/// Parses `--numstat -z` into path -> (additions, deletions). Binary files report `-`.
pub(super) fn parse_numstat(raw: &[u8]) -> HashMap<String, (Option<u32>, Option<u32>)> {
    let mut map = HashMap::new();
    let mut tokens = raw
        .split(|b| *b == 0)
        .map(|t| String::from_utf8_lossy(t).into_owned());
    while let Some(tok) = tokens.next() {
        let mut fields = tok.splitn(3, '\t');
        let (Some(a), Some(d), Some(path)) = (fields.next(), fields.next(), fields.next()) else {
            continue;
        };
        // Renames leave the path field empty and put old\0new in the next two tokens.
        let path = if path.is_empty() {
            tokens.next();
            match tokens.next() {
                Some(p) => p,
                None => break,
            }
        } else {
            path.to_string()
        };
        map.insert(path, (a.parse().ok(), d.parse().ok()));
    }
    map
}

pub(super) fn apply_numstat(
    list: &mut [FileChange],
    stats: &HashMap<String, (Option<u32>, Option<u32>)>,
) {
    for f in list {
        if let Some((a, d)) = stats.get(&f.path) {
            f.additions = *a;
            f.deletions = *d;
        }
    }
}

/// What one status may read to count lines of untracked files not counted before. A big
/// generated folder shows `?` for the rest instead of stalling the refresh; each refresh
/// counts more of it, as counts are cached.
pub(super) struct CountBudget {
    files: usize,
    bytes: u64,
}

impl Default for CountBudget {
    fn default() -> Self {
        CountBudget {
            files: 2_000,
            bytes: 64 << 20,
        }
    }
}

/// Line count of an untracked file (None: binary or unreadable), or None when it's past
/// the budget. Status runs on every change on disk, so counts are cached by size and mtime:
/// a big untracked folder is read once, not on each refresh.
pub(super) fn count_lines(repo: &Path, rel: &str, budget: &mut CountBudget) -> Option<Option<u32>> {
    type Key = (std::path::PathBuf, u64, Option<std::time::SystemTime>);
    static CACHE: OnceLock<std::sync::Mutex<HashMap<Key, Option<u32>>>> = OnceLock::new();
    let path = repo.join(rel);
    let Ok(meta) = std::fs::metadata(&path) else {
        return Some(None);
    };
    let key = (path, meta.len(), meta.modified().ok());
    let cache = CACHE.get_or_init(Default::default);
    if let Some(n) = cache.lock().unwrap().get(&key) {
        return Some(*n);
    }
    // Past MAX_TEXT_BYTES nothing is read.
    let cost = if meta.len() > MAX_TEXT_BYTES as u64 {
        0
    } else {
        meta.len()
    };
    if budget.files == 0 || cost > budget.bytes {
        return None;
    }
    budget.files -= 1;
    budget.bytes -= cost;
    let Ok(Some(bytes)) = read_regular(&key.0) else {
        return Some(None);
    };
    let n = (!is_binary(&bytes)).then(|| {
        let n = bytes.iter().filter(|b| **b == b'\n').count();
        let trailing = !bytes.is_empty() && *bytes.last().unwrap() != b'\n';
        (n + trailing as usize) as u32
    });
    let mut cache = cache.lock().unwrap();
    // Stale keys (old mtimes) pile up as files change; start over rather than track them.
    if cache.len() > 50_000 {
        cache.clear();
    }
    cache.insert(key, n);
    Some(n)
}

pub fn status(repo: &Path) -> Result<RepoStatus, String> {
    let raw = run(
        repo,
        &[
            "status",
            "--porcelain=v2",
            "-z",
            "--branch",
            "--untracked-files=all",
        ],
    )?;
    let mut st = RepoStatus {
        root: repo.to_string_lossy().into_owned(),
        branch: None,
        head: None,
        upstream: None,
        push: None,
        remotes: vec![],
        publish: None,
        ahead: 0,
        behind: 0,
        staged: vec![],
        unstaged: vec![],
        conflicted: vec![],
        operation: operation(repo),
    };

    let mut nested_roots = std::collections::HashSet::new();
    let mut budget = CountBudget::default();
    let mut records = raw
        .split(|b| *b == 0)
        .map(|t| String::from_utf8_lossy(t).into_owned());
    while let Some(rec) = records.next() {
        if let Some(h) = rec.strip_prefix("# ") {
            let (key, val) = h.split_once(' ').unwrap_or((h, ""));
            match key {
                "branch.oid" if val != "(initial)" => st.head = Some(val.chars().take(7).collect()),
                "branch.head" if val != "(detached)" => st.branch = Some(val.to_string()),
                "branch.upstream" => st.upstream = Some(val.to_string()),
                "branch.ab" => {
                    for part in val.split(' ') {
                        if let Some(n) = part.strip_prefix('+') {
                            st.ahead = n.parse().unwrap_or(0);
                        } else if let Some(n) = part.strip_prefix('-') {
                            st.behind = n.parse().unwrap_or(0);
                        }
                    }
                }
                _ => {}
            }
            continue;
        }
        let kind = rec.chars().next().unwrap_or(' ');
        match kind {
            '1' | '2' => {
                // 1 XY sub mH mI mW hH hI path  |  2 XY sub mH mI mW hH hI Xscore path \0 orig
                let fields: Vec<&str> = rec.splitn(if kind == '1' { 9 } else { 10 }, ' ').collect();
                let Some(path) = fields.last() else { continue };
                let orig = if kind == '2' { records.next() } else { None };
                let xy: Vec<char> = fields.get(1).copied().unwrap_or("..").chars().collect();
                let (x, y) = (xy[0], xy[1]);
                if x != '.' {
                    let mut f = change(path, orig.as_deref(), x);
                    f.oid = fields.get(7).map(|h| h.to_string());
                    st.staged.push(f);
                }
                if y != '.' {
                    // In the worktree the rename is already recorded in the index, so show it as M.
                    let mut f = change(path, None, y);
                    if new_gitlink(&fields) {
                        f.nested = Some(nested(repo, path));
                    }
                    st.unstaged.push(f);
                }
            }
            'u' => {
                // u XY sub m1 m2 m3 mW h1 h2 h3 path
                let fields: Vec<&str> = rec.splitn(11, ' ').collect();
                if let Some(path) = fields.last() {
                    let mut f = change(path, None, 'U');
                    f.conflict = fields.get(1).map(|xy| xy.to_string());
                    st.conflicted.push(f);
                }
            }
            '?' => {
                let path = &rec[2..];
                let mut f = change(path, None, '?');
                if let Some(root) = untracked_nested_root(repo, path) {
                    // Files of a broken worktree collapse into one entry for its folder.
                    if !nested_roots.insert(root.clone()) {
                        continue;
                    }
                    f.path = format!("{root}/");
                    f.nested = Some(nested(repo, &root));
                } else if let Some(n) = count_lines(repo, path, &mut budget) {
                    f.additions = n;
                    f.deletions = Some(0);
                }
                st.unstaged.push(f);
            }
            _ => {}
        }
    }

    for f in &mut st.unstaged {
        f.oid = disk_oid(repo, &f.path);
    }
    if let Some(b) = &st.branch {
        st.push = push_target(repo, b);
    }
    st.remotes = remotes(repo);
    if st.upstream.is_none() && st.branch.is_some() && !st.remotes.is_empty() {
        st.publish = publish_remote_among(repo, &st.remotes, st.branch.as_deref()).ok();
    }
    if st.unstaged.iter().any(|f| f.nested.is_some()) {
        drop_worktrees(repo, &mut st.unstaged);
    }
    if !st.unstaged.is_empty() {
        let stats = parse_numstat(&run(repo, &["diff", "--numstat", "-z"])?);
        apply_numstat(&mut st.unstaged, &stats);
    }
    if !st.staged.is_empty() {
        let stats = parse_numstat(&run(repo, &["diff", "--cached", "--numstat", "-z", "-M"])?);
        apply_numstat(&mut st.staged, &stats);
    }
    Ok(st)
}

/// A working-tree file's `oid`: its size and mtime, None once it's gone.
pub(super) fn disk_oid(repo: &Path, path: &str) -> Option<String> {
    let meta = std::fs::metadata(repo.join(path)).ok()?;
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos());
    Some(format!("{}:{mtime}", meta.len()))
}

/// With `--untracked-files=all` git lists a directory only when it is another repository's
/// root, which it won't descend into. The `.git` check confirms it.
pub(super) fn is_nested_repo(repo: &Path, path: &str) -> bool {
    path.ends_with('/') && repo.join(path).join(".git").exists()
}

/// Root of the nested repository an untracked path is, or lies in. The second case is a
/// worktree whose `.git` link broke (e.g. the main repo moved): git then lists its files
/// one by one as if they were ours, and `git worktree repair` would reconnect it.
pub(super) fn untracked_nested_root(repo: &Path, path: &str) -> Option<String> {
    if is_nested_repo(repo, path) {
        return Some(path.trim_end_matches('/').to_string());
    }
    Path::new(path)
        .ancestors()
        .skip(1)
        .filter(|a| !a.as_os_str().is_empty() && repo.join(a).join(".git").exists())
        .last()
        .map(|a| a.to_string_lossy().into_owned())
}

/// A tracked path that became a repository with commits shows as a type change to a gitlink
/// (mode 160000); `git add` would record it. A submodule already has one in the index.
/// `fields` is a split porcelain v2 `1`/`2` record: mI at 4, mW at 5.
pub(super) fn new_gitlink(fields: &[&str]) -> bool {
    fields.get(5) == Some(&"160000") && fields.get(4) != Some(&"160000")
}

fn nested(repo: &Path, rel: &str) -> Nested {
    Nested {
        path: repo.join(rel).to_string_lossy().into(),
    }
}

/// Removes this repo's own linked worktrees, keeping unrelated nested repositories. A worktree
/// whose link broke (the repo moved) no longer matches its listed path, so it stays visible.
fn drop_worktrees(repo: &Path, files: &mut Vec<FileChange>) {
    let Ok(list) = worktrees(repo) else { return };
    let real = |p: &str| Path::new(p).canonicalize().ok();
    let known: Vec<_> = list.iter().filter_map(|w| real(&w.path)).collect();
    files.retain(|f| {
        f.nested
            .as_ref()
            .and_then(|n| real(&n.path))
            .is_none_or(|here| !known.contains(&here))
    });
}

/// Returns the subset of `paths` that .gitignore excludes.
pub fn ignored(repo: &Path, paths: &[String]) -> Vec<String> {
    if paths.is_empty() {
        return vec![];
    }
    let input = paths.join("\0");
    // Exit code 1 just means "nothing ignored". These paths come from our own directory
    // listing, and literal pathspecs would break the trailing "/" that marks directories.
    let mut cmd = command(repo, &["check-ignore", "-z", "--stdin"]);
    cmd.env("GIT_LITERAL_PATHSPECS", "0");
    exec(cmd, "git check-ignore", &[1], Some(input.as_bytes()), None)
        .map(|raw| {
            raw.split(|b| *b == 0)
                .filter(|t| !t.is_empty())
                .map(|t| String::from_utf8_lossy(t).trim_end_matches('/').to_string())
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn line_counts_stop_at_the_budget() {
        // Counting reads the disk only, so any folder does.
        let repo =
            std::env::temp_dir().join(format!("gitviber-test-budget-{}", std::process::id()));
        let _ = fs::remove_dir_all(&repo);
        fs::create_dir_all(&repo).unwrap();
        for i in 0..3 {
            fs::write(repo.join(format!("f{i}.txt")), "a\nb\n").unwrap();
        }
        let mut budget = CountBudget {
            files: 2,
            bytes: u64::MAX,
        };
        let got: Vec<_> = (0..3)
            .map(|i| count_lines(&repo, &format!("f{i}.txt"), &mut budget))
            .collect();
        assert_eq!(got, [Some(Some(2)), Some(Some(2)), None]);
        // Counted once, they cost nothing on the next refresh.
        let mut none = CountBudget { files: 0, bytes: 0 };
        assert_eq!(count_lines(&repo, "f1.txt", &mut none), Some(Some(2)));
        let _ = fs::remove_dir_all(&repo);
    }
}
