//! The terminal's commit links: what `known_commits` says of the hex a screen shows.

use super::*;

fn rev(repo: &Path, spec: &str) -> String {
    run_text(repo, &["rev-parse", spec])
        .unwrap()
        .trim()
        .to_string()
}

/// A screen of hints asks once for everything that looks like a SHA: short and full ids of
/// commits, a tag's (its commit), trees and blobs (`git ls-tree`), other repos' ids and words
/// that are hex, each answered in its place.
#[test]
fn a_screen_of_candidates_is_answered_in_order() {
    let sb = Sandbox::new("tl-screen");
    let r = sb.path("r");
    init(&r);
    for i in 0..30 {
        write_commit(
            &r,
            &format!("f{i}.txt"),
            &format!("{i}\n"),
            &format!("c{i}"),
        );
    }
    run(&r, &["tag", "-a", "-m", "release", "v1", "HEAD~3"]).unwrap();
    let tag = rev(&r, "v1");
    let tagged = rev(&r, "v1^{commit}");
    let blob = rev(&r, "HEAD:f0.txt");
    let tree = rev(&r, "HEAD^{tree}");
    let mut asked = Vec::new();
    let mut want = Vec::new();
    for i in 0..30 {
        let id = rev(&r, &format!("HEAD~{i}"));
        for len in [7, 9, 12, 40] {
            asked.push(id[..len].to_string());
            want.push(Some(id.clone()));
        }
    }
    for (s, w) in [
        (tag.clone(), Some(tagged.clone())),
        (tag[..8].to_string(), Some(tagged)),
        (blob.clone(), None),
        (blob[..7].to_string(), None),
        (tree, None),
        // Another repo's, sha1sum's, words and things that aren't hex at all.
        ("0123456789abcdef0123456789abcdef01234567".into(), None),
        ("defaced".into(), None),
        ("deadbeef".into(), None),
        ("".into(), None),
        ("abc".into(), None),
        ("g123456".into(), None),
        ("a".repeat(41), None),
        ("HEAD".into(), None),
        ("--all".into(), None),
        ("main".into(), None),
        ("12345678\n9abcdef".into(), None),
    ] {
        asked.push(s);
        want.push(w);
    }
    while asked.len() < 500 {
        asked.push(format!("{:040x}", asked.len() * 7919));
        want.push(None);
    }
    assert_eq!(known_commits(&r, &asked).unwrap(), want);
    asked.push("abcdef1".into());
    assert!(known_commits(&r, &asked).is_err(), "501 at once");
    assert_eq!(
        known_commits(&r, &[]).unwrap(),
        Vec::<Option<String>>::new()
    );
}

/// A short id two commits share is no link, and the answers after it stay in line.
#[test]
fn an_ambiguous_short_id_is_no_link() {
    let sb = Sandbox::new("tl-ambiguous");
    let r = sb.path("r");
    init(&r);
    // 40000 commits (fixed dates, so the same ids each run) make 7-digit prefixes collide.
    let mut stream = String::from("blob\nmark :1\ndata 2\na\n");
    for i in 0..40_000 {
        let msg = format!("c{i}");
        stream += &format!(
            "commit refs/heads/main\nmark :{}\ncommitter T <t@example.com> 1700000000 +0000\ndata {}\n{msg}\n",
            i + 2,
            msg.len()
        );
        stream += &if i == 0 {
            "M 100644 :1 a.txt\n".to_string()
        } else {
            format!("from :{}\n", i + 1)
        };
    }
    run_with(
        &r,
        &["fast-import", "--quiet"],
        &[],
        Some(stream.as_bytes()),
    )
    .unwrap();
    let ids = run_text(&r, &["rev-list", "main"]).unwrap();
    let mut seen = std::collections::HashMap::new();
    let shared = ids
        .lines()
        .find_map(|id| seen.insert(&id[..7], id).map(|_| id[..7].to_string()))
        .expect("a shared 7-digit prefix");
    let head = rev(&r, "main");
    let asked = [
        head[..7].to_string(),
        shared.clone(),
        head.clone(),
        format!("{shared}0"),
    ];
    let known = known_commits(&r, &asked).unwrap();
    assert_eq!(known[0], Some(head.clone()));
    assert_eq!(known[1], None, "{shared} names two commits");
    assert_eq!(known[2], Some(head));
}

/// A repo whose folder has spaces and non-ASCII in it, an unborn one, and a linked worktree.
#[test]
fn odd_repos() {
    let sb = Sandbox::new("tl-odd");
    let r = sb.path("my repo ş $x");
    init(&r);
    assert_eq!(
        known_commits(&r, &["abcdef1".into(), "0".repeat(40)]).unwrap(),
        [None, None],
        "nothing committed yet"
    );
    write_commit(&r, "a.txt", "a\n", "first");
    let head = rev(&r, "HEAD");
    let wt = sb.path("wt ü");
    run(
        &r,
        &["worktree", "add", "-q", "-b", "side", wt.to_str().unwrap()],
    )
    .unwrap();
    assert_eq!(
        known_commits(&wt, &[head[..7].to_string()]).unwrap(),
        [Some(head)]
    );
}

/// A partial clone (`--filter=blob:none`, as big monorepos are cloned) fetches an object it
/// lacks on demand. Hovering a line with another repo's SHA, or `sha1sum` output, mustn't reach
/// the network: git fetched from origin twice per unknown full id (40 fetches for 20 ids).
#[cfg(unix)]
#[test]
fn a_partial_clone_is_never_fetched_from() {
    let sb = Sandbox::new("tl-partial");
    let origin = sb.path("origin");
    init(&origin);
    write_commit(&origin, "a.txt", "a\n", "one");
    write_commit(&origin, "b.txt", "b\n", "two");
    run(&origin, &["config", "uploadpack.allowFilter", "true"]).unwrap();
    run(
        &origin,
        &["config", "uploadpack.allowAnySHA1InWant", "true"],
    )
    .unwrap();
    let blob = rev(&origin, "HEAD:b.txt");
    let part = sb.path("part");
    let url = format!("file://{}", origin.display());
    run(
        &sb.0,
        &[
            "clone",
            "-q",
            "--filter=blob:none",
            &url,
            part.to_str().unwrap(),
        ],
    )
    .unwrap();
    // Each fetch from origin leaves a line here.
    let log = sb.path("fetches.log");
    let recorder = sb.path("upload-pack");
    fs::write(
        &recorder,
        format!(
            "#!/bin/sh\necho fetch >> '{}'\nexec git-upload-pack \"$@\"\n",
            log.display()
        ),
    )
    .unwrap();
    std::process::Command::new("chmod")
        .args(["+x", recorder.to_str().unwrap()])
        .status()
        .unwrap();
    run(
        &part,
        &[
            "config",
            "remote.origin.uploadpack",
            recorder.to_str().unwrap(),
        ],
    )
    .unwrap();
    let head = rev(&part, "HEAD");
    let asked = [
        head.clone(),
        "0123456789abcdef0123456789abcdef01234567".into(),
        blob,
    ];
    assert_eq!(
        known_commits(&part, &asked).unwrap(),
        [Some(head), None, None]
    );
    let fetched = fs::read_to_string(&log).unwrap_or_default();
    assert_eq!(
        fetched.lines().count(),
        0,
        "known_commits fetched from origin"
    );
}
