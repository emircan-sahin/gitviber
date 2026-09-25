import assert from "node:assert/strict";
import { test } from "node:test";
import { basename, dirname, folderName, isInside, joinPath, parentFolder, splitPath } from "./path.ts";

test("repo paths", () => {
  assert.equal(basename("src/lib/a.ts"), "a.ts");
  assert.equal(basename("a.ts"), "a.ts");
  assert.equal(dirname("src/lib/a.ts"), "src/lib");
  assert.equal(dirname("a.ts"), "");
  assert.deepEqual(splitPath("src/a.ts"), { dir: "src/", name: "a.ts" });
  assert.deepEqual(splitPath("a.ts"), { dir: "", name: "a.ts" });
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

test("joined paths keep the folder's own separator", () => {
  assert.equal(joinPath("/Users/me/worktrees", "app"), "/Users/me/worktrees/app");
  assert.equal(joinPath("/Users/me/worktrees/", "app"), "/Users/me/worktrees/app");
  assert.equal(joinPath("/", "app"), "/app");
  assert.equal(joinPath("C:\\worktrees\\", "app"), "C:\\worktrees\\app");
  assert.equal(joinPath("C:/worktrees", "app"), "C:/worktrees/app");
});

test("a folder's parent keeps its separator", () => {
  assert.equal(parentFolder("/Users/me/app"), "/Users/me/");
  assert.equal(parentFolder("C:\\code\\app"), "C:\\code\\");
});
