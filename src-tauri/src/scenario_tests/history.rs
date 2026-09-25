//! The log, its filters and graph, blame, comparisons, reflog and bisect.

use super::*;

/// A fork's view of its original: another branch's history, marking what HEAD lacks.
#[test]
fn log_of_a_remote_branch_marks_what_head_lacks() {
    let sb = Sandbox::new("logrev");
    let c = sb.remote_with_clones(2);
    let (a, b) = (&c[0], &c[1]);
    write_commit(a, "a.txt", "new\n", "upstream moved on");
    run(a, &["push", "-q"]).unwrap();
    fetch(b, &Net::default()).unwrap();
    let theirs = log(b, Some("refs/remotes/origin/main"), 0, 10).unwrap();
    assert_eq!(theirs[0].subject, "upstream moved on");
    assert!(theirs[0].not_in_head && !theirs[1].not_in_head);
    assert!(theirs.iter().all(|c| !c.unpushed));
    // HEAD's own log never marks anything.
    assert!(log(b, None, 0, 10).unwrap().iter().all(|c| !c.not_in_head));
    // Only remote-tracking branches, never an option or a local ref.
    assert!(log(b, Some("--all"), 0, 10).is_err());
    assert!(log(b, Some("refs/heads/main"), 0, 10).is_err());
    assert!(log(b, Some("refs/remotes/--output=x/y"), 0, 10).is_err());
}

/// Blame: each line's commit with its whole message, lines not committed yet, files git
/// doesn't have yet, and the user's blame.ignoreRevsFile.
#[test]
fn blame_attributes_lines() {
    let sb = Sandbox::new("blame");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "f.txt", "a\nb\n", "First");
    write_commit(
        &r,
        "f.txt",
        "a\nB\n",
        "Second\n\nWhy it changed.\n\nCo-Authored-By: Claude <noreply@anthropic.com>",
    );
    fs::write(r.join("f.txt"), "a\nB\nc\n").unwrap();

    let b = blame(&r, "f.txt").unwrap();
    let at = |b: &Blame, i: usize| b.commits[b.lines[i] as usize].message.clone();
    assert_eq!(b.lines.len(), 3);
    assert_eq!(at(&b, 0), "First");
    assert!(at(&b, 1).starts_with("Second\n\nWhy it changed."));
    assert!(at(&b, 1).ends_with("Co-Authored-By: Claude <noreply@anthropic.com>"));
    let new = &b.commits[b.lines[2] as usize];
    assert_eq!(new.sha, "0".repeat(40));
    let second = &b.commits[b.lines[1] as usize];
    assert_eq!(
        (second.author_name.as_str(), second.path.as_str()),
        ("T", "f.txt")
    );

    // Untracked, or only staged: every line is new, not an error.
    fs::write(r.join("u.txt"), "x\n").unwrap();
    assert!(blame(&r, "u.txt").unwrap().lines.is_empty());
    fs::write(r.join("s.txt"), "x\n").unwrap();
    stage(&r, &["s.txt".into()]).unwrap();
    let staged = blame(&r, "s.txt").unwrap();
    assert!(staged.lines.is_empty() || staged.commits.iter().all(|c| c.sha == "0".repeat(40)));

    // git skips the revisions the user listed to ignore.
    let sha = second.sha.clone();
    fs::write(r.join(".git-blame-ignore-revs"), format!("{sha}\n")).unwrap();
    run(
        &r,
        &["config", "blame.ignoreRevsFile", ".git-blame-ignore-revs"],
    )
    .unwrap();
    assert_eq!(at(&blame(&r, "f.txt").unwrap(), 1), "First");
    assert!(blame(&r, "f.txt").unwrap().unavailable.is_none());

    // A Git LFS file: git has only its pointer, whose lines say nothing about the file's.
    let pointer = format!(
        "version https://git-lfs.github.com/spec/v1\noid sha256:{}\nsize 12345\n",
        "a".repeat(64)
    );
    write_commit(&r, "big.bin", &pointer, "Add big file");
    let lfs = blame(&r, "big.bin").unwrap();
    assert!(lfs.unavailable.is_some() && lfs.lines.is_empty());
}

