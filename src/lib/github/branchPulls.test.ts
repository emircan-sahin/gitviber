import assert from "node:assert/strict";
import { test } from "node:test";
import type { Pull } from "../api/github.ts";
import { pullForBranch } from "./branchPulls.ts";

const pr = (number: number, state: Pull["state"], updatedAt: string, headRef = "fix", headRepo: string | null = "me/app", headSha = "abc1234def"): Pull => ({
  number,
  title: "",
  state,
  draft: false,
  author: "me",
  headRef,
  headSha,
  headRepo,
  baseRef: "main",
  baseSha: "",
  createdAt: updatedAt,
  updatedAt,
  url: `https://github.com/me/app/pull/${number}`,
});

test("open beats merged beats closed, whatever was updated last", () => {
  const list = [pr(1, "closed", "2026-03-01"), pr(2, "merged", "2026-02-01"), pr(3, "open", "2026-01-01")];
  assert.equal(pullForBranch(list, "fix", "me/app")?.number, 3);
  assert.equal(pullForBranch(list.slice(0, 2), "fix", "me/app")?.number, 2);
});

test("among the same state, the most recently updated", () => {
  assert.equal(pullForBranch([pr(1, "merged", "2026-01-01"), pr(2, "merged", "2026-02-01")], "fix", "me/app")?.number, 2);
});

test("only that branch of that repository, compared as GitHub does, ignoring case", () => {
  const list = [pr(1, "open", "2026-01-01", "fix", "other/app"), pr(2, "open", "2026-01-01", "fix-2"), pr(3, "open", "2026-01-01", "fix", null)];
  assert.equal(pullForBranch(list, "fix", "me/app"), undefined);
  assert.equal(pullForBranch([pr(4, "open", "2026-01-01", "fix", "Me/App")], "fix", "me/app")?.number, 4);
});

test("a list's stale copy of a PR, still open, loses to a newer one that says it merged", () => {
  const merged = pullForBranch([pr(5, "open", "2026-01-01"), pr(5, "merged", "2026-02-01")], "fix", "me/app");
  assert.equal(merged?.state, "merged");
});

test("a closed or merged PR must end at the branch's tip; an open one needn't, being pushed to", () => {
  const list = [pr(1, "merged", "2026-01-01", "fix", "me/app", "0000000aaa")];
  assert.equal(pullForBranch(list, "fix", "me/app", "abc1234"), undefined);
  assert.equal(pullForBranch(list, "fix", "me/app", "0000000")?.number, 1);
  assert.equal(pullForBranch(list, "fix", "me/app", null)?.number, 1);
  assert.equal(pullForBranch([pr(2, "open", "2026-01-01")], "fix", "me/app", "fffffff")?.number, 2);
});
