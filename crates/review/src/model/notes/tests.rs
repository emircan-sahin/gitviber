//! notes.test.ts, ported.

use super::*;

fn l(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

fn note() -> ReviewNote {
    ReviewNote {
        id: "1".into(),
        path: "src/a.ts".into(),
        body: "Use the helper here.".into(),
        anchor: Anchor {
            start: 2,
            end: 3,
            code: l(&["b", "c"]),
            ..Default::default()
        },
        ..Default::default()
    }
}

/// A note written on lines `start`–`end` of `lines`.
fn on(lines: &[String], start: usize, end: usize) -> ReviewNote {
    ReviewNote {
        id: "1".into(),
        path: "src/a.ts".into(),
        body: "x".into(),
        anchor: anchor_at(lines, start, end),
        ..Default::default()
    }
}

fn file() -> Vec<String> {
    l(&[
        "function a() {",
        "  if (x) {",
        "    return null;",
        "  }",
        "}",
        "",
        "function b() {",
        "  if (y) {",
        "    return null;",
        "  }",
        "}",
        "",
    ])
}

fn check(notes: &[ReviewNote], lines: &[String], prev: Option<&[String]>) -> Vec<ReviewNote> {
    check_notes(notes, "src/a.ts", Some(lines), prev).unwrap_or_else(|| notes.to_vec())
}

#[test]
fn a_note_keeps_the_lines_around_it() {
    assert_eq!(
        anchor_at(&file(), 3, 3),
        Anchor {
            start: 3,
            end: 3,
            code: l(&["    return null;"]),
            tail: None,
            hash: None,
            before: Some(l(&["function a() {", "  if (x) {"])),
            after: Some(l(&["  }", "}"])),
        }
    );
    assert_eq!(anchor_at(&file(), 1, 1).before, Some(vec![]));
    assert_eq!(anchor_at(&file(), 12, 12).after, Some(vec![]));
}

fn hundred() -> Vec<String> {
    (1..=100).map(|i| format!("l{i}")).collect()
}

#[test]
fn a_long_run_keeps_its_start_and_end_only() {
    let lines = hundred();
    let n = on(&lines, 1, 100);
    assert_eq!(n.anchor.code.len(), 20);
    assert_eq!(n.anchor.tail.as_ref().unwrap().last().unwrap(), "l100");
    let mut more = l(&["new"]);
    more.extend(lines.clone());
    assert_eq!(find_note(&more, &n.anchor, None), Some(2));
    let edited: Vec<String> = lines
        .iter()
        .map(|x| if x == "l100" { "x".into() } else { x.clone() })
        .collect();
    assert_eq!(find_note(&edited, &n.anchor, None), None);
    assert!(notes_prompt(&[n]).contains("\nl20\n⋯ 60 more lines ⋯\nl81\n"));
}

#[test]
fn a_long_run_rewritten_in_its_middle_turns_outdated() {
    let lines = hundred();
    let n = on(&lines, 1, 100);
    let edited: Vec<String> = lines
        .iter()
        .map(|x| if x == "l50" { "x".into() } else { x.clone() })
        .collect();
    assert_eq!(find_note(&edited, &n.anchor, None), None);
    assert!(check(std::slice::from_ref(&n), &edited, None)[0].outdated);
    assert!(n.anchor.hash.is_some());
    let mut older = n.clone();
    older.anchor.hash = None;
    assert_eq!(find_note(&edited, &older.anchor, None), Some(1));
}

#[test]
fn lines_inserted_between_two_same_looking_lines() {
    let n = on(&file(), 9, 9);
    let f = file();
    let mut edited = f[..6].to_vec();
    edited.extend((0..30).map(|i| format!("// {i}")));
    edited.extend(f[6..].iter().cloned());
    assert_eq!(find_note(&edited, &n.anchor, None), Some(39));
    let moved = check(std::slice::from_ref(&n), &edited, None);
    assert_eq!((moved[0].anchor.start, moved[0].outdated), (39, false));
}

#[test]
fn its_line_rewritten_while_a_same_looking_one_is_elsewhere() {
    let n = on(&file(), 9, 9);
    let edited: Vec<String> = file()
        .iter()
        .enumerate()
        .map(|(i, x)| {
            if i == 8 {
                "    return fallback;".into()
            } else {
                x.clone()
            }
        })
        .collect();
    assert_eq!(find_note(&edited, &n.anchor, None), None);
    assert!(check(std::slice::from_ref(&n), &edited, None)[0].outdated);
    assert!(check(std::slice::from_ref(&n), &edited, Some(&file()))[0].outdated);
}

#[test]
fn a_line_edited_right_next_to_it_keeps_it_where_it_was() {
    let n = on(&file(), 9, 9);
    let swap = |pairs: &[(usize, &str)]| -> Vec<String> {
        file()
            .iter()
            .enumerate()
            .map(|(i, x)| {
                pairs
                    .iter()
                    .find(|p| p.0 == i)
                    .map_or(x.clone(), |p| p.1.to_string())
            })
            .collect()
    };
    assert_eq!(
        find_note(&swap(&[(7, "  if (z) {")]), &n.anchor, None),
        Some(9)
    );
    let rewritten = swap(&[(7, "  if (z) {"), (9, "  } // done")]);
    assert_eq!(find_note(&rewritten, &n.anchor, None), Some(9));
    let mut moved = l(&["// moved"]);
    moved.extend(rewritten.clone());
    assert_eq!(find_note(&moved, &n.anchor, None), None);
    // Unless the edits say it moved there.
    assert_eq!(
        find_note(&moved, &n.anchor, Some(shifted(&file(), &moved, 9, 9))),
        Some(10)
    );
}

#[test]
fn the_lines_a_short_side_kept_at_the_files_start_still_have_to_match() {
    let n = on(&l(&["x", "a"]), 2, 2);
    assert_eq!(find_note(&l(&["x", "a", "more"]), &n.anchor, None), Some(2));
    assert_eq!(find_note(&l(&["new", "x", "a"]), &n.anchor, None), Some(3));
    assert_eq!(find_note(&l(&["y", "z", "a"]), &n.anchor, None), None);
}

#[test]
fn of_several_equal_places_the_nearest_to_where_its_expected() {
    let lines = l(&["a", "b", "c", "a", "b", "c", "a", "b", "c", "a", "b", "c"]);
    let n = on(&lines, 5, 5);
    assert_eq!(find_note(&lines, &n.anchor, None), Some(5));
    assert_eq!(
        find_note(
            &lines,
            &n.anchor,
            Some(Near {
                expect: 8.,
                lo: 0,
                hi: 99
            })
        ),
        Some(8)
    );
    // A tie in distance goes to the first.
    assert_eq!(
        find_note(
            &lines,
            &n.anchor,
            Some(Near {
                expect: 6.5,
                lo: 0,
                hi: 99
            })
        ),
        Some(5)
    );
    let abc = l(&["a", "b", "c"]);
    assert_eq!(
        find_note(&abc, &on(&abc, 3, 3).anchor, None),
        Some(3),
        "the last line"
    );
}

#[test]
fn notes_from_before_the_lines_around_were_kept() {
    let n = note();
    assert_eq!(find_note(&l(&["a", "b", "c"]), &n.anchor, None), Some(2));
    let mut later = n.anchor.clone();
    later.start = 5;
    later.end = 6;
    assert_eq!(
        find_note(&l(&["b", "c", "x", "x", "x", "b", "c"]), &later, None),
        Some(6)
    );
    let mut none = n.anchor.clone();
    none.code = vec![];
    assert_eq!(find_note(&l(&["a"]), &none, None), None);
}

#[test]
fn edits_move_a_notes_expected_place() {
    assert_eq!(
        shifted(
            &l(&["u", "x", "x"]),
            &l(&["new", "new", "u", "x", "x"]),
            3,
            3
        ),
        Near {
            expect: 5.,
            lo: 3,
            hi: 6
        }
    );
    assert_eq!(
        shifted(&l(&["x", "x", "v"]), &l(&["x", "x", "w", "v"]), 2, 2),
        Near {
            expect: 3.,
            lo: 0,
            hi: 4
        },
        "counted from below"
    );
    assert_eq!(
        shifted(&l(&["x", "", "x"]), &l(&["x", "", "x"]), 2, 2),
        Near {
            expect: 2.,
            lo: 0,
            hi: 4
        },
        "repeated and blank lines anchor nothing"
    );
    let prev = l(&["head", "a", "b", "c", "a", "b", "c"]);
    let next = l(&["head", "1", "2", "3", "a", "b", "c", "a", "b", "c"]);
    let moved = check(&[on(&prev, 3, 3)], &next, Some(&prev));
    assert_eq!((moved[0].anchor.start, moved[0].outdated), (6, false));
}

#[test]
fn live_notes_follow_their_lines_on_disk() {
    let notes = vec![note()];
    assert_eq!(
        check_notes(&notes, "src/a.ts", Some(&l(&["a", "b", "c"])), None),
        None
    );
    let moved = check(&notes, &l(&["new", "a", "b", "c"]), None);
    let mut want = note();
    want.anchor.start = 3;
    want.anchor.end = 4;
    assert_eq!(moved[0], want);
    let rewritten = check(&moved, &l(&["a", "b", "C"]), None);
    assert!(rewritten[0].outdated);
    assert_eq!(rewritten[0].anchor.start, 3);
    assert!(!check(&rewritten, &l(&["a", "b", "c"]), None)[0].outdated);
    assert!(check_notes(&notes, "src/a.ts", None, None).unwrap()[0].outdated);
}

#[test]
fn notes_on_other_files_old_sides_commits_and_resolved_ones_are_left() {
    let mut b = note();
    b.path = "b.ts".into();
    let mut old = note();
    old.old = true;
    let mut at = note();
    at.at = Some("commit 1a2b3c4".into());
    let mut done = note();
    done.resolved = true;
    assert_eq!(
        check_notes(&[b, old, at, done], "src/a.ts", Some(&l(&["x"])), None),
        None
    );
}

#[test]
fn a_file_on_show_places_its_open_notes() {
    let mut newer = note();
    newer.id = "new".into();
    newer.anchor.start = 5;
    newer.anchor.end = 6;
    let mut old = note();
    old.id = "old".into();
    old.path = "src/old.ts".into();
    old.old = true;
    old.anchor = Anchor {
        start: 1,
        end: 1,
        code: l(&["x"]),
        ..Default::default()
    };
    let mut done = note();
    done.id = "done".into();
    done.resolved = true;
    let mut other = note();
    other.id = "other".into();
    other.path = "c.ts".into();
    let (old_lines, new_lines) = (l(&["x"]), l(&["a", "b", "c"]));
    let placed = place_notes(
        &[newer.clone(), old, done, other],
        &Shown {
            path: "src/a.ts",
            old_path: "src/old.ts",
            old_lines: Some(&old_lines),
            new_lines: Some(&new_lines),
        },
    );
    let got: Vec<_> = placed
        .iter()
        .map(|p| (p.note.id.as_str(), p.old, p.start, p.end))
        .collect();
    assert_eq!(got, [("new", false, 2, 3), ("old", true, 1, 1)]);
    assert!(
        place_notes(
            &[newer],
            &Shown {
                path: "src/a.ts",
                old_path: "src/a.ts",
                old_lines: None,
                new_lines: None
            }
        )
        .is_empty()
    );
}

#[test]
fn the_prompt_gives_each_notes_place_its_code_fenced_and_the_comment() {
    assert_eq!(
        notes_prompt(&[note()]),
        "`src/a.ts:2-3`\n```ts\nb\nc\n```\nUse the helper here."
    );
    let mut first = note();
    first.anchor.start = 7;
    first.anchor.end = 7;
    first.anchor.code = l(&["x"]);
    first.body = "Why?\n".into();
    first.old = true;
    first.at = Some("commit 1a2b3c4".into());
    let mut second = note();
    second.path = "Makefile".into();
    second.outdated = true;
    assert_eq!(
        notes_prompt(&[first, second]),
        "`src/a.ts:7` (the old version; in commit 1a2b3c4)\n```ts\nx\n```\nWhy?\n\n`Makefile:2-3` (outdated: these lines have changed since)\n```\nb\nc\n```\nUse the helper here."
    );
}

#[test]
fn code_holding_backticks_gets_a_longer_fence() {
    let mut n = note();
    n.anchor.code = l(&["const s = ```;"]);
    assert!(notes_prompt(&[n]).contains("\n````ts\nconst s = ```;\n````\n"));
}

#[test]
fn text_for_the_terminal_loses_control_characters() {
    assert_eq!(
        for_terminal("a\r\nb\x1b[201~c\x07\tok\n\n"),
        "a\nb[201~c\tok"
    );
    assert_eq!(for_terminal("a\rb\x7fc\r"), "a\nbc");
}

#[test]
fn stored_notes_that_arent_notes_are_dropped() {
    let good = serde_json::to_value(note()).unwrap();
    assert!(ReviewNote::read(&good).is_some());
    let bare = serde_json::json!({ "id": "1", "path": "p", "body": "", "start": 1, "end": 1, "code": ["a"], "before": [], "after": [] });
    assert!(ReviewNote::read(&bare).is_some());
    let with = |k: &str, v: serde_json::Value| {
        let mut x = good.clone();
        x[k] = v;
        x
    };
    for bad in [
        serde_json::json!(null),
        serde_json::json!([]),
        serde_json::json!("x"),
        with("code", serde_json::json!("b")),
        with("start", serde_json::json!("2")),
        with("end", serde_json::json!(1)),
        with("before", serde_json::json!([1])),
        with("hash", serde_json::json!(1)),
        with("id", serde_json::Value::Null),
    ] {
        assert!(ReviewNote::read(&bad).is_none(), "{bad}");
    }
}

#[test]
fn the_hash_is_the_old_apps() {
    // FNV-1a of "", as the old app wrote it: 0x811c9dc5 in base 36.
    assert_eq!(hash(""), "ztntfp");
    assert_ne!(hash("a\nb"), hash("a\nc"));
}
