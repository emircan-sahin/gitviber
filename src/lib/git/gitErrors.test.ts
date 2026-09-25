import assert from "node:assert/strict";
import { test } from "node:test";
import { explainGitError } from "./gitErrors.ts";

// What git 2.51 printed in scratch repos, as the app gets it (stderr, trimmed). Git 2.15 and
// Apple Git 2.50 print the matched lines the same; only the hints around them differ.
const DIVERGED = `From /tmp/gv64/remote
   96427c5..6e15ff8  main       -> origin/main
hint: Diverging branches can't be fast-forwarded, you need to either:
hint:
hint: 	git merge --no-ff
hint:
hint: or:
hint:
hint: 	git rebase
hint:
hint: Disable this message with "git config set advice.diverging false"
fatal: Not possible to fast-forward, aborting.`;

const FETCH_FIRST = `To /tmp/gv64/remote.git
 ! [rejected]        main -> main (fetch first)
error: failed to push some refs to '/tmp/gv64/remote.git'
hint: Updates were rejected because the remote contains work that you do not
hint: have locally. This is usually caused by another repository pushing to
hint: the same ref. If you want to integrate the remote changes, use
hint: 'git pull' before pushing again.
hint: See the 'Note about fast-forwards' in 'git push --help' for details.`;

const NON_FAST_FORWARD = `To /tmp/gv64/remote.git
 ! [rejected]        main -> main (non-fast-forward)
error: failed to push some refs to '/tmp/gv64/remote.git'
hint: Updates were rejected because the tip of your current branch is behind
hint: its remote counterpart. If you want to integrate the remote changes,
hint: use 'git pull' before pushing again.`;

const OVERWRITTEN_BY_MERGE = `error: Your local changes to the following files would be overwritten by merge:
	f
Please commit your changes or stash them before you merge.
Aborting`;

const REBASE_UNSTAGED = `error: cannot pull with rebase: You have unstaged changes.
error: Please commit or stash them.`;

const REBASE_STAGED = `error: cannot pull with rebase: Your index contains uncommitted changes.
error: Please commit or stash them.`;

const OVERWRITTEN_BY_CHECKOUT = `error: Your local changes to the following files would be overwritten by checkout:
	f
Please commit your changes or stash them before you switch branches.
Aborting`;

const NO_IDENTITY = `Author identity unknown

*** Please tell me who you are.

Run

  git config --global user.email "you@example.com"
  git config --global user.name "Your Name"

to set your account's default identity.
Omit --global to set the identity only in this repository.

fatal: no email was given and auto-detection is disabled`;

const GPG = `error: gpg failed to sign the data:
(no gpg output)
fatal: failed to write commit object`;

const NO_USERNAME = "fatal: could not read Username for 'https://github.com': terminal prompts disabled";

const BAD_TOKEN = `remote: Invalid username or token. Password authentication is not supported for Git operations.
fatal: Authentication failed for 'https://github.com/owner/repo.git/'`;

const SSH_KEY = `git@github.com: Permission denied (publickey).
fatal: Could not read from remote repository.

Please make sure you have the correct access rights
and the repository exists.`;

const fix = (message: string) => explainGitError(message)?.fix;

test("a diverged pull and a push behind the remote are told apart", () => {
  assert.equal(fix(DIVERGED), "diverged");
  assert.equal(fix(FETCH_FIRST), "fetch-first");
});

test("uncommitted changes in a pull's way offer autostash", () => {
  for (const message of [OVERWRITTEN_BY_MERGE, REBASE_UNSTAGED, REBASE_STAGED]) assert.equal(fix(message), "autostash");
});

test("a missing identity or a failed signature say what to set up", () => {
  assert.equal(fix(NO_IDENTITY), "identity");
  assert.equal(fix("fatal: empty ident name (for <a@b>) not allowed"), "identity");
  assert.equal(fix(GPG), "signing");
});

test("credentials are explained, with nothing to click", () => {
  for (const message of [NO_USERNAME, BAD_TOKEN, SSH_KEY]) {
    const help = explainGitError(message);
    assert.ok(help, message);
    assert.equal(help.fix, undefined);
  }
});

// A pre-commit hook's output ends up in the same message; words it quotes aren't git's verdict.
const HOOK_OUTPUT = `husky - pre-commit script failed (code 1)
✖ test "retries when Authentication failed for the mirror"
  expected: "fatal: Not possible to fast-forward, aborting."
  log: error: gpg failed to sign the data (fixture)
  ! [rejected]  main -> main (fetch first)  (fixture)`;

test("a hook quoting git's messages isn't taken for them", () => {
  assert.equal(explainGitError(HOOK_OUTPUT), null);
});

test("what the app handles elsewhere, or doesn't know, stays git's own words", () => {
  // useRepoActions asks to force push, and to stash and switch.
  assert.equal(explainGitError(NON_FAST_FORWARD), null);
  assert.equal(explainGitError(OVERWRITTEN_BY_CHECKOUT), null);
  assert.equal(explainGitError("fatal: 'origin' does not appear to be a git repository"), null);
  assert.equal(explainGitError(""), null);
});
