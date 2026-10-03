import assert from "node:assert/strict";
import { test } from "node:test";
import type { Commit } from "../api/types.ts";
import { filesSelection, onDisk, selectionKey, selectionPath } from "./selection.ts";

test("two working-tree files compared make one tab per pair, the right one's path its own", () => {
  const ab = filesSelection("src/a.ts", "src/b.ts");
  assert.equal(selectionPath(ab), "src/b.ts");
  assert.equal(selectionKey(ab), selectionKey(filesSelection("src/a.ts", "src/b.ts")));
  // Another left side, or the sides swapped, is another comparison.
  assert.notEqual(selectionKey(ab), selectionKey(filesSelection("src/c.ts", "src/b.ts")));
  assert.notEqual(selectionKey(ab), selectionKey(filesSelection("src/b.ts", "src/a.ts")));
  // Not the file's own tab.
  assert.notEqual(selectionKey(ab), selectionKey({ kind: "file", path: "src/b.ts" }));
  assert.equal(onDisk(ab), false);
});

test("names with spaces, unicode and a leading dash keep their sides apart", () => {
  const odd = filesSelection("-dash $x.txt", "ünï code.txt");
  assert.equal(selectionPath(odd), "ünï code.txt");
  assert.ok(selectionKey(odd).includes("-dash $x.txt"));
});

const commit = (sha: string) => ({ sha, shortSha: sha.slice(0, 7), subject: "s", parents: [] }) as unknown as Commit;

test("a commit's whole list is its own tab, apart from the lists Changes keeps", () => {
  const a = selectionKey({ kind: "changes", list: "commit", commit: commit("a".repeat(40)) });
  assert.equal(a, selectionKey({ kind: "changes", list: "commit", commit: commit("a".repeat(40)), url: "https://example.test/c" }));
  assert.notEqual(a, selectionKey({ kind: "changes", list: "commit", commit: commit("b".repeat(40)) }));
  assert.notEqual(a, selectionKey({ kind: "changes", list: "unstaged" }));
  assert.equal(selectionKey({ kind: "changes", list: "branch" }), "changes::All Branch Changes");
  assert.equal(selectionPath({ kind: "changes", list: "commit", commit: commit("a".repeat(40)) }), "Commit aaaaaaa");
});

test("a range's list is told apart by its ends, and by being a PR's", () => {
  const range = { label: "main...feature", base: "1".repeat(40), head: "2".repeat(40) };
  const key = selectionKey({ kind: "changes", list: "range", range });
  assert.notEqual(key, selectionKey({ kind: "changes", list: "range", range: { ...range, head: "3".repeat(40) } }));
  assert.notEqual(key, selectionKey({ kind: "changes", list: "range", range: { ...range, number: 4 } }));
  assert.equal(selectionPath({ kind: "changes", list: "range", range }), "Compare main...feature");
  assert.equal(selectionPath({ kind: "changes", list: "range", range: { base: "1".repeat(40), head: "2".repeat(40) } }), "Compare 1111111..2222222");
});

test("the Compare screen is one tab, whichever two points it shows", () => {
  const side = (label: string) => ({ ref: `refs/heads/${label}`, label });
  const one = selectionKey({ kind: "compare", base: side("main"), head: side("a"), mergeBase: true });
  assert.equal(one, selectionKey({ kind: "compare", base: side("dev"), head: side("b"), mergeBase: false }));
  assert.equal(selectionPath({ kind: "compare", base: side("main"), head: side("a"), mergeBase: true }), "Compare");
});
