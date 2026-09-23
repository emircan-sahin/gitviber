//! Thin wrapper over the `git` CLI. We shell out instead of linking libgit2 so the
//! user's config, hooks, credential helpers and signing all behave exactly like the
//! terminal — GitViber never keeps state of its own inside the repo.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

const MAX_TEXT_BYTES: usize = 8 * 1024 * 1024;

/// Apps launched from Finder get a bare PATH, which hides Homebrew git and the
/// credential helpers / ssh that live next to it.
pub(crate) fn search_path() -> &'static str {
    static PATH: OnceLock<String> = OnceLock::new();
    PATH.get_or_init(|| {
        let current = std::env::var("PATH").unwrap_or_default();
        let mut parts: Vec<&str> = vec!["/opt/homebrew/bin", "/usr/local/bin"];
        parts.extend(current.split(':').filter(|p| !p.is_empty()));
        parts.dedup();
        parts.join(":")
    })
}

fn command(repo: &Path, args: &[&str]) -> Command {
    let mut cmd = Command::new("git");
    cmd.current_dir(repo)
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
        stdin.write_all(data).map_err(|e| e.to_string())?;
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

/// Network commands can stall on a dead connection; don't let them spin forever.
const NETWORK_TIMEOUT: Duration = Duration::from_secs(300);

fn run_network(repo: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    exec(
        command(repo, args),
        &format!("git {}", args[0]),
        &[],
        None,
        Some(NETWORK_TIMEOUT),
    )
}

pub fn run(repo: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    run_with(repo, args, &[], None)
}

pub(crate) fn run_text(repo: &Path, args: &[&str]) -> Result<String, String> {
    run(repo, args).map(|b| String::from_utf8_lossy(&b).into_owned())
}

fn has_head(repo: &Path) -> bool {
    run(repo, &["rev-parse", "--verify", "-q", "HEAD"]).is_ok()
}

pub fn toplevel(path: &Path) -> Result<String, String> {
    run_text(path, &["rev-parse", "--show-toplevel"])
        .map(|s| s.trim().to_string())
        .map_err(|_| "This folder is not inside a git repository.".to_string())
}

fn validate_rev(rev: &str) -> Result<(), String> {
    if rev.len() >= 4 && rev.len() <= 64 && rev.chars().all(|c| c.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(format!("invalid commit id: {rev}"))
    }
}

fn validate_branch(repo: &Path, name: &str) -> Result<(), String> {
    // check-ref-format also rejects a leading '-', so the name can't be read as a flag.
    // "@" means HEAD wherever a revision is read, so a branch by that name is a trap.
    if name == "@" {
        return Err(format!("invalid branch name: {name}"));
    }
    run(repo, &["check-ref-format", "--branch", name])
        .map(|_| ())
        .map_err(|_| format!("invalid branch name: {name}"))
}

// ---------------------------------------------------------------- status

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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PushTarget {
    pub remote: String,
    /// The remote branch it lands on, e.g. origin/dev; None until it exists there.
    pub branch: Option<String>,
    /// Commits it doesn't have yet.
    pub ahead: u32,
}

/// `@{push}` for a branch. Under the default `push.default=simple`, git won't name it for a
/// triangular setup although the push itself works; `current` is what it does then.
fn push_target(repo: &Path, branch: &str) -> Option<PushTarget> {
    let mode = run_text(repo, &["config", "--get", "push.default"]).unwrap_or_default();
    let mut args = vec![];
    if matches!(mode.trim(), "" | "simple") {
        args.extend(["-c", "push.default=current"]);
    }
    let reference = format!("refs/heads/{branch}");
    args.extend([
        "for-each-ref",
        "--format=%(push:remotename)%1f%(push:short)%1f%(push:track,nobracket)",
        &reference,
    ]);
    let out = run_text(repo, &args).ok()?;
    let f: Vec<&str> = out.trim_end_matches('\n').split('\x1f').collect();
    let remote = f.first().filter(|r| !r.is_empty())?.to_string();
    let track = f.get(2).copied().unwrap_or_default();
    let exists = f.get(1).is_some_and(|b| !b.is_empty()) && track != "gone";
    let ahead = track
        .split(", ")
        .find_map(|p| p.strip_prefix("ahead "))
        .and_then(|n| n.parse().ok())
        .unwrap_or(0);
    Some(PushTarget {
        branch: exists.then(|| f[1].to_string()),
        remote,
        ahead,
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PullDraft {
    /// Commits HEAD has that `base` doesn't: what the PR would bring.
    pub commits: u32,
    /// The one commit's message, when there's exactly one: GitHub titles the PR with it.
    pub subject: Option<String>,
    pub body: Option<String>,
}

/// What a PR from HEAD into `base` (a remote-tracking branch) would carry, to fill its title
/// and description the way GitHub does.
pub fn pull_draft(repo: &Path, base: &str) -> Result<PullDraft, String> {
    let r = base
        .strip_prefix("refs/remotes/")
        .filter(|r| !r.starts_with('-') && r.contains('/'))
        .ok_or_else(|| format!("not a remote-tracking branch: {base}"))?;
    run(repo, &["rev-parse", "--verify", "-q", base])
        .map_err(|_| format!("unknown branch: {r}"))?;
    let range = format!("{base}..HEAD");
    let commits = run_text(repo, &["rev-list", "--count", &range, "--"])?
        .trim()
        .parse()
        .unwrap_or(0);
    let one =
        |fmt: &str| run_text(repo, &["log", "-1", fmt, "HEAD", "--"]).map(|s| s.trim().to_string());
    Ok(PullDraft {
        subject: (commits == 1).then(|| one("--format=%s")).transpose()?,
        body: (commits == 1).then(|| one("--format=%b")).transpose()?,
        commits,
    })
}

/// What counts as pushed for HEAD: the branch `git push` lands on (a fork pushes to origin
/// while pulling from upstream), else the upstream.
fn pushed_base(repo: &Path) -> String {
    run_text(repo, &["symbolic-ref", "--short", "-q", "HEAD"])
        .ok()
        .and_then(|b| push_target(repo, b.trim()))
        .and_then(|p| p.branch)
        .unwrap_or_else(|| "@{upstream}".into())
}

/// Makes `git push` go to `remote` for every branch, whatever each pulls from: a fork's
/// branches can then follow upstream and still be pushed to origin.
pub fn set_push_default(repo: &Path, remote: &str) -> Result<(), String> {
    if remote_url(repo, remote).is_none() {
        return Err(format!("no remote named {remote}"));
    }
    run(repo, &["config", "remote.pushDefault", remote]).map(|_| ())
}

fn change(path: &str, old_path: Option<&str>, status: char) -> FileChange {
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
fn parse_numstat(raw: &[u8]) -> HashMap<String, (Option<u32>, Option<u32>)> {
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

fn apply_numstat(list: &mut [FileChange], stats: &HashMap<String, (Option<u32>, Option<u32>)>) {
    for f in list {
        if let Some((a, d)) = stats.get(&f.path) {
            f.additions = *a;
            f.deletions = *d;
        }
    }
}

/// Line count of an untracked file. Status runs on every change on disk, so counts are
/// cached by size and mtime: a big untracked folder is read once, not on each refresh.
fn count_lines(repo: &Path, rel: &str) -> Option<u32> {
    type Key = (std::path::PathBuf, u64, Option<std::time::SystemTime>);
    static CACHE: OnceLock<std::sync::Mutex<HashMap<Key, Option<u32>>>> = OnceLock::new();
    let path = repo.join(rel);
    let meta = std::fs::metadata(&path).ok()?;
    let key = (path, meta.len(), meta.modified().ok());
    let cache = CACHE.get_or_init(Default::default);
    if let Some(n) = cache.lock().unwrap().get(&key) {
        return *n;
    }
    let bytes = read_regular(&key.0).ok()??;
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
    n
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
                } else {
                    f.additions = count_lines(repo, path);
                    f.deletions = Some(0);
                }
                st.unstaged.push(f);
            }
            _ => {}
        }
    }

    for f in &mut st.unstaged {
        if let Ok(meta) = std::fs::metadata(repo.join(&f.path)) {
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_nanos());
            f.oid = Some(format!("{}:{mtime}", meta.len()));
        }
    }
    if let Some(b) = &st.branch {
        st.push = push_target(repo, b);
    }
    st.remotes = remotes(repo);
    if st.upstream.is_none() && st.branch.is_some() && !st.remotes.is_empty() {
        st.publish = publish_remote(repo).ok();
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

/// With `--untracked-files=all` git lists a directory only when it is another repository's
/// root, which it won't descend into. The `.git` check confirms it.
fn is_nested_repo(repo: &Path, path: &str) -> bool {
    path.ends_with('/') && repo.join(path).join(".git").exists()
}

/// Root of the nested repository an untracked path is, or lies in. The second case is a
/// worktree whose `.git` link broke (e.g. the main repo moved): git then lists its files
/// one by one as if they were ours, and `git worktree repair` would reconnect it.
fn untracked_nested_root(repo: &Path, path: &str) -> Option<String> {
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
fn new_gitlink(fields: &[&str]) -> bool {
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

// ---------------------------------------------------------------- worktrees

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

/// Marks worktrees whose lock names a live pid. Kept out of `worktrees()`, which status
/// refreshes call constantly: this spawns a process per such lock.
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

#[cfg(unix)]
fn process_alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
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

/// Checks `branch` out in a new worktree beside the main one, at
/// `<parent>/<project>.worktrees/<branch>`, and returns its path. A branch only on a
/// remote gets a local tracking branch (git's own DWIM for `worktree add`).
pub fn add_worktree(repo: &Path, branch: &str) -> Result<String, String> {
    validate_branch(repo, branch)?;
    let main = main_worktree(repo).ok_or("this repository has no main worktree")?;
    let main = Path::new(&main);
    let (Some(parent), Some(name)) = (main.parent(), main.file_name()) else {
        return Err(format!("no folder beside {}", main.display()));
    };
    let path = parent
        .join(format!("{}.worktrees", name.to_string_lossy()))
        .join(branch.replace('/', "-"));
    if path.exists() {
        return Err(format!("{} already exists", path.display()));
    }
    let target = path.to_string_lossy().into_owned();
    run(repo, &["worktree", "add", &target, branch])?;
    Ok(target)
}

/// One of this repo's worktrees by path. Only paths `git worktree list` reports are
/// accepted, so the frontend can't point git at an arbitrary folder.
fn listed_worktree(repo: &Path, path: &str) -> Result<Worktree, String> {
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
    pub commits: u32,
    /// Committed on, then fully taken into the default branch. A branch that never moved
    /// is in it too, but has nothing to call merged.
    pub merged: bool,
}

/// Where one of this repo's worktrees stands: uncommitted files, and commits found nowhere else.
pub fn worktree_state(repo: &Path, path: &str) -> Result<WorktreeState, String> {
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
    let mut uncommitted = 0;
    let mut records = raw.split(|b| *b == 0);
    while let Some(rec) = records.next() {
        match rec.first() {
            Some(b'?') => {
                let p = String::from_utf8_lossy(&rec[2..]);
                let root = real(&dir.join(p.as_ref()));
                if !(is_nested_repo(dir, &p) && root.is_some_and(|r| others.contains(&r))) {
                    uncommitted += 1;
                }
            }
            Some(b'1' | b'u') => uncommitted += 1,
            // A rename's original path follows as its own record.
            Some(b'2') => {
                uncommitted += 1;
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
    Ok(WorktreeState {
        uncommitted,
        commits,
        merged: !bases.is_empty() && commits == 0 && w.branch.as_deref().is_some_and(moved),
    })
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
    run(repo, &args).map(|_| ())
}

// ---------------------------------------------------------------- history

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Commit {
    pub sha: String,
    pub short_sha: String,
    pub author_name: String,
    pub author_email: String,
    pub timestamp: i64,
    pub parents: Vec<String>,
    pub refs: Vec<String>,
    pub subject: String,
    pub body: String,
    /// Ahead of the upstream. False when there is no upstream or it is gone: unknown, not pushed.
    pub unpushed: bool,
    /// Reachable from a remote-tracking branch of origin, so it exists on the origin host.
    pub on_origin: bool,
    /// Logging another branch: this commit isn't in HEAD yet, so merging would bring it in.
    pub not_in_head: bool,
    /// A followed file's history: its path in this commit (it may have been renamed since).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
}

/// History search. Every part narrows the list: messages must contain every `grep` (any
/// case, as typed, not a regex), and a commit must match an `author`, add or remove `code`
/// (`-S`), and touch one of `paths`.
#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct LogFilter {
    pub grep: Vec<String>,
    pub author: Vec<String>,
    pub code: Option<String>,
    pub paths: Vec<String>,
    /// Follow a single file's `paths` entry through renames.
    pub follow: bool,
}

impl LogFilter {
    fn follows(&self) -> bool {
        self.follow && self.paths.len() == 1
    }

    /// Options for `git log`, before the revisions. Values are glued to their flags, so none
    /// can be read as an option of its own.
    fn args(&self) -> Vec<String> {
        let mut out: Vec<String> = self.grep.iter().map(|g| format!("--grep={g}")).collect();
        out.extend(self.author.iter().map(|a| format!("--author={a}")));
        if !out.is_empty() {
            out.extend(["--regexp-ignore-case".into(), "--fixed-strings".into()]);
        }
        if self.grep.len() > 1 {
            out.push("--all-match".into());
        }
        if let Some(code) = self.code.as_deref().filter(|c| !c.is_empty()) {
            out.push(format!("-S{code}"));
        }
        if self.follows() {
            out.push("--follow".into());
        }
        out
    }
}

/// `log_filtered` with no filter, as the tests read history.
#[cfg(test)]
pub fn log(repo: &Path, rev: Option<&str>, skip: u32, limit: u32) -> Result<Vec<Commit>, String> {
    log_filtered(repo, rev, skip, limit, &LogFilter::default())
}

/// HEAD's history, or `rev`'s (a remote-tracking branch such as a fork's upstream/main),
/// narrowed to the commits `filter` matches.
pub fn log_filtered(
    repo: &Path,
    rev: Option<&str>,
    skip: u32,
    limit: u32,
    filter: &LogFilter,
) -> Result<Vec<Commit>, String> {
    if !has_head(repo) {
        return Ok(vec![]);
    }
    let tip = match rev {
        Some(r) => {
            let r = r
                .strip_prefix("refs/remotes/")
                .filter(|r| !r.starts_with('-') && r.contains('/'))
                .ok_or_else(|| format!("not a remote-tracking branch: {r}"))?;
            run(repo, &["check-ref-format", "--branch", r])
                .map_err(|_| format!("not a remote-tracking branch: {r}"))?;
            format!("refs/remotes/{r}")
        }
        None => "HEAD".to_string(),
    };
    commits(
        repo,
        &tip,
        rev.is_none(),
        rev.is_some(),
        skip,
        limit,
        filter,
    )
}

/// The commit a SHA (or a prefix of one) names, wherever it is: a pasted SHA goes first in a search.
pub fn find_commit(repo: &Path, sha: &str) -> Result<Option<Commit>, String> {
    if validate_rev(sha).is_err() || sha.len() > 40 || !has_head(repo) {
        return Ok(None);
    }
    // Unknown, or a prefix of several: no commit, not an error.
    let Ok(full) = run_text(
        repo,
        &["rev-parse", "--verify", "-q", &format!("{sha}^{{commit}}")],
    ) else {
        return Ok(None);
    };
    let found = commits(repo, full.trim(), true, true, 0, 1, &LogFilter::default())?;
    Ok(found.into_iter().next())
}

/// `limit` commits of `tip`'s history after `skip`. `mark_unpushed` / `mark_not_in_head`: work
/// out those flags (HEAD's own history is all in HEAD; another branch's is never unpushed).
fn commits(
    repo: &Path,
    tip: &str,
    mark_unpushed: bool,
    mark_not_in_head: bool,
    skip: u32,
    limit: u32,
    filter: &LogFilter,
) -> Result<Vec<Commit>, String> {
    let lines = |s: String| -> std::collections::HashSet<String> {
        s.lines().map(str::to_string).collect()
    };
    let unpushed = if mark_unpushed {
        run_text(repo, &["rev-list", &format!("{}..HEAD", pushed_base(repo))])
            .map(lines)
            .unwrap_or_default()
    } else {
        Default::default()
    };
    let filter_args = filter.args();
    let mut paths: Vec<&str> = vec!["--"];
    paths.extend(filter.paths.iter().map(String::as_str));
    // Of `tip`'s matching commits, the ones not reachable from `not`. Log order is the same
    // with or without `--not`, so the first skip+limit of them cover this page. A search
    // goes through the same filter: its page can sit anywhere in the full history.
    let outside = |not: &str| -> Result<std::collections::HashSet<String>, String> {
        let n = format!("-n{}", skip + limit);
        let mut args = vec!["log", "--format=%H", &n];
        args.extend(filter_args.iter().map(String::as_str));
        args.extend([tip, "--not", not]);
        args.extend(&paths);
        run_text(repo, &args).map(lines)
    };
    // No origin refs at all means nothing is on origin; skip the walk, it would list the whole history.
    let has_origin = run_text(repo, &["for-each-ref", "--count=1", "refs/remotes/origin"])
        .is_ok_and(|s| !s.trim().is_empty());
    let off_origin = if has_origin {
        Some(outside("--remotes=origin")?)
    } else {
        None
    };
    let not_in_head = if mark_not_in_head {
        outside("HEAD")?
    } else {
        Default::default()
    };

    // git skips before `-S` and `--follow` drop commits, so --skip would pass over commits that
    // don't match too (-n counts only matches): skip those here instead.
    let skip_here = if filter.code.is_some() || filter.follows() {
        skip as usize
    } else {
        0
    };
    let skip_arg = format!("--skip={}", skip as usize - skip_here);
    let limit = format!("-n{}", limit as usize + skip_here);
    // Records start with \x1e so that `--name-only`'s file list (after the format) stays in its own.
    let mut args = vec![
        "log",
        &skip_arg,
        &limit,
        "--format=%x1e%H%x1f%h%x1f%an%x1f%ae%x1f%at%x1f%P%x1f%D%x1f%s%x1f%b%x1f",
    ];
    if filter.follows() {
        args.push("--name-only");
    }
    args.extend(filter_args.iter().map(String::as_str));
    args.push(tip);
    args.extend(&paths);
    let raw = run_text(repo, &args)?;
    Ok(raw
        .split('\x1e')
        .filter_map(|rec| {
            let f: Vec<&str> = rec.split('\x1f').collect();
            if f.len() < 10 {
                return None;
            }
            Some(Commit {
                sha: f[0].to_string(),
                short_sha: f[1].to_string(),
                author_name: f[2].to_string(),
                author_email: f[3].to_string(),
                timestamp: f[4].parse().unwrap_or(0),
                parents: f[5].split_whitespace().map(str::to_string).collect(),
                refs: f[6]
                    .split(", ")
                    .filter(|r| !r.is_empty())
                    .map(str::to_string)
                    .collect(),
                subject: f[7].to_string(),
                body: f[8].trim().to_string(),
                unpushed: unpushed.contains(f[0]),
                on_origin: off_origin.as_ref().is_some_and(|off| !off.contains(f[0])),
                not_in_head: not_in_head.contains(f[0]),
                file: filter
                    .follows()
                    .then(|| f[9].lines().find(|l| !l.is_empty()).map(str::to_string))
                    .flatten(),
            })
        })
        .skip(skip_here)
        .collect())
}

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BlameCommit {
    /// All zeros for lines not committed yet.
    pub sha: String,
    pub author_name: String,
    pub author_email: String,
    pub timestamp: i64,
    /// The whole message, trailers (Co-Authored-By…) included.
    pub message: String,
    /// The file's path in this commit (it may have been renamed since).
    pub path: String,
}

#[derive(Serialize, Default)]
pub struct Blame {
    pub commits: Vec<BlameCommit>,
    /// For each line of the working-tree file, its commit's index in `commits`.
    pub lines: Vec<u32>,
}

/// Which commit last changed each line of the working-tree file. It's git's own blame, so the
/// user's blame.ignoreRevsFile applies. A file that isn't in HEAD yet has no lines: all new.
pub fn blame(repo: &Path, path: &str) -> Result<Blame, String> {
    if !has_head(repo) {
        return Ok(Blame::default());
    }
    // Unquoted: the path a commit had goes back to the frontend to find the file in it.
    let args = [
        "-c",
        "core.quotePath=false",
        "blame",
        "--porcelain",
        "--",
        path,
    ];
    let out = match run_text(repo, &args) {
        Ok(out) => out,
        Err(_) if run(repo, &["cat-file", "-e", &format!("HEAD:{path}")]).is_err() => {
            return Ok(Blame::default())
        }
        Err(e) => return Err(e),
    };
    let mut blame = Blame::default();
    let mut index: HashMap<String, u32> = HashMap::new();
    // The commit and final line of the entry being read; the line's text (after a tab) ends it.
    let mut entry: Option<(u32, usize)> = None;
    for l in out.lines() {
        if l.starts_with('\t') {
            if let Some((i, line)) = entry.take() {
                if blame.lines.len() < line {
                    blame.lines.resize(line, u32::MAX);
                }
                blame.lines[line - 1] = i;
            }
            continue;
        }
        let mut words = l.split(' ');
        let first = words.next().unwrap_or_default();
        let final_line = words.nth(1).and_then(|n| n.parse::<usize>().ok());
        if let (40, Some(line)) = (first.len(), final_line.filter(|&n| n > 0)) {
            if first.bytes().all(|b| b.is_ascii_hexdigit()) {
                let i = *index.entry(first.to_string()).or_insert_with(|| {
                    blame.commits.push(BlameCommit {
                        sha: first.to_string(),
                        ..Default::default()
                    });
                    blame.commits.len() as u32 - 1
                });
                entry = Some((i, line));
                continue;
            }
        }
        // A commit's details, the first time it comes up.
        let Some((i, _)) = entry else { continue };
        let c = &mut blame.commits[i as usize];
        if let Some(v) = l.strip_prefix("author ") {
            c.author_name = v.to_string();
        } else if let Some(v) = l.strip_prefix("author-mail ") {
            c.author_email = v.trim_matches(['<', '>']).to_string();
        } else if let Some(v) = l.strip_prefix("author-time ") {
            c.timestamp = v.parse().unwrap_or(0);
        } else if let Some(v) = l.strip_prefix("summary ") {
            c.message = v.to_string();
        } else if let Some(v) = l.strip_prefix("filename ") {
            c.path = v.to_string();
        }
    }
    // Porcelain has only the subject; the hover shows the whole message.
    let shas: String = blame
        .commits
        .iter()
        .filter(|c| c.sha.bytes().any(|b| b != b'0'))
        .map(|c| format!("{}\n", c.sha))
        .collect();
    if !shas.is_empty() {
        let out = run_with(
            repo,
            &[
                "log",
                "--no-walk=unsorted",
                "--stdin",
                "--format=%H%x1f%B%x1e",
            ],
            &[],
            Some(shas.as_bytes()),
        )?;
        for rec in String::from_utf8_lossy(&out).split('\x1e') {
            if let Some((sha, message)) = rec.trim_start().split_once('\x1f') {
                if let Some(&i) = index.get(sha) {
                    blame.commits[i as usize].message = message.trim_end().to_string();
                }
            }
        }
    }
    Ok(blame)
}

/// Files changed by a commit, compared with its first parent (so merges show what they brought in).
pub fn commit_files(repo: &Path, sha: &str) -> Result<Vec<FileChange>, String> {
    validate_rev(sha)?;
    let parent = format!("{sha}^");
    let has_parent = run(repo, &["rev-parse", "--verify", "-q", &parent]).is_ok();
    if has_parent {
        range_files(repo, &parent, sha)
    } else {
        tree_files(repo, &["--root", sha])
    }
}

/// Files changed between two commits (e.g. a PR's merge base and its head).
pub fn range_files(repo: &Path, from: &str, to: &str) -> Result<Vec<FileChange>, String> {
    tree_files(repo, &[from, to])
}

fn tree_files(repo: &Path, range: &[&str]) -> Result<Vec<FileChange>, String> {
    let mut args = vec![
        "diff-tree",
        "-r",
        "-z",
        "-M",
        "--no-commit-id",
        "--name-status",
    ];
    args.extend(range);
    let raw = run(repo, &args)?;
    let mut files = vec![];
    let mut tokens = raw
        .split(|b| *b == 0)
        .map(|t| String::from_utf8_lossy(t).into_owned());
    while let Some(code) = tokens.next() {
        let Some(letter) = code.chars().next() else {
            continue;
        };
        if letter == 'R' || letter == 'C' {
            let (Some(old), Some(new)) = (tokens.next(), tokens.next()) else {
                break;
            };
            files.push(change(&new, Some(&old), letter));
        } else if let Some(path) = tokens.next() {
            files.push(change(&path, None, letter));
        }
    }

    let mut args = vec!["diff-tree", "-r", "-z", "-M", "--no-commit-id", "--numstat"];
    args.extend(range);
    apply_numstat(&mut files, &parse_numstat(&run(repo, &args)?));
    Ok(files)
}

pub fn merge_base(repo: &Path, a: &str, b: &str) -> Result<String, String> {
    validate_rev(a)?;
    validate_rev(b)?;
    run_text(repo, &["merge-base", a, b]).map(|s| s.trim().to_string())
}

/// Brings in objects for these refs from a remote. Writes no FETCH_HEAD and creates no
/// local branch (a configured remote-tracking ref like origin/<base> may still update).
pub fn fetch_objects(repo: &Path, remote: &str, refspecs: &[String]) -> Result<(), String> {
    let mut args = vec![
        "fetch",
        "--quiet",
        "--no-write-fetch-head",
        "--no-tags",
        remote,
    ];
    args.extend(refspecs.iter().map(String::as_str));
    run_network(repo, &args).map(|_| ())
}

pub fn remote_url(repo: &Path, remote: &str) -> Option<String> {
    run_text(repo, &["remote", "get-url", remote])
        .ok()
        .map(|s| s.trim().to_string())
}

/// The token git already stores for github.com (osxkeychain, GitHub Desktop, GCM…).
pub fn credential_token(repo: &Path) -> Option<String> {
    let mut cmd = command(repo, &["-c", "core.askPass=", "credential", "fill"]);
    // Never pop a login window from the background; no stored credential means "none".
    cmd.env("GCM_INTERACTIVE", "never")
        .env_remove("GIT_ASKPASS")
        .env_remove("SSH_ASKPASS");
    let out = exec(
        cmd,
        "git credential",
        &[],
        Some(b"protocol=https\nhost=github.com\n\n"),
        Some(Duration::from_secs(10)),
    )
    .ok()?;
    String::from_utf8_lossy(&out)
        .lines()
        .find_map(|l| l.strip_prefix("password=").map(str::to_string))
        .filter(|t| !t.is_empty())
}

// ---------------------------------------------------------------- file contents

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FileText {
    pub text: String,
    pub binary: bool,
    pub too_large: bool,
    /// False when the file does not exist on that side (added / deleted).
    pub exists: bool,
    /// Not valid UTF-8 (e.g. Latin-1); text was decoded lossily, so never write it back.
    pub lossy: bool,
}

pub fn is_binary(bytes: &[u8]) -> bool {
    bytes.iter().take(8000).any(|b| *b == 0)
}

/// Reads a regular file up to MAX_TEXT_BYTES. Ok(None) = too large. FIFOs, devices and
/// sockets are refused: reading /dev/zero or a pipe would hang or eat memory.
pub fn read_regular(path: &Path) -> Result<Option<Vec<u8>>, String> {
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if !meta.is_file() {
        return Err("not a regular file".into());
    }
    if meta.len() > MAX_TEXT_BYTES as u64 {
        return Ok(None);
    }
    let mut buf = Vec::new();
    std::fs::File::open(path)
        .and_then(|f| f.take(MAX_TEXT_BYTES as u64 + 1).read_to_end(&mut buf))
        .map_err(|e| e.to_string())?;
    Ok((buf.len() <= MAX_TEXT_BYTES).then_some(buf))
}

pub fn to_file_text(bytes: Vec<u8>) -> FileText {
    if bytes.len() > MAX_TEXT_BYTES {
        return FileText {
            too_large: true,
            exists: true,
            ..Default::default()
        };
    }
    if is_binary(&bytes) {
        return FileText {
            binary: true,
            exists: true,
            ..Default::default()
        };
    }
    match String::from_utf8(bytes) {
        Ok(text) => FileText {
            text,
            exists: true,
            ..Default::default()
        },
        Err(e) => FileText {
            text: String::from_utf8_lossy(e.as_bytes()).into_owned(),
            exists: true,
            lossy: true,
            ..Default::default()
        },
    }
}

/// Reads `<rev>:<path>` (rev "" means the index). A missing blob is not an error. The size is
/// checked first so a huge file in some old commit is never read into memory.
fn blob(repo: &Path, rev: &str, path: &str) -> FileText {
    let spec = format!("{rev}:{path}");
    let Ok(size) = run_text(repo, &["cat-file", "-s", &spec]) else {
        return FileText::default();
    };
    if size
        .trim()
        .parse::<u64>()
        .is_ok_and(|n| n > MAX_TEXT_BYTES as u64)
    {
        return FileText {
            too_large: true,
            exists: true,
            ..Default::default()
        };
    }
    match run(repo, &["cat-file", "blob", &spec]) {
        Ok(bytes) => to_file_text(bytes),
        Err(_) => FileText::default(),
    }
}

/// A submodule on one side of a diff. Git stores only its commit (mode 160000), which
/// `cat-file blob` can't read, so it's shown the way `git diff` does: "Subproject commit <sha>".
/// `rev` "" is the index, None the worktree.
fn gitlink(repo: &Path, rev: Option<&str>, path: &str) -> Option<FileText> {
    // "<mode> <oid> <stage>\t<path>"; also confirms `path` is a submodule of this repo
    // before the worktree side runs git inside it.
    let staged = || -> Option<String> {
        let out = run_text(repo, &["ls-files", "-s", "--", path]).ok()?;
        let mut f = out.split_whitespace();
        (f.next()? == "160000").then(|| f.next().map(str::to_string))?
    };
    let sha = match rev {
        Some("") => staged()?,
        Some(rev) => {
            // "<mode> <type> <oid>\t<path>"
            let out = run_text(repo, &["ls-tree", rev, "--", path]).ok()?;
            let mut f = out.split_whitespace();
            (f.next()? == "160000").then(|| f.nth(1).map(str::to_string))??
        }
        None => {
            staged()?;
            let dir = repo.join(path);
            // Not checked out (no `.git`): git would walk up and report this repo's HEAD.
            if !dir.join(".git").exists() {
                return None;
            }
            let head = run_text(&dir, &["rev-parse", "HEAD"]).ok()?;
            let dirty = run(&dir, &["status", "--porcelain", "--untracked-files=no"])
                .is_ok_and(|o| !o.is_empty());
            format!("{}{}", head.trim(), if dirty { "-dirty" } else { "" })
        }
    };
    Some(FileText {
        text: format!("Subproject commit {sha}\n"),
        exists: true,
        ..Default::default()
    })
}

#[derive(Serialize)]
pub struct DiffPair {
    pub original: FileText,
    pub modified: FileText,
    pub rows: Vec<crate::diff::Row>,
}

/// Where each side of a diff lives: a git revision ("" = the index), or None for the worktree.
fn sides(
    kind: &str,
    sha: Option<&str>,
    base: Option<&str>,
) -> Result<(Option<String>, Option<String>), String> {
    let rev = |r: &str| Some(r.to_string());
    Ok(match kind {
        "unstaged" => (rev(""), None),
        "staged" => (rev("HEAD"), rev("")),
        "worktree" => (rev("HEAD"), None),
        "commit" => {
            let sha = sha.ok_or("missing commit")?;
            validate_rev(sha)?;
            (Some(format!("{sha}^")), rev(sha))
        }
        "range" => {
            let (Some(base), Some(sha)) = (base, sha) else {
                return Err("missing range".into());
            };
            validate_rev(base)?;
            validate_rev(sha)?;
            (rev(base), rev(sha))
        }
        other => return Err(format!("unknown diff kind: {other}")),
    })
}

/// Media previews load whole files into the webview; past this they are refused.
pub const MAX_MEDIA_BYTES: u64 = 512 * 1024 * 1024;

/// Raw bytes of one side of a diff, for image / audio / video previews.
#[allow(clippy::too_many_arguments)]
pub fn media(
    repo: &Path,
    kind: &str,
    path: &str,
    old_path: Option<&str>,
    sha: Option<&str>,
    base: Option<&str>,
    original: bool,
    worktree: impl Fn(&str) -> Result<Vec<u8>, String>,
) -> Result<Vec<u8>, String> {
    let (a, b) = sides(kind, sha, base)?;
    let (rev, path) = if original {
        (a, old_path.unwrap_or(path))
    } else {
        (b, path)
    };
    let Some(rev) = rev else {
        return worktree(path);
    };
    let spec = format!("{rev}:{path}");
    let size: u64 = run_text(repo, &["cat-file", "-s", &spec])?
        .trim()
        .parse()
        .map_err(|_| "bad blob size")?;
    if size > MAX_MEDIA_BYTES {
        return Err("File is too large to preview".into());
    }
    run(repo, &["cat-file", "blob", &spec])
}

/// `kind`: "unstaged" (index → worktree), "staged" (HEAD → index), "worktree" (HEAD → worktree),
/// "commit" (parent → commit) or "range" (base → sha, e.g. a pull request).
pub fn diff_pair(
    repo: &Path,
    kind: &str,
    path: &str,
    old_path: Option<&str>,
    sha: Option<&str>,
    base: Option<&str>,
    worktree: impl Fn(&str) -> FileText,
) -> Result<DiffPair, String> {
    let (a, b) = sides(kind, sha, base)?;
    let read = |rev: Option<String>, p: &str| {
        let f = match &rev {
            Some(rev) => blob(repo, rev, p),
            None => worktree(p),
        };
        if f.exists {
            f
        } else {
            gitlink(repo, rev.as_deref(), p).unwrap_or(f)
        }
    };
    let original = read(a, old_path.unwrap_or(path));
    let modified = read(b, path);
    let textual = |f: &FileText| !f.binary && !f.too_large;
    let rows = if textual(&original) && textual(&modified) {
        crate::diff::rows(&original.text, &modified.text)
    } else {
        vec![]
    };
    Ok(DiffPair {
        original,
        modified,
        rows,
    })
}

// ---------------------------------------------------------------- merge / rebase

fn git_dir(repo: &Path) -> Option<std::path::PathBuf> {
    run_text(repo, &["rev-parse", "--absolute-git-dir"])
        .ok()
        .map(|s| s.trim().into())
}

fn read_trim(path: std::path::PathBuf) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
}

pub fn operation(repo: &Path) -> Option<Operation> {
    let dir = git_dir(repo)?;
    // `git am` also uses rebase-apply/, marked by an `applying` file.
    if dir.join("rebase-apply/applying").exists() {
        return Some(Operation {
            kind: "am".into(),
            subject: None,
            step: None,
            total: None,
        });
    }
    for rebase in ["rebase-merge", "rebase-apply"] {
        let d = dir.join(rebase);
        if d.is_dir() {
            let (step, total) = if rebase == "rebase-merge" {
                ("msgnum", "end")
            } else {
                ("next", "last")
            };
            return Some(Operation {
                kind: "rebase".into(),
                subject: read_trim(d.join("head-name"))
                    .map(|h| h.trim_start_matches("refs/heads/").to_string()),
                step: read_trim(d.join(step)).and_then(|v| v.parse().ok()),
                total: read_trim(d.join(total)).and_then(|v| v.parse().ok()),
            });
        }
    }
    let simple = |kind: &str, file: &str| {
        dir.join(file).exists().then(|| Operation {
            kind: kind.into(),
            subject: read_trim(dir.join("MERGE_MSG"))
                .and_then(|m| m.lines().next().map(str::to_string)),
            step: None,
            total: None,
        })
    };
    simple("merge", "MERGE_HEAD")
        .or_else(|| simple("cherry-pick", "CHERRY_PICK_HEAD"))
        .or_else(|| simple("revert", "REVERT_HEAD"))
        .or_else(|| {
            // A multi-commit cherry-pick/revert paused between picks leaves only sequencer/.
            let todo = read_trim(dir.join("sequencer/todo"))?;
            let kind = if todo.starts_with("revert") {
                "revert"
            } else {
                "cherry-pick"
            };
            Some(Operation {
                kind: kind.into(),
                subject: None,
                step: None,
                total: None,
            })
        })
}

/// Accepts a local or remote-tracking branch (or any commit-ish) that isn't an option.
fn validate_ref(repo: &Path, name: &str) -> Result<(), String> {
    let spec = format!("{name}^{{commit}}");
    if name.starts_with('-') || run(repo, &["rev-parse", "--verify", "-q", &spec]).is_err() {
        return Err(format!("unknown branch or commit: {name}"));
    }
    Ok(())
}

/// Runs an operation that may stop on conflicts. Stopping is not an error: it returns
/// Ok(true) so the UI can switch to resolving. Anything else that fails is an error.
fn has_conflicts(repo: &Path) -> bool {
    run(repo, &["diff", "--name-only", "--diff-filter=U"]).is_ok_and(|o| !o.is_empty())
}

fn run_stoppable(repo: &Path, args: &[&str]) -> Result<bool, String> {
    stoppable(repo, run(repo, args))
}

/// Ok(true) only when the operation stopped on conflicts. Any other failure (a hook, GPG,
/// dirty worktree) is returned as the real git error instead of looking like conflicts.
fn stoppable(repo: &Path, result: Result<Vec<u8>, String>) -> Result<bool, String> {
    let conflicts = has_conflicts(repo);
    match result {
        Ok(_) => Ok(conflicts),
        Err(_) if conflicts => Ok(true),
        Err(e) => Err(e),
    }
}

/// Starting a new merge/rebase/pull on top of an unfinished one would be misreported as conflicts.
fn ensure_idle(repo: &Path) -> Result<(), String> {
    match operation(repo) {
        Some(op) => Err(format!(
            "A {} is in progress. Continue or abort it first.",
            op.kind
        )),
        None => Ok(()),
    }
}

pub fn merge(repo: &Path, name: &str) -> Result<bool, String> {
    ensure_idle(repo)?;
    validate_ref(repo, name)?;
    run_stoppable(repo, &["merge", "--no-edit", name])
}

pub fn rebase(repo: &Path, onto: &str) -> Result<bool, String> {
    ensure_idle(repo)?;
    validate_ref(repo, onto)?;
    run_stoppable(repo, &["rebase", onto])
}

pub fn op_continue(repo: &Path) -> Result<bool, String> {
    let op = operation(repo).ok_or("Nothing to continue.")?;
    match op.kind.as_str() {
        // `merge --continue` refuses without an editor on some git versions; commit is equivalent.
        "merge" => run_stoppable(repo, &["commit", "--no-edit"]),
        "rebase" => run_stoppable(repo, &["rebase", "--continue"]),
        "cherry-pick" => run_stoppable(repo, &["cherry-pick", "--continue"]),
        "am" => run_stoppable(repo, &["am", "--continue"]),
        _ => run_stoppable(repo, &["revert", "--continue"]),
    }
}

pub fn op_abort(repo: &Path) -> Result<(), String> {
    let op = operation(repo).ok_or("Nothing to abort.")?;
    let kind = op.kind.as_str();
    run(repo, &[kind, "--abort"]).map(|_| ())
}

pub fn rebase_skip(repo: &Path) -> Result<bool, String> {
    run_stoppable(repo, &["rebase", "--skip"])
}

/// Resolves a conflicted file by taking one side whole. `side` is "ours" or "theirs";
/// if that side deleted the file, the resolution is to delete it.
pub fn resolve_side(repo: &Path, path: &str, side: &str) -> Result<(), String> {
    let (flag, stage_no) = match side {
        "ours" => ("--ours", "2"),
        "theirs" => ("--theirs", "3"),
        other => return Err(format!("unknown side: {other}")),
    };
    let paths = [path.to_string()];
    // Unmerged index stages: 1 base, 2 ours, 3 theirs. Decide from them, never from a
    // failed checkout, so an unrelated error can't turn into deleting the file.
    let raw = run_text(repo, &with_paths(vec!["ls-files", "-u", "-z"], &paths))?;
    let stages: Vec<&str> = raw
        .split('\0')
        .filter_map(|l| l.split('\t').next()?.split(' ').nth(2))
        .collect();
    if stages.is_empty() {
        return Err(format!("{path} is not in conflict"));
    }
    if stages.contains(&stage_no) {
        run(repo, &with_paths(vec!["checkout", flag], &paths))?;
        stage(repo, &paths)
    } else {
        run(repo, &with_paths(vec!["rm", "-q"], &paths)).map(|_| ())
    }
}

// ---------------------------------------------------------------- history actions

/// `seen` is the HEAD the user saw: an agent may have committed since, and moving HEAD
/// based on the old history would silently drop that commit.
fn ensure_head(repo: &Path, seen: &str) -> Result<(), String> {
    validate_rev(seen)?;
    let head = run_text(repo, &["rev-parse", "HEAD"])?;
    if head.trim() != seen {
        return Err("HEAD has moved since the history was loaded. Refresh and try again.".into());
    }
    Ok(())
}

/// Undoes the last commit (`sha`, the HEAD the user saw), keeping its changes staged.
pub fn undo_commit(repo: &Path, sha: &str) -> Result<(), String> {
    ensure_idle(repo)?;
    ensure_head(repo, sha)?;
    run(repo, &["reset", "--soft", "HEAD~1"]).map(|_| ())
}

/// Whether moving HEAD to `sha` takes commits off the branch that its push target (or
/// upstream) already has, i.e. would need a force-push. Decided by ancestry, not log order,
/// so merges count right. No upstream, or one that is gone, counts as not pushed.
pub fn drops_pushed(repo: &Path, sha: &str) -> Result<bool, String> {
    validate_rev(sha)?;
    drops_pushed_from(repo, "HEAD", &[sha])
}

/// Whether the push target has commits reachable from `from` but from none of `keep`.
pub(crate) fn drops_pushed_from(repo: &Path, from: &str, keep: &[&str]) -> Result<bool, String> {
    // Commits both sides have: everything reachable from their merge bases.
    let Ok(bases) = run_text(repo, &["merge-base", "--all", from, &pushed_base(repo)]) else {
        return Ok(false);
    };
    let mut args = vec!["rev-list", "-n1"];
    args.extend(bases.split_whitespace());
    args.push("--not");
    args.extend(keep);
    Ok(!run_text(repo, &args)?.trim().is_empty())
}

/// The commit HEAD's push target (or upstream) is at, if it has one.
pub(crate) fn pushed_tip(repo: &Path) -> Option<String> {
    let spec = format!("{}^{{commit}}", pushed_base(repo));
    run_text(repo, &["rev-parse", "--verify", "-q", &spec])
        .ok()
        .map(|s| s.trim().to_string())
}

/// Moves the current branch (or detached HEAD) from `head` (as the user saw it) to `sha`.
/// `mode` is "soft", "mixed" or "hard".
pub fn reset(repo: &Path, sha: &str, mode: &str, head: &str) -> Result<(), String> {
    validate_rev(sha)?;
    ensure_idle(repo)?;
    ensure_head(repo, head)?;
    let flag = match mode {
        "soft" => "--soft",
        "mixed" => "--mixed",
        "hard" => "--hard",
        other => return Err(format!("unknown reset mode: {other}")),
    };
    run(repo, &["reset", "-q", flag, sha]).map(|_| ())
}

/// `git revert`, returning true if it stopped on conflicts.
pub fn revert(repo: &Path, sha: &str) -> Result<bool, String> {
    validate_rev(sha)?;
    ensure_idle(repo)?;
    // A merge commit needs a mainline; relative to its first parent is what "this commit" means.
    let merge = run(repo, &["rev-parse", "--verify", "-q", &format!("{sha}^2")]).is_ok();
    let mut args = vec!["revert", "--no-edit"];
    if merge {
        args.extend(["-m", "1"]);
    }
    args.push(sha);
    let result = stoppable(repo, run(repo, &args));
    // An empty revert (already undone) fails with no conflicts, no operation left and nothing
    // changed, explaining why only on stdout. Other failures leave one of those behind.
    if result.is_err()
        && operation(repo).is_none()
        && run(repo, &["diff", "--quiet", "HEAD"]).is_ok()
    {
        return Err("This commit's changes are already undone; nothing to revert.".into());
    }
    result
}

/// Detached checkout of a commit. Git refuses if local changes would be overwritten.
pub fn checkout_commit(repo: &Path, sha: &str) -> Result<(), String> {
    validate_rev(sha)?;
    run(repo, &["switch", "--detach", sha]).map(|_| ())
}

pub fn create_branch_at(repo: &Path, name: &str, sha: &str) -> Result<(), String> {
    validate_rev(sha)?;
    validate_branch(repo, name)?;
    run(repo, &["switch", "-c", name, sha]).map(|_| ())
}

pub fn create_tag(repo: &Path, name: &str, sha: &str) -> Result<(), String> {
    validate_rev(sha)?;
    let full = format!("refs/tags/{name}");
    // "@" alone is valid in a full ref but means HEAD wherever a revision is read.
    if name == "@" || name.starts_with('-') || run(repo, &["check-ref-format", &full]).is_err() {
        return Err(format!("invalid tag name: {name}"));
    }
    run(repo, &["tag", name, sha]).map(|_| ())
}

// ---------------------------------------------------------------- branches

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Branch {
    pub name: String,
    /// Remote-tracking branch like origin/main (can be merged/rebased onto, or checked out).
    pub remote: bool,
    pub current: bool,
    pub upstream: Option<String>,
    pub timestamp: i64,
    /// Checked out in another worktree (its path), where git refuses to switch to it.
    pub worktree: Option<String>,
    /// Local, not HEAD, and fully contained in HEAD: deleting it loses no commits. The
    /// default branch never counts, so "clean up merged" can't take main from under a feature.
    pub merged: bool,
    /// What its remote's HEAD points at (origin/main): never offered for deletion.
    pub remote_default: bool,
}

pub fn branches(repo: &Path) -> Result<Vec<Branch>, String> {
    let raw = run_text(
        repo,
        &[
            "for-each-ref",
            "--sort=-committerdate",
            "--format=%(refname)%1f%(refname:short)%1f%(HEAD)%1f%(upstream:short)%1f%(committerdate:unix)%1f%(worktreepath)%1f%(symref)",
            "refs/heads",
            "refs/remotes",
        ],
    )?;
    // A bare main repo "holds" its HEAD branch too, but has no working tree to open.
    let bare: Vec<String> = worktrees(repo)
        .unwrap_or_default()
        .into_iter()
        .filter(|w| w.bare)
        .map(|w| w.path)
        .collect();
    // Empty on an unborn HEAD, where nothing is merged yet.
    let merged = run_text(
        repo,
        &[
            "for-each-ref",
            "--merged=HEAD",
            "--format=%(refname)",
            "refs/heads",
        ],
    )
    .unwrap_or_default();
    let merged: Vec<&str> = merged.lines().collect();
    let default = default_branch(repo);
    // origin/HEAD's own row names the remote default; the row itself is skipped below.
    let remote_heads: Vec<&str> = raw
        .lines()
        .filter_map(|l| l.split('\x1f').nth(6).filter(|s| !s.is_empty()))
        .collect();
    Ok(raw
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\x1f').collect();
            let elsewhere =
                f.len() == 7 && f[2] != "*" && !f[5].is_empty() && !bare.iter().any(|b| b == f[5]);
            (f.len() == 7 && !f[0].ends_with("/HEAD")).then(|| Branch {
                name: f[1].to_string(),
                remote: f[0].starts_with("refs/remotes/"),
                current: f[2] == "*",
                upstream: (!f[3].is_empty()).then(|| f[3].to_string()),
                timestamp: f[4].parse().unwrap_or(0),
                worktree: elsewhere.then(|| f[5].to_string()),
                merged: f[2] != "*" && f[1] != default && merged.contains(&f[0]),
                remote_default: remote_heads.contains(&f[0]),
            })
        })
        .collect())
}

/// What origin/HEAD points at, else "main".
fn default_branch(repo: &Path) -> String {
    run_text(
        repo,
        &[
            "symbolic-ref",
            "--quiet",
            "--short",
            "refs/remotes/origin/HEAD",
        ],
    )
    .ok()
    .and_then(|r| r.trim().strip_prefix("origin/").map(str::to_string))
    .unwrap_or_else(|| "main".into())
}

/// `git branch -d`, or `-D` when `force`: -d refuses a branch with commits found nowhere else.
pub fn delete_branches(repo: &Path, names: &[String], force: bool) -> Result<(), String> {
    for n in names {
        validate_branch(repo, n)?;
    }
    let mut args = vec!["branch", if force { "-D" } else { "-d" }];
    args.extend(names.iter().map(String::as_str));
    run(repo, &args).map(|_| ())
}

/// Deletes "origin/feat" on origin. Refuses the remote's default branch: hosts either reject
/// it or let it go and leave every clone without one.
pub fn delete_remote_branch(repo: &Path, name: &str) -> Result<(), String> {
    let remotes = run_text(repo, &["remote"])?;
    let remote = remotes
        .lines()
        .filter(|r| name.starts_with(&format!("{r}/")))
        .max_by_key(|r| r.len())
        .ok_or_else(|| format!("{name} isn't a remote branch"))?;
    let branch = &name[remote.len() + 1..];
    validate_branch(repo, branch)?;
    let head = format!("refs/remotes/{remote}/HEAD");
    if run_text(repo, &["symbolic-ref", "--quiet", "--short", &head])
        .is_ok_and(|h| h.trim() == name)
    {
        return Err(format!("{name} is {remote}'s default branch"));
    }
    let target = format!("refs/heads/{branch}");
    run_network(repo, &["push", remote, "--delete", &target]).map(|_| ())
}

pub fn switch_branch(repo: &Path, name: &str, create: bool) -> Result<(), String> {
    validate_branch(repo, name)?;
    let args: Vec<&str> = if create {
        vec!["switch", "-c", name]
    } else {
        vec!["switch", name]
    };
    run(repo, &args).map(|_| ())
}

/// Switches to the local branch for a remote-tracking one ("upstream/dev" → dev), creating it
/// to track exactly that ref. Not `git switch dev`: with origin/dev and upstream/dev both
/// there, git's guess refuses. An existing local branch is switched to as it is; the UI asks
/// first when it tracks something else.
pub fn switch_tracking(repo: &Path, remote_ref: &str) -> Result<(), String> {
    let bad = || format!("not a remote branch: {remote_ref}");
    if remote_ref.starts_with('-') {
        return Err(bad());
    }
    let (_, local) = remote_ref.split_once('/').ok_or_else(bad)?;
    run(
        repo,
        &[
            "rev-parse",
            "--verify",
            "-q",
            &format!("refs/remotes/{remote_ref}"),
        ],
    )
    .map_err(|_| bad())?;
    validate_branch(repo, local)?;
    if run(
        repo,
        &[
            "rev-parse",
            "--verify",
            "-q",
            &format!("refs/heads/{local}"),
        ],
    )
    .is_ok()
    {
        return run(repo, &["switch", local]).map(|_| ());
    }
    run(repo, &["switch", "-c", local, "--track", remote_ref]).map(|_| ())
}

// ---------------------------------------------------------------- mutations

fn with_paths<'a>(mut args: Vec<&'a str>, paths: &'a [String]) -> Vec<&'a str> {
    args.push("--");
    args.extend(paths.iter().map(String::as_str));
    args
}

pub fn stage(repo: &Path, paths: &[String]) -> Result<(), String> {
    stage_with(repo, paths, false)
}

/// Refuses untracked nested repositories (an agent's worktree) unless `allow_nested`: git
/// would stage one as a gitlink, a pointer to its current commit, and none of its files.
pub fn stage_with(repo: &Path, paths: &[String], allow_nested: bool) -> Result<(), String> {
    // `git add -A --` with no paths stages the whole tree, nested repos included.
    if paths.is_empty() {
        return Ok(());
    }
    if !allow_nested {
        if let Some(p) = nested_repos(repo, paths)?.first() {
            return Err(format!(
                "{p} is a separate git repository (a worktree or nested repo), so it was not staged. \
                 git would record only a pointer to its current commit, not its files. \
                 Commit inside it instead."
            ));
        }
    }
    run(repo, &with_paths(vec!["add", "-A"], paths)).map(|_| ())
}

/// Nested repositories at or under `paths` that `git add` would turn into new gitlinks (or
/// whose files it would take as ours). Plain files outside any nested repo skip the status call.
fn nested_repos(repo: &Path, paths: &[String]) -> Result<Vec<String>, String> {
    if !paths
        .iter()
        .any(|p| repo.join(p).is_dir() || untracked_nested_root(repo, p).is_some())
    {
        return Ok(vec![]);
    }
    let args = with_paths(
        vec!["status", "--porcelain=v2", "-z", "--untracked-files=all"],
        paths,
    );
    let raw = run_text(repo, &args)?;
    let mut found = vec![];
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
    Ok(found)
}

pub fn unstage(repo: &Path, paths: &[String]) -> Result<(), String> {
    let base = if has_head(repo) {
        vec!["restore", "--staged"]
    } else {
        vec!["rm", "--cached", "-q", "-r"]
    };
    run(repo, &with_paths(base, paths)).map(|_| ())
}

/// Reverts tracked files in the worktree to their index version. Untracked files are left alone.
pub fn discard(repo: &Path, paths: &[String]) -> Result<(), String> {
    run(repo, &with_paths(vec!["restore", "--worktree"], paths)).map(|_| ())
}

pub fn commit(repo: &Path, message: &str, amend: bool) -> Result<(), String> {
    if amend && message.trim().is_empty() {
        // Amending with no new message keeps the old one.
        return run(repo, &["commit", "--amend", "--no-edit"]).map(|_| ());
    }
    // Message goes through stdin so it is never parsed as arguments.
    let mut args = vec!["commit", "-F", "-"];
    if amend {
        args.push("--amend");
    }
    run_with(repo, &args, &[], Some(message.as_bytes())).map(|_| ())
}

/// `force`: after a rebase or amend the remote has the branch's old commits; replace them,
/// but only if it still has what was last fetched (`--force-with-lease`), so a push made
/// meanwhile by someone else is refused rather than lost.
/// A branch without an upstream is published (`-u`) to `remote`, or else to `publish_remote`.
pub fn push(repo: &Path, force: bool, remote: Option<&str>) -> Result<(), String> {
    let has_upstream = run(repo, &["rev-parse", "--abbrev-ref", "@{upstream}"]).is_ok();
    let mut args = vec!["push"];
    if force {
        args.push("--force-with-lease");
    }
    let target;
    if !has_upstream {
        target = match remote {
            Some(r) if remotes(repo).iter().any(|x| x == r) => r.to_string(),
            Some(r) => return Err(format!("no remote named {r}")),
            None => publish_remote(repo)?,
        };
        args.extend(["-u", &target, "HEAD"]);
    }
    run_network(repo, &args).map(|_| ())
}

pub fn remotes(repo: &Path) -> Vec<String> {
    run_text(repo, &["remote"])
        .map(|s| s.lines().map(str::to_string).collect())
        .unwrap_or_default()
}

/// Where a branch with no upstream is first pushed: its `pushRemote`, `remote.pushDefault`,
/// the only remote, or origin among several. Anything else is the user's call.
pub fn publish_remote(repo: &Path) -> Result<String, String> {
    let all = remotes(repo);
    let exists = |r: &String| all.contains(r);
    let config = |key: &str| {
        run_text(repo, &["config", "--get", key])
            .ok()
            .map(|s| s.trim().to_string())
    };
    let branch = run_text(repo, &["symbolic-ref", "--short", "-q", "HEAD"])
        .ok()
        .map(|b| b.trim().to_string());
    let configured = branch
        .and_then(|b| config(&format!("branch.{b}.pushRemote")))
        .filter(exists)
        .or_else(|| config("remote.pushDefault").filter(exists));
    if let Some(r) = configured {
        return Ok(r);
    }
    match all.as_slice() {
        [] => Err("This repository has no remote to publish to. Add one first, e.g. `git remote add origin <url>`.".into()),
        [only] => Ok(only.clone()),
        _ if all.iter().any(|r| r == "origin") => Ok("origin".into()),
        _ => Err(format!(
            "This repository has several remotes ({}). Choose one to publish to.",
            all.join(", ")
        )),
    }
}

/// `mode`: "ff" (fast-forward only), "merge" or "rebase". Returns true if it stopped on conflicts.
pub fn pull(repo: &Path, mode: &str) -> Result<bool, String> {
    ensure_idle(repo)?;
    let flag = match mode {
        "merge" => "--no-rebase",
        "rebase" => "--rebase",
        _ => "--ff-only",
    };
    stoppable(repo, run_network(repo, &["pull", "--no-edit", flag]))
}

/// Every remote: a plain fetch takes only the current branch's (on a fork's dev tracking
/// upstream/dev, upstream alone), leaving origin's branches stale.
pub fn fetch(repo: &Path) -> Result<(), String> {
    run_network(repo, &["fetch", "--all", "--prune"]).map(|_| ())
}

/// Fetches one configured remote, e.g. a fork's upstream.
pub fn fetch_remote(repo: &Path, name: &str) -> Result<(), String> {
    if remote_url(repo, name).is_none() {
        return Err(format!("no remote named {name}"));
    }
    run_network(repo, &["fetch", "--prune", name]).map(|_| ())
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
    use std::path::PathBuf;

    fn temp_repo(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("gitviber-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        for args in [
            vec!["init", "-q", "-b", "main"],
            vec!["config", "user.name", "Test"],
            vec!["config", "user.email", "test@example.com"],
            vec!["config", "commit.gpgsign", "false"],
        ] {
            run(&dir, &args).unwrap();
        }
        dir
    }

    #[test]
    fn status_history_and_diffs() {
        let repo = temp_repo("status");
        fs::write(repo.join("a.txt"), "one\ntwo\n").unwrap();
        fs::write(repo.join("old name.txt"), "rename me\n").unwrap();
        stage(&repo, &["a.txt".into(), "old name.txt".into()]).unwrap();
        commit(&repo, "first\n\nbody line", false).unwrap();

        fs::write(repo.join("a.txt"), "one\nTWO\nthree\n").unwrap();
        fs::rename(repo.join("old name.txt"), repo.join("new name.txt")).unwrap();
        stage(&repo, &["old name.txt".into(), "new name.txt".into()]).unwrap();
        fs::create_dir_all(repo.join("dir")).unwrap();
        fs::write(repo.join("dir/new.rs"), "fn main() {}\n").unwrap();

        let st = status(&repo).unwrap();
        assert_eq!(st.branch.as_deref(), Some("main"));
        let renamed = st
            .staged
            .iter()
            .find(|f| f.status == "R")
            .expect("rename staged");
        assert_eq!(renamed.path, "new name.txt");
        assert_eq!(renamed.old_path.as_deref(), Some("old name.txt"));
        let a = st.unstaged.iter().find(|f| f.path == "a.txt").unwrap();
        assert_eq!(
            (a.status.as_str(), a.additions, a.deletions),
            ("M", Some(2), Some(1))
        );
        let untracked = st.unstaged.iter().find(|f| f.path == "dir/new.rs").unwrap();
        assert_eq!(
            (untracked.status.as_str(), untracked.additions),
            ("?", Some(1))
        );

        let pair = diff_pair(&repo, "unstaged", "a.txt", None, None, None, |p| {
            to_file_text(fs::read(repo.join(p)).unwrap())
        })
        .unwrap();
        assert_eq!(pair.original.text, "one\ntwo\n");
        assert_eq!(pair.modified.text, "one\nTWO\nthree\n");
        let pair = diff_pair(
            &repo,
            "staged",
            "new name.txt",
            Some("old name.txt"),
            None,
            None,
            |_| FileText::default(),
        )
        .unwrap();
        assert_eq!(
            (pair.original.text.as_str(), pair.modified.text.as_str()),
            ("rename me\n", "rename me\n")
        );

        commit(&repo, "second", false).unwrap();
        let commits = log(&repo, None, 0, 10).unwrap();
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[1].body, "body line");
        assert!(commits[0].refs.iter().any(|r| r == "HEAD -> main"));

        let files = commit_files(&repo, &commits[0].sha).unwrap();
        assert!(files
            .iter()
            .any(|f| f.status == "R" && f.old_path.as_deref() == Some("old name.txt")));
        let root_files = commit_files(&repo, &commits[1].sha).unwrap();
        assert_eq!(root_files.len(), 2);

        assert_eq!(ignored(&repo, &["a.txt".into()]), Vec::<String>::new());
        fs::write(repo.join(".gitignore"), "target/\n").unwrap();
        assert_eq!(
            ignored(&repo, &["target/".into(), "a.txt".into()]),
            vec!["target".to_string()]
        );

        assert!(switch_branch(&repo, "--evil", true).is_err());
        switch_branch(&repo, "feat/x", true).unwrap();
        assert!(branches(&repo)
            .unwrap()
            .iter()
            .any(|b| b.name == "feat/x" && b.current));

        switch_branch(&repo, "main", false).unwrap();
        assert!(add_worktree(&repo, "--evil").is_err());
        let wt = add_worktree(&repo, "feat/x").unwrap();
        // git reports real paths: /var/folders is /private/var/folders on macOS.
        let real = repo.canonicalize().unwrap();
        let expected = real.with_file_name(format!(
            "{}.worktrees",
            real.file_name().unwrap().to_string_lossy()
        ));
        assert_eq!(Path::new(&wt), expected.join("feat-x"));
        assert!(worktrees(&repo)
            .unwrap()
            .iter()
            .any(|w| w.branch.as_deref() == Some("feat/x")));
        assert!(add_worktree(&repo, "feat/x").is_err());
        let _ = fs::remove_dir_all(&repo);
        let _ = fs::remove_dir_all(&expected);
    }

    fn commit_file(repo: &Path, path: &str, content: &str, msg: &str) {
        fs::write(repo.join(path), content).unwrap();
        stage(repo, &[path.into()]).unwrap();
        commit(repo, msg, false).unwrap();
    }

    #[test]
    fn merge_conflict_resolve_and_continue() {
        let repo = temp_repo("merge");
        commit_file(&repo, "a.txt", "base\n", "base");
        commit_file(&repo, "gone.txt", "keep?\n", "add gone");
        switch_branch(&repo, "feature", true).unwrap();
        commit_file(&repo, "a.txt", "feature\n", "feature edit");
        run(&repo, &["rm", "-q", "gone.txt"]).unwrap();
        commit(&repo, "feature deletes gone", false).unwrap();
        switch_branch(&repo, "main", false).unwrap();
        commit_file(&repo, "a.txt", "main\n", "main edit");
        commit_file(&repo, "gone.txt", "edited on main\n", "main edits gone");

        assert!(
            merge(&repo, "feature").unwrap(),
            "merge should stop on conflicts"
        );
        let st = status(&repo).unwrap();
        assert_eq!(
            st.operation.as_ref().map(|o| o.kind.as_str()),
            Some("merge")
        );
        let code = |p: &str| {
            st.conflicted
                .iter()
                .find(|f| f.path == p)
                .and_then(|f| f.conflict.clone())
        };
        assert_eq!(code("a.txt").as_deref(), Some("UU"));
        assert_eq!(code("gone.txt").as_deref(), Some("UD"));
        assert!(fs::read_to_string(repo.join("a.txt"))
            .unwrap()
            .contains("<<<<<<<"));

        // Starting another operation now must be refused, not reported as conflicts.
        assert!(rebase(&repo, "feature").is_err());

        resolve_side(&repo, "a.txt", "theirs").unwrap();
        resolve_side(&repo, "gone.txt", "theirs").unwrap(); // theirs deleted it
        assert_eq!(fs::read_to_string(repo.join("a.txt")).unwrap(), "feature\n");
        assert!(!repo.join("gone.txt").exists());
        assert!(!op_continue(&repo).unwrap());
        assert!(operation(&repo).is_none());
        assert_eq!(log(&repo, None, 0, 1).unwrap()[0].parents.len(), 2);
        let _ = fs::remove_dir_all(&repo);
    }

    #[test]
    fn rebase_conflict_abort() {
        let repo = temp_repo("rebase");
        commit_file(&repo, "a.txt", "base\n", "base");
        switch_branch(&repo, "feature", true).unwrap();
        commit_file(&repo, "a.txt", "feature\n", "feature edit");
        switch_branch(&repo, "main", false).unwrap();
        commit_file(&repo, "a.txt", "main\n", "main edit");
        switch_branch(&repo, "feature", false).unwrap();

        assert!(rebase(&repo, "main").unwrap());
        let op = operation(&repo).unwrap();
        assert_eq!(
            (op.kind.as_str(), op.subject.as_deref(), op.step, op.total),
            ("rebase", Some("feature"), Some(1), Some(1))
        );
        op_abort(&repo).unwrap();
        assert!(operation(&repo).is_none());
        assert_eq!(fs::read_to_string(repo.join("a.txt")).unwrap(), "feature\n");
        assert!(validate_ref(&repo, "--help").is_err());
        let _ = fs::remove_dir_all(&repo);
    }

    #[test]
    fn unborn_branch() {
        let repo = temp_repo("unborn");
        fs::write(repo.join("x"), "x\n").unwrap();
        stage(&repo, &["x".into()]).unwrap();
        assert_eq!(status(&repo).unwrap().staged.len(), 1);
        assert!(log(&repo, None, 0, 10).unwrap().is_empty());
        unstage(&repo, &["x".into()]).unwrap();
        assert_eq!(status(&repo).unwrap().staged.len(), 0);
        let _ = fs::remove_dir_all(&repo);
    }
}
