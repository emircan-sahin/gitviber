import assert from "node:assert/strict";
import { test } from "node:test";
import type { FileChange } from "./api.ts";
import { folderName, isInside, joinPath, shortPath, stageable } from "./worktrees.ts";

test("short worktree paths", () => {
  const main = "/Users/me/code/app";
  assert.equal(shortPath(main, main), ".");
  assert.equal(shortPath(`${main}/.claude/worktrees/agent-1`, main), ".claude/worktrees/agent-1");
  assert.equal(shortPath("/Users/me/code/app-hotfix", main), "../app-hotfix");
  assert.equal(shortPath("/Volumes/x/app", main), "/Volumes/x/app");
  // A sibling whose name starts like the main one is not inside it.
  assert.equal(shortPath("/Users/me/code/app2", main), "../app2");
});

test("folder names ignore git's trailing slash", () => {
  assert.equal(folderName(".claude/worktrees/agent-1/"), "agent-1");
  assert.equal(folderName("/a/b"), "b");
});

test("Windows paths split on either separator, others only on /", () => {
  assert.equal(folderName("C:\\Users\\me\\app\\"), "app");
  assert.equal(folderName("C:/Users/me/app"), "app");
  assert.equal(folderName("\\\\server\\share\\app"), "app");
  assert.equal(folderName("/Users/me/a\\b"), "a\\b");
  assert.ok(isInside("C:\\code\\app\\.claude\\worktrees\\x", "C:\\code\\app"));
  assert.ok(isInside("/code/app/.claude/worktrees/x", "/code/app"));
  assert.ok(!isInside("/code/app2", "/code/app"));
  assert.ok(!isInside("/code/app", "/code/app"));
});

test("stage all leaves nested repositories out", () => {
  const file = (path: string, nested = false): FileChange => ({
    path,
    oldPath: null,
    status: "?",
    additions: null,
    deletions: null,
    oid: null,
    conflict: null,
    nested: nested ? { path: `/r/${path}` } : null,
  });
  assert.deepEqual(stageable([file("a.txt"), file("vendor/lib/", true), file("b.txt")]), { paths: ["a.txt", "b.txt"], skipped: 1 });
  assert.deepEqual(stageable([]), { paths: [], skipped: 0 });
});

test("joined paths keep the folder's own separator", () => {
  assert.equal(joinPath("/Users/me/worktrees", "app"), "/Users/me/worktrees/app");
  assert.equal(joinPath("/Users/me/worktrees/", "app"), "/Users/me/worktrees/app");
  assert.equal(joinPath("/", "app"), "/app");
  assert.equal(joinPath("C:\\worktrees\\", "app"), "C:\\worktrees\\app");
  assert.equal(joinPath("C:/worktrees", "app"), "C:/worktrees/app");
});
