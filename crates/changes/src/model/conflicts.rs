//! What the conflict view says beside the blocks: each block's merge base, the prompt that
//! hands the conflicts to an agent, and the merge tool by the name its app goes by.

use super::text::Block;

fn same_line(a: &str, b: &str) -> bool {
    a.strip_suffix('\r').unwrap_or(a) == b.strip_suffix('\r').unwrap_or(b)
}

fn same_run(a: &[String], b: &[String]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| same_line(x, y))
}

/// Where `inner` first runs as lines within `outer` at or after `from`.
fn run_at(outer: &[String], inner: &[String], from: usize) -> Option<usize> {
    (from..=outer.len().checked_sub(inner.len())?).find(|&i| {
        inner
            .iter()
            .enumerate()
            .all(|(j, l)| same_line(&outer[i + j], l))
    })
}

/// Each block's base: its own (diff3 style wrote one), else that of the block in `rebuilt` (the
/// file merged again in diff3 style, from its index stages) that holds both its sides. Git's
/// default style trims and splits the conflicts diff3 style keeps whole, so a block can be a
/// part of one, or joins two a line or so apart. None for a block edited since.
pub fn bases_for(blocks: &[&Block], rebuilt: &[&Block]) -> Vec<Option<Vec<String>>> {
    // Walked in file order, only forward from where the last block was found: twins (the same
    // edit in two places) each find their own, and two parts split from one find it in turn.
    let (mut at, mut ours, mut theirs) = (0, 0, 0);
    blocks
        .iter()
        .map(|b| {
            if let Some(base) = &b.base {
                return Some(base.clone());
            }
            for (i, r) in rebuilt.iter().enumerate().skip(at) {
                let (fo, ft) = if i == at { (ours, theirs) } else { (0, 0) };
                let Some(o) = run_at(&r.ours, &b.ours, fo) else {
                    continue;
                };
                let Some(t) = run_at(&r.theirs, &b.theirs, ft) else {
                    continue;
                };
                (at, ours, theirs) = (i, o + b.ours.len().max(1), t + b.theirs.len().max(1));
                return r.base.clone();
            }
            // Git's default style also joins conflicts a line or two apart that diff3 style
            // keeps as two: such a block is those rebuilt ones with the lines between them.
            let (base, last) =
                (at..rebuilt.len().saturating_sub(1)).find_map(|i| joined_from(b, rebuilt, i))?;
            (at, ours, theirs) = (last, rebuilt[last].ours.len(), rebuilt[last].theirs.len());
            Some(base)
        })
        .collect()
}

/// The base of `b` if it is `rebuilt[from]` and the blocks after it, with the lines both sides
/// share between them.
fn joined_from(b: &Block, rebuilt: &[&Block], from: usize) -> Option<(Vec<String>, usize)> {
    let mut base = vec![];
    let (mut o, mut t) = (0, 0);
    let slice = |v: &[String], at: usize, n: usize| v.get(at..at + n).map(<[String]>::to_vec);
    for k in from..rebuilt.len() {
        let r = rebuilt[k];
        let r_base = r.base.as_ref()?;
        if !slice(&b.ours, o, r.ours.len()).is_some_and(|s| same_run(&s, &r.ours))
            || !slice(&b.theirs, t, r.theirs.len()).is_some_and(|s| same_run(&s, &r.theirs))
        {
            return None;
        }
        base.extend(r_base.iter().cloned());
        o += r.ours.len();
        t += r.theirs.len();
        if o == b.ours.len() && t == b.theirs.len() {
            return (k > from).then_some((base, k));
        }
        // What lies between is the same in both sides; how much, the next block's start tells.
        let next = rebuilt.get(k + 1)?;
        let room = (b.ours.len() - o).min(b.theirs.len() - t);
        let between = (0..=room).find(|&n| {
            next.base.is_some()
                && same_run(&b.ours[o..o + n], &b.theirs[t..t + n])
                && slice(&b.ours, o + n, next.ours.len()).is_some_and(|s| same_run(&s, &next.ours))
                && slice(&b.theirs, t + n, next.theirs.len())
                    .is_some_and(|s| same_run(&s, &next.theirs))
        })?;
        base.extend(b.ours[o..o + between].iter().cloned());
        o += between;
        t += between;
    }
    None
}

