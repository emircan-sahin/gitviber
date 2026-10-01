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

// A force push (with lease) refused: the remote moved past what was fetched, or was fetched but
// never in the branch.
const STALE_INFO = `To /tmp/gv64/remote.git
 ! [rejected]        main -> main (stale info)
error: failed to push some refs to '/tmp/gv64/remote.git'`;

const UPDATED_SINCE_CHECKOUT = `To /tmp/gv64/remote.git
 ! [rejected]        main -> main (remote ref updated since checkout)
error: failed to push some refs to '/tmp/gv64/remote.git'
hint: Updates were rejected because the tip of the remote-tracking branch has
hint: been updated since the last checkout. If you want to integrate the
hint: remote changes, use 'git pull' before pushing again.`;

const OVERWRITTEN_BY_MERGE = `error: Your local changes to the following files would be overwritten by merge:
	f
Please commit your changes or stash them before you merge.
Aborting`;

const REBASE_UNSTAGED = `error: cannot pull with rebase: You have unstaged changes.
error: Please commit or stash them.`;

const REBASE_STAGED = `error: cannot pull with rebase: Your index contains uncommitted changes.
error: Please commit or stash them.`;

// Rebase from the branch picker, which refuses any uncommitted change.
const REBASE_REFUSED = `error: cannot rebase: You have unstaged changes.
error: Please commit or stash them.`;

const REBASE_REFUSED_STAGED = `error: cannot rebase: Your index contains uncommitted changes.
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

// A prompt the user cancelled (askpass.rs).
const CANCELLED_LOGIN = `error: unable to read askpass response from '/Applications/GitViber.app/Contents/MacOS/gitviber'
${NO_USERNAME}`;

const HOST_KEY = `Host key verification failed.
fatal: Could not read from remote repository.`;

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

test("uncommitted changes in a pull's, merge's or rebase's way offer autostash", () => {
  for (const message of [OVERWRITTEN_BY_MERGE, REBASE_UNSTAGED, REBASE_STAGED, REBASE_REFUSED, REBASE_REFUSED_STAGED]) assert.equal(fix(message), "autostash");
});

test("a missing identity or a failed signature say what to set up", () => {
  assert.equal(fix(NO_IDENTITY), "identity");
  assert.equal(fix("fatal: empty ident name (for <a@b>) not allowed"), "identity");
  assert.equal(fix(GPG), "signing");
});

test("credentials are explained, with nothing to click", () => {
  for (const message of [NO_USERNAME, CANCELLED_LOGIN, BAD_TOKEN, SSH_KEY, HOST_KEY]) {
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

const INDEX_LOCK = `fatal: Unable to create '/Users/me/my repo/.git/worktrees/fix/index.lock': File exists.

Another git process seems to be running in this repository, e.g.
an editor opened by 'git commit'. Please make sure all processes
are terminated then try again. If it still fails, a git process
may have crashed in this repository earlier:
remove the file manually to continue.`;

test("a lock git can't take offers removing that exact file", () => {
  const help = explainGitError(INDEX_LOCK);
  assert.equal(help?.fix, "index-lock");
  assert.equal(help?.target, "/Users/me/my repo/.git/worktrees/fix/index.lock");
  // Some other file's lock isn't the index's.
  assert.equal(explainGitError("fatal: Unable to create '/r/.git/refs/heads/main.lock': File exists."), null);
});

// commit.rs names the hooks set up when a commit fails; git prints nothing of its own then.
const HOOK_FAILED = `${HOOK_OUTPUT}
hint: Commit hooks set up here: pre-commit, commit-msg.`;

test("a failed commit with hooks set up offers committing without them, unless git said why", () => {
  assert.equal(fix(HOOK_FAILED), "hooks");
  assert.equal(fix(`${GPG}\nhint: Commit hooks set up here: pre-commit.`), "signing");
});

test("a hook quoting git's messages isn't taken for them", () => {
  assert.equal(explainGitError(HOOK_OUTPUT), null);
});

test("a push refused over commits someone else pushed, fetched or not, wants a pull", () => {
  // useRepoActions asks to force push first when the remote's commits were the branch's own.
  for (const refused of [FETCH_FIRST, NON_FAST_FORWARD, STALE_INFO, UPDATED_SINCE_CHECKOUT]) assert.equal(explainGitError(refused)?.fix, "fetch-first");
});

test("what the app handles elsewhere, or doesn't know, stays git's own words", () => {
  // useRepoActions asks to stash and switch.
  assert.equal(explainGitError(OVERWRITTEN_BY_CHECKOUT), null);
  assert.equal(explainGitError("fatal: 'origin' does not appear to be a git repository"), null);
  assert.equal(explainGitError(""), null);
});
