//! Commits: options, hooks, templates, identity, and a first commit in a new repo.

use super::*;

fn executable(path: &Path, script: &str) {
    fs::write(path, script).unwrap();
    std::process::Command::new("chmod")
        .args(["+x", path.to_str().unwrap()])
        .status()
        .unwrap();
}

/// Launched from Finder the app has a bare PATH; a hook calling a tool only the login shell
/// adds (node from nvm, say) failed with 127. The "login shell" here prepends one folder.
#[cfg(unix)]
#[test]
fn hooks_find_tools_on_the_login_shell_path() {
    use crate::process::{exec, merge_paths};
    use std::ffi::OsStr;
    let sb = Sandbox::new("login-path");
    let tools = sb.path("tools");
    fs::create_dir_all(&tools).unwrap();
    executable(&tools.join("gitviber-lint"), "#!/bin/sh\nexit 0\n");
    let shell = sb.path("login-sh");
    // Called as `login-sh -ilc <command>`.
    executable(
        &shell,
        &format!(
            "#!/bin/sh\necho 'Welcome back!'\nPATH=\"{}:$PATH\"; export PATH\nexec /bin/sh -c \"$2\"\n",
            tools.display()
        ),
    );
    let r = sb.path("r");
    init(&r);
    executable(
        &r.join(".git/hooks/pre-commit"),
        "#!/bin/sh\nexec gitviber-lint\n",
    );
    fs::write(r.join("a.txt"), "a\n").unwrap();
    stage(&r, &["a.txt".into()]).unwrap();
    let commit_with = |path: &OsStr| {
        let mut cmd = command(&r, &["commit", "-q", "-m", "hooked"]);
        cmd.env("PATH", path);
        exec(cmd, "git commit", &[], None, None)
    };

    let app_path = OsStr::new("/usr/bin:/bin:/usr/sbin:/sbin");
    let err = commit_with(&merge_paths(None, app_path)).unwrap_err();
    assert!(err.contains("gitviber-lint"), "{err}");

    let login = crate::shell::probe_path(&shell, std::time::Duration::from_secs(3)).unwrap();
    let merged = merge_paths(Some(&login), app_path);
    assert!(merged
        .to_str()
        .unwrap()
        .starts_with(tools.to_str().unwrap()));
    commit_with(&merged).unwrap();
    assert_eq!(log(&r, None, 0, 1).unwrap()[0].subject, "hooked");
}

#[test]
fn amend_without_message_keeps_the_old_one() {
    let sb = Sandbox::new("amend");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "a\n", "original message");
    fs::write(r.join("b.txt"), "b\n").unwrap();
    stage(&r, &["b.txt".into()]).unwrap();
    commit(&r, "  ", &AMEND).unwrap();
    let head = &log(&r, None, 0, 5).unwrap()[0];
    assert_eq!(head.subject, "original message");
    assert_eq!(log(&r, None, 0, 5).unwrap().len(), 1);
}

/// Co-authors become trailers git itself formats, sign-off adds the committer's line and
/// --no-verify gets past a failing hook; the commit header reads the trailers back.
#[test]
fn commit_options_trailers_sign_off_and_skipped_hooks() {
    let sb = Sandbox::new("commit-options");
    let r = sb.path("r");
    init(&r);
    write_commit(&r, "a.txt", "a\n", "base");
    let hook = r.join(".git/hooks/pre-commit");
    fs::write(&hook, "#!/bin/sh\necho 'lint failed' >&2\nexit 1\n").unwrap();
    std::process::Command::new("chmod")
        .args(["+x", hook.to_str().unwrap()])
        .status()
        .unwrap();
    fs::write(r.join("b.txt"), "b\n").unwrap();
    stage(&r, &["b.txt".into()]).unwrap();

    let err = commit(&r, "Add b", &CommitOptions::default()).unwrap_err();
    assert!(err.contains("lint failed"), "{err}");
    let claude = "Claude <noreply@anthropic.com>";
    let opts = CommitOptions {
        sign_off: true,
        no_verify: true,
        co_authors: vec![claude.into()],
        ..Default::default()
    };
    commit(&r, "Add b\n\nWhy it matters.", &opts).unwrap();
    let head = &log(&r, None, 0, 1).unwrap()[0];
    assert_eq!(head.subject, "Add b");
    assert_eq!(
        head.body,
        format!("Why it matters.\n\nSigned-off-by: T <t@example.com>\nCo-authored-by: {claude}")
    );
    let details = commit_details(&r, &head.sha).unwrap();
    assert_eq!(
        (details.signature.as_str(), details.sign_expected),
        ("N", false)
    );
    assert_eq!(
        details.trailers,
        [
            ("Signed-off-by".to_string(), "T <t@example.com>".to_string()),
            ("Co-authored-by".to_string(), claude.to_string()),
        ]
    );

    // Amending without a new message keeps it and still takes a new co-author; the hook
    // runs again once --no-verify is off.
    let ada = "Ada <ada@example.com>";
    let amend = |no_verify| CommitOptions {
        amend: true,
        no_verify,
        co_authors: vec![ada.into()],
        ..Default::default()
    };
    assert!(commit(&r, "", &amend(false)).is_err());
    commit(&r, "", &amend(true)).unwrap();
    let head = &log(&r, None, 0, 1).unwrap()[0];
    assert!(
        head.body.ends_with(&format!("Co-authored-by: {ada}")),
        "{}",
        head.body
    );
    assert_eq!(log(&r, None, 0, 5).unwrap().len(), 2);

    // A co-author can't smuggle in a line of its own.
    let bad = CommitOptions {
        co_authors: vec!["Eve <e@x>\nSigned-off-by: Mallory <m@x>".into()],
        no_verify: true,
        ..Default::default()
    };
    assert!(commit(&r, "x", &bad).is_err());

    // Suggestions: co-authors and authors, newest first, never the user.
    assert_eq!(recent_authors(&r).unwrap(), [claude, ada]);
}

