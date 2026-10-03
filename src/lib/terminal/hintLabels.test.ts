import { test } from "node:test";
import assert from "node:assert/strict";
import { HINT_ALPHABET, hintLabels } from "./hintLabels.ts";

test("hint labels: one letter while they last, then two, none the start of another", () => {
  assert.deepEqual(hintLabels(0), []);
  assert.deepEqual(hintLabels(1), ["s"]);
  assert.deepEqual(hintLabels(3, "abc"), ["a", "b", "c"]);
  // kitty's own example: with 5 from "abc", "a" is skipped so "aa"… can follow.
  assert.deepEqual(hintLabels(5, "abc"), ["b", "c", "aa", "ab", "ac"]);
  assert.deepEqual(hintLabels(HINT_ALPHABET.length), [...HINT_ALPHABET]);
  for (const n of [2, 13, 14, 15, 27, 100, 196, 197, 500]) {
    const labels = hintLabels(n);
    assert.equal(new Set(labels).size, n, `${n} distinct`);
    for (const a of labels) for (const b of labels) assert.ok(a === b || !b.startsWith(a), `${n}: ${a} starts ${b}`);
    assert.ok(labels.every((l) => [...l].every((c) => HINT_ALPHABET.includes(c))));
    // Shortest first.
    assert.ok(labels.every((l, i) => i === 0 || l.length >= labels[i - 1].length));
  }
  assert.equal(hintLabels(196).at(-1)!.length, 2, "two letters reach 196");
});
