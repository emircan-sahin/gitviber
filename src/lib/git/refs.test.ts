import assert from "node:assert/strict";
import { test } from "node:test";
import { refNameCheck, reviewBase, sanitizedRefName } from "./refs.ts";

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
