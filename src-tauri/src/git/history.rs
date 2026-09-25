//! The commit log, its filters and graph, reflog and branch comparisons.

use super::{
    has_head, pushed_base, range_files, run, run_text, validate_branch, validate_full_ref,
    validate_rev, FileChange, REF_KINDS,
};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Commit {
    pub sha: String,
    pub short_sha: String,
    pub author_name: String,
    pub author_email: String,
    /// When it was written (author date).
    pub timestamp: i64,
    /// Who put it on the branch, and when: a rebase, cherry-pick or amend moves this, not `timestamp`.
    pub committer_name: String,
    pub committed_at: i64,
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
            validate_branch(repo, r).map_err(|_| format!("not a remote-tracking branch: {r}"))?;
            format!("refs/remotes/{r}")
        }
        None => "HEAD".to_string(),
    };
    commits(
        repo,
        &[&tip],
        rev.is_none(),
        rev.is_some(),
        skip,
        limit,
        filter,
    )
}

/// Which refs the all-branches history walks. Refs are full names: refs/heads/…, refs/remotes/…,
/// refs/tags/….
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphRefs {
    pub local: bool,
    pub remote: bool,
    pub tags: bool,
    /// Left out, though still listed where another shown ref reaches them.
    pub hidden: Vec<String>,
    /// Just this ref's history, in place of all the others and HEAD.
    pub only: Option<String>,
}

impl Default for GraphRefs {
    fn default() -> Self {
        Self {
            local: true,
            remote: true,
            tags: true,
            hidden: vec![],
            only: None,
        }
    }
}

/// Every branch's history, remote-tracking branches' and tags' too, and HEAD's (it may be
/// detached), as far as `refs` lets through. Commits HEAD doesn't have yet are marked, as in
/// another branch's log.
pub fn log_all(
    repo: &Path,
    refs: &GraphRefs,
    skip: u32,
    limit: u32,
    filter: &LogFilter,
) -> Result<Vec<Commit>, String> {
    // git tracks a rename across the whole walk: another branch's commits from before it would
    // drop out of the file's history. Following a file stays on HEAD's.
    if filter.follows() {
        return log_filtered(repo, None, skip, limit, filter);
    }
    // A new orphan branch has no history of its own yet; the other branches still do, with
    // nothing to mark as missing from it.
    let head = has_head(repo);
    let mut tips: Vec<String> = vec![];
    if let Some(only) = &refs.only {
        validate_full_ref(repo, only)?;
        if run(repo, &["rev-parse", "--verify", "-q", only]).is_err() {
            return Err(format!("{only} no longer exists"));
        }
        tips.push(only.clone());
    } else {
        for h in &refs.hidden {
            validate_full_ref(repo, h)?;
        }
        let on = [refs.local, refs.remote, refs.tags];
        for ((prefix, flag), on) in REF_KINDS.iter().zip(on) {
            if !on {
                continue;
            }
            // --exclude applies to the next --branches/--remotes/--tags, without its prefix.
            // Ref names can't hold glob characters, so each pattern matches only its ref.
            let hidden = refs.hidden.iter().filter_map(|h| h.strip_prefix(prefix));
            tips.extend(hidden.map(|h| format!("--exclude={h}")));
            tips.push(flag.to_string());
        }
        if head {
            tips.push("HEAD".into());
        } else {
            // With no ref to walk, git would fall back to the unborn HEAD and fail.
            let kinds = REF_KINDS.iter().zip(on).filter(|(_, on)| *on);
            let mut args = vec!["for-each-ref", "--format=%(refname)"];
            args.extend(kinds.map(|((prefix, _), _)| *prefix));
            let listed = if args.len() > 2 {
                run_text(repo, &args)?
            } else {
                String::new()
            };
            if listed.lines().all(|r| refs.hidden.iter().any(|h| h == r)) {
                return Ok(vec![]);
            }
        }
    }
    let tips: Vec<&str> = tips.iter().map(String::as_str).collect();
    commits(repo, &tips, head, head, skip, limit, filter)
}

/// Comparing HEAD with `with` (a full ref): the commits `with` has that HEAD doesn't
/// (`incoming`), or those HEAD has that `with` doesn't.
pub fn log_compare(
    repo: &Path,
    with: &str,
    incoming: bool,
    skip: u32,
    limit: u32,
) -> Result<Vec<Commit>, String> {
    validate_full_ref(repo, with)?;
    if !has_head(repo) {
        return Ok(vec![]);
    }
    let range = if incoming {
        format!("HEAD..{with}")
    } else {
        format!("{with}..HEAD")
    };
    commits(
        repo,
        &[&range],
        true,
        true,
        skip,
        limit,
        &LogFilter::default(),
    )
}

/// How many commits HEAD has that `with` doesn't, and `with` has that HEAD doesn't.
/// What `with` changed since it and HEAD parted, as a pull request of it would show: the
/// merge base, `with`'s commit, and the files between them.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompareFiles {
    pub base: String,
    pub head: String,
    pub files: Vec<FileChange>,
}

pub fn compare_files(repo: &Path, with: &str) -> Result<CompareFiles, String> {
    validate_full_ref(repo, with)?;
    let base = run_text(repo, &["merge-base", "HEAD", with])
        .map_err(|_| "They have no commit in common.".to_string())?
        .trim()
        .to_string();
    let head = run_text(repo, &["rev-parse", &format!("{with}^{{commit}}")])?
        .trim()
        .to_string();
    let files = range_files(repo, &base, &head)?;
    Ok(CompareFiles { base, head, files })
}