/// History search: words, author, pickaxe, a path, a file followed through a rename, a SHA.
#[test]
fn log_search_narrows_and_pages() {
    let sb = Sandbox::new("logsearch");
    let c = sb.remote_with_clones(1);
    let a = &c[0];
    write_commit(
        a,
        "src/auth.rs",
        "fn login() {}\n",
        "Fix auth (login) [urgent]",
    );
    run(a, &["push", "-q"]).unwrap();
    write_commit(
        a,
        "src/auth.rs",
        "fn login() {}\nfn logout() {}\n",
        "Add logout",
    );
    run(a, &["mv", "src/auth.rs", "src/session.rs"]).unwrap();
    commit(a, "Rename auth to session", &CommitOptions::default()).unwrap();
    run(
        a,
        &[
            "-c",
            "user.name=Other",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "Tidy",
        ],
    )
    .unwrap();

    let subjects = |f: &LogFilter| -> Vec<String> {
        let log = log_filtered(a, None, 0, 50, f).unwrap();
        log.into_iter().map(|c| c.subject).collect()
    };
    // Regex characters are taken as typed, case is ignored, and every word must match.
    let words = |w: &[&str]| LogFilter {
        grep: w.iter().map(|s| s.to_string()).collect(),
        ..Default::default()
    };
    assert_eq!(
        subjects(&words(&["(LOGIN) [urgent"])),
        ["Fix auth (login) [urgent]"]
    );
    assert_eq!(
        subjects(&words(&["auth", "rename"])),
        ["Rename auth to session"]
    );
    let author = LogFilter {
        author: vec!["other".into()],
        ..Default::default()
    };
    assert_eq!(subjects(&author), ["Tidy"]);
    let code = LogFilter {
        code: Some("logout".into()),
        ..Default::default()
    };
    assert_eq!(subjects(&code), ["Add logout"]);

    // A path alone stops at the rename; followed, it goes on under the old name.
    let mut path = LogFilter {
        paths: vec!["src/session.rs".into()],
        ..Default::default()
    };
    assert_eq!(subjects(&path), ["Rename auth to session"]);
    path.follow = true;
    let followed = log_filtered(a, None, 0, 50, &path).unwrap();
    let files: Vec<_> = followed.iter().map(|c| c.file.as_deref()).collect();
    assert_eq!(
        files,
        [
            Some("src/session.rs"),
            Some("src/auth.rs"),
            Some("src/auth.rs")
        ]
    );
    // The flags hold deep in a filtered history: only the first auth commit was pushed.
    assert!(followed[1].unpushed && !followed[1].on_origin);
    assert!(!followed[2].unpushed && followed[2].on_origin);
    // Pages are pages of the matches (git's own --skip counts every commit with -S or --follow).
    assert!(log_filtered(a, None, 1, 1, &code).unwrap().is_empty());
    assert_eq!(
        log_filtered(a, None, 1, 1, &path).unwrap()[0].subject,
        "Add logout"
    );

    // A SHA prefix finds its commit; unknown or malformed ones find nothing.
    let head = log(a, None, 0, 1).unwrap()[0].sha.clone();
    assert_eq!(find_commit(a, &head[..8]).unwrap().unwrap().sha, head);
    assert!(find_commit(a, "0000000").unwrap().is_none());
    assert!(find_commit(a, "--all").unwrap().is_none());
    // A branch's tip by its full name, for going to it in the graph; a short name isn't one.
    assert_eq!(
        find_commit(a, "refs/heads/main").unwrap().unwrap().sha,
        head
    );
    assert!(find_commit(a, "main").unwrap().is_none());
    assert!(find_commit(a, "refs/heads/gone").unwrap().is_none());
}

#[test]
fn drops_pushed_follows_ancestry_not_log_order() {
    let sb = Sandbox::new("drops");
    let c = sb.remote_with_clones(2);
    let (a, b) = (&c[0], &c[1]);
    // P is pushed; merged in as the second parent, "target" lists above it.
    write_commit(a, "p.txt", "p\n", "old pushed");
    run(a, &["push", "-q"]).unwrap();
    write_commit(b, "t.txt", "t\n", "target");
    let t = run_text(b, &["rev-parse", "HEAD"]).unwrap();
    run(b, &["fetch", "-q"]).unwrap();
    run(b, &["reset", "-q", "--hard", "origin/main"]).unwrap();
    run(b, &["merge", "-q", "--no-ff", "--no-edit", t.trim()]).unwrap();

    let commits = log(b, None, 0, 10).unwrap();
    let subjects: Vec<&str> = commits.iter().map(|x| x.subject.as_str()).collect();
    assert_eq!(subjects[1..], ["target", "old pushed", "base"]);
    let (merge, target) = (&commits[0].sha, &commits[1].sha);
    // Only unpushed commits sit above "target", yet resetting to it drops the pushed P.
    assert!(commits[0].unpushed && commits[1].unpushed && !commits[2].unpushed);
    assert!(drops_pushed(b, target).unwrap());
    assert!(!drops_pushed(b, merge).unwrap());
    // Undoing the merge (moving to its first parent, P) drops nothing pushed.
    assert!(!drops_pushed(b, &commits[0].parents[0]).unwrap());
    assert!(commits[2].on_origin && !commits[1].on_origin);
}