/// The template's text comes back the way git starts the editor with it, comments gone.
#[test]
fn commit_template_is_read_without_comments() {
    let sb = Sandbox::new("template");
    let r = sb.path("r");
    init(&r);
    assert_eq!(commit_template(&r), None);
    fs::write(
        r.join(".git/msg"),
        "\nWhy:\n# say why, not what\n\n\nRefs:\n",
    )
    .unwrap();
    run(&r, &["config", "commit.template", ".git/msg"]).unwrap();
    assert_eq!(commit_template(&r).as_deref(), Some("Why:\n\nRefs:"));
}

/// A new repository has no commits yet: every view reads it as empty, and staging, unstaging
/// and the first commit work before HEAD exists.
#[test]
fn init_then_first_commit() {
    let sb = Sandbox::new("init");
    let r = sb.path("new");
    fs::create_dir_all(&r).unwrap();
    crate::git::init(&r).unwrap();
    identity(&r);
    assert!(crate::git::init(&r).is_err(), "already a repository");
    let first = run_text(&r, &["config", "--get", "init.defaultBranch"])
        .map(|b| b.trim().to_string())
        .unwrap_or_else(|_| "main".into());
    let st = status(&r).unwrap();
    assert_eq!(st.branch.as_deref(), Some(first.as_str()));
    assert!(st.head.is_none() && st.upstream.is_none() && st.remotes.is_empty());
    assert!(log(&r, None, 0, 10).unwrap().is_empty());
    branches(&r).unwrap();
    worktrees(&r).unwrap();

    fs::write(r.join("a.txt"), "a\n").unwrap();
    stage(&r, &["a.txt".into()]).unwrap();
    unstage(&r, &["a.txt".into()]).unwrap();
    assert_eq!(status(&r).unwrap().unstaged.len(), 1);
    stage(&r, &["a.txt".into()]).unwrap();
    let j = Journal::default();
    j.record(&r, Action::new("Commit", Mode::Soft), |r| {
        commit(r, "first", &CommitOptions::default())
    })
    .unwrap();
    assert_eq!(log(&r, None, 0, 10).unwrap().len(), 1);
    assert!(publish_remote(&r).unwrap_err().contains("no remote"));
    // Undoing the first commit makes the branch unborn again, its file still staged.
    step(&j, &r, false).unwrap();
    let st = status(&r).unwrap();
    assert!(st.head.is_none() && st.staged.len() == 1);
}

#[test]
fn a_repository_of_its_own_identity_and_back_to_global() {
    let sb = Sandbox::new("repo-identity");
    let r = sb.path("r");
    fs::create_dir_all(&r).unwrap();
    run(&r, &["init", "-q", "-b", "main"]).unwrap();
    assert!(repo_identity(&r).name.is_none());
    set_repo_identity(&r, Some(("Work Me", "me@work.example"))).unwrap();
    let own = repo_identity(&r);
    assert_eq!(
        (own.name.as_deref(), own.email.as_deref()),
        (Some("Work Me"), Some("me@work.example"))
    );
    assert_eq!(
        crate::git::identity(&r).email.as_deref(),
        Some("me@work.example")
    );
    assert!(set_repo_identity(&r, Some(("two\nlines", "a@b"))).is_err());
    set_repo_identity(&r, None).unwrap();
    assert!(repo_identity(&r).name.is_none() && repo_identity(&r).email.is_none());
    // Unsetting what isn't set is fine.
    set_repo_identity(&r, None).unwrap();
}
