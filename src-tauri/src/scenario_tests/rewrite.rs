use super::*;
use crate::rewrite::{run as rewrite, Edit};

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
            sha: three,
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
            sha: four,
            message: None,
        },
    )
    .unwrap();
    assert_eq!(log(&r), ["two and three", "one"]);
    assert!(r.join("d.txt").exists());

    let (_sb, r) = repo("rw-drop");
    let two = sha_of(&r, "two");
    rewrite(&r, &head(&r), &Edit::Drop { sha: two }).unwrap();
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
            sha: first,
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
    assert!(rewrite(&r, &two, &Edit::Drop { sha: two.clone() })
        .unwrap_err()
        .contains("HEAD has moved"));
    // Two commits editing one line can't swap cleanly: the rebase stops.
    write_commit(&r, "a.txt", "a2\n", "five");
    write_commit(&r, "a.txt", "a3\n", "six");
    let six = sha_of(&r, "six");
    assert!(rewrite(
        &r,
        &head(&r),
        &Edit::Move {
            sha: six,
            up: false
        }
    )
    .unwrap());
    assert!(operation(&r).is_some_and(|o| o.kind == "rebase"));
    op_abort(&r).unwrap();
    assert_eq!(log(&r)[..2], ["six", "five"]);
    // A merge in the way.
    run(&r, &["switch", "-q", "-c", "side", "HEAD~2"]).unwrap();
    write_commit(&r, "e.txt", "e\n", "side");
    run(&r, &["switch", "-q", "main"]).unwrap();
    run(&r, &["merge", "-q", "--no-edit", "side"]).unwrap();
    let two = sha_of(&r, "two");
    assert!(rewrite(&r, &head(&r), &Edit::Drop { sha: two })
        .unwrap_err()
        .contains("merges"));
}
