//! What diffs show: binary, line endings, LFS, odd paths, symlinks, submodule bumps.

use super::*;

#[test]
fn binary_crlf_and_missing_trailing_newline() {
    let sb = Sandbox::new("content");
    let r = sb.path("r");
    init(&r);
    fs::write(r.join("bin.dat"), [0u8, 1, 2, 3, 0, 5]).unwrap();
    write_commit(&r, "crlf.txt", "a\r\nb\r\nc\r\n", "crlf");
    write_commit(&r, "nonl.txt", "x\ny", "no newline");
    stage(&r, &["bin.dat".into()]).unwrap();
    commit(&r, "bin", &CommitOptions::default()).unwrap();

    fs::write(r.join("bin.dat"), [0u8, 9, 9]).unwrap();
    fs::write(r.join("crlf.txt"), "a\r\nB\r\nc\r\n").unwrap();
    fs::write(r.join("nonl.txt"), "x\ny\n").unwrap();
    let wt = |p: &str| vfs::read_file(&r, p);

    let bin = diff_pair(&r, "unstaged", "bin.dat", None, None, None, None, wt).unwrap();
    assert!(bin.modified.binary && bin.rows.is_empty());

    let crlf = diff_pair(&r, "unstaged", "crlf.txt", None, None, None, None, wt).unwrap();
    let kinds: Vec<u8> = crlf.rows.iter().map(|x| x.k).collect();
    assert_eq!(kinds, vec![0, 2, 1, 0]);
    // Emphasis must not include the \r.
    assert!(crlf.rows[2].e.iter().all(|[_, end]| *end <= 1));

    let nonl = diff_pair(&r, "unstaged", "nonl.txt", None, None, None, None, wt).unwrap();
    assert!(
        nonl.rows.iter().all(|x| x.o <= 2 && x.n <= 2),
        "line counts match the frontend's split"
    );
}

#[test]
fn autocrlf_diffs_like_git() {
    let sb = Sandbox::new("autocrlf");
    let r = sb.path("r");
    init(&r);
    run(&r, &["config", "core.autocrlf", "true"]).unwrap();
    // Stored with LF, checked out (here: written) with CRLF.
    write_commit(&r, "a.txt", "a\r\nb\r\nc\r\n", "crlf");
    assert_eq!(
        run_text(&r, &["cat-file", "blob", "HEAD:a.txt"]).unwrap(),
        "a\nb\nc\n"
    );
    fs::write(r.join("a.txt"), "a\r\nB\r\nc\r\n").unwrap();
    let wt = |p: &str| vfs::read_file(&r, p);
    for kind in ["unstaged", "worktree"] {
        let pair = diff_pair(&r, kind, "a.txt", None, None, None, None, wt).unwrap();
        let kinds: Vec<u8> = pair.rows.iter().map(|x| x.k).collect();
        assert_eq!(kinds, vec![0, 2, 1, 0], "{kind}: only the changed line");
        assert!(!pair.eol_only);
    }
    stage(&r, &["a.txt".into()]).unwrap();
    commit(&r, "B", &CommitOptions::default()).unwrap();
    let head = rev(&r, "HEAD");
    let pair = diff_pair(&r, "commit", "a.txt", None, Some(&head), None, None, wt).unwrap();
    assert_eq!(pair.modified.text, "a\r\nB\r\nc\r\n");
    assert_eq!(pair.rows.iter().filter(|x| x.k != 0).count(), 2);

    // Without the setting a CRLF copy of an LF file is a real change, of line endings only.
    run(&r, &["config", "core.autocrlf", "false"]).unwrap();
    write_commit(&r, "lf.txt", "x\ny\n", "lf");
    fs::write(r.join("lf.txt"), "x\r\ny\r\n").unwrap();
    let pair = diff_pair(&r, "unstaged", "lf.txt", None, None, None, None, wt).unwrap();
    assert!(pair.eol_only && pair.rows.iter().any(|x| x.k != 0));
    // Ignoring whitespace hides it, and says so.
    let pair = diff_pair(
        &r,
        "unstaged",
        "lf.txt",
        None,
        None,
        None,
        Some("amount"),
        wt,
    )
    .unwrap();
    assert!(pair.whitespace_hidden && pair.rows.iter().all(|x| x.k == 0));
}

