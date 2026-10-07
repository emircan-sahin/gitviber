import assert from "node:assert/strict";
import { test } from "node:test";
import { branchTracking, refNameCheck, reviewBase, sanitizedRefName, worktreeBase, worktreeBranch } from "./refs.ts";

test("a review starts from origin's default branch, else the local one", () => {
  const b = (name: string, remote = false, remoteDefault = false) => ({ name, remote, remoteDefault });
  assert.equal(reviewBase([b("feature"), b("main"), b("origin/main", true)]), "refs/remotes/origin/main");
  assert.equal(reviewBase([b("dev"), b("origin/dev", true, true), b("origin/main", true)]), "refs/remotes/origin/dev");
  assert.equal(reviewBase([b("feature"), b("main")]), "refs/heads/main");
  assert.equal(reviewBase([b("feature"), b("master")]), null);
});

test("a typed ref name becomes one git takes", () => {
  const cases: [string, string][] = [
    ["fix login bug", "fix-login-bug"],
    ["feat/ok-name_1.2", "feat/ok-name_1.2"],
    ['a~b^c:d?e*f[g\\h|i"j<k>l', "a-b-c-d-e-f-g-h-i-j-k-l"],
    ["a  \t b", "a-b"],
    ["x@{1}..y", "x-1}-y"],
    [".hidden.", "hidden-"],
    ["topic.lock", "topic-"],
    ["topic/", "topic-"],
    ["--+-force", "force"],
    ["ünïcode-ok", "ünïcode-ok"],
    // What GitHub Desktop's own rule lets through and git refuses.
    ["-.hidden", "hidden"],
    ["~.x", "x"],
    ["-/a", "a"],
    ["/a", "a"],
    ["a/.b", "a/-b"],
    ["a/.git", "a/-git"],
    ["a/./b", "a/-/b"],
    ["a.lock/b", "a-/b"],
    ["a/b.lock/c", "a/b-/c"],
    ["a//b", "a/b"],
    ["HEAD", "HEAD-"],
    ["@", "@-"],
  ];
  for (const [typed, name] of cases) assert.equal(sanitizedRefName(typed), name, typed);
});

test("a typed name says what it becomes, or that it's taken", () => {
  assert.deepEqual(refNameCheck(" fix login ", ["main"]), { name: "fix-login", taken: false, hint: "Will be created as fix-login." });
  assert.deepEqual(refNameCheck("fix login", ["fix-login"]), { name: "fix-login", taken: true, hint: "A branch named fix-login already exists." });
  assert.deepEqual(refNameCheck("feat", ["main"]), { name: "feat", taken: false, hint: null });
  assert.deepEqual(refNameCheck("  ", []), { name: "", taken: false, hint: null });
  assert.equal(refNameCheck("a b", [], true).hint, "Will be renamed to a-b.");
  // Where refs ignore case (macOS, Windows), Master is master.
  assert.equal(refNameCheck("Master", ["master"], false, true).hint, "A branch named master already exists.");
  assert.equal(refNameCheck("Master", ["master"], false, false).taken, false);
});

const row = (name: string, o: { remote?: boolean; current?: boolean; worktree?: string; remoteDefault?: boolean } = {}) => ({
  name,
  remote: o.remote ?? false,
  current: o.current ?? false,
  worktree: o.worktree ?? null,
  remoteDefault: o.remoteDefault ?? false,
});

test("a new worktree checks out what the typed name already is", () => {
  const branches = [
    row("main", { current: true }),
    row("feat"),
    row("held", { worktree: "/w/repo.worktrees/held" }),
    row("origin/main", { remote: true }),
    row("origin/remote-only", { remote: true }),
    row("upstream/remote-only", { remote: true }),
    row("upstream/theirs", { remote: true }),
  ];
  const pick = (typed: string) => {
    const { name, base, track, taken } = worktreeBranch(typed, branches, false);
    return { name, base, ...(track && { track }), taken };
  };
  assert.deepEqual(pick("new thing"), { name: "new-thing", base: undefined, taken: false });
  assert.deepEqual(pick("feat"), { name: "feat", base: null, taken: false });
  assert.deepEqual(pick("main"), { name: "main", base: undefined, taken: true });
  assert.deepEqual(pick("held"), { name: "held", base: undefined, taken: true });
  assert.match(worktreeBranch("held", branches, false).hint!, /checked out in held;/);
  // Only on a remote: tracked from origin's, or from the remote typed.
  assert.deepEqual(pick("remote-only"), { name: "remote-only", base: "refs/remotes/origin/remote-only", track: true, taken: false });
  assert.deepEqual(pick("upstream/remote-only"), { name: "remote-only", base: "refs/remotes/upstream/remote-only", track: true, taken: false });
  assert.deepEqual(pick("theirs"), { name: "theirs", base: "refs/remotes/upstream/theirs", track: true, taken: false });
  // origin/main is main, which is checked out here; origin/nope would be an ambiguous name.
  assert.deepEqual(pick("origin/main"), { name: "main", base: undefined, taken: true });
  assert.deepEqual(pick("origin/nope"), { name: "origin/nope", base: undefined, taken: true });
});

test("a new worktree starts from the current branch, else the default one", () => {
  assert.equal(worktreeBase([row("main"), row("feat", { current: true })]), "refs/heads/feat");
  assert.equal(worktreeBase([row("dev"), row("origin/dev", { remote: true, remoteDefault: true })]), "refs/heads/dev");
  assert.equal(worktreeBase([row("origin/dev", { remote: true, remoteDefault: true })]), "refs/remotes/origin/dev");
  assert.equal(worktreeBase([row("main")]), "refs/heads/main");
  assert.equal(worktreeBase([]), "HEAD");
});

test("a local branch's standing with its upstream, in a few characters and in words", () => {
  const b = (t?: { ahead?: number; behind?: number; gone?: boolean }, upstream: string | null = "origin/feat", remote = false) =>
    branchTracking({ remote, upstream }, t && { ahead: 0, behind: 0, gone: false, ...t });
  assert.equal(b(), null);
  assert.deepEqual(b({ ahead: 2 }), { text: "↑2", label: "Compared with origin/feat: 2 commits ahead" });
  assert.deepEqual(b({ ahead: 1, behind: 3 }), { text: "↑1 ↓3", label: "Compared with origin/feat: 1 commit ahead, 3 commits behind" });
  assert.deepEqual(b({ behind: 1 }), { text: "↓1", label: "Compared with origin/feat: 1 commit behind" });
  assert.deepEqual(b(undefined, null), { text: "local only", label: "Not published: it has no upstream" });
  assert.deepEqual(b({ gone: true }), { text: "upstream gone", label: "Its upstream origin/feat is gone from the remote" });
  assert.equal(b(undefined, null, true), null);
});
