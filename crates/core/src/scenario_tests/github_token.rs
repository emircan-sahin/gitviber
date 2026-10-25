//! Looking for a GitHub token in git's credential store, with a recorder for the helper and for
//! askpass: nothing real (the Keychain, a login window) is ever reached.

use super::*;
use std::os::unix::fs::PermissionsExt;

fn script(path: &Path, body: &str) {
    fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

/// A repo with an ssh origin whose only credential helper is a recorder (`token`: what it
/// answers, None: nothing stored) and whose askpass is a recorder too. Returns the repo and the
/// two logs.
fn repo(sb: &Sandbox, token: Option<&str>) -> (PathBuf, PathBuf, PathBuf) {
    let r = sb.path("r");
    init(&r);
    run(
        &r,
        &[
            "remote",
            "add",
            "origin",
            "git@github.com:octo-one/demo.git",
        ],
    )
    .unwrap();
    let (helper, askpass) = (sb.path("helper.sh"), sb.path("askpass.sh"));
    let (helper_log, askpass_log) = (sb.path("helper.log"), sb.path("askpass.log"));
    let answer = token
        .map(|t| format!("printf 'username=octo-one\\npassword={t}\\n'"))
        .unwrap_or_default();
    script(
        &helper,
        &format!(
            "[ \"$1\" = get ] || exit 0\necho get >> '{}'\ncat > '{}'\n{answer}",
            helper_log.display(),
            sb.path("helper.in").display()
        ),
    );
    script(
        &askpass,
        &format!("echo \"$1\" >> '{}'", askpass_log.display()),
    );
    // The empty value first drops the user's own helpers (osxkeychain).
    run(&r, &["config", "credential.helper", ""]).unwrap();
    run(
        &r,
        &[
            "config",
            "--add",
            "credential.helper",
            &helper.display().to_string(),
        ],
    )
    .unwrap();
    run(
        &r,
        &["config", "core.askPass", &askpass.display().to_string()],
    )
    .unwrap();
    (r, helper_log, askpass_log)
}

fn lines(log: &Path) -> usize {
    fs::read_to_string(log).map_or(0, |s| s.lines().count())
}

#[test]
fn a_stored_token_is_read_with_one_helper_call_and_no_prompt() {
    let sb = Sandbox::new("ghtoken-stored");
    let (r, helper, askpass) = repo(&sb, Some("tok-1234"));
    assert_eq!(credential_token(&r).as_deref(), Some("tok-1234"));
    assert_eq!(lines(&helper), 1);
    assert_eq!(lines(&askpass), 0);
}

#[test]
fn nothing_stored_is_none_and_never_asks_for_a_password() {
    let sb = Sandbox::new("ghtoken-none");
    let (r, helper, askpass) = repo(&sb, None);
    assert_eq!(credential_token(&r), None);
    assert_eq!(lines(&helper), 1);
    assert_eq!(lines(&askpass), 0, "git fell through to askpass");
}

/// The request names github.com whatever origin says: a look-alike or odd remote never
/// becomes the host the helper is asked about.
#[test]
fn the_helper_is_only_asked_about_github_com() {
    let sb = Sandbox::new("ghtoken-host");
    let (r, _, _) = repo(&sb, Some("tok-1234"));
    run(
        &r,
        &[
            "remote",
            "set-url",
            "origin",
            "https://github.com.example.org/o/r",
        ],
    )
    .unwrap();
    assert_eq!(credential_token(&r).as_deref(), Some("tok-1234"));
    let asked = fs::read_to_string(sb.path("helper.in")).unwrap();
    assert_eq!(asked, "protocol=https\nhost=github.com\n");
}
