use super::*;
use crate::rewrite::{run as rewrite, Edit, Outcome};

fn head(r: &Path) -> String {
    run_text(r, &["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string()
}
/// Subjects, newest first.
fn log(r: &Path) -> Vec<String> {
    run_text(r, &["log", "--format=%s"])
        .unwrap()
        .lines()
        .map(str::to_string)
        .collect()
}
fn sha_of(r: &Path, subject: &str) -> String {
    run_text(
        r,
        &["log", "--format=%H", "--grep", &format!("^{subject}$")],
    )
    .unwrap()
    .trim()
    .to_string()
}
fn repo(name: &str) -> (Sandbox, PathBuf) {
    let sb = Sandbox::new(name);
    let r = sb.path("r");
    init(&r);
    for (f, s) in [("a", "one"), ("b", "two"), ("c", "three"), ("d", "four")] {
        write_commit(&r, &format!("{f}.txt"), &format!("{f}\n"), s);
    }
    (sb, r)
}

#[test]
fn reword_the_newest_and_an_older_commit() {
    let (_sb, r) = repo("rw-reword");
    // HEAD: an amend that leaves staged changes alone.
    fs::write(r.join("a.txt"), "staged\n").unwrap();
    stage(&r, &["a.txt".into()]).unwrap();
    rewrite(
        &r,
        &head(&r),
        &Edit::Reword {
            sha: head(&r),
            message: "four, reworded".into(),
        },
    )
    .unwrap();
    assert_eq!(log(&r), ["four, reworded", "three", "two", "one"]);
    assert_eq!(
        run_text(&r, &["diff", "--cached", "--name-only"]).unwrap(),
        "a.txt\n"
    );
    // An older one, with a body; the staged change rides along (autostash).
    let two = sha_of(&r, "two");
    rewrite(
        &r,
        &head(&r),
        &Edit::Reword {
            sha: two,
            message: "two, better\n\nWith a body.".into(),
        },
    )
    .unwrap();
    assert_eq!(log(&r), ["four, reworded", "three", "two, better", "one"]);
    assert_eq!(
        run_text(&r, &["log", "-1", "--skip=2", "--format=%b"])
            .unwrap()
            .trim(),
        "With a body."
    );
    assert_eq!(fs::read_to_string(r.join("a.txt")).unwrap(), "staged\n");
    // The root commit too.
    let one = sha_of(&r, "one");
    rewrite(
        &r,
        &head(&r),
        &Edit::Reword {
            sha: one,
            message: "first".into(),
        },
    )
    .unwrap();
    assert_eq!(log(&r).last().unwrap(), "first");
    assert!(rewrite(
        &r,
        &head(&r),
        &Edit::Reword {
            sha: head(&r),
            message: "  ".into()
        }
    )
    .is_err());
}

#[test]
fn squash_fixup_drop_and_move() {
    let (_sb, r) = repo("rw-edit");
    let three = sha_of(&r, "three");
    rewrite(
        &r,
        &head(&r),
        &Edit::Squash {
            shas: vec![three],
            onto: sha_of(&r, "two"),
            message: Some("two and three".into()),
        },
    )
    .unwrap();
    assert_eq!(log(&r), ["four", "two and three", "one"]);
    assert!(r.join("b.txt").exists() && r.join("c.txt").exists());
    let four = sha_of(&r, "four");
    rewrite(
        &r,
        &head(&r),
        &Edit::Squash {
            shas: vec![four],
            onto: sha_of(&r, "two and three"),
            message: None,
        },
    )
    .unwrap();
    assert_eq!(log(&r), ["two and three", "one"]);
    assert!(r.join("d.txt").exists());

    let (_sb, r) = repo("rw-drop");
    let two = sha_of(&r, "two");
    rewrite(&r, &head(&r), &Edit::Drop { shas: vec![two] }).unwrap();
    assert_eq!(log(&r), ["four", "three", "one"]);
    assert!(!r.join("b.txt").exists());
    let one = sha_of(&r, "one");
    rewrite(
        &r,
        &head(&r),
        &Edit::Move {
            sha: one.clone(),
            up: true,
        },
    )
    .unwrap();
    assert_eq!(log(&r), ["four", "one", "three"]);
    let four = sha_of(&r, "four");
    rewrite(
        &r,
        &head(&r),
        &Edit::Move {
            sha: four,
            up: false,
        },
    )
    .unwrap();
    assert_eq!(log(&r), ["one", "four", "three"]);
    assert!(rewrite(
        &r,
        &head(&r),
        &Edit::Move {
            sha: head(&r),
            up: true
        }
    )
    .is_err());
    let first = sha_of(&r, "three");
    assert!(rewrite(
        &r,
        &head(&r),
        &Edit::Squash {
            shas: vec![first.clone()],
            onto: first,
            message: None
        }
    )
    .is_err());
}

#[test]
fn a_stale_view_merges_and_conflicts() {
    let (_sb, r) = repo("rw-guard");
    let two = sha_of(&r, "two");
    // HEAD moved since the history was shown.
    assert!(rewrite(
        &r,
        &two,
        &Edit::Drop {
            shas: vec![two.clone()]
        }
    )
    .unwrap_err()
    .contains("HEAD has moved"));
    // Two commits editing one line can't swap cleanly: the rebase stops.
    write_commit(&r, "a.txt", "a2\n", "five");
    write_commit(&r, "a.txt", "a3\n", "six");
    let six = sha_of(&r, "six");
    assert_eq!(
        rewrite(
            &r,
            &head(&r),
            &Edit::Move {
                sha: six,
                up: false
            }
        )
        .unwrap(),
        Outcome::Conflicts
    );
    assert!(operation(&r).is_some_and(|o| o.kind == "rebase"));
    op_abort(&r).unwrap();
    assert_eq!(log(&r)[..2], ["six", "five"]);
    // A merge in the way.
    run(&r, &["switch", "-q", "-c", "side", "HEAD~2"]).unwrap();
    write_commit(&r, "e.txt", "e\n", "side");
    run(&r, &["switch", "-q", "main"]).unwrap();
    run(&r, &["merge", "-q", "--no-edit", "side"]).unwrap();
    let two = sha_of(&r, "two");
    assert!(rewrite(&r, &head(&r), &Edit::Drop { shas: vec![two] })
        .unwrap_err()
        .contains("merges"));
}

#[test]
fn squash_apart_keeps_the_oldest_author() {
    let sb = Sandbox::new("rw-many");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "a\n", "one");
    // "two" by someone else, written long ago.
    fs::write(r.join("b.txt"), "b\n").unwrap();
    stage(&r, &["b.txt".into()]).unwrap();
    let by = [
        "--author=Ada <ada@example.com>",
        "--date=2001-02-03T04:05:06Z",
    ];
    run(&r, &["commit", "-q", by[0], by[1], "-m", "two"]).unwrap();
    for (f, s) in [("c", "three"), ("d", "four"), ("e", "five")] {
        write_commit(&r, &format!("{f}.txt"), &format!("{f}\n"), s);
    }
    // Two goes up to four, past three; the oldest gives the author and date.
    rewrite(
        &r,
        &head(&r),
        &Edit::Squash {
            shas: vec![sha_of(&r, "two")],
            onto: sha_of(&r, "four"),
            message: Some("two and four\n\nBoth.".into()),
        },
    )
    .unwrap();
    assert_eq!(log(&r), ["five", "two and four", "three", "one"]);
    let squashed = sha_of(&r, "two and four");
    assert_eq!(
        run_text(
            &r,
            &["show", "--format=%an %at%n%b", "--name-only", &squashed]
        )
        .unwrap(),
        "Ada 981173106\nBoth.\n\n\nb.txt\nd.txt\n"
    );
    // Fixup: five comes down to three, whose message stays.
    rewrite(
        &r,
        &head(&r),
        &Edit::Squash {
            shas: vec![sha_of(&r, "five")],
            onto: sha_of(&r, "three"),
            message: None,
        },
    )
    .unwrap();
    assert_eq!(log(&r), ["two and four", "three", "one"]);
    assert_eq!(
        run_text(&r, &["show", "--format=", "--name-only", "HEAD~1"]).unwrap(),
        "c.txt\ne.txt\n"
    );
}

#[test]
fn drop_and_reorder_many() {
    let (_sb, r) = repo("rw-drop-many");
    let shas = |s: &[&str]| s.iter().map(|s| sha_of(&r, s)).collect::<Vec<_>>();
    rewrite(
        &r,
        &head(&r),
        &Edit::Drop {
            shas: shas(&["one", "three"]),
        },
    )
    .unwrap();
    assert_eq!(log(&r), ["four", "two"]);
    assert!(!r.join("a.txt").exists() && !r.join("c.txt").exists());

    let (_sb, r) = repo("rw-reorder");
    let shas = |s: &[&str]| s.iter().map(|s| sha_of(&r, s)).collect::<Vec<_>>();
    // Under the root commit, keeping their order.
    rewrite(
        &r,
        &head(&r),
        &Edit::Reorder {
            shas: shas(&["four", "two"]),
            before: Some(sha_of(&r, "one")),
        },
    )
    .unwrap();
    assert_eq!(log(&r), ["three", "one", "four", "two"]);
    rewrite(
        &r,
        &head(&r),
        &Edit::Reorder {
            shas: shas(&["two"]),
            before: None,
        },
    )
    .unwrap();
    assert_eq!(log(&r), ["two", "three", "one", "four"]);
    assert!(rewrite(
        &r,
        &head(&r),
        &Edit::Reorder {
            shas: shas(&["two"]),
            before: None,
        },
    )
    .unwrap_err()
    .contains("there already"));
}

#[test]
fn squash_apart_stops_on_conflicts() {
    let (_sb, r) = repo("rw-squash-conflict");
    write_commit(&r, "a.txt", "a2\n", "five");
    write_commit(&r, "a.txt", "a3\n", "six");
    write_commit(&r, "f.txt", "f\n", "seven");
    let before = log(&r);
    // Six comes down to four past five, which made the line it changes.
    assert_eq!(
        rewrite(
            &r,
            &head(&r),
            &Edit::Squash {
                shas: vec![sha_of(&r, "six")],
                onto: sha_of(&r, "four"),
                message: Some("four and six".into()),
            },
        )
        .unwrap(),
        Outcome::Conflicts
    );
    assert!(operation(&r).is_some_and(|o| o.kind == "rebase"));
    op_abort(&r).unwrap();
    assert_eq!(log(&r), before);
}

// Real-life histories: long branches, odd messages, user config and hooks, interrupted rebases.

/// Raw git for setups the app's own commands don't make.
fn git(r: &Path, args: &[&str]) -> String {
    run_text(r, args).unwrap()
}
/// A commit of `file` with exactly `message`: empty, `#` lines, trailers, anything.
fn commit_raw(r: &Path, file: &str, content: &str, message: &str) {
    fs::write(r.join(file), content).unwrap();
    run(r, &["add", "--", file]).unwrap();
    let args = [
        "commit",
        "-q",
        "--allow-empty-message",
        "--cleanup=verbatim",
        "-F",
        "-",
    ];
    run_with(r, &args, &[], Some(message.as_bytes())).unwrap();
}
/// The message as stored, past the headers.
fn raw_message(r: &Path, rev: &str) -> String {
    git(r, &["cat-file", "commit", rev])
        .split_once("\n\n")
        .map(|(_, m)| m.to_string())
        .unwrap_or_default()
}
fn tree(r: &Path) -> String {
    git(r, &["rev-parse", "HEAD^{tree}"])
}
/// `n` commits each adding its own file, subjects c0, c1, ...
fn many(r: &Path, n: usize) {
    for i in 0..n {
        let f = format!("f{i}.txt");
        fs::write(r.join(&f), format!("{i}\n")).unwrap();
        run(r, &["add", "--", &f]).unwrap();
        run(r, &["commit", "-q", "-m", &format!("c{i}")]).unwrap();
    }
}
/// Subject -> sha for the whole branch.
fn by_subject(r: &Path) -> std::collections::HashMap<String, String> {
    git(r, &["log", "--format=%s%x00%H"])
        .lines()
        .filter_map(|l| l.split_once('\0'))
        .map(|(s, h)| (s.to_string(), h.to_string()))
        .collect()
}
fn idle(r: &Path) -> bool {
    operation(r).is_none()
}

#[test]
fn a_long_branch_with_picks_far_apart() {
    let sb = Sandbox::new("rw-long");
    let r = sb.path("r");
    init(&r);
    many(&r, 300);
    let tree0 = tree(&r);
    let s = by_subject(&r);
    let pick = |names: &[usize]| -> Vec<String> {
        names.iter().map(|i| s[&format!("c{i}")].clone()).collect()
    };
    // Three from all over the branch go to c100.
    rewrite(
        &r,
        &head(&r),
        &Edit::Squash {
            shas: pick(&[10, 150, 299]),
            onto: s["c100"].clone(),
            message: Some("together".into()),
        },
    )
    .unwrap();
    let mut want: Vec<String> = (0..300)
        .filter(|i| ![10, 100, 150, 299].contains(i))
        .map(|i| format!("c{i}"))
        .collect();
    let at = want.iter().position(|c| c == "c101").unwrap();
    want.insert(at, "together".into());
    want.reverse();
    assert_eq!(log(&r), want);
    assert_eq!(tree(&r), tree0, "nothing lost, nothing added");
    let together = sha_of(&r, "together");
    assert_eq!(
        git(&r, &["show", "--format=", "--name-only", &together]),
        "f10.txt\nf100.txt\nf150.txt\nf299.txt\n"
    );

    // Every sixth commit dropped, root included.
    let s = by_subject(&r);
    let mut gone: Vec<String> = log(&r).into_iter().step_by(6).collect();
    gone.push("c0".into());
    let shas = gone.iter().map(|c| s[c].clone()).collect();
    rewrite(&r, &head(&r), &Edit::Drop { shas }).unwrap();
    assert_eq!(log(&r).len(), 297 - gone.len());
    assert!(log(&r).iter().all(|c| !gone.contains(c)));
    assert!(!r.join("f0.txt").exists() && r.join("f1.txt").exists());

    // Twenty scattered ones to the top, keeping their order; then under the new root.
    let tree1 = tree(&r);
    let s = by_subject(&r);
    let before = log(&r);
    let moved: Vec<String> = before
        .iter()
        .skip(3)
        .step_by(12)
        .take(20)
        .cloned()
        .collect();
    rewrite(
        &r,
        &head(&r),
        &Edit::Reorder {
            shas: moved.iter().map(|c| s[c].clone()).collect(),
            before: None,
        },
    )
    .unwrap();
    let after = log(&r);
    assert_eq!(after[..20], moved[..]);
    assert_eq!(
        after[20..],
        before
            .iter()
            .filter(|c| !moved.contains(c))
            .cloned()
            .collect::<Vec<_>>()[..]
    );
    assert_eq!(tree(&r), tree1);
    let s = by_subject(&r);
    let root = after.last().unwrap().clone();
    rewrite(
        &r,
        &head(&r),
        &Edit::Reorder {
            shas: moved.iter().map(|c| s[c].clone()).collect(),
            before: Some(s[&root].clone()),
        },
    )
    .unwrap();
    let last = log(&r);
    assert_eq!(last[last.len() - 20..], moved[..]);
    assert_eq!(tree(&r), tree1);
    assert!(idle(&r));
}

#[test]
fn the_root_commit_and_everything_picked() {
    // Root squashed up into a newer one, past one in between.
    let (_sb, r) = repo("rw-root-up");
    rewrite(
        &r,
        &head(&r),
        &Edit::Squash {
            shas: vec![sha_of(&r, "one")],
            onto: sha_of(&r, "three"),
            message: Some("one and three".into()),
        },
    )
    .unwrap();
    assert_eq!(log(&r), ["four", "one and three", "two"]);
    assert_eq!(
        git(&r, &["rev-list", "--max-parents=0", "HEAD"])
            .lines()
            .count(),
        1
    );
    assert_eq!(
        git(&r, &["log", "-1", "--format=%s", "--max-parents=0"]).trim(),
        "two"
    );

    // Everything into the root, and everything into the newest as a fixup.
    let (_sb, r) = repo("rw-root-all");
    let all = |r: &Path| ["two", "three", "four"].map(|s| sha_of(r, s)).to_vec();
    rewrite(
        &r,
        &head(&r),
        &Edit::Squash {
            shas: all(&r),
            onto: sha_of(&r, "one"),
            message: Some("all of it".into()),
        },
    )
    .unwrap();
    assert_eq!(log(&r), ["all of it"]);
    assert_eq!(git(&r, &["ls-files"]), "a.txt\nb.txt\nc.txt\nd.txt\n");
    let (_sb, r) = repo("rw-root-all-top");
    let shas = ["one", "two", "three"].map(|s| sha_of(&r, s)).to_vec();
    rewrite(
        &r,
        &head(&r),
        &Edit::Squash {
            shas,
            onto: sha_of(&r, "four"),
            message: None,
        },
    )
    .unwrap();
    assert_eq!(log(&r), ["one"]);
    assert_eq!(git(&r, &["ls-files"]), "a.txt\nb.txt\nc.txt\nd.txt\n");

    // The root alone dropped: the next one becomes a root of its own.
    let (_sb, r) = repo("rw-root-drop");
    rewrite(
        &r,
        &head(&r),
        &Edit::Drop {
            shas: vec![sha_of(&r, "one")],
        },
    )
    .unwrap();
    assert_eq!(log(&r), ["four", "three", "two"]);
    assert_eq!(
        git(&r, &["rev-list", "--max-parents=0", "HEAD"]).trim(),
        sha_of(&r, "two")
    );
    // The root to the top and back down.
    rewrite(
        &r,
        &head(&r),
        &Edit::Reorder {
            shas: vec![sha_of(&r, "two")],
            before: None,
        },
    )
    .unwrap();
    assert_eq!(log(&r), ["two", "four", "three"]);
    rewrite(
        &r,
        &head(&r),
        &Edit::Move {
            sha: sha_of(&r, "two"),
            up: false,
        },
    )
    .unwrap();
    assert_eq!(log(&r), ["four", "two", "three"]);
}

/// Every commit picked and dropped: git's rebase --root then leaves the branch on its own
/// placeholder, an empty commit with no message, which no one asked for.
#[test]
fn dropping_every_commit_is_refused() {
    let (_sb, r) = repo("rw-drop-all");
    let before = (head(&r), log(&r));
    let shas = ["one", "two", "three", "four"]
        .map(|s| sha_of(&r, s))
        .to_vec();
    let res = rewrite(&r, &head(&r), &Edit::Drop { shas });
    assert!(
        res.is_err(),
        "dropped everything, the branch is now {:?}",
        git(&r, &["log", "--format=%h [%s] tree %T"])
    );
    assert_eq!((head(&r), log(&r)), before);
    assert!(idle(&r));
}

#[test]
fn odd_messages_survive_the_replay() {
    let sb = Sandbox::new("rw-messages");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "base.txt", "base\n", "base");
    write_commit(&r, "x.txt", "x\n", "dropped");
    let hashy = "# not a comment\n\nBody line\n# also kept\n; semi too\n\nRefs: #12\nSigned-off-by: Ada <ada@example.com>\nCo-authored-by: Bo <bo@example.com>\n";
    let unicode = "Ünïcødé 🚀 日本語 — “quotes” 'single' $HOME `tick`\n\n…body…\n";
    let long = format!("{}\n\n{}\n", "s".repeat(5_000), "body ".repeat(20_000));
    let messages = [
        "",
        hashy,
        unicode,
        long.as_str(),
        "From Windows\r\n\r\nCRLF body\r\n",
    ];
    for (i, m) in messages.iter().enumerate() {
        commit_raw(&r, &format!("m{i}.txt"), "m\n", m);
    }
    let stored = |r: &Path| {
        (0..messages.len())
            .rev()
            .map(|skip| raw_message(r, &format!("HEAD~{skip}")))
            .collect::<Vec<_>>()
    };
    let before = stored(&r);
    // Dropping the one under them replays each; so does moving them over a new one.
    for char in ["auto", ";"] {
        run(&r, &["config", "core.commentChar", char]).unwrap();
        let dropped = rev(&r, "HEAD~5");
        rewrite(
            &r,
            &head(&r),
            &Edit::Drop {
                shas: vec![dropped],
            },
        )
        .unwrap();
        assert_eq!(stored(&r), before, "commentChar {char}");
        let shas = (0..messages.len())
            .map(|i| rev(&r, &format!("HEAD~{i}")))
            .collect();
        write_commit(&r, "y.txt", "y\n", "dropped");
        rewrite(&r, &head(&r), &Edit::Reorder { shas, before: None }).unwrap();
        assert_eq!(stored(&r), before, "commentChar {char}");
        run(&r, &["config", "--unset", "core.commentChar"]).unwrap();
    }

    // A fixup keeps the target's message byte for byte, `#` lines and all.
    run(&r, &["config", "core.commentChar", ";"]).unwrap();
    let target = run_text(&r, &["rev-parse", "HEAD~3"])
        .unwrap()
        .trim()
        .to_string();
    assert_eq!(raw_message(&r, &target), hashy);
    rewrite(
        &r,
        &head(&r),
        &Edit::Squash {
            shas: vec![run_text(&r, &["rev-parse", "HEAD~1"])
                .unwrap()
                .trim()
                .into()],
            onto: target,
            message: None,
        },
    )
    .unwrap();
    assert_eq!(raw_message(&r, "HEAD~2"), hashy);
    // A message given for a squash or reword is kept as given, trimmed: comment chars aren't
    // comments in it.
    let given = "Squashed ü\n\n# heading kept\n; this too\n\nSigned-off-by: Ada <ada@example.com>";
    rewrite(
        &r,
        &head(&r),
        &Edit::Squash {
            shas: vec![run_text(&r, &["rev-parse", "HEAD"]).unwrap().trim().into()],
            onto: run_text(&r, &["rev-parse", "HEAD~1"])
                .unwrap()
                .trim()
                .into(),
            message: Some(format!("\n  {given}\n\n")),
        },
    )
    .unwrap();
    assert_eq!(raw_message(&r, "HEAD").trim_end(), given);
    let old = run_text(&r, &["rev-parse", "HEAD~2"])
        .unwrap()
        .trim()
        .to_string();
    rewrite(
        &r,
        &head(&r),
        &Edit::Reword {
            sha: old,
            message: given.into(),
        },
    )
    .unwrap();
    assert_eq!(raw_message(&r, "HEAD~2").trim_end(), given);
    assert!(idle(&r));
}

