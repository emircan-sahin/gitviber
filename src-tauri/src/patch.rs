//! Patches out and in: a change copied as a patch, and one applied to the working tree, from the
//! clipboard or a commit's change backwards (revert.rs). The commands that write files run these
//! through the journal, so ⌘Z undoes them and what they replaced is in the Trash.

use crate::git::{self, parse_numstat, validate_rev, PINS};
use serde::{Deserialize, Serialize};
use similar::{Algorithm, TextDiff};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// Past this a patch isn't copied or applied: a clipboard that large is a mistake.
pub const MAX_BYTES: usize = 10 << 20;

/// `git diff` for a patch that `git apply` takes, whatever the user's diff settings say (see
/// PINS); against `index` instead of the real one when given.
pub(crate) fn diff(repo: &Path, index: Option<&Scratch>, rest: &[&str]) -> Result<Vec<u8>, String> {
    let head = [
        "diff",
        "--no-color",
        "--no-ext-diff",
        "--no-textconv",
        "--binary",
        "--submodule=short",
    ];
    let args = [&PINS[..], &head, rest].concat();
    match index {
        Some(index) => index.git(repo, &args, None),
        None => git::run(repo, &args),
    }
}

/// A patch as clipboard text, refused when it's empty or too large.
fn text(patch: Vec<u8>) -> Result<String, String> {
    if patch.is_empty() {
        return Err("There are no changes to copy.".into());
    }
    if patch.len() > MAX_BYTES {
        return Err("The patch is over 10 MB, too large to copy.".into());
    }
    String::from_utf8(patch)
        .map_err(|_| "The patch isn't UTF-8 text, so it can't be copied.".into())
}

/// Files' changes as a patch: "unstaged" (index → working tree, untracked files as new), "staged"
/// (HEAD → index) or "commit" (`sha`'s parent → `sha`). `paths` has a rename's old path too.
pub fn changes(
    repo: &Path,
    kind: &str,
    paths: &[String],
    sha: Option<&str>,
) -> Result<String, String> {
    if paths.is_empty() {
        return Err("No files to copy.".into());
    }
    let paths: Vec<&str> = paths.iter().map(String::as_str).collect();
    let out = match kind {
        "unstaged" => {
            let others = [
                &["ls-files", "-z", "--others", "--exclude-standard", "--"],
                &paths[..],
            ];
            let raw = git::run(repo, &others.concat())?;
            let untracked: Vec<String> = raw
                .split(|b| *b == 0)
                .filter(|p| !p.is_empty())
                .map(|p| String::from_utf8_lossy(p).into_owned())
                .collect();
            // Untracked files come along as new ones: marked to be added (`add -N`) in a copy
            // of the index, which one diff then shows.
            let index = (!untracked.is_empty())
                .then(|| Scratch::index(repo))
                .transpose()?;
            if let Some(index) = &index {
                let untracked: Vec<&str> = untracked.iter().map(String::as_str).collect();
                index.git(repo, &[&["add", "-N", "--"], &untracked[..]].concat(), None)?;
            }
            diff(repo, index.as_ref(), &[&["-M", "--"], &paths[..]].concat())?
        }
        "staged" => diff(
            repo,
            None,
            &[&["-M", "--cached", "--"], &paths[..]].concat(),
        )?,
        "commit" => {
            let sha = sha.ok_or("missing commit")?;
            validate_rev(sha)?;
            let parent = git::parent_or_empty(repo, sha)?;
            diff(
                repo,
                None,
                &[&["-M", &parent, sha, "--"], &paths[..]].concat(),
            )?
        }
        other => return Err(format!("can't copy a {other} change as a patch")),
    };
    text(out)
}

/// A commit as `git am` takes it, with its message and author. Not a merge: `format-patch -1` of
/// one is the commit before it, from its first parent's side.
pub fn commit(repo: &Path, sha: &str) -> Result<String, String> {
    validate_rev(sha)?;
    let parents = git::run_text(repo, &["rev-list", "--parents", "-n", "1", sha])?;
    if parents.split_whitespace().count() > 2 {
        return Err("A merge commit has no patch of its own.".into());
    }
    let args = [
        "format-patch",
        "-1",
        "--stdout",
        "--binary",
        "--no-color",
        "--no-signature",
        sha,
    ];
    text(git::run(repo, &[&PINS[..], &args].concat())?)
}

