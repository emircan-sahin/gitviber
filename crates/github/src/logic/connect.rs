//! What the signed-out screen says, by how the repository's remote signs git in.

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
        let mut chars = subject.chars();
        let subject = chars
            .next()
            .map(|c| c.to_uppercase().chain(chars).collect::<String>())
            .unwrap_or_default();
        return format!(
            "Your remote uses SSH, which only signs git in. {subject} come from GitHub's API, which needs a login of its own."
        );
    }
    "GitViber uses the login you already have. It never stores a token itself.".into()
}

#[cfg(test)]
mod tests {
    use super::*;

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