#[test]
fn a_commit_with_no_message_takes_a_fixup() {
    let sb = Sandbox::new("rw-empty-msg");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "a\n", "one");
    commit_raw(&r, "b.txt", "b\n", "");
    write_commit(&r, "c.txt", "c\n", "three");
    let blank = rev(&r, "HEAD~1");
    rewrite(
        &r,
        &head(&r),
        &Edit::Squash {
            shas: vec![head(&r)],
            onto: blank,
            message: None,
        },
    )
    .unwrap();
    assert_eq!(log(&r), ["", "one"]);
    assert_eq!(git(&r, &["ls-files"]), "a.txt\nb.txt\nc.txt\n");
    assert!(idle(&r));
}

#[test]
fn commits_that_end_up_empty() {
    let sb = Sandbox::new("rw-empty");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "f.txt", "1\n", "one");
    write_commit(&r, "f.txt", "2\n", "two");
    write_commit(&r, "f.txt", "1\n", "back");
    run(&r, &["commit", "-q", "--allow-empty", "-m", "ci: trigger"]).unwrap();
    write_commit(&r, "g.txt", "g\n", "gee");
    // Without "two", "back" changes nothing and goes; the empty-on-purpose one stays.
    rewrite(
        &r,
        &head(&r),
        &Edit::Drop {
            shas: vec![sha_of(&r, "two")],
        },
    )
    .unwrap();
    assert_eq!(log(&r), ["gee", "ci: trigger", "one"]);
}

