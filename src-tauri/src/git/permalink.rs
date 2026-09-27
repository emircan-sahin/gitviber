//! Links to a file on GitHub that keep pointing at the same code: at a commit, not a branch.

use super::run_text;
use serde::Serialize;
use std::path::Path;

#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Permalink {
    /// The newest commit of HEAD's that origin has.
    pub sha: String,
    /// A folder, not a file.
    pub tree: bool,
    /// The lines asked for, where they are in that commit.
    pub lines: Option<(u32, u32)>,
}

/// Where the working tree's `path` (or its `lines`, 1-based and inclusive) is on origin: in the
/// newest commit of HEAD's history that origin has, if those lines haven't changed since.
pub fn permalink(repo: &Path, path: &str, lines: Option<(u32, u32)>) -> Result<Permalink, String> {
    let sha = pushed_head(repo)?;
    let short = &sha[..sha.len().min(7)];
    let spec = format!("{sha}:{path}");
    let tree = match run_text(repo, &["cat-file", "-t", &spec]).map(|t| t.trim().to_string()) {
        Ok(t) if t == "tree" => true,
        Ok(t) if t == "blob" => false,
        _ => {
            return Err(format!(
                "{path} isn't on GitHub yet: {short}, the last commit of this branch there, doesn't have it. Push first."
            ))
        }
    };
    let lines = match lines.filter(|_| !tree) {
        None => None,
        Some((from, to)) => {
            let diff = run_text(
                repo,
                &[
                    "diff",
                    "-U0",
                    "--no-color",
                    "--no-ext-diff",
                    "--no-textconv",
                    &sha,
                    "--",
                    path,
                ],
            )?;
            Some(lines_before(&diff, from, to).ok_or_else(|| {
                format!("These lines changed after {short}, the last commit of this branch on GitHub. Push first, or pick lines that haven't changed.")
            })?)
        }
    };
    Ok(Permalink { sha, tree, lines })
}

/// HEAD if origin has it, else the newest of its ancestors that origin has: the commits on
/// the boundary of what isn't pushed. Walks only the unpushed commits.
fn pushed_head(repo: &Path) -> Result<String, String> {
    const NOT_PUSHED: &str = "Nothing on this branch is on GitHub yet. Push it first.";
    let has_origin = run_text(repo, &["for-each-ref", "--count=1", "refs/remotes/origin"])
        .is_ok_and(|s| !s.trim().is_empty());
    if !has_origin {
        return Err(NOT_PUSHED.into());
    }
    let out = run_text(
        repo,
        &[
            "rev-list",
            "--boundary",
            "HEAD",
            "--not",
            "--remotes=origin",
        ],
    )?;
    if out.trim().is_empty() {
        return run_text(repo, &["rev-parse", "HEAD"]).map(|s| s.trim().to_string());
    }
    out.lines()
        .find_map(|l| l.strip_prefix('-'))
        .map(str::to_string)
        .ok_or_else(|| NOT_PUSHED.into())
}

/// Lines `from..=to` of the new side of `diff` (`git diff -U0`) on its old side, if no change
/// touches them or falls between them.
pub(crate) fn lines_before(diff: &str, from: u32, to: u32) -> Option<(u32, u32)> {
    let mut shift: i64 = 0;
    for h in diff.lines().filter_map(hunk) {
        let (old_len, new_start, new_len) = (h.1 as i64, h.2 as i64, h.3 as i64);
        // A deletion (no new lines) sits after new line `new_start`.
        let (first, last) = if new_len == 0 {
            (new_start + 1, new_start)
        } else {
            (new_start, new_start + new_len - 1)
        };
        if last < from as i64 {
            shift += old_len - new_len;
        } else if first > to as i64 {
            break;
        } else {
            return None;
        }
    }
    let at = |n: u32| u32::try_from(n as i64 + shift).ok();
    Some((at(from)?, at(to)?))
}

/// `@@ -a[,b] +c[,d] @@` as (a, b, c, d).
fn hunk(line: &str) -> Option<(u32, u32, u32, u32)> {
    let mut parts = line.strip_prefix("@@ -")?.split(' ');
    let range = |s: &str| -> Option<(u32, u32)> {
        match s.split_once(',') {
            Some((start, len)) => Some((start.parse().ok()?, len.parse().ok()?)),
            None => Some((s.parse().ok()?, 1)),
        }
    };
    let (a, b) = range(parts.next()?)?;
    let (c, d) = range(parts.next()?.strip_prefix('+')?)?;
    Some((a, b, c, d))
}

#[cfg(test)]
mod tests {
    use super::lines_before;

    const DIFF: &str = "diff --git a/f b/f\n--- a/f\n+++ b/f\n\
        @@ -2,0 +3,2 @@ fn\n+x\n+y\n\
        @@ -10 +12 @@\n-a\n+b\n\
        @@ -20,3 +21,0 @@\n-p\n-q\n-r\n";

    #[test]
    fn lines_move_with_changes_before_them() {
        assert_eq!(lines_before(DIFF, 1, 2), Some((1, 2)));
        assert_eq!(lines_before(DIFF, 5, 11), Some((3, 9)));
        assert_eq!(lines_before(DIFF, 13, 21), Some((11, 19)));
        // Past the deletion after new line 21: three lines further on in the old file.
        assert_eq!(lines_before(DIFF, 22, 22), Some((23, 23)));
        assert_eq!(lines_before("", 4, 9), Some((4, 9)));
    }

    #[test]
    fn changed_lines_have_no_old_place() {
        assert_eq!(lines_before(DIFF, 3, 3), None);
        assert_eq!(lines_before(DIFF, 1, 12), None);
        assert_eq!(lines_before(DIFF, 12, 12), None);
        // Lines deleted between the two picked.
        assert_eq!(lines_before(DIFF, 21, 22), None);
    }
}
