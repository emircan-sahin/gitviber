/** Which way out fits: the caller that can take it supplies the buttons (a pull, a retry), except identity, signing and the index lock, which always fit. */
export type GitFix = "diverged" | "fetch-first" | "autostash" | "identity" | "signing" | "hooks" | "index-lock";

export interface GitErrorHelp {
  title: string;
  /** What went wrong and what to do, in a sentence or two. */
  explanation: string;
  fix?: GitFix;
  /** What the fix acts on, from git's words (a pattern's `target` group): the lock file to remove. */
  target?: string;
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
  // The last two refuse a force push (--force-with-lease --force-if-includes) over commits it
  // hasn't seen or never had.
  [
    /^ ?! \[rejected\] .*\((fetch first|non-fast-forward|stale info|remote ref updated since checkout)\)$/m,
    {
      title: "The remote has commits you don't",
      explanation: "Someone pushed to this branch since you last pulled. Pull their commits in, then push again.",
      fix: "fetch-first",
    },
  ],
  [
    /^error: Your local changes to the following files would be overwritten by merge:|^error: cannot (pull with rebase|rebase):/m,
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
  // state.rs retried once already. A killed agent's git leaves the lock behind for good.
  [
    /^fatal: Unable to create '(?<target>.+\/index\.lock)': File exists\./m,
    {
      title: "Another git command holds the index lock",
      explanation:
        "git takes index.lock while it changes what's staged, and a git that was killed (an agent stopped mid-command) leaves it behind. If no git command is running in this repository, in a terminal, an agent or a commit message editor left open, remove the lock and try again.",
      fix: "index-lock",
    },
  ],
  // commit.rs's line: git says nothing of its own when a pre-commit or commit-msg hook fails.
  // Last, so a failure git does explain (identity, signing) is told as that.
  [
    /^hint: Commit hooks set up here: /m,
    {
      title: "A commit hook stopped the commit",
      explanation: "What it printed is under Details. Fix what it reports and commit again, or commit without hooks this once.",
      fix: "hooks",
    },
  ],
];

/** What a failed git command's output means, for the failures that have a known way out; null for the rest. */
export function explainGitError(message: string): GitErrorHelp | null {
  for (const [re, help] of KNOWN) {
    const m = re.exec(message);
    if (m) return m.groups?.target ? { ...help, target: m.groups.target } : help;
  }
  return null;
}
