import assert from "node:assert/strict";
import { test } from "node:test";
import { reviewBase } from "./refs.ts";

test("a review starts from origin's default branch, else the local one", () => {
  const b = (name: string, remote = false, remoteDefault = false) => ({ name, remote, remoteDefault });
  assert.equal(reviewBase([b("feature"), b("main"), b("origin/main", true)]), "refs/remotes/origin/main");
  assert.equal(reviewBase([b("dev"), b("origin/dev", true, true), b("origin/main", true)]), "refs/remotes/origin/dev");
  assert.equal(reviewBase([b("feature"), b("main")]), "refs/heads/main");
  assert.equal(reviewBase([b("feature"), b("master")]), null);
});