#[test]
fn log_all_lists_other_branches_and_marks_what_head_lacks() {
    let sb = Sandbox::new("logall");
    let c = sb.remote_with_clones(1);
    let a = &c[0];
    switch_branch(a, "feat", true).unwrap();
    write_commit(a, "f.txt", "f\n", "on feat");
    switch_branch(a, "main", false).unwrap();
    write_commit(a, "m.txt", "m\n", "on main");

    assert!(log(a, None, 0, 10)
        .unwrap()
        .iter()
        .all(|x| x.subject != "on feat"));
    let every = GraphRefs::default();
    let all = log_all(a, &every, 0, 10, &LogFilter::default()).unwrap();
    let find = |s: &str| all.iter().find(|x| x.subject == s).unwrap();
    let (feat, main, base) = (find("on feat"), find("on main"), find("base"));
    assert!(feat.not_in_head && !main.not_in_head && !base.not_in_head);
    assert!(main.unpushed && !feat.unpushed && base.on_origin);
    // One branch or all of them; a search goes through the same filter.
    let grep = LogFilter {
        grep: vec!["feat".into()],
        ..Default::default()
    };
    let found = log_all(a, &every, 0, 10, &grep).unwrap();
    assert_eq!(
        found.iter().map(|x| x.subject.as_str()).collect::<Vec<_>>(),
        ["on feat"]
    );
}

#[test]
fn log_all_shows_only_the_refs_asked_for() {
    let sb = Sandbox::new("logrefs");
    let c = sb.remote_with_clones(1);
    let a = &c[0];
    // Each off main: a local branch, a branch only on the remote, and a tag nothing else reaches.
    let side = |name: &str, file: &str, subject: &str| {
        switch_branch(a, "main", false).unwrap();
        switch_branch(a, name, true).unwrap();
        write_commit(a, file, "x\n", subject);
    };
    side("feat", "f.txt", "on feat");
    side("rem", "r.txt", "on rem");
    run(a, &["push", "-q", "origin", "rem"]).unwrap();
    side("tagged", "t.txt", "tagged");
    run(a, &["tag", "v1"]).unwrap();
    switch_branch(a, "main", false).unwrap();
    run(a, &["branch", "-q", "-D", "rem", "tagged"]).unwrap();

    let subjects = |refs: &GraphRefs| -> Vec<String> {
        let log = log_all(a, refs, 0, 20, &LogFilter::default()).unwrap();
        log.into_iter().map(|x| x.subject).collect()
    };
    let has = |refs: GraphRefs, s: &str| subjects(&refs).iter().any(|x| x == s);
    let every = GraphRefs::default;
    for s in ["on feat", "on rem", "tagged", "base"] {
        assert!(has(every(), s), "{s}");
    }
    let no_local = GraphRefs {
        local: false,
        ..every()
    };
    assert!(
        !has(no_local, "on feat")
            && has(
                GraphRefs {
                    local: false,
                    ..every()
                },
                "on rem"
            )
    );
    assert!(!has(
        GraphRefs {
            remote: false,
            ..every()
        },
        "on rem"
    ));
    assert!(!has(
        GraphRefs {
            tags: false,
            ..every()
        },
        "tagged"
    ));
    // HEAD's history stays, whatever is turned off.
    let nothing = GraphRefs {
        local: false,
        remote: false,
        tags: false,
        ..every()
    };
    assert_eq!(subjects(&nothing), ["base"]);

    let hidden = GraphRefs {
        hidden: vec!["refs/heads/feat".into(), "refs/remotes/origin/rem".into()],
        ..every()
    };
    let shown = subjects(&hidden);
    assert!(!shown.contains(&"on feat".into()) && !shown.contains(&"on rem".into()));
    assert!(shown.contains(&"tagged".into()));

    let only = GraphRefs {
        only: Some("refs/heads/feat".into()),
        ..every()
    };
    assert_eq!(subjects(&only), ["on feat", "base"]);

    let gone = GraphRefs {
        only: Some("refs/heads/gone".into()),
        ..every()
    };
    assert!(log_all(a, &gone, 0, 20, &LogFilter::default()).is_err());
    for bad in ["--all", "HEAD", "refs/heads/a..b", "feat"] {
        let refs = GraphRefs {
            hidden: vec![bad.into()],
            ..every()
        };
        assert!(
            log_all(a, &refs, 0, 20, &LogFilter::default()).is_err(),
            "{bad}"
        );
    }
}

