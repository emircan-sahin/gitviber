import assert from "node:assert/strict";
import { test } from "node:test";
import type { Commit } from "../api/types.ts";
import { byDay, commitDay, pickedCommits, pickLabel } from "./pullCommits.ts";

const commit = (sha: string, parent: string | null, at = new Date(2026, 8, 28, 12)): Commit => ({
  sha,
  shortSha: sha.slice(0, 7),
  authorName: "",
  authorEmail: "",
  timestamp: at.getTime() / 1000,
  committerName: "",
  committedAt: at.getTime() / 1000,
  parents: parent ? [parent] : [],
  refs: [],
  subject: sha,
  body: "",
  unpushed: false,
  onOrigin: true,
  notInHead: false,
});

// base ← a ← b ← c, oldest first as the PR lists them.
const a = commit("aaaaaaa1", "base0000");
const b = commit("bbbbbbb2", a.sha);
const c = commit("ccccccc3", b.sha);
const list = [a, b, c];

test("commits either side of local midnight fall on two days", () => {
  // Built from local time, so the boundary is midnight wherever the test runs.
  const late = commit("late", null, new Date(2026, 8, 27, 23, 59));
  const early = commit("early", "late", new Date(2026, 8, 28, 0, 1));
  const later = commit("later", "early", new Date(2026, 8, 28, 9, 30));
  assert.deepEqual(byDay([late, early, later]), [
    { day: commitDay(late), list: [late] },
    { day: commitDay(early), list: [early, later] },
  ]);
  assert.notEqual(commitDay(late), commitDay(early));
});

test("a pick is named by its short id, a run by its first and last", () => {
  assert.equal(pickLabel([b]), "bbbbbbb");
  assert.equal(pickLabel([a, b, c]), "aaaaaaa–ccccccc");
});

test("a run is the same picked either way round", () => {
  assert.deepEqual(pickedCommits(list, { anchor: a.sha, to: b.sha }), [a, b]);
  assert.deepEqual(pickedCommits(list, { anchor: c.sha, to: b.sha }), [b, c]);
  assert.deepEqual(pickedCommits(list, { anchor: b.sha, to: b.sha }), [b]);
  assert.equal(pickedCommits(list, null), null);
});

test("a force-push that took an end away drops the pick", () => {
  assert.equal(pickedCommits(list, { anchor: "gone", to: b.sha }), null);
  assert.equal(pickedCommits(list, { anchor: a.sha, to: "gone" }), null);
});
