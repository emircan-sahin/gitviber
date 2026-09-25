/** Which way out fits: the caller that can take it supplies the buttons (a pull, a retry), except identity and signing, which always fit. */
export type GitFix = "diverged" | "fetch-first" | "autostash" | "identity" | "signing";

export interface GitErrorHelp {
  title: string;
  /** What went wrong and what to do, in a sentence or two. */
  explanation: string;
  fix?: GitFix;
}

/** GitHub's guide to git signing; its macOS steps set up pinentry-mac. */
export const SIGNING_HELP = "https://docs.github.com/en/authentication/managing-commit-signature-verification/telling-git-about-your-signing-key";

// git's English messages (cmd.rs runs git in English), the same from git 2.15 to 2.51. Anchored to
// a line's start so a hook's output quoting them doesn't match.
const KNOWN: [RegExp, GitErrorHelp][] = [
  [
    /^fatal: Not possible to fast-forward/m,
    {
      title: "Your branch and the remote have diverged",
      explanation: "Each has commits the other doesn't, so a fast-forward can't bring them together. A merge joins them with a merge commit; a rebase replays your commits on top.",
      fix: "diverged",
    },
  ],
  [
    /^ ?! \[rejected\] .*\((fetch first|non-fast-forward)\)$/m,
    {
      title: "The remote has commits you don't",
      explanation: "Someone pushed to this branch since you last pulled. Pull their commits in, then push again.",
      fix: "fetch-first",
    },
  ],
  [
    /^error: Your local changes to the following files would be overwritten by merge:|^error: cannot pull with rebase:/m,
    {
      title: "Uncommitted changes are in the way",
      explanation: "git won't overwrite files you've changed but not committed. Commit or stash them first.",
      fix: "autostash",
    },
  ],
  [
    /^\*\*\* Please tell me who you are|^fatal: empty ident name/m,
    {
      title: "git doesn't know who you are",
      explanation: "Every commit records a name and an email, and yours aren't set yet.",
      fix: "identity",
    },
  ],
  [
    /^error: gpg failed to sign the data/m,
    {
      title: "Signing the commit failed",
      explanation: "Commit signing is on and gpg couldn't sign, most often because it has no way to ask for your passphrase outside a terminal. Give gpg-agent a graphical pinentry (pinentry-mac on macOS), or turn off commit.gpgsign.",
      fix: "signing",
    },
  ],
  [
    /^fatal: (Authentication failed for|could not read (Username|Password) for)/m,
    {
      title: "The remote wants you to sign in",
      explanation: "git has no credentials that work here, and the sign-in was cancelled or refused. For GitHub, run `gh auth login` in a terminal and let it set up git, or give a personal access token as the password; elsewhere, store a token in git's credential helper.",
    },
  ],
  [
    /^Host key verification failed\./m,
    {
      title: "ssh doesn't trust this host",
      explanation: "The host's key is new and wasn't accepted, or it changed since you last connected (ssh warns above if so). Check the fingerprint your provider publishes before you trust it.",
    },
  ],
  [
    /^\S+: Permission denied \(publickey/m,
    {
      title: "The remote refused your SSH key",
      explanation: "Check that your key is loaded (`ssh-add -l`) and added to your account on the remote. For GitHub, `ssh -T git@github.com` tests it.",
    },
  ],
];

/** What a failed git command's output means, for the failures that have a known way out; null for the rest. */
export function explainGitError(message: string): GitErrorHelp | null {
  return KNOWN.find(([re]) => re.test(message))?.[1] ?? null;
}