/// A stash's changes, its untracked files included.
pub fn stash(repo: &Path, sha: &str) -> Result<String, String> {
    validate_rev(sha)?;
    let args = [
        "stash",
        "show",
        "-p",
        "--include-untracked",
        "--no-color",
        "--no-ext-diff",
        "--no-textconv",
        "--binary",
        sha,
    ];
    text(git::run(repo, &[&PINS[..], &args].concat())?)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinesPatch {
    pub path: String,
    pub old_path: Option<String>,
    /// The two sides as shown (None: the file isn't there).
    pub original: Option<String>,
    pub modified: Option<String>,
    /// The chosen lines, as lines::Request has them.
    pub removed: Vec<u32>,
    pub added: Vec<u32>,
}

/// Some of a diff's changed lines as a patch: the old side, and it with only those changes made.
pub fn lines(req: &LinesPatch) -> Result<String, String> {
    let old = req.original.as_deref().unwrap_or("");
    let new = req.modified.as_deref().unwrap_or("");
    let removed: HashSet<u32> = req.removed.iter().copied().collect();
    let added: HashSet<u32> = req.added.iter().copied().collect();
    let partial = crate::lines::apply(old, new, &removed, &added);
    let from = req.old_path.as_deref().unwrap_or(&req.path);
    let a = match req.original {
        Some(_) => format!("a/{from}"),
        None => "/dev/null".into(),
    };
    // Every line of a deleted file chosen: the patch deletes it.
    let b = match req.modified {
        None if partial.is_empty() => "/dev/null".into(),
        _ => format!("b/{}", req.path),
    };
    let diff = TextDiff::configure()
        .algorithm(Algorithm::Patience)
        .timeout(Duration::from_secs(2))
        .diff_lines(old, &partial);
    let hunks = diff
        .unified_diff()
        .context_radius(3)
        .header(&a, &b)
        .to_string();
    if hunks.is_empty() {
        return Err("No changed lines are selected.".into());
    }
    let mut out = format!("diff --git a/{from} b/{}\n", req.path);
    if req.original.is_none() {
        out.push_str("new file mode 100644\n");
    } else if b == "/dev/null" {
        out.push_str("deleted file mode 100644\n");
    }
    out.push_str(&hunks);
    Ok(out)
}

#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PatchFile {
    pub path: String,
    pub old_path: Option<String>,
    /// "A" new, "D" deleted, "R" renamed, "C" copied, else "M".
    pub status: String,
    /// None for a binary file.
    pub additions: Option<u32>,
    pub deletions: Option<u32>,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub files: Vec<PatchFile>,
    /// "clean": it applies as it is; "merge": only with a three-way merge, which may leave
    /// conflicts; None: not at all, and `error` says why.
    pub applies: Option<String>,
    pub error: Option<String>,
}

/// Refuses a patch that's too large, and git's own words for one that isn't a patch.
fn readable(patch: &[u8]) -> Result<(), String> {
    if patch.len() > MAX_BYTES {
        return Err("The patch is over 10 MB, too large to apply.".into());
    }
    if patch.iter().all(u8::is_ascii_whitespace) {
        return Err("The clipboard is empty.".into());
    }
    Ok(())
}

fn apply_args<'a>(reverse: bool, rest: &[&'a str]) -> Vec<&'a str> {
    let mut args = vec!["apply", "--whitespace=nowarn"];
    if reverse {
        args.push("-R");
    }
    args.extend(rest);
    args.push("-");
    args
}