/// "add debug log" + "remove debug log" squashed: git won't amend a fixup into an empty
/// commit, and that stopped the rebase; the squash is made one empty commit instead.
#[test]
fn squashing_commits_that_cancel_out() {
    let (_sb, r) = repo("rw-cancel");
    write_commit(&r, "h.txt", "h\n", "add h");
    fs::remove_file(r.join("h.txt")).unwrap();
    run(&r, &["add", "-A"]).unwrap();
    run(&r, &["commit", "-q", "-m", "remove h"]).unwrap();
    let res = rewrite(
        &r,
        &head(&r),
        &Edit::Squash {
            shas: vec![sha_of(&r, "remove h")],
            onto: sha_of(&r, "add h"),
            message: Some("h and back".into()),
        },
    );
    assert_eq!(res, Ok(Outcome::Done));
    assert!(idle(&r));
    assert!(!r.join("h.txt").exists());
    // One empty commit with the new message, where "add h" was.
    assert_eq!(log(&r), ["h and back", "four", "three", "two", "one"]);
    assert_eq!(git(&r, &["diff", "--name-only", "HEAD~1", "HEAD"]), "");
}

#[test]
fn merges_below_the_edit_are_fine() {
    let (_sb, r) = repo("rw-merge-below");
    run(&r, &["switch", "-q", "-c", "side", "HEAD~1"]).unwrap();
    write_commit(&r, "s.txt", "s\n", "side");
    run(&r, &["switch", "-q", "main"]).unwrap();
    run(&r, &["merge", "-q", "--no-ff", "-m", "merge side", "side"]).unwrap();
    for (f, s) in [("x", "x1"), ("y", "y1"), ("z", "z1")] {
        write_commit(&r, &format!("{f}.txt"), "v\n", s);
    }
    rewrite(
        &r,
        &head(&r),
        &Edit::Squash {
            shas: vec![sha_of(&r, "z1")],
            onto: sha_of(&r, "x1"),
            message: None,
        },
    )
    .unwrap();
    assert_eq!(log(&r)[..3], ["y1", "x1", "merge side"]);
    assert_eq!(
        git(&r, &["rev-list", "--merges", "--count", "HEAD"]).trim(),
        "1"
    );
    // Picks on both sides of it are refused, nothing touched.
    let before = head(&r);
    for edit in [
        Edit::Drop {
            shas: vec![sha_of(&r, "y1"), sha_of(&r, "two")],
        },
        Edit::Reorder {
            shas: vec![sha_of(&r, "y1")],
            before: Some(sha_of(&r, "three")),
        },
        Edit::Squash {
            shas: vec![sha_of(&r, "x1")],
            onto: sha_of(&r, "one"),
            message: None,
        },
        // On the merged-in side, not the branch's own line.
        Edit::Drop {
            shas: vec![sha_of(&r, "side")],
        },
    ] {
        assert!(rewrite(&r, &head(&r), &edit).is_err(), "{edit:?}");
        assert_eq!(head(&r), before);
        assert!(idle(&r));
    }
}

