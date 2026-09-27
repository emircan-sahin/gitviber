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
  assert.equal(sanitizedRefName("fix login bug"), "fix-login-bug");
  assert.equal(sanitizedRefName("feat/ok-name_1.2"), "feat/ok-name_1.2");
  assert.equal(sanitizedRefName("a~b^c:d?e*f[g\\h|i\"j<k>l"), "a-b-c-d-e-f-g-h-i-j-k-l");
  assert.equal(sanitizedRefName("a  \t b"), "a-b");
  assert.equal(sanitizedRefName("x@{1}..y"), "x-1}-y");
  assert.equal(sanitizedRefName(".hidden."), "hidden-");
  assert.equal(sanitizedRefName("topic.lock"), "topic-");
  assert.equal(sanitizedRefName("topic/"), "topic-");
  assert.equal(sanitizedRefName("--+-force"), "force");
  assert.equal(sanitizedRefName("ünïcode-ok"), "ünïcode-ok");
});

test("a typed name says what it becomes, or that it's taken", () => {
  assert.deepEqual(refNameCheck(" fix login ", ["main"]), { name: "fix-login", taken: false, hint: "Will be created as fix-login" });
  assert.deepEqual(refNameCheck("fix login", ["fix-login"]), { name: "fix-login", taken: true, hint: "A branch named fix-login already exists." });
  assert.deepEqual(refNameCheck("feat", ["main"]), { name: "feat", taken: false, hint: null });
  assert.deepEqual(refNameCheck("  ", []), { name: "", taken: false, hint: null });
  assert.equal(refNameCheck("a b", [], "renamed to").hint, "Will be renamed to a-b");
});
