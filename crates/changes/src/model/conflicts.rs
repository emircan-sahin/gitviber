//! The conflict view as data: the choices made and the file they make, each block's merge
//! base, what a conflict with no blocks offers, the prompt that hands the conflicts to an agent,
//! and the merge tool by the name its app goes by.

use super::text::{Block, Parsed, Segment};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pick {
    Ours,
    Theirs,
    Both,
    /// Written by hand.
    Custom,
}

/// What a block was resolved to, and the lines that make.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    pub kind: Pick,
    pub lines: Vec<String>,
}

impl Choice {
    pub fn of(block: &Block, kind: Pick) -> Choice {
        let lines = match kind {
            Pick::Ours => block.ours.clone(),
            Pick::Theirs => block.theirs.clone(),
            Pick::Both | Pick::Custom => block.ours.iter().chain(&block.theirs).cloned().collect(),
        };
        Choice { kind, lines }
    }

    pub fn custom(text: &str) -> Choice {
        Choice {
            kind: Pick::Custom,
            lines: text.split('\n').map(str::to_string).collect(),
        }
    }
}

/// The two sides as the user knows them: in a rebase HEAD is the branch being rebased onto, and
/// the incoming side is their own commit being replayed.
pub fn side_names(rebase: bool) -> (&'static str, &'static str) {
    if rebase {
        ("Current (base)", "Incoming (your commit)")
    } else {
        ("Current", "Incoming")
    }
}

/// The file's lines as the choices make it; a block not chosen yet keeps its markers (the
/// preview shows them). A CRLF file's lines written by hand get their `\r` back.
pub fn result_lines(parsed: &Parsed, choices: &HashMap<usize, Choice>, crlf: bool) -> Vec<String> {
    let mut out = vec![];
    let mut block = 0;
    for seg in &parsed.segments {
        match seg {
            Segment::Text(lines) => out.extend(lines.iter().cloned()),
            Segment::Conflict(b) => {
                match choices.get(&block) {
                    None => {
                        out.push("<<<<<<< current".into());
                        out.extend(b.ours.iter().cloned());
                        out.push("=======".into());
                        out.extend(b.theirs.iter().cloned());
                        out.push(">>>>>>> incoming".into());
                    }
                    Some(c) if c.kind == Pick::Custom && crlf => {
                        out.extend(c.lines.iter().map(|l| {
                            if l.ends_with('\r') {
                                l.clone()
                            } else {
                                format!("{l}\r")
                            }
                        }))
                    }
                    Some(c) => out.extend(c.lines.iter().cloned()),
                }
                block += 1;
            }
        }
    }
    out
}

/// Whether the file the choices make ends with a newline. Git ends a closing marker line with
/// one even where neither side's file did, so for a conflict at the very end it comes from the
/// sides (`sides`: whether ours and theirs end with one, None while unknown): the one chosen, the
/// incoming one for both (it comes last), either for an edit by hand.
pub fn ends_with_newline(parsed: &Parsed, last: Option<Pick>, sides: Option<(bool, bool)>) -> bool {
    let ends_in_conflict = matches!(parsed.segments.last(), Some(Segment::Conflict(_)));
    match (ends_in_conflict, last, sides) {
        (true, Some(kind), Some((ours, theirs))) => match kind {
            Pick::Ours => ours,
            Pick::Custom => ours || theirs,
            Pick::Theirs | Pick::Both => theirs,
        },
        _ => parsed.trailing_newline,
    }
}

/// Most of its lines end with `\r`: a text box gives them back without it.
pub fn is_crlf(text: &str) -> bool {
    let lines = text.split('\n');
    let total = lines.clone().count();
    lines.filter(|l| l.ends_with('\r')).count() * 2 > total
}

/// Text to paste into a terminal: nothing in it ends a bracketed paste or acts as a key, and no
/// Enter at the end.
pub fn for_terminal(text: &str) -> String {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let clean: String = text
        .chars()
        .filter(|&c| !(c.is_ascii_control() && c != '\t' && c != '\n'))
        .collect();
    clean.trim_end_matches('\n').to_string()
}

