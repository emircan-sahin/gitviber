/** The way out the app can offer: pull with merge or rebase, retry the pull with `--autostash`, set an identity, open the signing guide. */
export type GitFix = "pull" | "autostash" | "identity" | "signing";

export interface GitErrorHelp {
  title: string;
  /** What went wrong and what to do, in a sentence or two. */
  explanation: string;
  fix?: GitFix;
}

/** GitHub's guide to git signing; its macOS steps set up pinentry-mac. */
export const SIGNING_HELP = "https://docs.github.com/en/authentication/managing-commit-signature-verification/telling-git-about-your-signing-key";

// Fragments of git's English messages (cmd.rs runs git in English), the same from git 2.15 to 2.51.
const KNOWN: [RegExp, GitErrorHelp][] = [
  [
    /Not possible to fast-forward/,
    {
      title: "Your branch and the remote have diverged",
      explanation: "Each has commits the other doesn't, so a fast-forward can't bring them together. A merge joins them with a merge commit; a rebase replays your commits on top.",
      fix: "pull",
    },
  ],
  [
    /\(fetch first\)/,
    {
      title: "The remote has commits you don't",
      explanation: "Someone pushed to this branch since you last pulled. Pull their commits in, then push again.",
      fix: "pull",
    },
  ],
  [
    /would be overwritten by merge|cannot pull with rebase/,
    {
      title: "Uncommitted changes are in the way",
      explanation: "git won't touch files you've changed but not committed. Commit or stash them first, or pull with autostash: it sets them aside and puts them back after.",
      fix: "autostash",
    },
  ],
  [
    /Please tell me who you are|empty ident name/,
    {
      title: "git doesn't know who you are",
      explanation: "Every commit records a name and an email, and yours aren't set yet.",
      fix: "identity",
    },
  ],
  [
    /gpg failed to sign the data/,
    {
      title: "Signing the commit failed",
      explanation: "Commit signing is on and gpg couldn't sign, most often because it has no way to ask for your passphrase outside a terminal. Give gpg-agent a graphical pinentry (pinentry-mac on macOS), or turn off commit.gpgsign.",
      fix: "signing",
    },
  ],
  [
    /Authentication failed for|could not read (Username|Password)/,
    {
      title: "The remote wants you to sign in",
      explanation: "git has no credentials that work here, and can't ask for them without a terminal. For GitHub, run `gh auth login` in a terminal and let it set up git; elsewhere, store a token in git's credential helper.",
    },
  ],
  [
    /Permission denied \(publickey/,
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
