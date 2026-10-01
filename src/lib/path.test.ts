import assert from "node:assert/strict";
import { test } from "node:test";
import { basename, compareEntries, dirname, folderName, isInside, joinPath, parentFolder, sharedNames, splitPath } from "./path.ts";

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

test("the explorer sorts folders first, then numbers by value", () => {
  const entry = (name: string, isDir = false) => ({ name, isDir });
  const sorted = [entry("file10"), entry("b"), entry("File2"), entry("z", true), entry("a1", true)].sort(compareEntries);
  assert.deepEqual(sorted.map((e) => e.name), ["a1", "z", "b", "File2", "file10"]);
  const ties = ["readme.md", "file2", "README.md", "file02"].map((n) => entry(n));
  assert.deepEqual(ties.sort(compareEntries).map((e) => e.name), ["file02", "file2", "README.md", "readme.md"]);
});

test("only names two different paths share are marked", () => {
  assert.deepEqual([...sharedNames(["src/a/index.ts", "src/b/index.ts", "README.md", "src/a/index.ts", "docs/README.txt"])], ["src/a/index.ts", "src/b/index.ts"]);
  // The same file twice (its diff and its staged copy) isn't a clash.
  assert.equal(sharedNames(["a.ts", "a.ts"]).size, 0);
  assert.deepEqual([...sharedNames(["index.ts", "src/index.ts"])], ["index.ts", "src/index.ts"]);
});
