import assert from "node:assert/strict";
import { test } from "node:test";
import { formatBytes, isoToUnix, plural, relativeTime, shortDuration } from "./format.ts";

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

test("durations and sizes at a glance", () => {
  assert.equal(shortDuration(-5), "<1m");
  assert.equal(shortDuration(59), "<1m");
  assert.equal(shortDuration(42 * 60 + 30), "42m");
  assert.equal(shortDuration(3 * 3600 + 5 * 60), "3h 5m");
  assert.equal(shortDuration(50 * 3600), "2d");
  assert.equal(formatBytes(9), "9 B");
  assert.equal(formatBytes(4002), "3.9 KB");
  assert.equal(formatBytes(312 * 1024 * 1024), "312 MB");
});

test("sizes just under a unit read as that unit, never 1024 of the smaller", () => {
  assert.equal(formatBytes(1023), "1023 B");
  assert.equal(formatBytes(1024), "1.0 KB");
  assert.equal(formatBytes(1024 * 1024 - 1), "1.0 MB");
  assert.equal(formatBytes(10 * 1024 - 1), "10 KB");
  assert.equal(formatBytes(3 * 1024 ** 4), "3.0 TB");
});