/// Resolves every conflicted path by taking it out, then continues, until the rebase ends.
fn resolve_by_removing(r: &Path) {
    for _ in 0..10 {
        if idle(r) {
            return;
        }
        for p in git(r, &["diff", "--name-only", "--diff-filter=U"]).lines() {
            run(r, &["rm", "-q", "--ignore-unmatch", "--", p]).unwrap();
        }
        let _ = op_continue(r);
    }
    panic!("the rebase never ended: {}", git(r, &["status"]));
}

#[test]
fn rename_delete_conflicts_continue_and_abort() {
    let sb = Sandbox::new("rw-rename");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "base.txt", "base\n", "base");
    write_commit(&r, "f.txt", "one\ntwo\nthree\nfour\n", "add f");
    run(&r, &["mv", "f.txt", "g.txt"]).unwrap();
    run(&r, &["commit", "-q", "-m", "rename f"]).unwrap();
    run(&r, &["rm", "-q", "g.txt"]).unwrap();
    run(&r, &["commit", "-q", "-m", "delete g"]).unwrap();
    write_commit(&r, "h.txt", "h\n", "add h");
    let before = (head(&r), log(&r));
    // The delete goes under the rename: rename/delete.
    let edit = Edit::Reorder {
        shas: vec![sha_of(&r, "delete g")],
        before: Some(sha_of(&r, "rename f")),
    };
    assert_eq!(rewrite(&r, &head(&r), &edit), Ok(Outcome::Conflicts));
    assert!(operation(&r).is_some_and(|o| o.kind == "rebase"));
    // Another edit can't start on top of it.
    assert!(rewrite(&r, &before.0, &edit)
        .unwrap_err()
        .contains("in progress"));
    op_abort(&r).unwrap();
    assert_eq!((head(&r), log(&r)), before);
    assert!(idle(&r));
    // Again, seen through this time.
    assert_eq!(rewrite(&r, &head(&r), &edit), Ok(Outcome::Conflicts));
    resolve_by_removing(&r);
    assert_eq!(log(&r)[0], "add h");
    assert!(!r.join("f.txt").exists() && !r.join("g.txt").exists());
}

