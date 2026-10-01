import assert from "node:assert/strict";
import { test } from "node:test";
import { basename, compareEntries, dirname, distinctFolders, folderName, isInside, joinPath, parentFolder, splitPath } from "./path.ts";

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

test("tabs of one name show the fewest folders that tell them apart", () => {
  const folders = (paths: string[]) => Object.fromEntries(distinctFolders(paths));
  assert.deepEqual(folders(["src/a/index.ts", "src/b/index.ts", "lib/c/index.ts", "README.md"]), { "src/a/index.ts": "a", "src/b/index.ts": "b", "lib/c/index.ts": "c" });
  // The same parent's name at different depths.
  assert.deepEqual(folders(["a/x/index.ts", "b/x/index.ts", "x/index.ts"]), { "a/x/index.ts": "a/x", "b/x/index.ts": "b/x", "x/index.ts": "x" });
  // A root-level file has no folder to show; the other's tells them apart.
  assert.deepEqual(folders(["index.ts", "src/index.ts"]), { "index.ts": "", "src/index.ts": "src" });
  // The same file twice (its diff and its staged copy) isn't a clash.
  assert.equal(distinctFolders(["a.ts", "a.ts", "src/a/b.ts"]).size, 0);
});