const LFS_OID: &str = "4d7a214614ab2935c943f9e0ff69d22eadbb8f32b1258daaa5e2ca24d17e2393";

fn lfs_pointer(oid: &str, size: usize) -> String {
    format!("version https://git-lfs.github.com/spec/v1\noid sha256:{oid}\nsize {size}\n")
}

/// Pointers committed as plain files: no git-lfs needed to read what the store has or lacks.
#[test]
fn lfs_pointers_show_the_object_or_its_size() {
    let sb = Sandbox::new("lfs-pointer");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "big.bin", &lfs_pointer(LFS_OID, 13_002_342), "pointer");
    write_commit(&r, "pic.png", &lfs_pointer(LFS_OID, 5), "pointer");
    let head = rev(&r, "HEAD");
    let none = |_: &str| FileText::default();
    let pair = diff_pair(&r, "commit", "big.bin", None, Some(&head), None, None, none).unwrap();
    assert_eq!(
        pair.modified.lfs_missing.as_deref(),
        Some("LFS object not downloaded (12.4 MB)")
    );
    assert!(pair.rows.is_empty());
    let got = media(
        &r,
        "commit",
        "pic.png",
        None,
        Some(&head),
        None,
        false,
        |_| Err("unused".into()),
    );
    assert_eq!(got.unwrap_err(), "LFS object not downloaded (5 B)");

    // Once downloaded, the object stands in for the pointer on every side.
    let dir = r.join(format!(
        ".git/lfs/objects/{}/{}",
        &LFS_OID[..2],
        &LFS_OID[2..4]
    ));
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join(LFS_OID), "hello").unwrap();
    let got = media(
        &r,
        "commit",
        "pic.png",
        None,
        Some(&head),
        None,
        false,
        |_| Err("unused".into()),
    );
    assert_eq!(got.unwrap(), b"hello");
    let wt = |p: &str| vfs::read_file(&r, p);
    let pair = diff_pair(&r, "worktree", "pic.png", None, None, None, None, wt).unwrap();
    assert_eq!(
        (pair.original.text.as_str(), pair.modified.text.as_str()),
        ("hello", "hello")
    );
}

/// The real thing, when git-lfs is installed: the stored side is the object, never a download.
#[test]
fn lfs_tracked_file_diffs_as_its_content() {
    let sb = Sandbox::new("lfs");
    let r = sb.path("r");
    init(&r);
    if run(&r, &["lfs", "install", "--local"]).is_err() {
        eprintln!("git-lfs is not installed; skipping");
        return;
    }
    write_commit(
        &r,
        ".gitattributes",
        "*.png filter=lfs diff=lfs merge=lfs -text\n",
        "lfs",
    );
    write_commit(&r, "pic.png", "\u{89}PNG old", "pic");
    let stored = run(&r, &["cat-file", "blob", "HEAD:pic.png"]).unwrap();
    assert!(
        crate::lfs::pointer(&stored).is_some(),
        "stored as a pointer"
    );
    fs::write(r.join("pic.png"), "\u{89}PNG new").unwrap();
    let wt = |p: &str| vfs::read_media(&r, p);
    let before = media(&r, "unstaged", "pic.png", None, None, None, true, wt).unwrap();
    assert_eq!(before, "\u{89}PNG old".as_bytes());

    // Gone from the store (a partial clone, say): it says so rather than fetching it.
    let oid = crate::lfs::pointer(&stored).unwrap().oid;
    let common = r.join(".git/lfs/objects");
    fs::remove_file(common.join(&oid[..2]).join(&oid[2..4]).join(&oid)).unwrap();
    let err = media(&r, "unstaged", "pic.png", None, None, None, true, wt).unwrap_err();
    assert!(err.starts_with("LFS object not downloaded"), "{err}");
}