#[test]
fn a_squash_message_outlasts_a_conflict() {
    let (_sb, r) = repo("rw-squash-continue");
    write_commit(&r, "a.txt", "a2\n", "five");
    write_commit(&r, "a.txt", "a3\n", "six");
    write_commit(&r, "f.txt", "f\n", "seven");
    let edit = Edit::Squash {
        shas: vec![sha_of(&r, "six")],
        onto: sha_of(&r, "four"),
        message: Some("four and six\n\n# kept".into()),
    };
    assert_eq!(rewrite(&r, &head(&r), &edit), Ok(Outcome::Conflicts));
    // Take theirs ("a3") and go on: five then conflicts on top of it too.
    for _ in 0..5 {
        if idle(&r) {
            break;
        }
        for p in git(&r, &["diff", "--name-only", "--diff-filter=U"]).lines() {
            fs::write(r.join(p), "a3\n").unwrap();
            run(&r, &["add", "--", p]).unwrap();
        }
        let _ = op_continue(&r);
    }
    assert!(idle(&r), "{}", git(&r, &["status"]));
    let subjects = log(&r);
    assert!(
        subjects.contains(&"four and six".to_string()),
        "{subjects:?}"
    );
    let sha = sha_of(&r, "four and six");
    assert_eq!(raw_message(&r, &sha).trim_end(), "four and six\n\n# kept");
}

