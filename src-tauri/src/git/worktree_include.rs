//! `.worktreeinclude`: the gitignored files (`.env`, local secrets) a new worktree gets copied
//! from the main one. Claude Code's file and rules, so a repo set up for it works here unchanged.

use super::{command, main_worktree, run};
use crate::process::exec;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

const INCLUDE_FILE: &str = ".worktreeinclude";

/// Where a new worktree's ignored files come from: the main worktree, or `repo` when that is
/// bare or gone.
pub fn include_source(repo: &Path) -> PathBuf {
    main_worktree(repo).map_or_else(|| repo.to_path_buf(), PathBuf::from)
}

/// The files in `root` that `.worktreeinclude` matches and git ignores, relative to it; none
/// without the file. Tracked files are never listed.
pub fn worktree_includes(root: &Path) -> Result<Vec<String>, String> {
    let Some(file) = crate::fs::resolve(root, INCLUDE_FILE)
        .ok()
        .filter(|f| f.is_file())
    else {
        return Ok(vec![]);
    };
    let patterns = std::fs::read_to_string(file).map_err(|e| e.to_string())?;
    let patterns: Vec<&str> = patterns.lines().filter_map(pattern).collect();
    // A wholly ignored folder comes back as one entry (node_modules/), so this walk skips it.
    let args = [
        "ls-files",
        "-z",
        "--others",
        "--ignored",
        "--exclude-standard",
        "--directory",
    ];
    let ignored = run(root, &args)?;
    let listed: Vec<&str> = entries(&ignored).collect();
    // A folder holding only ignored files is listed too, and then what's in it: only one with
    // nothing listed under it is collapsed.
    let parents: HashSet<&str> = listed
        .iter()
        .flat_map(|p| {
            let p = p.trim_end_matches('/');
            p.match_indices('/').map(move |(i, _)| &p[..i])
        })
        .collect();
    let (dirs, files): (Vec<&str>, Vec<&str>) = listed.into_iter().partition(|p| p.ends_with('/'));
    let dirs: HashSet<&str> = dirs
        .iter()
        .map(|d| d.trim_end_matches('/'))
        .filter(|d| !parents.contains(d))
        .collect();
    let files: HashSet<&str> = files.into_iter().collect();
    let skip: Vec<String> = dirs
        .iter()
        .filter(|d| !patterns.iter().any(|p| reaches(p, d)))
        .map(|d| format!(":(exclude,literal){d}/"))
        .collect();
    let from = format!("--exclude-from={INCLUDE_FILE}");
    let mut args = vec!["ls-files", "-z", "--others", "--ignored", &from, "--", "."];
    args.extend(skip.iter().map(String::as_str));
    let mut cmd = command(root, &args);
    // The folders left out are pathspec magic.
    cmd.env_remove("GIT_LITERAL_PATHSPECS");
    let matched = exec(cmd, "git ls-files", &[], None, None)?;
    Ok(entries(&matched)
        // A nested repository shows as its folder; it isn't copied.
        .filter(|p| !p.ends_with('/'))
        .filter(|p| files.contains(p) || p.match_indices('/').any(|(i, _)| dirs.contains(&p[..i])))
        .map(String::from)
        .collect())
}

fn entries(out: &[u8]) -> impl Iterator<Item = &str> {
    out.split(|&b| b == 0)
        .filter(|p| !p.is_empty())
        .filter_map(|p| std::str::from_utf8(p).ok())
}

/// A line as `reaches` reads it: none for a blank line, a comment or a negation, which adds
/// nothing to copy.
fn pattern(line: &str) -> Option<&str> {
    if line.starts_with('#') || line.starts_with('!') {
        return None;
    }
    let p = line.trim_end();
    let p = p.strip_prefix('\\').unwrap_or(p);
    (!p.is_empty()).then_some(p)
}

/// Whether the ignored folder `dir` is worth walking for `pattern`: it names the folder, one
/// above it, or a way into it. As in Claude Code, `**/name` (and a bare `name`, the same thing)
/// reaches only a folder with `name` in its path, so `.env` leaves node_modules unwalked.
fn reaches(pattern: &str, dir: &str) -> bool {
    let p = pattern.trim_end_matches('/');
    // A slash anywhere but the end anchors it to the root.
    let anchored = p.contains('/');
    let segs: Vec<&str> = p.trim_start_matches('/').split('/').collect();
    if !anchored || segs[0] == "**" {
        let first = if anchored { segs.get(1) } else { segs.first() };
        return first.is_none_or(|f| dir.split('/').any(|name| glob(f, name)));
    }
    for (i, name) in dir.split('/').enumerate() {
        match segs.get(i) {
            None | Some(&"**") => return true,
            Some(s) if !glob(s, name) => return false,
            _ => {}
        }
    }
    true
}

/// A gitignore glob against one name, loosely: a `[class]` takes any character, and case is
/// ignored. Loose at worst walks a folder for nothing; git decides what matches.
fn glob(pattern: &str, name: &str) -> bool {
    fn go(p: &[char], s: &[char]) -> bool {
        match p {
            [] => s.is_empty(),
            ['*', rest @ ..] => (0..=s.len()).any(|i| go(rest, &s[i..])),
            ['?', rest @ ..] => !s.is_empty() && go(rest, &s[1..]),
            ['[', ..] if p.len() > 2 && p[2..].contains(&']') => {
                let end = 2 + p[2..].iter().position(|&c| c == ']').unwrap();
                !s.is_empty() && go(&p[end + 1..], &s[1..])
            }
            ['\\', c, rest @ ..] | [c, rest @ ..] => {
                s.first()
                    .is_some_and(|x| x.to_lowercase().eq(c.to_lowercase()))
                    && go(rest, &s[1..])
            }
        }
    }
    let p: Vec<char> = pattern.chars().collect();
    let s: Vec<char> = name.chars().collect();
    go(&p, &s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walks_only_the_folders_a_pattern_reaches() {
        // Bare names and **/ reach a folder only through a name in its path.
        assert!(!reaches(".env", "node_modules"));
        assert!(!reaches("**/.env", "node_modules"));
        assert!(reaches("node_modules/", "node_modules"));
        assert!(reaches("**/.claude/skills/*.md", "pkg/.claude"));
        assert!(reaches("*.cache", "dist/build.cache"));
        // Anchored ones through their leading folders.
        assert!(reaches("config/secrets.json", "config"));
        assert!(reaches("/config/", "config/local"));
        assert!(reaches("vendor/**/config.json", "vendor/a/b"));
        assert!(reaches("app/[a-z]*/env", "app/x"));
        assert!(!reaches("config/secrets.json", "configs"));
        assert!(!reaches("a/b/c", "a/x"));
    }

    #[test]
    fn globs_one_name() {
        assert!(glob("*.env", "prod.env"));
        assert!(glob(".ENV", ".env"));
        assert!(glob("?b[cd]", "abx"));
        assert!(glob("\\*", "*"));
        assert!(!glob("\\*", "a"));
        assert!(!glob("*.env", "env"));
    }

    #[test]
    fn skips_comments_negations_and_blanks() {
        let lines = ["# note", "!.env.example", "  ", ".env  ", "\\#hash"];
        let kept: Vec<&str> = lines.into_iter().filter_map(pattern).collect();
        assert_eq!(kept, [".env", "#hash"]);
    }
}
