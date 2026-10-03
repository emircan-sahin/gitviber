import assert from "node:assert/strict";
import { test } from "node:test";
import { branchPoint, commitPoint, defaultPoints, githubCompareUrl, HEAD_POINT, pointKind, tagPoint } from "./comparePoints.ts";

const sha = "0123456789abcdef0123456789abcdef01234567";

test("points name branches, remote branches, tags and commits by full ref or id", () => {
  assert.deepEqual(branchPoint({ name: "feature/x", remote: false }), { ref: "refs/heads/feature/x", label: "feature/x" });
  assert.deepEqual(branchPoint({ name: "origin/main", remote: true }), { ref: "refs/remotes/origin/main", label: "origin/main" });
  assert.deepEqual(tagPoint("v1.0"), { ref: "refs/tags/v1.0", label: "v1.0" });
  assert.deepEqual(commitPoint(sha), { ref: sha, label: "0123456" });
  assert.deepEqual(["HEAD", "refs/heads/a", "refs/remotes/o/a", "refs/tags/t", sha].map(pointKind), ["head", "branch", "remote", "tag", "commit"]);
});

test("the screen starts on origin's default branch against the branch checked out", () => {
  const branches = [
    { name: "main", remote: false, remoteDefault: false },
    { name: "origin/main", remote: true, remoteDefault: true },
  ];
  assert.deepEqual(defaultPoints(branches, "fix"), { base: { ref: "refs/remotes/origin/main", label: "origin/main" }, head: { ref: "refs/heads/fix", label: "fix" } });
  // Detached, and no default branch to be found: HEAD on both sides, for the person to pick from.
  assert.deepEqual(defaultPoints([], null), { base: HEAD_POINT, head: HEAD_POINT });
});

test("GitHub's compare page needs names it knows", () => {
  const web = "https://github.com/acme/app";
  const feature = branchPoint({ name: "feature/x y", remote: false });
  const main = branchPoint({ name: "origin/main", remote: true });
  assert.equal(githubCompareUrl(web, main, feature, true), `${web}/compare/main...feature/x%20y`);
  assert.equal(githubCompareUrl(web, main, feature, false), `${web}/compare/main..feature/x%20y`);
  assert.equal(githubCompareUrl(web, tagPoint("v1"), commitPoint(sha), true), `${web}/compare/v1...${sha}`);
  // Another remote's branch, HEAD, or no GitHub: nothing to link.
  assert.equal(githubCompareUrl(web, branchPoint({ name: "fork/main", remote: true }), feature, true), null);
  assert.equal(githubCompareUrl(web, HEAD_POINT, feature, true), null);
  assert.equal(githubCompareUrl(null, main, feature, true), null);
});
