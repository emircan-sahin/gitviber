//! Line-by-line blame, with the commits it names.

use super::{has_head, run, run_text, run_with};
use crate::lfs;
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;

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
    /// Why this file has no blame (a Git LFS file), in place of all-new lines.
    pub unavailable: Option<String>,
}

/// `path` in HEAD is a Git LFS pointer: git blames the pointer's three lines, not the file.
fn lfs_in_head(repo: &Path, path: &str) -> bool {
    let spec = format!("HEAD:{path}");
    let small = run_text(repo, &["cat-file", "-s", &spec])
        .ok()
        .and_then(|n| n.trim().parse::<u64>().ok())
        .is_some_and(|n| n < 1024);
    small && run(repo, &["cat-file", "blob", &spec]).is_ok_and(|b| lfs::pointer(&b).is_some())
}

/// Which commit last changed each line of the working-tree file. It's git's own blame, so the
/// user's blame.ignoreRevsFile applies. A file that isn't in HEAD yet has no lines: all new.
pub fn blame(repo: &Path, path: &str) -> Result<Blame, String> {
    if !has_head(repo) {
        return Ok(Blame::default());
    }
    if lfs_in_head(repo, path) {
        return Ok(Blame {
            unavailable: Some("No blame for Git LFS files: git only has their pointers.".into()),
            ..Default::default()
        });
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