#[test]
fn a_dirty_worktree() {
    // Staged and unstaged changes elsewhere come back after the edit.
    let (_sb, r) = repo("rw-dirty");
    fs::write(r.join("a.txt"), "edited\n").unwrap();
    fs::write(r.join("d.txt"), "staged\n").unwrap();
    run(&r, &["add", "d.txt"]).unwrap();
    fs::write(r.join("notes.txt"), "untracked\n").unwrap();
    rewrite(
        &r,
        &head(&r),
        &Edit::Squash {
            shas: vec![sha_of(&r, "two")],
            onto: sha_of(&r, "four"),
            message: Some("two and four".into()),
        },
    )
    .unwrap();
    assert_eq!(log(&r), ["two and four", "three", "one"]);
    assert_eq!(fs::read_to_string(r.join("a.txt")).unwrap(), "edited\n");
    assert_eq!(fs::read_to_string(r.join("d.txt")).unwrap(), "staged\n");
    assert_eq!(
        fs::read_to_string(r.join("notes.txt")).unwrap(),
        "untracked\n"
    );
    assert!(idle(&r));

    // An untracked file where a replayed commit puts one: refused before anything moves.
    let (_sb, r) = repo("rw-untracked");
    run(&r, &["rm", "-q", "b.txt"]).unwrap();
    run(&r, &["commit", "-q", "-m", "remove b"]).unwrap();
    fs::write(r.join("b.txt"), "mine\n").unwrap();
    let before = (head(&r), log(&r));
    let res = rewrite(
        &r,
        &head(&r),
        &Edit::Drop {
            shas: vec![sha_of(&r, "remove b")],
        },
    );
    assert!(res.is_err(), "{res:?}");
    assert_eq!((head(&r), log(&r)), before);
    assert_eq!(fs::read_to_string(r.join("b.txt")).unwrap(), "mine\n");
    assert!(idle(&r));

    // An edit to a file the dropped commit made: git keeps it in the stash, never loses it.
    let (_sb, r) = repo("rw-dirty-dropped");
    fs::write(r.join("c.txt"), "my edit\n").unwrap();
    let _ = rewrite(
        &r,
        &head(&r),
        &Edit::Drop {
            shas: vec![sha_of(&r, "three")],
        },
    );
    assert_eq!(log(&r), ["four", "two", "one"]);
    let kept = fs::read_to_string(r.join("c.txt"))
        .unwrap_or_default()
        .contains("my edit")
        || git(&r, &["stash", "list"]).contains("autostash");
    assert!(kept);
}

#[test]
fn a_plan_from_a_stale_list() {
    let (_sb, r) = repo("rw-stale");
    let seen = head(&r);
    let three = sha_of(&r, "three");
    // An agent commits, then amends one, meanwhile.
    write_commit(&r, "e.txt", "e\n", "five");
    for edit in [
        Edit::Drop {
            shas: vec![three.clone()],
        },
        Edit::Squash {
            shas: vec![three.clone()],
            onto: sha_of(&r, "two"),
            message: None,
        },
        Edit::Reorder {
            shas: vec![three.clone()],
            before: None,
        },
    ] {
        assert!(rewrite(&r, &seen, &edit)
            .unwrap_err()
            .contains("HEAD has moved"));
    }
    // A commit that is no longer on the branch, with HEAD as the list showed it.
    run(&r, &["commit", "-q", "--amend", "-m", "five, amended"]).unwrap();
    run(&r, &["switch", "-q", "-c", "other", "HEAD~2"]).unwrap();
    write_commit(&r, "o.txt", "o\n", "elsewhere");
    run(&r, &["switch", "-q", "main"]).unwrap();
    let elsewhere = sha_of(&r, "elsewhere");
    for edit in [
        Edit::Drop {
            shas: vec![three.clone(), elsewhere.clone()],
        },
        Edit::Squash {
            shas: vec![three.clone()],
            onto: elsewhere.clone(),
            message: None,
        },
        Edit::Reorder {
            shas: vec![three.clone()],
            before: Some(elsewhere.clone()),
        },
    ] {
        let e = rewrite(&r, &head(&r), &edit).unwrap_err();
        assert!(e.contains("isn't on this branch"), "{e}");
    }
    assert!(idle(&r));
}