#[test]
fn compare_lists_both_sides_and_counts_them() {
    let sb = Sandbox::new("compare");
    let c = sb.remote_with_clones(1);
    let a = &c[0];
    switch_branch(a, "feat", true).unwrap();
    write_commit(a, "f.txt", "f\n", "on feat");
    write_commit(a, "g.txt", "g\n", "more on feat");
    switch_branch(a, "main", false).unwrap();
    write_commit(a, "m.txt", "m\n", "on main");

    let with = "refs/heads/feat";
    assert_eq!(compare_counts(a, with).unwrap(), (1, 2));
    let subjects = |incoming| -> Vec<String> {
        let log = log_compare(a, with, incoming, 0, 20).unwrap();
        log.into_iter().map(|x| x.subject).collect()
    };
    assert_eq!(subjects(true), ["more on feat", "on feat"]);
    assert_eq!(subjects(false), ["on main"]);
    // What HEAD lacks is marked, so it can be picked from here.
    assert!(log_compare(a, with, true, 0, 20)
        .unwrap()
        .iter()
        .all(|x| x.not_in_head));
    assert!(log_compare(a, "HEAD", true, 0, 20).is_err());
    assert!(compare_counts(a, "refs/heads/feat..main").is_err());
}

#[test]
fn log_all_on_an_orphan_branch_lists_the_other_branches() {
    let sb = Sandbox::new("orphan");
    let c = sb.remote_with_clones(1);
    let a = &c[0];
    run(a, &["switch", "-q", "--orphan", "lonely"]).unwrap();

    let every = GraphRefs::default();
    let all = log_all(a, &every, 0, 20, &LogFilter::default()).unwrap();
    assert_eq!(
        all.iter().map(|x| x.subject.as_str()).collect::<Vec<_>>(),
        ["base"]
    );
    // No HEAD to be missing from, nothing of HEAD's to push.
    assert!(all.iter().all(|x| !x.not_in_head && !x.unpushed));
    // With every kind of ref turned off there's nothing to walk: an empty list, not HEAD's error.
    let nothing = GraphRefs {
        local: false,
        remote: false,
        tags: false,
        ..every
    };
    assert!(log_all(a, &nothing, 0, 20, &LogFilter::default())
        .unwrap()
        .is_empty());
}

#[test]
fn log_all_follows_a_file_on_head_only() {
    let sb = Sandbox::new("allfollow");
    let c = sb.remote_with_clones(1);
    let a = &c[0];
    write_commit(a, "old.txt", "one\ntwo\nthree\nfour\n", "add old");
    switch_branch(a, "side", true).unwrap();
    write_commit(a, "old.txt", "one\ntwo\nthree\nfour\nfive\n", "on side");
    switch_branch(a, "main", false).unwrap();
    run(a, &["mv", "old.txt", "new.txt"]).unwrap();
    run(a, &["commit", "-q", "-m", "rename"]).unwrap();

    let follow = LogFilter {
        paths: vec!["new.txt".into()],
        follow: true,
        ..Default::default()
    };
    let subjects = |log: Vec<Commit>| log.into_iter().map(|x| x.subject).collect::<Vec<_>>();
    let all = subjects(log_all(a, &GraphRefs::default(), 0, 20, &follow).unwrap());
    // Through the rename to the old name, as HEAD's own history has it; the side branch's
    // edit of the old name isn't in it.
    assert!(all.starts_with(&["rename".to_string(), "add old".to_string()]));
    assert!(!all.contains(&"on side".to_string()));
    assert_eq!(
        all,
        subjects(log_filtered(a, None, 0, 20, &follow).unwrap())
    );
}

