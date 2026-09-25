import assert from "node:assert/strict";
import { test } from "node:test";
import { isoToUnix, plural, relativeTime } from "./format.ts";

test("relative times round down to their largest unit", () => {
  const now = Date.now() / 1000;
  assert.equal(relativeTime(now), "just now");
  assert.equal(relativeTime(now - 59), "just now");
  assert.equal(relativeTime(now - 90), "1m ago");
  assert.equal(relativeTime(now - 3 * 3600 - 5), "3h ago");
  assert.equal(relativeTime(now - 8 * 86400), "1w ago");
  // A clock a little ahead of the commit's isn't "in the future".
  assert.equal(relativeTime(now + 30), "just now");
});

test("GitHub times and plurals", () => {
  assert.equal(isoToUnix("1970-01-01T00:01:00Z"), 60);
  assert.equal(plural(1, "commit"), "1 commit");
  assert.equal(plural(0, "commit"), "0 commits");
  assert.equal(plural(3, "worktree"), "3 worktrees");
});