#[test]
fn user_config_that_shapes_rebases() {
    let (_sb, r) = repo("rw-config");
    fs::write(r.join("template.txt"), "# from the template\n").unwrap();
    for (k, v) in [
        ("rebase.autoSquash", "true"),
        ("rebase.updateRefs", "true"),
        ("rebase.instructionFormat", "%s [%an] %d"),
        ("rebase.abbreviateCommands", "true"),
        ("rebase.missingCommitsCheck", "error"),
        ("rebase.rebaseMerges", "true"),
        ("rebase.autoStash", "false"),
        ("commit.verbose", "true"),
        ("commit.cleanup", "scissors"),
        ("commit.template", "template.txt"),
        ("sequence.editor", "false"),
        ("core.editor", "false"),
        ("rerere.enabled", "true"),
    ] {
        run(&r, &["config", k, v]).unwrap();
    }
    // A "fixup!" commit autosquash would move, and a branch stacked mid-way.
    write_commit(&r, "b.txt", "b2\n", "fixup! two");
    write_commit(&r, "e.txt", "e\n", "five");
    run(&r, &["branch", "stacked", "HEAD~1"]).unwrap();
    rewrite(
        &r,
        &head(&r),
        &Edit::Drop {
            shas: vec![sha_of(&r, "three")],
        },
    )
    .unwrap();
    assert_eq!(log(&r), ["five", "fixup! two", "four", "two", "one"]);
    rewrite(
        &r,
        &head(&r),
        &Edit::Squash {
            shas: vec![sha_of(&r, "five")],
            onto: sha_of(&r, "two"),
            message: Some("two and five".into()),
        },
    )
    .unwrap();
    assert_eq!(log(&r), ["fixup! two", "four", "two and five", "one"]);
    rewrite(
        &r,
        &head(&r),
        &Edit::Reword {
            sha: sha_of(&r, "four"),
            message: "four, again".into(),
        },
    )
    .unwrap();
    assert_eq!(
        log(&r),
        ["fixup! two", "four, again", "two and five", "one"]
    );
    assert!(idle(&r));
}

#[test]
fn hooks_that_reject_commits() {
    let (_sb, r) = repo("rw-hooks");
    let hooks = r.join(".git/hooks");
    fs::create_dir_all(&hooks).unwrap();
    for hook in ["pre-commit", "commit-msg", "pre-merge-commit"] {
        let p = hooks.join(hook);
        fs::write(&p, "#!/bin/sh\necho rejected by hook >&2\nexit 1\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
        }
    }
    rewrite(
        &r,
        &head(&r),
        &Edit::Reword {
            sha: sha_of(&r, "two"),
            message: "two, reworded".into(),
        },
    )
    .unwrap();
    rewrite(
        &r,
        &head(&r),
        &Edit::Squash {
            shas: vec![sha_of(&r, "four")],
            onto: sha_of(&r, "one"),
            message: Some("one and four".into()),
        },
    )
    .unwrap();
    rewrite(
        &r,
        &head(&r),
        &Edit::Reword {
            sha: head(&r),
            message: "three, reworded".into(),
        },
    )
    .unwrap();
    assert_eq!(
        log(&r),
        ["three, reworded", "two, reworded", "one and four"]
    );
    // A pre-rebase hook that says no: nothing happens, and nothing is left half done.
    let p = hooks.join("pre-rebase");
    fs::write(&p, "#!/bin/sh\nexit 1\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let before = head(&r);
    assert!(rewrite(
        &r,
        &head(&r),
        &Edit::Drop {
            shas: vec![sha_of(&r, "two, reworded")]
        }
    )
    .is_err());
    assert_eq!(head(&r), before);
    assert!(idle(&r));
}

/// commit.gpgsign on with no way to sign: the replay can't make commits.
#[test]
fn signing_on_without_a_key() {
    let (_sb, r) = repo("rw-gpg");
    run(&r, &["config", "commit.gpgsign", "true"]).unwrap();
    run(&r, &["config", "gpg.program", "false"]).unwrap();
    let before = (head(&r), log(&r));
    let e = rewrite(
        &r,
        &head(&r),
        &Edit::Drop {
            shas: vec![sha_of(&r, "two")],
        },
    )
    .unwrap_err();
    assert!(e.contains("gpg"), "{e}");
    // Rolled back: no rebase left waiting.
    assert!(idle(&r));
    assert_eq!((head(&r), log(&r)), before);
    // Rewording HEAD is one commit: refused outright.
    assert!(rewrite(
        &r,
        &head(&r),
        &Edit::Reword {
            sha: head(&r),
            message: "x".into()
        }
    )
    .is_err());
    assert_eq!((head(&r), log(&r)), before);
}

#[test]
fn a_repo_at_an_odd_path() {
    let sb = Sandbox::new("rw-path");
    let r = sb.path("my repo's $HOME `x` ü");
    init(&r);
    for (f, s) in [("a b.txt", "one"), ("ç'd.txt", "two"), ("$x.txt", "three")] {
        write_commit(&r, f, "v\n", s);
    }
    rewrite(
        &r,
        &head(&r),
        &Edit::Squash {
            shas: vec![sha_of(&r, "three")],
            onto: sha_of(&r, "one"),
            message: Some("one and three".into()),
        },
    )
    .unwrap();
    assert_eq!(log(&r), ["two", "one and three"]);
    rewrite(
        &r,
        &head(&r),
        &Edit::Reword {
            sha: sha_of(&r, "one and three"),
            message: "first".into(),
        },
    )
    .unwrap();
    assert_eq!(log(&r), ["two", "first"]);
    assert!(idle(&r));
}

#[test]
fn every_commit_of_a_long_branch_into_one() {
    let sb = Sandbox::new("rw-all-one");
    let r = sb.path("r");
    init(&r);
    many(&r, 200);
    let tree0 = tree(&r);
    let mut all: Vec<String> = git(&r, &["rev-list", "HEAD"])
        .lines()
        .map(String::from)
        .collect();
    // Picked all, squashed into the oldest as the menu does.
    let root = all.pop().unwrap();
    rewrite(
        &r,
        &head(&r),
        &Edit::Squash {
            shas: all,
            onto: root,
            message: Some("everything".into()),
        },
    )
    .unwrap();
    assert_eq!(log(&r), ["everything"]);
    assert_eq!(tree(&r), tree0);
    assert!(idle(&r));
}

#[test]
fn in_a_linked_worktree_and_on_a_detached_head() {
    let (sb, r) = repo("rw-wt");
    let wt = sb.path("wt dir");
    run(
        &r,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "feature",
            wt.to_str().unwrap(),
        ],
    )
    .unwrap();
    write_commit(&wt, "e.txt", "e\n", "five");
    write_commit(&wt, "f.txt", "f\n", "six");
    rewrite(
        &wt,
        &head(&wt),
        &Edit::Squash {
            shas: vec![sha_of(&wt, "six")],
            onto: sha_of(&wt, "four"),
            message: Some("four and six".into()),
        },
    )
    .unwrap();
    assert_eq!(log(&wt)[..2], ["five", "four and six"]);
    // main, checked out in the first one, is left alone.
    assert_eq!(log(&r)[0], "four");
    assert!(idle(&wt) && idle(&r));

    run(&r, &["switch", "-q", "--detach", "HEAD"]).unwrap();
    rewrite(
        &r,
        &head(&r),
        &Edit::Drop {
            shas: vec![sha_of(&r, "two")],
        },
    )
    .unwrap();
    assert_eq!(log(&r), ["four", "three", "one"]);
    assert_eq!(
        git(&r, &["log", "-1", "--format=%s", "main"]).trim(),
        "four"
    );
    assert!(
        run(&r, &["symbolic-ref", "-q", "HEAD"]).is_err(),
        "still detached"
    );
}

