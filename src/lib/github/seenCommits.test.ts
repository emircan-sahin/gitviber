import assert from "node:assert/strict";
import { test } from "node:test";

// Node has no localStorage: a Map stands in for it.
const items = new Map<string, string>();
globalThis.localStorage = {
  getItem: (k: string) => items.get(k) ?? null,
  setItem: (k: string, v: string) => void items.set(k, v),
} as Storage;

const { saveSeenCommits, seenCommits } = await import("./seenCommits.ts");

test("a PR never opened has no seen commits; one opened keeps those still in it", () => {
  assert.equal(seenCommits("pr/1"), null);
  saveSeenCommits("pr/1", ["a", "b"], new Set(["a", "b"]));
  assert.deepEqual(seenCommits("pr/1"), new Set(["a", "b"]));
  // A force-push replaced b with c: b is left behind, c isn't seen yet.
  saveSeenCommits("pr/1", ["a", "c"], new Set(["a", "b"]));
  assert.deepEqual(seenCommits("pr/1"), new Set(["a"]));
});

test("only the 50 most recently opened PRs are kept", () => {
  for (let i = 0; i < 51; i++) saveSeenCommits(`pr/${i}`, ["a"], new Set(["a"]));
  // pr/1 was saved before, and again in the loop: pr/0 is the oldest now.
  assert.equal(seenCommits("pr/0"), null);
  assert.deepEqual(seenCommits("pr/1"), new Set(["a"]));
  assert.deepEqual(seenCommits("pr/50"), new Set(["a"]));
});
