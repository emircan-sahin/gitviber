//! What a failed git command's output means, for the failures that have a known way out. git's
//! English messages (core runs git in English), the same from git 2.15 to 2.51; each is matched
//! at a line's start, so a hook's output quoting them doesn't match.

/// Which way out fits: the caller that can take it supplies the buttons (a pull, a retry);
/// identity, signing, the index lock and GitHub's page on a secret always fit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GitFix {
    Diverged,
    FetchFirst,
    Autostash,
    Conflicted,
    Identity,
    Signing,
    Hooks,
    IndexLock,
    SafeDirectory,
    Secret,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitErrorHelp {
    pub title: &'static str,
    /// What went wrong and what to do, in a sentence or two.
    pub explanation: &'static str,
    pub fix: Option<GitFix>,
    /// What the fix acts on, from git's words: the lock file to remove, the page GitHub linked.
    pub target: Option<String>,
}

/// GitHub's guide to git signing; its macOS steps set up pinentry-mac.
pub const SIGNING_HELP: &str = "https://docs.github.com/en/authentication/managing-commit-signature-verification/telling-git-about-your-signing-key";

fn help(title: &'static str, explanation: &'static str, fix: Option<GitFix>) -> GitErrorHelp {
    GitErrorHelp {
        title,
        explanation,
        fix,
        target: None,
    }
}

fn starts(message: &str, prefixes: &[&str]) -> bool {
    message
        .lines()
        .any(|l| prefixes.iter().any(|p| l.starts_with(p)))
}

fn rejected(line: &str) -> bool {
    let line = line.strip_prefix(' ').unwrap_or(line);
    let Some(rest) = line.strip_prefix("! [rejected] ") else {
        return false;
    };
    [
        "(fetch first)",
        "(non-fast-forward)",
        "(stale info)",
        "(remote ref updated since checkout)",
    ]
    .iter()
    .any(|why| rest.ends_with(why))
}

fn reset_in_merge(line: &str) -> bool {
    line.strip_prefix("fatal: Cannot do a ")
        .and_then(|r| r.split_once(' '))
        .is_some_and(|(word, rest)| {
            !word.is_empty()
                && word.chars().all(|c| c.is_alphanumeric() || c == '_')
                && rest.starts_with("reset in the middle of a merge.")
        })
}

fn denied_publickey(line: &str) -> bool {
    line.split_once(": Permission denied (publickey")
        .is_some_and(|(who, _)| !who.is_empty() && !who.contains(char::is_whitespace))
}

/// "remote:" and some space, then what's after it.
fn remote_line(line: &str) -> Option<&str> {
    let rest = line.strip_prefix("remote:")?;
    let trimmed = rest.trim_start();
    (trimmed.len() < rest.len()).then_some(trimmed)
}

fn secret(message: &str) -> Option<GitErrorHelp> {
    let lines: Vec<&str> = message.lines().collect();
    let at = lines.iter().position(|l| {
        l.starts_with("remote: error: GH009: Secrets detected!")
            || remote_line(l).is_some_and(|r| r.starts_with("- Push cannot contain secrets"))
    })?;
    let target = lines[at + 1..].iter().find_map(|l| {
        let url = remote_line(l)?.split_whitespace().next()?;
        let path = url.strip_prefix("https://github.com/")?;
        let (repo, page) = path.split_once("/unblock-secret/")?;
        (!repo.is_empty() && !page.is_empty()).then(|| url.to_string())
    });
    Some(GitErrorHelp {
        target,
        ..help(
            "GitHub found a secret in your commits",
            "The push was refused because a commit holds what looks like a token or key; Details lists which commits and files. If only the last commit has it, undo that commit, take the secret out and commit again; an earlier commit has to be rewritten (an interactive rebase) before you push. Revoke the secret if it was real.",
            Some(GitFix::Secret),
        )
    })
}

fn index_lock(message: &str) -> Option<String> {
    message.lines().find_map(|l| {
        let rest = l.strip_prefix("fatal: Unable to create '")?;
        let end = rest.rfind("': File exists.")?;
        let path = &rest[..end];
        (path.len() > "/index.lock".len() && path.ends_with("/index.lock"))
            .then(|| path.to_string())
    })
}