#[test]
fn stacked_branches_follow_with_update_refs() {
    let (_sb, r) = repo("rw-update-refs");
    run(&r, &["branch", "at-three", "HEAD~1"]).unwrap();
    run(&r, &["branch", "at-two", "HEAD~2"]).unwrap();
    let squash = |r: &Path| {
        let edit = Edit::Squash {
            shas: vec![sha_of(r, "three")],
            onto: sha_of(r, "two"),
            message: Some("two and three".into()),
        };
        rewrite(r, &head(r), &edit).unwrap();
    };
    // Off: they stay on the old commits, as git leaves them.
    let old = git(&r, &["rev-parse", "at-three"]);
    squash(&r);
    assert_eq!(git(&r, &["rev-parse", "at-three"]), old);

    let (_sb, r) = repo("rw-update-refs-on");
    run(&r, &["config", "rebase.updateRefs", "true"]).unwrap();
    run(&r, &["branch", "at-three", "HEAD~1"]).unwrap();
    run(&r, &["branch", "at-two", "HEAD~2"]).unwrap();
    squash(&r);
    let squashed = sha_of(&r, "two and three");
    assert_eq!(git(&r, &["rev-parse", "at-three"]).trim(), squashed);
    assert_eq!(git(&r, &["rev-parse", "at-two"]).trim(), squashed);
    // A dropped commit's branch goes to the one under it.
    run(&r, &["branch", "at-four"]).unwrap();
    let edit = Edit::Drop {
        shas: vec![head(&r)],
    };
    rewrite(&r, &head(&r), &edit).unwrap();
    assert_eq!(git(&r, &["rev-parse", "at-four"]).trim(), squashed);
    assert!(idle(&r));
}

#[test]
fn the_outcome_says_whose_conflicts_they_are() {
    let (_sb, r) = repo("rw-outcome");
    // Uncommitted edits to a file the dropped commit made: the rebase finishes, the stash's
    // changes come back conflicted.
    fs::write(r.join("c.txt"), "my edit\n").unwrap();
    let outcome = rewrite(
        &r,
        &head(&r),
        &Edit::Drop {
            shas: vec![sha_of(&r, "three")],
        },
    );
    assert_eq!(outcome, Ok(Outcome::StashConflicts));
    // And conflicted files left behind refuse the next one.
    let e = rewrite(
        &r,
        &head(&r),
        &Edit::Drop {
            shas: vec![sha_of(&r, "two")],
        },
    )
    .unwrap_err();
    assert!(e.contains("conflicted"), "{e}");

    let (_sb, r) = repo("rw-outcome-rebase");
    write_commit(&r, "a.txt", "a2\n", "five");
    write_commit(&r, "a.txt", "a3\n", "six");
    let edit = Edit::Move {
        sha: sha_of(&r, "six"),
        up: false,
    };
    assert_eq!(rewrite(&r, &head(&r), &edit), Ok(Outcome::Conflicts));
    op_abort(&r).unwrap();
}

/// prepare-commit-msg runs even with --no-verify. git's own rebase runs it once for each commit
/// it makes, a fixup's or squash's too: ours may add its line once per edited commit, no more.
#[test]
fn a_prepare_commit_msg_hook_runs_once_per_commit() {
    let (_sb, r) = repo("rw-prepare-hook");
    let p = r.join(".git/hooks/prepare-commit-msg");
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(&p, "#!/bin/sh\necho hooked >> \"$1\"\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let hooked = |r: &Path, rev: &str| raw_message(r, rev).matches("hooked").count();
    rewrite(
        &r,
        &head(&r),
        &Edit::Squash {
            shas: vec![sha_of(&r, "four")],
            onto: sha_of(&r, "two"),
            message: Some("two and four".into()),
        },
    )
    .unwrap();
    let squashed = git(&r, &["log", "--format=%H", "--grep=^two and four"]);
    assert_eq!(raw_message(&r, squashed.trim()), "two and four\nhooked\n");
    // A fixup keeps the message it had, and the hook adds to it once.
    rewrite(
        &r,
        &head(&r),
        &Edit::Squash {
            shas: vec![sha_of(&r, "three")],
            onto: squashed.trim().into(),
            message: None,
        },
    )
    .unwrap();
    assert_eq!(log(&r).len(), 2);
    assert_eq!(hooked(&r, "HEAD"), 2);
    rewrite(
        &r,
        &head(&r),
        &Edit::Reword {
            sha: rev(&r, "HEAD~1"),
            message: "first".into(),
        },
    )
    .unwrap();
    assert_eq!(raw_message(&r, "HEAD~1"), "first\nhooked\n");
    assert!(idle(&r));
}