/// A conflict without blocks to choose in (a deleted side, a binary, markers fixed or broken by
/// hand): a side of the whole file to keep, or the file marked resolved as it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WholeFile {
    pub title: &'static str,
    pub note: String,
    /// "Mark resolved as it is" makes sense: the file holds no markers to resolve, or can't be
    /// edited here.
    pub as_is: bool,
    /// Ours, then theirs: the button's label and what that side did.
    pub sides: [(&'static str, &'static str); 2],
}

/// `code`: git's two-letter conflict status (UU, AA, UD, DU, AU, UA, DD).
pub fn whole_file(code: &str, lossy: bool, unterminated: bool, rebase: bool) -> WholeFile {
    let (ours, theirs) = match code {
        "UD" => (
            "Modified on the current side",
            "Deleted on the incoming side",
        ),
        "DU" => (
            "Deleted on the current side",
            "Modified on the incoming side",
        ),
        "AU" => ("Added on the current side", "Not on the incoming side"),
        "UA" => ("Not on the current side", "Added on the incoming side"),
        "DD" => ("Deleted on both sides", "Deleted on both sides"),
        _ => ("Current version", "Incoming version"),
    };
    let b = code.as_bytes();
    let (o, t) = (
        b.first().copied().unwrap_or(b'U'),
        b.get(1).copied().unwrap_or(b'U'),
    );
    // A side "deletes" the file if it removed it (D) or never had it (U next to the other's A).
    let deletes = |mine: u8, other: u8| mine == b'D' || (mine == b'U' && other == b'A');
    let label = |del: bool, keep: &'static str| if del { "Delete the file" } else { keep };
    let hand_merged = (code == "UU" || code == "AA") && !lossy && !unterminated;
    let (title, note) = if hand_merged {
        (
            "No conflict markers left",
            "The file was already merged by hand. Mark it resolved as it is, or replace it with one side.".to_string(),
        )
    } else if lossy {
        (
            "This conflict can't be resolved line by line",
            "The file isn't UTF-8, so it can't be edited here safely. Keep one side, or fix it in your editor and mark it resolved.".to_string(),
        )
    } else if unterminated {
        (
            "The conflict markers are incomplete",
            "Fix the markers in your editor, or keep one side.".to_string(),
        )
    } else {
        (
            "This conflict can't be resolved line by line",
            format!(
                "Choose which side of the file to keep{}.",
                if rebase {
                    " (in a rebase, \u{201c}incoming\u{201d} is your own commit)"
                } else {
                    ""
                }
            ),
        )
    };
    WholeFile {
        title,
        note,
        as_is: hand_merged || lossy || unterminated,
        sides: [
            (label(deletes(o, t), "Keep current"), ours),
            (label(deletes(t, o), "Keep incoming"), theirs),
        ],
    }
}

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

/// Why Mark resolved asks about a file both sides changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unsure {
    /// It still has conflict markers.
    Markers,
    /// It can't be checked: a read error, too large to read, an LFS object not downloaded.
    Unread,
}

/// What Mark resolved makes of a file read as it is now; None when there's nothing to ask (no
/// markers, or deleted or binary: git writes no markers into those).
pub fn unsure(read: &Result<gitviber_core::git::FileText, String>) -> Option<Unsure> {
    let Ok(t) = read else {
        return Some(Unsure::Unread);
    };
    if !t.exists || t.binary {
        None
    } else if t.too_large || t.lfs_missing.is_some() {
        Some(Unsure::Unread)
    } else {
        super::text::has_conflict_markers(&t.text).then_some(Unsure::Markers)
    }
}

