//! What a guided review sends of a change of any size: a list of every changed file, then the
//! whole diffs of as many as fit. The whole patch goes in a file beside it (suggest.rs) for agents
//! that can read one; the list's tags say which diffs are only there.

use crate::git::FileChange;

/// The list stays whole up to here, about 2,000 files with their hunks' names; past it they go,
/// then past this with paths alone the list is cut.
pub const MAX_LIST: usize = 200 * 1024;
const MAX_CONTEXTS: usize = 3;
const MAX_CONTEXT: usize = 60;
const FILE_ONLY: &str = " [file only]";

/// What the prompt gets of a change.
pub struct Manifest {
    /// A line a file: status, line counts, path, tags and the names its hunks change.
    pub list: String,
    pub listed: usize,
    /// The diffs that fit, each whole, in the list's order.
    pub inline: String,
    /// Files whose diff is only in the patch file: not inlined, and not binary.
    pub file_only: usize,
    /// Bytes of diff a reader would go through (generated and binary files left out), for the timeout.
    pub reading: usize,
    /// The patch couldn't be split by file: `inline` is its start, cut at a line's end.
    pub prefix: bool,
}

/// Lockfiles and other files no one writes by hand, which a reviewer skims: their diffs stay out
/// of the prompt (they'd fill it) and wait in the patch file.
pub fn is_generated(path: &str) -> bool {
    const NAMES: [&str; 12] = [
        "pnpm-lock.yaml",
        "package-lock.json",
        "npm-shrinkwrap.json",
        "yarn.lock",
        "bun.lockb",
        "go.sum",
        "Pipfile.lock",
        "packages.lock.json",
        "pubspec.lock",
        "composer.lock",
        "Gemfile.lock",
        "Podfile.lock",
    ];
    let name = path.rsplit('/').next().unwrap_or(path);
    NAMES.contains(&name)
        || [".lock", ".snap", ".map", ".pb.go", "_pb2.py"]
            .iter()
            .any(|end| name.ends_with(end))
        || name.contains(".min.")
        || name.contains(".generated.")
        || path
            .split('/')
            .rev()
            .skip(1)
            .any(|dir| matches!(dir, "dist" | "__snapshots__" | "node_modules" | "vendor"))
}

/// Where each file's diff starts in `patch`: at each "diff --git" line.
fn chunks(patch: &str) -> Vec<usize> {
    let mut starts = Vec::new();
    let mut at = 0;
    for line in patch.split_inclusive('\n') {
        if line.starts_with("diff --git ") {
            starts.push(at);
        }
        at += line.len();
    }
    starts
}

/// Each file's diff, in the list's order (git writes both in the same order). A type change
/// (file to symlink) is two diffs, a deletion and a creation. None when they don't line up, as
/// with a path git quotes for its odd characters.
fn pair<'a>(files: &[FileChange], patch: &'a str) -> Option<Vec<&'a str>> {
    let starts = chunks(patch);
    let end = |i: usize| starts.get(i).copied().unwrap_or(patch.len());
    let mut diffs = Vec::with_capacity(files.len());
    let mut i = 0;
    for f in files {
        let n = if f.status == "T" { 2 } else { 1 };
        if i + n > starts.len() {
            return None;
        }
        let diff = &patch[starts[i]..end(i + n)];
        let header = diff.lines().next().unwrap_or("");
        if !header.ends_with(&format!(" b/{}", f.path)) {
            return None;
        }
        diffs.push(diff);
        i += n;
    }
    (i == starts.len()).then_some(diffs)
}

/// The names a diff's hunks change, as git writes them after `@@ … @@` (a function, a heading).
fn contexts(diff: &str) -> Vec<&str> {
    let mut out: Vec<&str> = Vec::new();
    for line in diff.lines().filter(|l| l.starts_with("@@ ")) {
        let name = line[3..].split_once(" @@").map_or("", |(_, n)| n.trim());
        if !name.is_empty() && !out.contains(&name) {
            out.push(name);
            if out.len() == MAX_CONTEXTS {
                break;
            }
        }
    }
    out
}