#[test]
fn tree_paths_and_text_at_read_a_commit_and_its_parent() {
    let sb = Sandbox::new("tree-paths");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "src/a.ts", "one\n", "a");
    write_commit(&r, "src/lib/b.ts", "two\n", "b");
    let head = log(&r, None, 0, 1).unwrap()[0].sha.clone();
    assert_eq!(tree_paths(&r, &head).unwrap(), ["src/a.ts", "src/lib/b.ts"]);
    assert_eq!(tree_paths(&r, &format!("{head}^")).unwrap(), ["src/a.ts"]);
    assert_eq!(text_at(&r, &head, "src/lib/b.ts").unwrap().text, "two\n");
    assert!(
        !text_at(&r, &format!("{head}^"), "src/lib/b.ts")
            .unwrap()
            .exists
    );
    assert!(tree_paths(&r, "HEAD").is_err(), "only commit ids");
    assert!(tree_paths(&r, &format!("{head}^^")).is_err());
}

#[test]
fn compare_files_are_what_the_other_branch_changed_since_they_parted() {
    let sb = Sandbox::new("compare-files");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "a\n", "base");
    run(&r, &["switch", "-q", "-c", "feature"]).unwrap();
    write_commit(&r, "b.txt", "b\n", "on feature");
    run(&r, &["switch", "-q", "main"]).unwrap();
    // main moved on too: that isn't feature's change.
    write_commit(&r, "c.txt", "c\n", "on main");
    let c = compare_files(&r, "refs/heads/feature").unwrap();
    assert_eq!(
        c.files.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(),
        ["b.txt"]
    );
    assert_eq!(
        c.head,
        run_text(&r, &["rev-parse", "feature"]).unwrap().trim()
    );
    assert!(compare_files(&r, "feature").is_err(), "a full ref only");
}

#[test]
fn the_reflog_keeps_what_a_reset_left_behind() {
    let sb = Sandbox::new("reflog");
    let r = sb.path("r");
    assert!(reflog(&sb.0, 10).is_err() || reflog(&sb.0, 10).unwrap().is_empty());
    init(&r);
    assert!(reflog(&r, 10).unwrap().is_empty(), "no HEAD yet");
    write_commit(&r, "a.txt", "a\n", "one");
    write_commit(&r, "a.txt", "b\n", "two");
    let two = run_text(&r, &["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();
    run(&r, &["reset", "-q", "--hard", "HEAD~1"]).unwrap();
    let log = reflog(&r, 10).unwrap();
    assert_eq!(log[0].selector, "HEAD@{0}");
    assert!(
        log[0].message.starts_with("reset: moving to"),
        "{}",
        log[0].message
    );
    assert_eq!(log[1].sha, two, "the dropped commit is still listed");
    assert_eq!(reflog(&r, 1).unwrap().len(), 1);
}

#[test]
fn bisect_finds_the_first_bad_commit_and_stops() {
    let sb = Sandbox::new("bisect");
    let r = sb.path("r");
    init(&r);
    for i in 1..=8 {
        let content = format!("{} {i}\n", if i >= 5 { "broken" } else { "fine" });
        write_commit(&r, "state.txt", &content, &format!("c{i}"));
        write_commit(&r, &format!("f{i}.txt"), "x\n", &format!("f{i}"));
    }
    let branch = run_text(&r, &["branch", "--show-current"]).unwrap();
    let good = run_text(&r, &["log", "--format=%H", "--grep", "^c1$"])
        .unwrap()
        .trim()
        .to_string();
    let bad = run_text(&r, &["log", "--format=%H", "--grep", "^c5$"])
        .unwrap()
        .trim()
        .to_string();
    let mut step = bisect_start(&r, &good).unwrap();
    assert!(operation(&r).is_some_and(|o| o.kind == "bisect"));
    assert!(
        merge(&r, "main", MergeKind::Ff)
            .unwrap_err()
            .contains("bisect"),
        "nothing else starts meanwhile"
    );
    let mut rounds = 0;
    while step.first_bad.is_none() {
        rounds += 1;
        assert!(rounds < 10, "{step:?}");
        let broken = fs::read_to_string(r.join("state.txt"))
            .unwrap()
            .starts_with("broken");
        step = bisect_mark(&r, if broken { "bad" } else { "good" }).unwrap();
    }
    assert_eq!(step.first_bad.as_deref(), Some(bad.as_str()));
    assert!(bisect_mark(&r, "maybe").is_err());
    op_abort(&r).unwrap();
    assert!(operation(&r).is_none());
    assert_eq!(run_text(&r, &["branch", "--show-current"]).unwrap(), branch);
    assert!(bisect_mark(&r, "good").is_err());
}