#[test]
fn paths_with_spaces_unicode_and_renames() {
    let sb = Sandbox::new("paths");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "dir with space/ümlaut é.txt", "1\n2\n", "odd names");
    run(
        &r,
        &[
            "mv",
            "dir with space/ümlaut é.txt",
            "dir with space/renamed ü.txt",
        ],
    )
    .unwrap();
    fs::write(r.join("dir with space/renamed ü.txt"), "1\n2\n3\n").unwrap();
    let st = status(&r).unwrap();
    let staged = st
        .staged
        .iter()
        .find(|f| f.status == "R")
        .expect("staged rename");
    assert_eq!(staged.path, "dir with space/renamed ü.txt");
    assert_eq!(
        staged.old_path.as_deref(),
        Some("dir with space/ümlaut é.txt")
    );
    let unstaged = st
        .unstaged
        .iter()
        .find(|f| f.path == "dir with space/renamed ü.txt")
        .expect("worktree edit");
    assert_eq!((unstaged.additions, unstaged.deletions), (Some(1), Some(0)));
    let listing = vfs::list_dir(&r, "dir with space").unwrap();
    assert_eq!(listing[0].name, "renamed ü.txt");
}

/// git runs in English (the app reads its messages) and UTF-8, so a translated git can't
/// break the checks, and non-ASCII names and messages still come back as written.
#[test]
fn english_git_keeps_utf8_names_and_messages() {
    use crate::process::search_path;
    use std::ffi::OsStr;
    let cmd = command(Path::new("."), &["status"]);
    let env: Vec<_> = cmd.get_envs().collect();
    let lc_all = OsStr::new("LC_ALL");
    let language = OsStr::new("LANGUAGE");
    assert!(env.contains(&(lc_all, Some(OsStr::new("en_US.UTF-8")))));
    assert!(env.contains(&(language, None)), "LANGUAGE is cleared");

    let sb = Sandbox::new("utf8");
    let r = sb.path("r");
    init(&r);
    let (path, message) = ("şehir/ağaç 🌳.txt", "Grüße, çay ve 日本語 🎉");
    write_commit(&r, path, "x\n", message);
    let head = &log(&r, None, 0, 1).unwrap()[0];
    assert_eq!(head.subject, message);
    let files = commit_files(&r, &head.sha).unwrap();
    assert_eq!(files[0].path, path);
    fs::write(r.join(path), "y\n").unwrap();
    assert_eq!(status(&r).unwrap().unstaged[0].path, path);
    let outside = sb.path("plain");
    fs::create_dir_all(&outside).unwrap();
    assert_eq!(toplevel(&outside).unwrap_err(), NOT_A_REPO);

    // A German setup (Homebrew's git ships its translations) still gets English.
    let mut german = std::process::Command::new("git");
    german
        .env("LANGUAGE", "de")
        .env("LANG", "de_DE.UTF-8")
        .env("LC_ALL", "de_DE.UTF-8")
        .env("PATH", search_path())
        .current_dir(&outside)
        .args(["rev-parse", "--show-toplevel"]);
    let out = in_english(&mut german).output().unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("not a git repository"), "{err}");
}

