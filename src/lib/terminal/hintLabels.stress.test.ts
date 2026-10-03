import { test } from "node:test";
import assert from "node:assert/strict";
import { HINT_ALPHABET, hintLabels } from "./hintLabels.ts";

test("every count up to 600 links: distinct, prefix-free, few keys, home-row letters only", () => {
  assert.equal(new Set(HINT_ALPHABET).size, HINT_ALPHABET.length, "no letter twice");
  assert.match(HINT_ALPHABET, /^[a-z]+$/, "lower case: Shift is the copy key");
  for (let n = 0; n <= 600; n++) {
    const labels = hintLabels(n);
    assert.equal(labels.length, n);
    const set = new Set(labels);
    assert.equal(set.size, n, `${n}: distinct`);
    // No label is another's start: each one's prefixes are not labels.
    for (const l of labels) for (let k = 1; k < l.length; k++) assert.ok(!set.has(l.slice(0, k)), `${n}: ${l.slice(0, k)} starts ${l}`);
    // Two keys reach 196 links, three the rest of a screen.
    const longest = Math.max(0, ...labels.map((l) => l.length));
    assert.ok(longest <= (n <= 1 ? n : n <= 14 ? 1 : n <= 196 ? 2 : 3), `${n}: ${longest} keys`);
  }
  // 26 links: half of them one key.
  assert.equal(hintLabels(26).filter((l) => l.length === 1).length, 13);
});
