import assert from "node:assert/strict";
import { test } from "node:test";
import type { Worktree } from "../../lib/api/types.ts";
import { awayOf } from "./projectAway.ts";

const wt = (path: string, branch: string | null, main = false) => ({ path, branch, main }) as Worktree;
const worktrees = [wt("/code/app", "develop", true), wt("/code/app.worktrees/fix", "fix")];

test("a window in the main folder is not away", () => {
  assert.equal(awayOf("/code/app", "/code/app", worktrees), undefined);
});

test("a window in a linked worktree is away, with the main folder's branch", () => {
  assert.deepEqual(awayOf("/code/app.worktrees/fix", "/code/app", worktrees), { folder: "fix", mainBranch: "develop" });
});

test("the main folder's branch is null while worktrees are unknown or it is detached", () => {
  assert.deepEqual(awayOf("/code/app.worktrees/fix", "/code/app", []), { folder: "fix", mainBranch: null });
  assert.deepEqual(awayOf("/code/app.worktrees/fix", "/code/app", [wt("/code/app", null, true)]), { folder: "fix", mainBranch: null });
});