/// Mark resolved's question about the files with markers and those it couldn't check.
pub fn resolve_question(markers: &[String], unread: &[String]) -> String {
    let name = |list: &[String]| match list {
        [one] => one.clone(),
        _ => super::list::files(list.len()),
    };
    let mut said = vec![];
    if !markers.is_empty() {
        let has = if markers.len() == 1 { "has" } else { "have" };
        said.push(format!("{} still {has} conflict markers.", name(markers)));
    }
    if !unread.is_empty() {
        let what = if markers.is_empty() {
            "conflict markers"
        } else {
            "them"
        };
        said.push(format!(
            "{} couldn't be read to check for {what}.",
            name(unread)
        ));
    }
    let it = if markers.len() + unread.len() == 1 {
        "it"
    } else {
        "them"
    };
    format!("{} Mark {it} resolved anyway?", said.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::text::{Segment, parse_conflicts};

    #[test]
    fn mark_resolved_asks_about_markers_and_what_it_cant_read() {
        let text = |text: &str| gitviber_core::git::FileText {
            text: text.into(),
            exists: true,
            ..Default::default()
        };
        assert_eq!(
            unsure(&Ok(text("a\n<<<<<<< HEAD\nb\n=======\nc\n>>>>>>> x\n"))),
            Some(Unsure::Markers)
        );
        assert_eq!(unsure(&Ok(text("a\n"))), None);
        assert_eq!(
            unsure(&Err("Permission denied".into())),
            Some(Unsure::Unread)
        );
        let lfs = gitviber_core::git::FileText {
            lfs_missing: Some("12 MB".into()),
            ..text("version https://git-lfs.github.com/spec/v1\n")
        };
        assert_eq!(unsure(&Ok(lfs)), Some(Unsure::Unread));
        let large = gitviber_core::git::FileText {
            too_large: true,
            ..text("")
        };
        assert_eq!(unsure(&Ok(large)), Some(Unsure::Unread));
        let deleted = gitviber_core::git::FileText::default();
        assert_eq!(unsure(&Ok(deleted)), None);

        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(
            resolve_question(&s(&["a.rs"]), &[]),
            "a.rs still has conflict markers. Mark it resolved anyway?"
        );
        assert_eq!(
            resolve_question(&[], &s(&["big.bin"])),
            "big.bin couldn't be read to check for conflict markers. Mark it resolved anyway?"
        );
        assert_eq!(
            resolve_question(&s(&["a.rs", "b.rs"]), &s(&["c.psd"])),
            "2 files still have conflict markers. c.psd couldn't be read to check for them. Mark them resolved anyway?"
        );
    }

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
    fn zdiff3_bases_stand_edited_blocks_get_none_and_a_deleted_side_fits_anywhere() {
        let z = "<<<<<<< HEAD\nx\n||||||| 1a2b3c4\no\n=======\ny\n>>>>>>> feat\n";
        assert_eq!(
            bases(z, "<<<<<<< c\nx\n||||||| b\nWRONG\n=======\ny\n>>>>>>> i\n"),
            [lines(&["o"])]
        );
        let edited = "<<<<<<< HEAD\nours, edited\n=======\ntheirs\n>>>>>>> feat\n";
        assert_eq!(
            bases(
                edited,
                "<<<<<<< c\nours\n||||||| b\nbase\n=======\ntheirs\n>>>>>>> i\n"
            ),
            [None]
        );
        assert_eq!(
            bases("<<<<<<< HEAD\na\n=======\nb\n>>>>>>> f\n", ""),
            [None]
        );
        let del = "<<<<<<< HEAD\n=======\nkept\n>>>>>>> f\n";
        let del_rebuilt = "<<<<<<< c\nother\n||||||| b\nb1\n=======\nunrelated\n>>>>>>> i\nmid\n<<<<<<< c\n||||||| b\nb2\n=======\nkept\n>>>>>>> i\n";
        assert_eq!(bases(del, del_rebuilt), [lines(&["b2"])]);
    }

    /// conflicts.test.ts: the closing marker as the file's last line, with no newline after it.
    #[test]
    fn a_conflict_at_the_end_without_a_newline_still_parses_and_gets_its_base() {
        let file = "a\n<<<<<<< HEAD\nlast-ours\n=======\nlast-theirs\n>>>>>>> feat";
        let parsed = parse_conflicts(file).unwrap();
        assert!(!parsed.trailing_newline);
        let Some(Segment::Conflict(b)) = parsed.segments.last() else {
            panic!("ends in a conflict")
        };
        assert_eq!(
            (b.theirs.as_slice(), b.theirs_label.as_str()),
            (&["last-theirs".to_string()][..], "feat")
        );
        let rebuilt = "a\n<<<<<<< current\nlast-ours\n||||||| base\nlast\n=======\nlast-theirs\n>>>>>>> incoming\n";
        assert_eq!(bases(file, rebuilt), [Some(vec!["last".to_string()])]);
    }

    #[test]
    fn a_conflict_at_the_end_keeps_the_files_own_last_newline_or_its_lack() {
        let parsed =
            parse_conflicts("a\n<<<<<<< HEAD\nend-ours\n=======\nend-theirs\n>>>>>>> feat\n")
                .unwrap();
        assert!(parsed.trailing_newline);
        for kind in [Pick::Ours, Pick::Theirs, Pick::Both, Pick::Custom] {
            assert!(
                !ends_with_newline(&parsed, Some(kind), Some((false, false))),
                "{kind:?}"
            );
        }
        let mixed = Some((true, false));
        assert!(ends_with_newline(&parsed, Some(Pick::Ours), mixed));
        assert!(!ends_with_newline(&parsed, Some(Pick::Theirs), mixed));
        assert!(!ends_with_newline(&parsed, Some(Pick::Both), mixed));
        assert!(ends_with_newline(&parsed, Some(Pick::Custom), mixed));
        assert!(ends_with_newline(&parsed, Some(Pick::Ours), None));
        let inner = parse_conflicts("<<<<<<< HEAD\nx\n=======\ny\n>>>>>>> feat\nlast").unwrap();
        assert!(!ends_with_newline(
            &inner,
            Some(Pick::Ours),
            Some((false, false))
        ));
    }

    #[test]
    fn the_choices_make_the_file_and_open_blocks_keep_markers() {
        let p = parse_conflicts("a\n<<<<<<< HEAD\nb\n=======\nc\n>>>>>>> main\nd\n<<<<<<< HEAD\ne\n=======\nf\n>>>>>>> main\n").unwrap();
        let blocks: Vec<&Block> = p
            .segments
            .iter()
            .filter_map(|s| match s {
                Segment::Conflict(b) => Some(b),
                _ => None,
            })
            .collect();
        let mut choices = HashMap::from([(0, Choice::of(blocks[0], Pick::Both))]);
        assert_eq!(
            result_lines(&p, &choices, false),
            [
                "a",
                "b",
                "c",
                "d",
                "<<<<<<< current",
                "e",
                "=======",
                "f",
                ">>>>>>> incoming"
            ]
        );
        choices.insert(1, Choice::custom("E\nF"));
        assert_eq!(
            result_lines(&p, &choices, true),
            ["a", "b", "c", "d", "E\r", "F\r"]
        );
        assert!(is_crlf("a\r\nb\r\nc") && !is_crlf("a\nb\r\nc\n"));
    }

    #[test]
    fn a_conflict_without_blocks_offers_a_side_of_the_whole_file() {
        let ud = whole_file("UD", false, false, false);
        assert_eq!(
            ud.sides,
            [
                ("Keep current", "Modified on the current side"),
                ("Delete the file", "Deleted on the incoming side")
            ]
        );
        assert!(!ud.as_is);
        assert_eq!(
            whole_file("UA", false, false, false).sides[0].0,
            "Delete the file"
        );
        let merged = whole_file("UU", false, false, false);
        assert_eq!(merged.title, "No conflict markers left");
        assert!(merged.as_is);
        assert!(whole_file("UU", false, true, false).as_is);
        assert!(
            whole_file("DU", false, false, true)
                .note
                .contains("your own commit")
        );
        assert_eq!(
            side_names(true),
            ("Current (base)", "Incoming (your commit)")
        );
    }

    #[test]
    fn the_prompt_pasted_into_a_terminal_has_no_control_characters_and_no_enter() {
        let names: Vec<String> = [
            "it's \"quoted\".ts",
            "back`tick`$(id).ts",
            "new\nline.ts",
            "esc\x1b[201~\rrun.ts",
            "trail\n",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let pasted = for_terminal(&resolve_prompt(
            &names,
            merging_what(Some("merge"), None, Some("main"), Some("feat\x1b]0;x\x07")).as_deref(),
        ));
        assert!(
            !pasted
                .chars()
                .any(|c| c.is_ascii_control() && c != '\t' && c != '\n')
        );
        assert!(!pasted.ends_with('\n'));
        assert!(
            pasted.starts_with("Resolve the merge conflicts in ")
                && pasted.contains("back`tick`$(id).ts")
        );
        assert_eq!(
            merging_what(
                Some("revert"),
                None,
                Some("main"),
                Some("parent of 1a2b3c4 (Fix)")
            )
            .as_deref(),
            Some("reverting parent of 1a2b3c4 (Fix)")
        );
        assert_eq!(merging_what(None, None, Some("main"), Some("x")), None);
        assert_eq!(
            merging_what(Some("merge"), None, None, None).as_deref(),
            Some("merging the incoming branch into HEAD")
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