/// The commits GitHub's push protection lists a secret in ("- commit: <sha>"), as it printed them.
pub fn secret_commits(message: &str) -> Vec<String> {
    message
        .lines()
        .filter_map(|l| {
            let sha = remote_line(l)?.strip_prefix("- commit: ")?;
            (sha.len() >= 7
                && sha.len() <= 64
                && sha.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')))
            .then(|| sha.to_string())
        })
        .collect()
}

/// What a failed git command's output means, for the failures that have a known way out; None for the rest.
pub fn explain_git_error(message: &str) -> Option<GitErrorHelp> {
    use GitFix::*;
    if starts(message, &["fatal: Not possible to fast-forward"]) {
        return Some(help(
            "Your branch and the remote have diverged",
            "Each has commits the other doesn't, so a fast-forward can't bring them together. A merge joins them with a merge commit; a rebase replays your commits on top.",
            Some(Diverged),
        ));
    }
    // The last two refuse a force push (--force-with-lease --force-if-includes) over commits it
    // hasn't seen or never had.
    if message.lines().any(rejected) {
        return Some(help(
            "The remote has commits you don't",
            "Someone pushed to this branch since you last pulled. Pull their commits in, then push again.",
            Some(FetchFirst),
        ));
    }
    if starts(
        message,
        &[
            "error: Your local changes to the following files would be overwritten by merge:",
            "error: cannot pull with rebase:",
            "error: cannot rebase:",
        ],
    ) {
        return Some(help(
            "Uncommitted changes are in the way",
            "git won't overwrite files you've changed but not committed. Commit or stash them first.",
            Some(Autostash),
        ));
    }
    // Unmerged files with nothing in progress: changes a rewrite, pull or stash pop brought back
    // from the stash that conflicted. An undo's reset says the first.
    if message.lines().any(reset_in_merge)
        || starts(
            message,
            &["error: you need to resolve your current index first"],
        )
    {
        return Some(help(
            "Conflicted files are in the way",
            "Some files are still marked conflicted, most often uncommitted changes that came back from the stash and clashed. Resolve or discard them in Changes, then try again.",
            Some(Conflicted),
        ));
    }
    if starts(
        message,
        &["*** Please tell me who you are", "fatal: empty ident name"],
    ) {
        return Some(help(
            "git doesn't know who you are",
            "Every commit records a name and an email, and yours aren't set yet.",
            Some(Identity),
        ));
    }
    if starts(message, &["error: gpg failed to sign the data"]) {
        return Some(help(
            "Signing the commit failed",
            "Commit signing is on and gpg couldn't sign, most often because it has no way to ask for your passphrase outside a terminal. Give gpg-agent a graphical pinentry (pinentry-mac on macOS), or turn off commit.gpgsign.",
            Some(Signing),
        ));
    }
    if starts(
        message,
        &[
            "fatal: Authentication failed for",
            "fatal: could not read Username for",
            "fatal: could not read Password for",
        ],
    ) {
        return Some(help(
            "The remote wants you to sign in",
            "git has no credentials that work here, and the sign-in was cancelled or refused. For GitHub, run `gh auth login` in a terminal and let it set up git, or give a personal access token as the password; elsewhere, store a token in git's credential helper.",
            None,
        ));
    }
    if starts(message, &["Host key verification failed."]) {
        return Some(help(
            "ssh doesn't trust this host",
            "The host's key is new and wasn't accepted, or it changed since you last connected (ssh warns above if so). Check the fingerprint your provider publishes before you trust it.",
            None,
        ));
    }
    if message.lines().any(denied_publickey) {
        return Some(help(
            "The remote refused your SSH key",
            "Check that your key is loaded (`ssh-add -l`) and added to your account on the remote. For GitHub, `ssh -T git@github.com` tests it.",
            None,
        ));
    }
    if starts(
        message,
        &["fatal: detected dubious ownership in repository at '"],
    ) {
        return Some(help(
            "git won't open a folder another user owns",
            "This repository belongs to another account on this computer (copied from another disk, or made with sudo), and git refuses to run where someone else could have set up what it runs. If you trust it, Trust this folder adds it to safe.directory in your global git config.",
            Some(SafeDirectory),
        ));
    }
    if let Some(found) = secret(message) {
        return Some(found);
    }
    // core retried once already. A killed agent's git leaves the lock behind for good.
    if let Some(lock) = index_lock(message) {
        return Some(GitErrorHelp {
            target: Some(lock),
            ..help(
                "Another git command holds the index lock",
                "git takes index.lock while it changes what's staged, and a git that was killed (an agent stopped mid-command) leaves it behind. Remove it only if no git command is running in this repository: a `git commit` in a terminal or an agent that waits on its hooks or editor holds it too, and would break.",
                Some(IndexLock),
            )
        });
    }
    // core's line: git says nothing of its own when a commit hook fails. Last, so a failure git
    // does explain (identity, signing) is told as that. `--no-verify` still runs this one.
    if message
        .lines()
        .any(|l| l == "hint: Commit hooks set up here: prepare-commit-msg.")
    {
        return Some(help(
            "The prepare-commit-msg hook stopped the commit",
            "What it printed is under Details. git runs this hook even when committing without hooks, so fix what it reports, or the hook itself, and commit again.",
            None,
        ));
    }
    if starts(message, &["hint: Commit hooks set up here: "]) {
        return Some(help(
            "A commit hook stopped the commit",
            "What it printed is under Details. Fix what it reports and commit again, or commit without hooks this once.",
            Some(Hooks),
        ));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    // What git 2.51 printed in scratch repos, as the app gets it (stderr, trimmed).
    const DIVERGED: &str = "From /tmp/gv64/remote
   96427c5..6e15ff8  main       -> origin/main
hint: Diverging branches can't be fast-forwarded, you need to either:
hint:
hint: \tgit merge --no-ff
hint:
hint: or:
hint:
hint: \tgit rebase
hint:
hint: Disable this message with \"git config set advice.diverging false\"
fatal: Not possible to fast-forward, aborting.";

    const FETCH_FIRST: &str = "To /tmp/gv64/remote.git
 ! [rejected]        main -> main (fetch first)
error: failed to push some refs to '/tmp/gv64/remote.git'
hint: Updates were rejected because the remote contains work that you do not
hint: have locally.";

    const NON_FAST_FORWARD: &str = "To /tmp/gv64/remote.git
 ! [rejected]        main -> main (non-fast-forward)
error: failed to push some refs to '/tmp/gv64/remote.git'";

    const STALE_INFO: &str = "To /tmp/gv64/remote.git
 ! [rejected]        main -> main (stale info)
error: failed to push some refs to '/tmp/gv64/remote.git'";

    const UPDATED_SINCE_CHECKOUT: &str = "To /tmp/gv64/remote.git
 ! [rejected]        main -> main (remote ref updated since checkout)
error: failed to push some refs to '/tmp/gv64/remote.git'";

    const OVERWRITTEN_BY_MERGE: &str =
        "error: Your local changes to the following files would be overwritten by merge:
\tf
Please commit your changes or stash them before you merge.
Aborting";
    const REBASE_UNSTAGED: &str = "error: cannot pull with rebase: You have unstaged changes.\nerror: Please commit or stash them.";
    const REBASE_STAGED: &str = "error: cannot pull with rebase: Your index contains uncommitted changes.\nerror: Please commit or stash them.";
    const REBASE_REFUSED: &str =
        "error: cannot rebase: You have unstaged changes.\nerror: Please commit or stash them.";
    const REBASE_REFUSED_STAGED: &str = "error: cannot rebase: Your index contains uncommitted changes.\nerror: Please commit or stash them.";
    const OVERWRITTEN_BY_CHECKOUT: &str =
        "error: Your local changes to the following files would be overwritten by checkout:
\tf
Please commit your changes or stash them before you switch branches.
Aborting";
    const NO_IDENTITY: &str = "Author identity unknown

*** Please tell me who you are.

Run

  git config --global user.email \"you@example.com\"

fatal: no email was given and auto-detection is disabled";
    const GPG: &str = "error: gpg failed to sign the data:\n(no gpg output)\nfatal: failed to write commit object";
    const NO_USERNAME: &str =
        "fatal: could not read Username for 'https://github.com': terminal prompts disabled";
    const HOST_KEY: &str =
        "Host key verification failed.\nfatal: Could not read from remote repository.";
    const BAD_TOKEN: &str = "remote: Invalid username or token. Password authentication is not supported for Git operations.
fatal: Authentication failed for 'https://github.com/owner/repo.git/'";
    const SSH_KEY: &str = "git@github.com: Permission denied (publickey).\nfatal: Could not read from remote repository.";

    fn fix(message: &str) -> Option<GitFix> {
        explain_git_error(message).and_then(|h| h.fix)
    }

    #[test]
    fn a_diverged_pull_and_a_push_behind_the_remote_are_told_apart() {
        assert_eq!(fix(DIVERGED), Some(GitFix::Diverged));
        assert_eq!(fix(FETCH_FIRST), Some(GitFix::FetchFirst));
    }

    #[test]
    fn uncommitted_changes_in_the_way_offer_autostash() {
        for m in [
            OVERWRITTEN_BY_MERGE,
            REBASE_UNSTAGED,
            REBASE_STAGED,
            REBASE_REFUSED,
            REBASE_REFUSED_STAGED,
        ] {
            assert_eq!(fix(m), Some(GitFix::Autostash), "{m}");
        }
    }

    #[test]
    fn a_missing_identity_or_a_failed_signature_say_what_to_set_up() {
        assert_eq!(fix(NO_IDENTITY), Some(GitFix::Identity));
        assert_eq!(
            fix("fatal: empty ident name (for <a@b>) not allowed"),
            Some(GitFix::Identity)
        );
        assert_eq!(fix(GPG), Some(GitFix::Signing));
    }

    #[test]
    fn conflicted_files_with_nothing_in_progress_offer_opening_changes() {
        assert_eq!(
            fix("fatal: Cannot do a keep reset in the middle of a merge."),
            Some(GitFix::Conflicted)
        );
        assert_eq!(
            fix("f1: needs merge\nerror: you need to resolve your current index first"),
            Some(GitFix::Conflicted)
        );
    }

    #[test]
    fn credentials_are_explained_with_nothing_to_click() {
        let cancelled = format!(
            "error: unable to read askpass response from '/Applications/GitViber.app/Contents/MacOS/gitviber'\n{NO_USERNAME}"
        );
        for m in [
            NO_USERNAME,
            cancelled.as_str(),
            BAD_TOKEN,
            SSH_KEY,
            HOST_KEY,
        ] {
            let h = explain_git_error(m).unwrap_or_else(|| panic!("{m}"));
            assert_eq!(h.fix, None);
        }
    }

    const HOOK_OUTPUT: &str = "husky - pre-commit script failed (code 1)
✖ test \"retries when Authentication failed for the mirror\"
  expected: \"fatal: Not possible to fast-forward, aborting.\"
  log: error: gpg failed to sign the data (fixture)
  ! [rejected]  main -> main (fetch first)  (fixture)";

    #[test]
    fn a_lock_git_cant_take_offers_removing_that_exact_file() {
        let lock = "fatal: Unable to create '/Users/me/my repo/.git/worktrees/fix/index.lock': File exists.

Another git process seems to be running in this repository.";
        let h = explain_git_error(lock).unwrap();
        assert_eq!(h.fix, Some(GitFix::IndexLock));
        assert_eq!(
            h.target.as_deref(),
            Some("/Users/me/my repo/.git/worktrees/fix/index.lock")
        );
        assert_eq!(
            explain_git_error(
                "fatal: Unable to create '/r/.git/refs/heads/main.lock': File exists."
            ),
            None
        );
    }

    #[test]
    fn a_repository_another_user_owns_offers_trusting_it() {
        let refused = "fatal: detected dubious ownership in repository at '/Volumes/Disk/app'
To add an exception for this directory, call:

\tgit config --global --add safe.directory /Volumes/Disk/app";
        assert_eq!(fix(refused), Some(GitFix::SafeDirectory));
    }

    const PUSH_PROTECTION: &str = "remote: error: GH013: Repository rule violations found for refs/heads/main.
remote:
remote: - GITHUB PUSH PROTECTION
remote:     Resolve the following violations before pushing again
remote:
remote:     - Push cannot contain secrets
remote:
remote:      (?) Learn how to resolve a blocked push
remote:      https://docs.github.com/code-security/secret-scanning/working-with-secret-scanning-and-push-protection/working-with-push-protection-from-the-command-line#resolving-a-blocked-push
remote:
remote:       —— GitHub Personal Access Token ——————————————————————
remote:        locations:
remote:          - commit: 8728dbe67d1a1c4b3a0b1a2c3d4e5f6a7b8c9d0e
remote:            path: config/env.ts:4
remote:
remote:        (?) To push, remove secret from commit(s) or follow this URL to allow the secret.
remote:        https://github.com/owner/repo/security/secret-scanning/unblock-secret/2abcDEF
remote:
remote:        https://github.com/owner/repo/security/secret-scanning/unblock-secret/3ghiJKL
To github.com:owner/repo.git
 ! [remote rejected] main -> main (push declined due to repository rule violations)
error: failed to push some refs to 'github.com:owner/repo.git'";

    #[test]
    fn a_secret_push_protection_found_is_explained_with_the_page_it_links() {
        let h = explain_git_error(PUSH_PROTECTION).unwrap();
        assert_eq!(h.fix, Some(GitFix::Secret));
        assert_eq!(
            h.target.as_deref(),
            Some("https://github.com/owner/repo/security/secret-scanning/unblock-secret/2abcDEF")
        );
        assert_eq!(
            secret_commits(PUSH_PROTECTION),
            vec!["8728dbe67d1a1c4b3a0b1a2c3d4e5f6a7b8c9d0e".to_string()]
        );
        let old = "remote: error: GH009: Secrets detected! This push failed.
remote:
remote:     GITHUB PUSH PROTECTION
remote:          - commit: 1111111111111111111111111111111111111111
remote:            path: a.txt:1
remote:          - commit: 2222222222222222222222222222222222222222
remote:            path: b.txt:1
remote:        https://github.com/owner/repo/security/secret-scanning/unblock-secret/9xyz";
        let h = explain_git_error(old).unwrap();
        assert_eq!(
            (h.fix, h.target.as_deref()),
            (
                Some(GitFix::Secret),
                Some("https://github.com/owner/repo/security/secret-scanning/unblock-secret/9xyz")
            )
        );
        assert_eq!(secret_commits(old).len(), 2);
        assert_eq!(
            fix("remote: error: GH009: Secrets detected! This push failed."),
            Some(GitFix::Secret)
        );
        assert_eq!(
            explain_git_error(
                "remote: error: GH013: Repository rule violations found for refs/heads/main.\nremote: - Commits must have verified signatures."
            ),
            None
        );
    }

    #[test]
    fn a_failed_commit_with_hooks_set_up_offers_committing_without_them_unless_git_said_why() {
        assert_eq!(
            fix(&format!(
                "{HOOK_OUTPUT}\nhint: Commit hooks set up here: pre-commit, commit-msg."
            )),
            Some(GitFix::Hooks)
        );
        assert_eq!(
            fix(&format!(
                "{GPG}\nhint: Commit hooks set up here: pre-commit."
            )),
            Some(GitFix::Signing)
        );
        let only = explain_git_error(&format!(
            "{HOOK_OUTPUT}\nhint: Commit hooks set up here: prepare-commit-msg."
        ))
        .unwrap();
        assert_eq!(
            (only.title, only.fix),
            ("The prepare-commit-msg hook stopped the commit", None)
        );
    }

    #[test]
    fn a_hook_quoting_gits_messages_isnt_taken_for_them() {
        assert_eq!(explain_git_error(HOOK_OUTPUT), None);
    }

    #[test]
    fn a_push_refused_over_commits_someone_else_pushed_wants_a_pull() {
        for m in [
            FETCH_FIRST,
            NON_FAST_FORWARD,
            STALE_INFO,
            UPDATED_SINCE_CHECKOUT,
        ] {
            assert_eq!(fix(m), Some(GitFix::FetchFirst), "{m}");
        }
    }

    #[test]
    fn what_the_app_handles_elsewhere_stays_gits_own_words() {
        assert_eq!(explain_git_error(OVERWRITTEN_BY_CHECKOUT), None);
        assert_eq!(
            explain_git_error("fatal: 'origin' does not appear to be a git repository"),
            None
        );
        assert_eq!(explain_git_error(""), None);
    }
}