fn clip(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max - 1;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &s[..end])
}

fn entry(f: &FileChange, diff: &str, names: bool) -> String {
    let counts = match (f.additions, f.deletions) {
        (Some(a), Some(d)) => format!("+{a} -{d}"),
        _ => "binary".to_string(),
    };
    let mut line = format!("{} {counts} {}", f.status, f.path);
    if let Some(old) = &f.old_path {
        line.push_str(&format!(" ← {old}"));
    }
    if f.additions.is_none() {
        line.push_str(" [binary]");
    } else if is_generated(&f.path) {
        line.push_str(" [generated]");
    }
    let names: Vec<String> = if names { contexts(diff) } else { vec![] }
        .into_iter()
        .map(|n| clip(n, MAX_CONTEXT))
        .collect();
    if !names.is_empty() {
        line.push_str(&format!(" — @@ {}", names.join(" · ")));
    }
    line
}

/// The list and the diffs for `files`, from `patch`, in `room` bytes or a little under. The list
/// comes first and stays whole up to MAX_LIST; the diffs that fit are whole ones, smallest first,
/// so a model without file tools sees as many files as it can whole; the rest it knows by their
/// line counts and hunk names. Generated and binary files' diffs are never inlined.
pub fn build(files: &[FileChange], patch: &str, room: usize) -> Manifest {
    let paired = pair(files, patch);
    let diff = |i: usize| paired.as_ref().map_or("", |d| d[i]);
    let lines = |names: bool| -> Vec<String> {
        (0..files.len())
            .map(|i| entry(&files[i], diff(i), names))
            .collect()
    };
    let readable = |i: usize| files[i].additions.is_some() && !is_generated(&files[i].path);
    // A line's size with room for the tag it could get.
    let sizes = |list: &[String]| -> Vec<usize> {
        (0..list.len())
            .map(|i| list[i].len() + 1 + if readable(i) { FILE_ONLY.len() } else { 0 })
            .collect()
    };
    let mut list = lines(true);
    if sizes(&list).iter().sum::<usize>() > MAX_LIST {
        list = lines(false);
    }
    let mut listed = list.len();
    let mut total = 0;
    for (i, size) in sizes(&list).into_iter().enumerate() {
        if total + size > MAX_LIST {
            listed = i;
            break;
        }
        total += size;
    }
    list.truncate(listed);

    let reading = match &paired {
        Some(d) => (0..files.len())
            .filter(|&i| readable(i))
            .map(|i| d[i].len())
            .sum(),
        None => patch.len(),
    };
    let Some(diffs) = paired else {
        // Room for the list as it is, then the patch's start.
        let mut end = room.saturating_sub(total).min(patch.len());
        while !patch.is_char_boundary(end) {
            end -= 1;
        }
        let end = patch[..end].rfind('\n').map_or(0, |i| i + 1);
        return Manifest {
            list: list.join("\n"),
            listed,
            inline: patch[..end].to_string(),
            file_only: files.len(),
            reading,
            prefix: true,
        };
    };

    let candidates: Vec<usize> = (0..listed).filter(|&i| readable(i)).collect();
    let mut budget = room.saturating_sub(total);
    let mut by_size = candidates.clone();
    by_size.sort_by_key(|&i| diffs[i].len());
    let mut inlined = vec![false; files.len()];
    for i in by_size {
        if diffs[i].len() > budget {
            break;
        }
        budget -= diffs[i].len();
        inlined[i] = true;
    }
    let mut file_only = 0;
    for &i in &candidates {
        if !inlined[i] {
            list[i].push_str(FILE_ONLY);
            file_only += 1;
        }
    }
    // Past the list's cut, each readable file is only in the patch file too.
    file_only += (listed..files.len()).filter(|&i| readable(i)).count();
    let inline = (0..files.len())
        .filter(|&i| inlined[i])
        .map(|i| diffs[i])
        .collect();
    Manifest {
        list: list.join("\n"),
        listed,
        inline,
        file_only,
        reading,
        prefix: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str, status: &str, counts: Option<(u32, u32)>) -> FileChange {
        let mut f = crate::git::change(path, None, status.chars().next().unwrap());
        (f.additions, f.deletions) = counts.map_or((None, None), |(a, d)| (Some(a), Some(d)));
        f
    }

    fn diff(path: &str, body: &str) -> String {
        format!(
            "diff --git a/{path} b/{path}\nindex 1..2 100644\n--- a/{path}\n+++ b/{path}\n{body}"
        )
    }

    #[test]
    fn generated_files_are_known_by_name_and_folder() {
        for p in [
            "pnpm-lock.yaml",
            "web/package-lock.json",
            "Cargo.lock",
            "app/flake.lock",
            "go.sum",
            "static/app.min.js",
            "api/client.generated.ts",
            "src/__snapshots__/view.test.ts.snap",
            "dist/index.js",
            "a/node_modules/x/index.js",
            "out.js.map",
        ] {
            assert!(is_generated(p), "{p}");
        }
        for p in [
            "src/lock.rs",
            "dist",
            "docs/dist.md",
            "src/minify.ts",
            "lockfile.md",
            "vendored.rs",
        ] {
            assert!(!is_generated(p), "{p}");
        }
    }

    #[test]
    fn the_list_has_counts_renames_tags_and_hunk_names() {
        let mut renamed = file("new/name.rs", "R", Some((1, 1)));
        renamed.old_path = Some("old/name.rs".into());
        let files = [
            file("src/app.ts", "M", Some((2, 1))),
            file("logo.png", "M", None),
            file("pnpm-lock.yaml", "M", Some((900, 800))),
            renamed,
        ];
        let patch = [
            diff(
                "src/app.ts",
                "@@ -1,2 +1,3 @@ function start()\n-a\n+b\n+c\n@@ -9 +10 @@ function start()\n x\n@@ -20 +21 @@ class Store {\n x\n",
            ),
            "diff --git a/logo.png b/logo.png\nBinary files a/logo.png and b/logo.png differ\n".into(),
            diff("pnpm-lock.yaml", &"+dep\n".repeat(900)),
            "diff --git a/old/name.rs b/new/name.rs\nsimilarity index 90%\n".into(),
        ]
        .concat();
        let m = build(&files, &patch, 400 * 1024);
        let list: Vec<&str> = m.list.lines().collect();
        assert_eq!(
            list,
            [
                "M +2 -1 src/app.ts — @@ function start() · class Store {",
                "M binary logo.png [binary]",
                "M +900 -800 pnpm-lock.yaml [generated]",
                "R +1 -1 new/name.rs ← old/name.rs",
            ]
        );
        // The lockfile waits in the patch file; the rest is inline, in the list's order.
        assert!(!m.inline.contains("+dep") && !m.prefix);
        assert!(m.inline.starts_with("diff --git a/src/app.ts"));
        assert!(m.inline.ends_with("similarity index 90%\n"));
        assert_eq!(m.file_only, 0);
        assert_eq!(m.listed, 4);
    }

    #[test]
    fn diffs_that_fit_are_whole_and_smallest_first() {
        let files: Vec<FileChange> = [("a.rs", 400), ("b.rs", 10), ("c.rs", 50), ("d.rs", 20)]
            .iter()
            .map(|(p, n)| file(p, "M", Some((*n, 0))))
            .collect();
        let patch: String = [("a.rs", 400), ("b.rs", 10), ("c.rs", 50), ("d.rs", 20)]
            .iter()
            .map(|(p, n)| diff(p, &format!("@@ -1 +1,{n} @@\n{}", "+line\n".repeat(*n))))
            .collect();
        // The list (~110 bytes with room for tags) plus b, d and c (~720), not a.
        let m = build(&files, &patch, 900);
        assert!(m.inline.len() + m.list.len() <= 900, "{}", m.inline.len());
        let order: Vec<&str> = m
            .inline
            .lines()
            .filter_map(|l| l.strip_prefix("diff --git a/"))
            .collect();
        assert_eq!(order, ["b.rs b/b.rs", "c.rs b/c.rs", "d.rs b/d.rs"]);
        assert!(m.list.contains("a.rs [file only]") && !m.list.contains("b.rs [file only]"));
        assert_eq!(m.file_only, 1);
        // No room: every diff is file only, and the list is still whole.
        let m = build(&files, &patch, 10);
        assert_eq!((m.inline.as_str(), m.file_only, m.listed), ("", 4, 4));
    }

    #[test]
    fn a_type_change_is_two_diffs_and_a_mismatch_falls_back_to_the_start() {
        let files = [
            file("x", "T", Some((1, 1))),
            file("y.txt", "M", Some((1, 0))),
        ];
        let patch = [
            "diff --git a/x b/x\ndeleted file mode 100644\n@@ -1 +0,0 @@\n-a\n",
            "diff --git a/x b/x\nnew file mode 120000\n@@ -0,0 +1 @@\n+y\n",
            &diff("y.txt", "@@ -0,0 +1 @@\n+y\n"),
        ]
        .concat();
        let m = build(&files, &patch, 4096);
        assert!(!m.prefix && m.inline == patch);

        // A path git quotes ("tab\there") doesn't line up: the patch's start goes instead.
        let files = [file("tab\there", "M", Some((1, 0)))];
        let patch = diff("\"tab\\there\"", "@@ -0,0 +1 @@\n+y\n").replace("b/\"", "\"b/");
        let m = build(&files, &patch, 4096);
        assert!(m.prefix && m.inline == patch);
        let m = build(&files, &patch, 100);
        assert!(
            !m.inline.is_empty() && m.inline.len() < patch.len() && m.inline.ends_with('\n'),
            "{:?}",
            m.inline
        );
    }

    #[test]
    fn a_huge_list_drops_hunk_names_then_is_cut() {
        let files: Vec<FileChange> = (0..3000)
            .map(|i| {
                file(
                    &format!("packages/module/src/file_{i:05}.ts"),
                    "M",
                    Some((1, 0)),
                )
            })
            .collect();
        let patch: String = files
            .iter()
            .map(|f| {
                diff(
                    &f.path,
                    "@@ -1 +1 @@ export function aFairlyLongFunctionName()\n+x\n",
                )
            })
            .collect();
        let m = build(&files, &patch, 400 * 1024);
        assert!(
            !m.list.contains("@@") && m.listed == 3000,
            "{}",
            m.list.len()
        );
        let files: Vec<FileChange> = (0..9000)
            .map(|i| {
                file(
                    &format!("packages/module/src/file_{i:05}.ts"),
                    "M",
                    Some((1, 0)),
                )
            })
            .collect();
        let patch: String = files.iter().map(|f| diff(&f.path, "+x\n")).collect();
        let m = build(&files, &patch, 400 * 1024);
        assert!(m.list.len() <= MAX_LIST && m.listed < 9000);
        assert!(m.file_only >= 9000 - m.listed);
    }

    #[test]
    fn source_folders_named_like_build_output_are_not_generated() {
        for p in [
            "src/generated-ui/Button.tsx",
            "src/codegen/emit.rs",
            "web/src/minimap.ts",
            "src/lockscreen/View.swift",
        ] {
            assert!(!is_generated(p), "{p}");
        }
    }

    #[test]
    #[ignore = "a `vendor` or `dist` folder anywhere in the path tags hand-written app code [generated]"]
    fn app_code_in_a_vendor_or_dist_folder_is_not_generated() {
        for p in [
            "src/pages/vendor/Profile.tsx",
            "app/services/vendor/payout.rb",
            "src/features/dist/Calculator.ts",
        ] {
            assert!(!is_generated(p), "{p}");
        }
    }
}