/// What's being merged, in an agent's words; `theirs`: the incoming side's marker label.
pub fn merging_what(
    kind: Option<&str>,
    subject: Option<&str>,
    branch: Option<&str>,
    theirs: Option<&str>,
) -> Option<String> {
    let theirs = theirs.filter(|t| !t.is_empty());
    Some(match kind? {
        "merge" => format!(
            "merging {} into {}",
            theirs.unwrap_or("the incoming branch"),
            branch.unwrap_or("HEAD")
        ),
        "rebase" => format!(
            "rebasing {}{}",
            subject.unwrap_or("the branch"),
            theirs
                .map(|t| format!(", replaying {t}"))
                .unwrap_or_default()
        ),
        "cherry-pick" => format!("cherry-picking {}", theirs.unwrap_or("a commit")),
        "revert" => format!("reverting {}", theirs.unwrap_or("a commit")),
        _ => return None,
    })
}

/// What "Ask agent to resolve" hands the agent.
pub fn resolve_prompt(files: &[String], merging: Option<&str>) -> String {
    let what = merging.map(|m| format!(" ({m})")).unwrap_or_default();
    format!(
        "Resolve the merge conflicts in {}{what}; keep both sides' intent, then stage the files.",
        files.join(", ")
    )
}

/// git's names for its built-in tools, as the apps call themselves.
pub fn tool_name(tool: &str) -> &str {
    match tool {
        "araxis" => "Araxis Merge",
        "bc" | "bc3" | "bc4" => "Beyond Compare",
        "codecompare" => "Code Compare",
        "deltawalker" => "DeltaWalker",
        "diffmerge" => "DiffMerge",
        "gvimdiff" => "gVim",
        "kdiff3" => "KDiff3",
        "meld" => "Meld",
        "opendiff" => "FileMerge",
        "p4merge" => "P4Merge",
        "smerge" => "Sublime Merge",
        "tortoisemerge" => "TortoiseMerge",
        "vscode" => "VS Code",
        "winmerge" => "WinMerge",
        other => other,
    }
}

