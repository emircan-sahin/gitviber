import assert from "node:assert/strict";
import { test } from "node:test";
import type { Commit } from "../../lib/api/types.ts";
import { editedShas, keptUpTo, reorderBefore } from "./edits.ts";

// Newest first, as History lists them.
const line = ["e", "d", "c", "b", "a"];
const place = (moved: string[], sha: string, where: "above" | "below") => reorderBefore(line, new Set(moved), { sha, where });

test("a gap names the commit the moved ones go under", () => {
  assert.equal(place(["a"], "c", "below"), "c");
  assert.equal(place(["a"], "c", "above"), "d");
  assert.equal(place(["a"], "e", "above"), null);
  // Above a row whose neighbour above is moving: under the next one that stays.
  assert.equal(place(["d", "a"], "c", "above"), "e");
  assert.equal(place(["e", "a"], "d", "above"), null);
});

test("dropping where they are changes nothing", () => {
  assert.equal(place(["c"], "d", "below"), undefined);
  assert.equal(place(["c"], "b", "above"), undefined);
  assert.equal(place(["e"], "d", "above"), undefined);
  assert.equal(place(["c", "b"], "a", "above"), undefined);
  // Apart, the same gap gathers them: that's a move.
  assert.equal(place(["d", "b"], "c", "above"), "e");
});

test("the ends of the list", () => {
  // Below the oldest listed: under it, so the root can be passed.
  assert.equal(place(["c"], "a", "below"), "a");
  // Everything moved: nowhere new to go.
  assert.equal(place(["e", "d", "c", "b"], "a", "above"), undefined);
  // The newest ones sent under the oldest.
  assert.equal(place(["e", "d"], "a", "below"), "a");
});

test("a gap between picked commits gathers them", () => {
  // d and b under c: e c d b a.
  assert.equal(place(["d", "b"], "c", "below"), "c");
  // a and e above c: under d, keeping their order.
  assert.equal(place(["e", "a"], "c", "above"), "d");
});

// Made-up commits, newest first: e d c b a, each the parent of the one before.
const commit = (sha: string, parent?: string) => ({ sha, parents: parent ? [parent] : [] }) as unknown as Commit;
const listed = ["e", "d", "c", "b", "a"].map((sha, i, all) => commit(sha, all[i + 1]));
const [e, d] = listed;

test("an edit names the commits it starts from", () => {
  assert.deepEqual(editedShas({ kind: "reword", sha: "c", message: "m" }, [listed[2]]), ["c"]);
  assert.deepEqual(editedShas({ kind: "move", sha: "d", up: true }, [d]), ["d"]);
  assert.deepEqual(editedShas({ kind: "move", sha: "d", up: false }, [d]), ["d", "c"]);
  assert.deepEqual(editedShas({ kind: "squash", shas: ["e"], onto: "b", message: null }, [e]), ["e", "b"]);
  assert.deepEqual(editedShas({ kind: "reorder", shas: ["e"], before: "c" }, [e]), ["e", "c"]);
  assert.deepEqual(editedShas({ kind: "reorder", shas: ["c"], before: null }, []), ["c"]);
  assert.deepEqual(editedShas({ kind: "drop", shas: ["d", "b"] }, []), ["d", "b"]);
});

test("the rewrite keeps what's under its oldest commit", () => {
  assert.equal(keptUpTo(["e", "c"], listed), "b");
  assert.equal(keptUpTo(["d", "a"], listed), null);
  // Not listed yet (the next page, a search's gap): it stands in for its parent.
  assert.equal(keptUpTo(["e", "z"], listed), "z");
});