/// A place HEAD has been: what took it there (git's "reflog subject") and when.
#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ReflogEntry {
    pub sha: String,
    /// HEAD@{n}
    pub selector: String,
    pub message: String,
    pub timestamp: i64,
}

/// Where HEAD has been, newest first: commits a reset or rebase left behind are still here.
pub fn reflog(repo: &Path, limit: u32) -> Result<Vec<ReflogEntry>, String> {
    if !has_head(repo) {
        return Ok(vec![]);
    }
    let n = limit.clamp(1, 1000).to_string();
    let out = run_text(
        repo,
        &[
            "reflog",
            "-n",
            &n,
            "--format=%H%x1f%gd%x1f%ct%x1f%gs",
            "HEAD",
        ],
    )?;
    Ok(out
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.splitn(4, '\x1f').collect();
            (f.len() == 4).then(|| ReflogEntry {
                sha: f[0].to_string(),
                selector: f[1].to_string(),
                timestamp: f[2].parse().unwrap_or(0),
                message: f[3].to_string(),
            })
        })
        .collect())
}

pub fn compare_counts(repo: &Path, with: &str) -> Result<(u32, u32), String> {
    validate_full_ref(repo, with)?;
    let out = run_text(
        repo,
        &[
            "rev-list",
            "--left-right",
            "--count",
            &format!("HEAD...{with}"),
        ],
    )?;
    let mut n = out.split_whitespace().map(|x| x.parse().unwrap_or(0));
    Ok((n.next().unwrap_or(0), n.next().unwrap_or(0)))
}

/// The commit a SHA (or a prefix of one) names, wherever it is: a pasted SHA goes first in a search.
/// A full ref name (refs/heads/…) finds its tip, for going to a branch in the graph.
pub fn find_commit(repo: &Path, sha: &str) -> Result<Option<Commit>, String> {
    let sha_ok = validate_rev(sha).is_ok() && sha.len() <= 40;
    let ref_ok = || sha.starts_with("refs/") && validate_full_ref(repo, sha).is_ok();
    if !(sha_ok || ref_ok()) || !has_head(repo) {
        return Ok(None);
    }
    // Unknown, or a prefix of several: no commit, not an error.
    let Ok(full) = run_text(
        repo,
        &["rev-parse", "--verify", "-q", &format!("{sha}^{{commit}}")],
    ) else {
        return Ok(None);
    };
    let found = commits(
        repo,
        &[full.trim()],
        true,
        true,
        0,
        1,
        &LogFilter::default(),
    )?;
    Ok(found.into_iter().next())
}

/// `limit` commits of `tips`' history after `skip`. `mark_unpushed` / `mark_not_in_head`: work
/// out those flags (HEAD's own history is all in HEAD; another branch's is never unpushed).
fn commits(
    repo: &Path,
    tips: &[&str],
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
    let mut filter_args = filter.args();
    // An unfiltered log is drawn as a graph: keep each branch's commits together rather than
    // interleaved by date. A search skips it: with paths, git would diff all of history first.
    if filter_args.is_empty() && filter.paths.is_empty() {
        filter_args.push("--topo-order".into());
    }
    let mut paths: Vec<&str> = vec!["--"];
    paths.extend(filter.paths.iter().map(String::as_str));
    // Of `tips`' matching commits, the ones not reachable from `not`. Log order is the same
    // with or without `--not`, so the first skip+limit of them cover this page. A search
    // goes through the same filter: its page can sit anywhere in the full history.
    let outside = |not: &str| -> Result<std::collections::HashSet<String>, String> {
        let n = format!("-n{}", skip + limit);
        let mut args = vec!["log", "--format=%H", &n];
        args.extend(filter_args.iter().map(String::as_str));
        args.extend(tips);
        args.extend(["--not", not]);
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
        "--format=%x1e%H%x1f%h%x1f%an%x1f%ae%x1f%at%x1f%cn%x1f%ct%x1f%P%x1f%D%x1f%s%x1f%b%x1f",
    ];
    if filter.follows() {
        args.push("--name-only");
    }
    args.extend(filter_args.iter().map(String::as_str));
    args.extend(tips);
    args.extend(&paths);
    let raw = run_text(repo, &args)?;
    Ok(raw
        .split('\x1e')
        .filter_map(|rec| {
            let f: Vec<&str> = rec.split('\x1f').collect();
            if f.len() < 12 {
                return None;
            }
            Some(Commit {
                sha: f[0].to_string(),
                short_sha: f[1].to_string(),
                author_name: f[2].to_string(),
                author_email: f[3].to_string(),
                timestamp: f[4].parse().unwrap_or(0),
                committer_name: f[5].to_string(),
                committed_at: f[6].parse().unwrap_or(0),
                parents: f[7].split_whitespace().map(str::to_string).collect(),
                refs: f[8]
                    .split(", ")
                    .filter(|r| !r.is_empty())
                    .map(str::to_string)
                    .collect(),
                subject: f[9].to_string(),
                body: f[10].trim().to_string(),
                unpushed: unpushed.contains(f[0]),
                on_origin: off_origin.as_ref().is_some_and(|off| !off.contains(f[0])),
                not_in_head: not_in_head.contains(f[0]),
                file: filter
                    .follows()
                    .then(|| f[11].lines().find(|l| !l.is_empty()).map(str::to_string))
                    .flatten(),
            })
        })
        .skip(skip_here)
        .collect())
}
