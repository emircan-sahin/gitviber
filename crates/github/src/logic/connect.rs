//! What a list panel shows in place of its lists (no GitHub repository, no login, one GitHub
//! doesn't show origin to), and what the signed-out screen says.

use crate::model::is_not_connected;

/// Core's answers when origin isn't a repository on GitHub, before any request is made.
const NO_ORIGIN: &str = "This repository has no 'origin' remote.";
const NOT_GITHUB: &str = "The 'origin' remote is not a github.com repository.";

/// What a list panel shows instead of its lists.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gate {
    /// No 'origin' remote: one to add.
    NoOrigin,
    /// Origin is a repository somewhere other than github.com.
    NotGitHub,
    /// No login, or (`hidden`) one GitHub doesn't show origin to.
    SignIn { hidden: bool },
}

/// GitHub's 404 for a repository the token can't see, as it answers for a private one to
/// anyone outside it (and for one that's gone).
pub fn is_hidden_repo(err: &str) -> bool {
    err.starts_with("GitHub 404") || err.starts_with("GitHub 401")
}

/// From the account's read and origin's list's: `read` whether the account was read, `shown` the
/// sign-in screen's `hidden` while it's up. A sign-in found there reads everything again; the
/// screen stays until the account answers, rather than flashing an empty list.
pub fn gate(
    read: bool,
    account_error: Option<&str>,
    list_error: Option<&str>,
    shown: Option<bool>,
) -> Option<Gate> {
    match list_error {
        Some(NO_ORIGIN) => return Some(Gate::NoOrigin),
        Some(NOT_GITHUB) => return Some(Gate::NotGitHub),
        _ => {}
    }
    let failure = account_error.or(list_error);
    if failure.is_some_and(is_not_connected) {
        return Some(Gate::SignIn { hidden: false });
    }
    if read {
        return None;
    }
    match (failure, shown) {
        (Some(e), _) if is_hidden_repo(e) => Some(Gate::SignIn { hidden: true }),
        (None, Some(hidden)) => Some(Gate::SignIn { hidden }),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RemoteKind {
    Ssh,
    Https,
    Other,
}

/// How a remote URL signs git in: an SSH key, or a password or token over HTTPS.
pub fn remote_kind(url: Option<&str>) -> RemoteKind {
    let u = url.unwrap_or("").trim();
    let lower = u.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return RemoteKind::Https;
    }
    if lower.starts_with("ssh://") || scp_like(u) {
        return RemoteKind::Ssh;
    }
    RemoteKind::Other
}

/// git@github.com:o/r, or an ~/.ssh/config alias, work:o/r. Hosts of one letter are drives
/// (C:\work), and `//` after the colon is a URL (file:///x).
fn scp_like(u: &str) -> bool {
    let plain = |c: char| c != '/' && c != ':' && c != '@' && !c.is_whitespace();
    let Some(colon) = u.find(':') else {
        return false;
    };
    let (head, rest) = (&u[..colon], &u[colon + 1..]);
    let host = match head.split_once('@') {
        Some((user, host)) if !user.is_empty() && user.chars().all(plain) => host,
        Some(_) => return false,
        None => head,
    };
    host.chars().count() >= 2 && host.chars().all(plain) && !rest.starts_with("//")
}

/// What the signed-out screen says under its title. An SSH remote signs git in, not the GitHub
/// API that lists pull requests and issues, so the screen says that instead of leaving it to guess.
pub fn signed_out_note(origin: Option<&str>, subject: &str) -> String {
    if remote_kind(origin) == RemoteKind::Ssh {
        let subject = capitalized(subject);
        return format!(
            "Your remote uses SSH, which only signs git in. {subject} come from GitHub's API, which needs a login of its own."
        );
    }
    "GitViber uses the login you already have. It never stores a token itself.".into()
}

fn capitalized(s: &str) -> String {
    let mut chars = s.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// The title and note shown when origin isn't on GitHub: there's nothing there to list.
pub fn no_github_text(gate: Gate, subject: &str) -> (&'static str, String) {
    let subject = capitalized(subject);
    if gate == Gate::NoOrigin {
        return (
            "No GitHub repository yet",
            format!(
                "{subject} come from the 'origin' remote on GitHub, and this repository has none."
            ),
        );
    }
    (
        "Origin isn't on GitHub",
        format!(
            "{subject} come from github.com, and this repository's origin is elsewhere. Point it \
             at a GitHub repository in Settings > Git."
        ),
    )
}

/// What the sign-in screen says when GitHub answered 404 for origin with the login found.
pub fn hidden_note(origin: Option<&str>, subject: &str) -> String {
    let repo = origin
        .and_then(gitviber_core::github::parse_remote)
        .map_or("this repository".to_string(), |r| {
            format!("{}/{}", r.owner, r.name)
        });
    format!(
        "The login in use can't see {repo} on GitHub. It may be private: sign in as an account \
         that can see its {subject}, or pick that account in Settings > Git."
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitviber_core::github::{self, NOT_CONNECTED};
    use std::process::Command;

    #[test]
    fn a_hidden_repository_is_named_and_may_be_private() {
        let note = hidden_note(Some("git@github.com:octo-one/secret.git"), "issues");
        assert!(note.starts_with("The login in use can't see octo-one/secret on GitHub."));
        assert!(note.contains("may be private") && note.contains("see its issues"));
        assert!(hidden_note(None, "issues").contains("can't see this repository"));
    }

    #[test]
    fn what_shows_in_place_of_the_lists() {
        let none = None;
        assert_eq!(gate(true, none, none, None), None);
        assert_eq!(
            gate(true, none, Some(NO_ORIGIN), None),
            Some(Gate::NoOrigin)
        );
        // No origin comes first: signing in wouldn't give the lists anything to read.
        assert_eq!(
            gate(false, Some(NOT_CONNECTED), Some(NO_ORIGIN), None),
            Some(Gate::NoOrigin)
        );
        assert_eq!(
            gate(true, none, Some(NOT_GITHUB), None),
            Some(Gate::NotGitHub)
        );
        let signed_out = Some(Gate::SignIn { hidden: false });
        assert_eq!(gate(false, Some(NOT_CONNECTED), none, None), signed_out);
        assert_eq!(gate(false, none, Some(NOT_CONNECTED), None), signed_out);
        let hidden = Some(Gate::SignIn { hidden: true });
        assert_eq!(
            gate(false, Some("GitHub 404: Not Found"), none, None),
            hidden
        );
        assert_eq!(
            gate(false, none, Some("GitHub 404: Not Found"), None),
            hidden
        );
        // With the account read, a 404 is the list's own error, shown with it.
        assert_eq!(gate(true, none, Some("GitHub 404: Not Found"), None), None);
        assert_eq!(
            gate(false, Some("GitHub 403: rate limit"), none, None),
            None
        );
        // While the account is read again after a sign-in, the screen stays as it was.
        assert_eq!(gate(false, none, none, Some(true)), hidden);
        assert_eq!(gate(false, none, none, Some(false)), signed_out);
        assert_eq!(gate(false, none, none, None), None);
        assert_eq!(gate(true, none, none, Some(true)), None);
    }

    /// The answers `gate` knows are core's own, read with no request (no token, no network).
    #[test]
    fn core_says_no_origin_as_gate_reads_it() {
        let dir = gitviber_ui::test_support::temp_dir("gate");
        let git = |args: &[&str]| {
            assert!(
                Command::new("git")
                    .args(args)
                    .current_dir(&dir)
                    .status()
                    .unwrap()
                    .success()
            )
        };
        git(&["init", "-q"]);
        let session = github::Session::default();
        let list = || {
            github::list(&session, &dir, None, "open", 1, &Default::default())
                .err()
                .unwrap_or_default()
        };
        assert_eq!(gate(false, None, Some(&list()), None), Some(Gate::NoOrigin));
        git(&["remote", "add", "origin", "https://gitlab.com/o/r.git"]);
        assert_eq!(
            gate(false, None, Some(&list()), None),
            Some(Gate::NotGitHub)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn remotes_are_told_apart_by_how_git_signs_in() {
        let kind = |u: &str| remote_kind(Some(u));
        assert_eq!(kind("git@github.com:octo-one/demo.git"), RemoteKind::Ssh);
        assert_eq!(
            kind("ssh://git@github.com/octo-one/demo.git"),
            RemoteKind::Ssh
        );
        assert_eq!(
            kind("https://github.com/octo-one/demo.git"),
            RemoteKind::Https
        );
        assert_eq!(kind("HTTP://github.com/octo-one/demo"), RemoteKind::Https);
        assert_eq!(
            kind("https://octo-one:secret@github.com/octo-one/demo.git"),
            RemoteKind::Https
        );
        assert_eq!(kind("git@gh-work:octo-one/demo.git"), RemoteKind::Ssh);
        assert_eq!(kind("gh-work:octo-one/demo.git"), RemoteKind::Ssh);
        assert_eq!(
            kind("  git@github.com:octo-one/demo.git\n"),
            RemoteKind::Ssh
        );
        assert_eq!(kind("file:///srv/demo.git"), RemoteKind::Other);
        assert_eq!(kind("C:/work/demo.git"), RemoteKind::Other);
        assert_eq!(kind(""), RemoteKind::Other);
        assert_eq!(kind("/Users/someone/demo.git"), RemoteKind::Other);
        assert_eq!(kind("C:\\work\\demo.git"), RemoteKind::Other);
        assert_eq!(remote_kind(None), RemoteKind::Other);
    }

    #[test]
    fn an_ssh_remote_is_told_its_key_does_not_reach_the_api() {
        let ssh = signed_out_note(Some("git@github.com:octo-one/demo.git"), "pull requests");
        assert!(ssh.starts_with("Your remote uses SSH"));
        assert!(ssh.contains("Pull requests come from GitHub's API"));
        assert!(
            signed_out_note(Some("ssh://git@github.com/o/r"), "issues")
                .contains("Issues come from")
        );
        for url in [Some("https://github.com/o/r"), None] {
            assert!(!signed_out_note(url, "issues").contains("SSH"));
        }
    }
}