/// A path as a patch header writes it: C-quoted ("\"a\\303\\244.txt\"") when it has odd bytes.
fn unquote(s: &str) -> String {
    let Some(inner) = s.strip_prefix('"').and_then(|s| s.strip_suffix('"')) else {
        return s.to_string();
    };
    let (b, mut out, mut i) = (inner.as_bytes(), Vec::new(), 0);
    while i < b.len() {
        let c = b[i];
        i += 1;
        if c != b'\\' || i == b.len() {
            out.push(c);
            continue;
        }
        let e = b[i];
        i += 1;
        out.push(match e {
            b'n' => b'\n',
            b't' => b'\t',
            b'r' => b'\r',
            b'a' => 7,
            b'b' => 8,
            b'f' => 12,
            b'v' => 11,
            b'0'..=b'3' if i + 1 < b.len() => {
                let n = (e - b'0') * 64 + (b[i] - b'0') * 8 + (b[i + 1] - b'0');
                i += 2;
                n
            }
            other => other,
        });
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// What `git apply --numstat` leaves out of a patch's headers: each rename or copy's other name,
/// by the name the file has once applied, and whether it bumps a submodule (mode 160000).
fn headers(patch: &[u8], reverse: bool) -> (HashMap<String, (String, &'static str)>, bool) {
    let text = String::from_utf8_lossy(patch);
    let (mut moved, mut gitlink) = (HashMap::new(), false);
    // Between a `diff --git` line and its first hunk: only there are these lines headers.
    let (mut header, mut from, mut kind) = (false, None::<String>, "R");
    for line in text.lines() {
        if line.starts_with("diff --git ") {
            (header, from) = (true, None);
            continue;
        }
        if !header {
            continue;
        }
        if line.starts_with("@@")
            || line.starts_with("--- ")
            || line.starts_with("GIT binary patch")
        {
            header = false;
        } else if line.ends_with(" 160000")
            && (line.starts_with("index ") || line.contains(" file mode "))
        {
            gitlink = true;
        } else if let Some(p) = line
            .strip_prefix("rename from ")
            .or_else(|| line.strip_prefix("copy from "))
        {
            kind = if line.starts_with("copy") { "C" } else { "R" };
            from = Some(unquote(p));
        } else if let Some(to) = line
            .strip_prefix("rename to ")
            .or_else(|| line.strip_prefix("copy to "))
        {
            let (Some(from), to) = (from.take(), unquote(to)) else {
                continue;
            };
            // Backwards, a copy is the copy deleted: no other name to it.
            match (reverse, kind) {
                (false, _) => drop(moved.insert(to, (from, kind))),
                (true, "R") => drop(moved.insert(from, (to, kind))),
                _ => {}
            }
        }
    }
    (moved, gitlink)
}

/// The files `patch` touches, one row per file, each checked to be in the working tree and
/// outside `.git` (git apply refuses paths that leave the repository, not ones inside its git
/// dir). A link is checked where it is, not where it points: git writes the link itself.
pub fn files(repo: &Path, patch: &[u8], reverse: bool) -> Result<Vec<PatchFile>, String> {
    readable(patch)?;
    let numstat = git::run_with(
        repo,
        &apply_args(reverse, &["--numstat", "-z"]),
        &[],
        Some(patch),
    )
    .map_err(|e| {
        if e.contains("No valid patches") {
            "The clipboard has no patch in it.".to_string()
        } else {
            e
        }
    })?;
    let (moved, gitlink) = headers(patch, reverse);
    if gitlink {
        return Err("It moves a submodule to another commit, which only a checkout inside the submodule can do.".into());
    }
    let summary = git::run_with(repo, &apply_args(reverse, &["--summary"]), &[], Some(patch))?;
    let summary = String::from_utf8_lossy(&summary);
    let made = |what: &str, path: &str| {
        summary.lines().any(|l| {
            l.trim_start()
                .strip_prefix(what)
                .and_then(|rest| rest.split_once(' '))
                .is_some_and(|(_mode, p)| unquote(p) == path)
        })
    };
    // By path: an mbox's commits can each change the same file.
    let counts: BTreeMap<_, _> = parse_numstat(&numstat).into_iter().collect();
    let files: Vec<PatchFile> = counts
        .into_iter()
        .map(|(path, (additions, deletions))| {
            let (old_path, status) = match moved.get(&path) {
                Some((old, kind)) => (Some(old.clone()), *kind),
                None if made("create mode ", &path) => (None, "A"),
                None if made("delete mode ", &path) => (None, "D"),
                None => (None, "M"),
            };
            PatchFile {
                path,
                old_path,
                status: status.into(),
                additions,
                deletions,
            }
        })
        .collect();
    if files.is_empty() {
        return Err("The clipboard has no patch in it.".into());
    }
    for p in touched(&files) {
        crate::fs::resolve_entry(repo, &p)?;
    }
    Ok(files)
}

/// Every path a patch writes: a rename's old one too.
pub fn touched(files: &[PatchFile]) -> Vec<String> {
    let all: BTreeSet<String> = files
        .iter()
        .flat_map(|f| f.old_path.iter().chain([&f.path]).cloned())
        .collect();
    all.into_iter().collect()
}

/// What applying `patch` would do, without doing it.
pub fn preview(repo: &Path, patch: &str) -> Result<Preview, String> {
    let patch = patch.as_bytes();
    let files = files(repo, patch, false)?;
    let clean = git::run_with(repo, &apply_args(false, &["--check"]), &[], Some(patch));
    let (applies, error) = match clean {
        Ok(_) => (Some("clean".to_string()), None),
        Err(_) => {
            let index = Scratch::index(repo)?;
            index.sync(repo, &touched(&files))?;
            match index.git(
                repo,
                &apply_args(false, &["--check", "--3way"]),
                Some(patch),
            ) {
                Ok(_) => (Some("merge".to_string()), None),
                Err(e) => (None, Some(e)),
            }
        }
    };
    Ok(Preview {
        files,
        applies,
        error,
    })
}

/// Applies `patch` (backwards with `reverse`) to the working tree alone, merging three ways
/// where the files moved on since; returns those of `paths` (the files it touches) left with
/// conflict markers. `--3way` takes its base from the index, stages its result and refuses
/// files with unstaged changes, so it runs against a copy of the index made to match the
/// working tree on `paths`, and the real one stays as it was. Conflicts elsewhere (a merge in
/// progress) aren't the patch's.
pub fn apply(
    repo: &Path,
    patch: &[u8],
    reverse: bool,
    paths: &[String],
) -> Result<Vec<String>, String> {
    let index = Scratch::index(repo)?;
    index.sync(repo, paths)?;
    let applied = index.git(repo, &apply_args(reverse, &["--3way"]), Some(patch));
    let ours: HashSet<&str> = paths.iter().map(String::as_str).collect();
    let raw = index.git(repo, &["ls-files", "-u", "-z"], None)?;
    let conflicts: BTreeSet<String> = raw
        .split(|b| *b == 0)
        .filter_map(|e| e.splitn(2, |b| *b == b'\t').nth(1))
        .map(|p| String::from_utf8_lossy(p).into_owned())
        .filter(|p| ours.contains(p.as_str()))
        .collect();
    // Conflicts end it with an error too, the patch applied; without any it applied nothing.
    match applied {
        Err(e) if conflicts.is_empty() => Err(e),
        _ => Ok(conflicts.into_iter().collect()),
    }
}

/// A file of ours for git to read, in the git dir and removed once dropped: a copy of the index,
/// or a side of a merge. In the git dir as `git stash` keeps its own index: a split index finds
/// its shared part next to it, and no one else can plant a file there for us to write through.
pub(crate) struct Scratch(PathBuf);

impl Scratch {
    /// A new path in the git dir, and the git dir's own `name` with it.
    fn paths(repo: &Path, kind: &str, name: &str) -> Result<(PathBuf, PathBuf), String> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let ours = format!("{kind}.gitviber.{}.{n}", std::process::id());
        let out = git::run_text(
            repo,
            &["rev-parse", "--git-path", &ours, "--git-path", name],
        )?;
        let mut lines = out.lines().map(|l| repo.join(l));
        match (lines.next(), lines.next()) {
            (Some(a), Some(b)) => Ok((a, b)),
            _ => Err("git didn't say where its folder is".into()),
        }
    }

    /// Ours that a killed run left in the git dir (each copy as big as the index), once older
    /// than any run of ours could still be using.
    pub(crate) fn sweep(repo: &Path, age: Duration) {
        let Ok((_, dir)) = Self::paths(repo, "index", ".") else {
            return;
        };
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for e in entries.flatten() {
            let name = e.file_name();
            let ours = name.to_string_lossy().contains(".gitviber.");
            let old = || {
                e.metadata()
                    .and_then(|m| m.modified())
                    .is_ok_and(|t| t.elapsed().is_ok_and(|a| a > age))
            };
            if ours && old() {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }

    /// `bytes` in a file made new (never one already there, nor through a link).
    pub(crate) fn file(repo: &Path, bytes: &[u8]) -> Result<Self, String> {
        let (path, _) = Self::paths(repo, "merge", "index")?;
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|e| e.to_string())?;
        let file = Scratch(path);
        f.write_all(bytes).map_err(|e| e.to_string())?;
        Ok(file)
    }

    /// A copy of the index.
    pub(crate) fn index(repo: &Path) -> Result<Self, String> {
        let (path, real) = Self::paths(repo, "index", "index")?;
        let index = Scratch(path);
        if real.exists() {
            std::fs::copy(&real, &index.0).map_err(|e| e.to_string())?;
            // A copy keeps the index's time, often hours back: `sweep` would take it for one a
            // killed run left, and git would read a missing index as an empty one.
            OpenOptions::new()
                .write(true)
                .open(&index.0)
                .and_then(|f| f.set_modified(std::time::SystemTime::now()))
                .map_err(|e| e.to_string())?;
        }
        Ok(index)
    }

    /// This index with `paths` as they are in the working tree, gone ones taken out.
    fn sync(&self, repo: &Path, paths: &[String]) -> Result<(), String> {
        let paths: Vec<&str> = paths.iter().map(String::as_str).collect();
        let args = [&["update-index", "--add", "--remove", "--"], &paths[..]].concat();
        self.git(repo, &args, None).map(|_| ())
    }

    pub(crate) fn path(&self) -> Result<&str, String> {
        self.0
            .to_str()
            .ok_or_else(|| "temporary path isn't UTF-8".into())
    }

    /// git with this as its index; given up after a read's timeout inside `git::reading`.
    pub(crate) fn git(
        &self,
        repo: &Path,
        args: &[&str],
        input: Option<&[u8]>,
    ) -> Result<Vec<u8>, String> {
        let mut cmd = git::command(repo, args);
        cmd.env("GIT_INDEX_FILE", &self.0);
        crate::process::exec(cmd, "git", &[], input, git::read_timeout())
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