/// git mergetool reads the path through a shell that loses a `"`, a `\` or a tab in it.
pub fn tool_can_open(path: &str) -> bool {
    !path.contains(['"', '\\', '\t', '\n'])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::text::{Segment, parse_conflicts};

    fn conflicts(text: &str) -> Vec<Block> {
        parse_conflicts(text)
            .unwrap()
            .segments
            .into_iter()
            .filter_map(|s| match s {
                Segment::Conflict(b) => Some(b),
                Segment::Text(_) => None,
            })
            .collect()
    }

    fn bases(file: &str, rebuilt: &str) -> Vec<Option<Vec<String>>> {
        let (f, r) = (
            conflicts(file),
            if rebuilt.is_empty() {
                vec![]
            } else {
                conflicts(rebuilt)
            },
        );
        bases_for(&f.iter().collect::<Vec<_>>(), &r.iter().collect::<Vec<_>>())
    }

    fn lines(v: &[&str]) -> Option<Vec<String>> {
        Some(v.iter().map(|s| s.to_string()).collect())
    }

    // As conflicts.test.ts.
    #[test]
    fn each_conflict_gets_the_base_of_the_diff3_conflict_that_holds_its_sides() {
        let file = "a\n<<<<<<< HEAD\nB1\n=======\nb1\n>>>>>>> f\nx\n<<<<<<< HEAD\nB2\n=======\nb2\n>>>>>>> f\nz\n<<<<<<< HEAD\nedited\n=======\nq\n>>>>>>> f\n";
        let rebuilt = "a\n<<<<<<< current\nB1\nx\nB2\n||||||| base\nb\nx\nc\n=======\nb1\nx\nb2\n>>>>>>> incoming\nz\n<<<<<<< current\np\n||||||| base\no\n=======\nq\n>>>>>>> incoming\n";
        assert_eq!(
            bases(file, rebuilt),
            [lines(&["b", "x", "c"]), lines(&["b", "x", "c"]), None]
        );
        let own = "<<<<<<< HEAD\nm\n||||||| base\nmine\n=======\nt\n>>>>>>> f\n";
        assert_eq!(bases(own, ""), [lines(&["mine"])]);
        assert_eq!(
            bases(
                "<<<<<<< HEAD\r\nB\r\n=======\r\nb\r\n>>>>>>> f\r\n",
                "<<<<<<< c\nB\n||||||| base\no\n=======\nb\n>>>>>>> i\n"
            ),
            [lines(&["o"])]
        );
    }

    #[test]
    fn a_block_that_joins_two_diff3_conflicts_gets_their_bases_and_the_lines_between() {
        let file = "l1\n<<<<<<< HEAD\nl2 MAIN\nl3\nl4 MAIN\n=======\nl2 SIDE\nl3\n>>>>>>> side\n";
        let rebuilt = "l1\n<<<<<<< current\nl2 MAIN\n||||||| base\nl2 v3\n=======\nl2 SIDE\n>>>>>>> incoming\nl3\n<<<<<<< current\nl4 MAIN\n||||||| base\nl4\n=======\n>>>>>>> incoming\n";
        assert_eq!(bases(file, rebuilt), [lines(&["l2 v3", "l3", "l4"])]);
        let other = "<<<<<<< HEAD\nx\nmid\ny\n=======\nX\nmid\n>>>>>>> f\nz\n<<<<<<< HEAD\nq\n=======\nr\n>>>>>>> f\n";
        let other_rebuilt = "<<<<<<< c\nx\n||||||| b\nbx\n=======\nX\n>>>>>>> i\nmid\n<<<<<<< c\ny\n||||||| b\nby\n=======\n>>>>>>> i\nz\n<<<<<<< c\nq\n||||||| b\nbq\n=======\nr\n>>>>>>> i\n";
        assert_eq!(
            bases(other, other_rebuilt),
            [lines(&["bx", "mid", "by"]), lines(&["bq"])]
        );
        assert_eq!(
            bases(&file.replace("l2 SIDE\nl3\n", "l2 SIDE\nl8\n"), rebuilt),
            [None]
        );
    }

    #[test]
    fn two_conflicts_with_the_same_text_each_get_their_own_base() {
        let file = "fn a() {\n<<<<<<< HEAD\n  ours();\n=======\n  theirs();\n>>>>>>> feat\n}\n1\n2\n3\nfn b() {\n<<<<<<< HEAD\n  ours();\n=======\n  theirs();\n>>>>>>> feat\n}\n";
        let rebuilt = "fn a() {\n<<<<<<< current\n  ours();\n||||||| base\n  old_a();\n=======\n  theirs();\n>>>>>>> incoming\n}\n1\n2\n3\nfn b() {\n<<<<<<< current\n  ours();\n||||||| base\n  old_b();\n=======\n  theirs();\n>>>>>>> incoming\n}\n";
        assert_eq!(
            bases(file, rebuilt),
            [lines(&["  old_a();"]), lines(&["  old_b();"])]
        );
    }

    #[test]
    fn the_prompt_names_the_files_and_whats_being_merged() {
        let files = ["a.ts".to_string(), "b.ts".to_string()];
        assert_eq!(
            resolve_prompt(
                &files,
                merging_what(
                    Some("merge"),
                    Some("Merge branch 'feat'"),
                    Some("main"),
                    Some("feat")
                )
                .as_deref()
            ),
            "Resolve the merge conflicts in a.ts, b.ts (merging feat into main); keep both sides' intent, then stage the files."
        );
        assert_eq!(
            merging_what(
                Some("rebase"),
                Some("feat"),
                None,
                Some("1a2b3c4 (Add login)")
            )
            .as_deref(),
            Some("rebasing feat, replaying 1a2b3c4 (Add login)")
        );
        assert_eq!(
            merging_what(Some("cherry-pick"), None, Some("main"), None).as_deref(),
            Some("cherry-picking a commit")
        );
        assert_eq!(
            resolve_prompt(&files[..1], None),
            "Resolve the merge conflicts in a.ts; keep both sides' intent, then stage the files."
        );
        assert_eq!(tool_name("opendiff"), "FileMerge");
        assert_eq!(tool_name("mytool"), "mytool");
        assert!(tool_can_open("src/a b.ts") && !tool_can_open("a\"b"));
    }
}
