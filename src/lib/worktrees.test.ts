import assert from "node:assert/strict";
import { test } from "node:test";
import type { FileChange } from "./api.ts";
import { folderName, nestedLabel, shortPath, stageable } from "./worktrees.ts";

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

test("nested entries say what they are", () => {
  assert.equal(nestedLabel({ path: "/r/w", worktree: true, branch: "agent-1" }), "agent-1");
  assert.equal(nestedLabel({ path: "/r/w", worktree: true, branch: null }), "detached");
  assert.equal(nestedLabel({ path: "/r/v", worktree: false, branch: null }), "nested repo");
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
    nested: nested ? { path: `/r/${path}`, worktree: true, branch: "b" } : null,
  });
  assert.deepEqual(stageable([file("a.txt"), file(".claude/worktrees/x/", true), file("b.txt")]), { paths: ["a.txt", "b.txt"], skipped: 1 });
  assert.deepEqual(stageable([]), { paths: [], skipped: 0 });
});