#[cfg(unix)]
#[test]
fn symlinks_cannot_escape_the_repo() {
    let sb = Sandbox::new("escape");
    let r = sb.path("r");
    init(&r);
    let outside = sb.path("outside");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("secret.txt"), "secret").unwrap();
    std::os::unix::fs::symlink(&outside, r.join("link")).unwrap();

    assert!(
        !vfs::read_file(&r, "link/secret.txt").exists,
        "read through a symlink"
    );
    assert!(
        vfs::write_file(&r, "link/secret.txt", "pwned").is_err(),
        "overwrite through a symlink"
    );
    assert!(
        vfs::write_file(&r, "link/new.txt", "pwned").is_err(),
        "create through a symlinked parent"
    );
    assert!(!outside.join("new.txt").exists());
    assert_eq!(
        fs::read_to_string(outside.join("secret.txt")).unwrap(),
        "secret"
    );
    // Normal writes, including new files in new-ish places, still work.
    vfs::write_file(&r, "ok.txt", "fine").unwrap();
}

#[test]
fn git_internals_are_off_limits() {
    let sb = Sandbox::new("dotgit");
    let r = sb.path("r");
    init(&r);
    for p in [
        ".git/config",
        ".GIT/config",
        "sub/.git/hooks/pre-commit",
        ".git/hooks/pre-commit",
    ] {
        assert!(vfs::write_file(&r, p, "x").is_err(), "{p}");
    }
    assert!(!vfs::read_file(&r, ".git/config").exists);
    assert_ne!(fs::read_to_string(r.join(".git/config")).unwrap(), "x");
}

#[test]
fn special_and_non_utf8_files_are_safe() {
    let sb = Sandbox::new("special");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "x.txt", "x\n", "base");
    // An untracked FIFO would block a naive read forever.
    assert!(std::process::Command::new("mkfifo")
        .arg(r.join("pipe"))
        .status()
        .unwrap()
        .success());
    // git skips non-regular files; what matters is that nothing blocks on the pipe.
    status(&r).unwrap();
    assert!(!vfs::read_file(&r, "pipe").exists);

    fs::write(r.join("latin1.txt"), [b'c', b'a', b'f', 0xE9, b'\n']).unwrap();
    let f = vfs::read_file(&r, "latin1.txt");
    assert!(f.exists && f.lossy);
    assert!(!vfs::read_file(&r, "x.txt").lossy);
}

#[test]
fn submodule_bump_diffs_as_subproject_commits() {
    let sb = Sandbox::new("subdiff");
    let r = repo_with_submodule(&sb);
    let sub = r.join("sub");
    identity(&sub);
    let old = run(&sub, &["rev-parse", "HEAD"]).unwrap();
    let old = String::from_utf8_lossy(&old).trim().to_string();
    write_commit(&sub, "l.txt", "l2\n", "bump");
    let new = run(&sub, &["rev-parse", "HEAD"]).unwrap();
    let new = String::from_utf8_lossy(&new).trim().to_string();
    let read = |p: &str| vfs::read_file(&r, p);

    let pair = diff_pair(&r, "unstaged", "sub", None, None, None, None, read).unwrap();
    assert_eq!(pair.original.text, format!("Subproject commit {old}\n"));
    assert_eq!(pair.modified.text, format!("Subproject commit {new}\n"));
    assert!(pair.rows.iter().any(|row| row.k != 0));

    fs::write(sub.join("l.txt"), "dirty\n").unwrap();
    let pair = diff_pair(&r, "unstaged", "sub", None, None, None, None, read).unwrap();
    assert_eq!(
        pair.modified.text,
        format!("Subproject commit {new}-dirty\n")
    );

    stage(&r, &["sub".into()]).unwrap();
    commit(&r, "bump sub", &CommitOptions::default()).unwrap();
    let head = log(&r, None, 0, 1).unwrap().remove(0).sha;
    let pair = diff_pair(&r, "commit", "sub", None, Some(&head), None, None, read).unwrap();
    assert_eq!(pair.original.text, format!("Subproject commit {old}\n"));
    assert_eq!(pair.modified.text, format!("Subproject commit {new}\n"));
    // A plain directory is still not a submodule.
    assert!(
        !diff_pair(&r, "unstaged", "nope", None, None, None, None, read)
            .unwrap()
            .modified
            .exists
    );
}
