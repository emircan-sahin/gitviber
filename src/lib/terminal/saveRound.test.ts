import assert from "node:assert/strict";
import { test } from "node:test";
import { dueForSave, SAVE_MS } from "./saveRound.ts";

test("a printing pane is saved once it stops, or every 30 s, one pane a round", () => {
  const pane = (dirty: boolean, wroteAt: number, serializedAt: number) => ({ dirty, wroteAt, serializedAt });
  const now = 100_000;
  const [quiet, busy, stale, older, clean] = [pane(true, now - SAVE_MS, now - 5000), pane(true, now - 10, now - 5000), pane(true, now - 10, now - 31_000), pane(true, now - 10, now - 40_000), pane(false, 0, 0)];
  assert.deepEqual(dueForSave([busy, clean], now, false), []);
  assert.deepEqual(dueForSave([busy, quiet], now, false), [quiet]);
  assert.deepEqual(dueForSave([quiet, stale, older, busy], now, false), [older], "the stalest first, alone");
  assert.deepEqual(dueForSave([quiet, busy, clean], now, true), [quiet, busy]);
  // Three agents printing for a minute, a round every 2 s: each saved every 30 s, never two in a round.
  const agents = [pane(true, 0, 0), pane(true, 0, 0), pane(true, 0, 0)];
  let serialized = 0;
  for (let t = now + SAVE_MS; t <= now + 60_000; t += SAVE_MS) {
    agents.forEach((a) => Object.assign(a, { dirty: true, wroteAt: t - 5 }));
    const due = dueForSave(agents, t, false);
    assert.ok(due.length <= 1);
    due.forEach((a) => Object.assign(a, { dirty: false, serializedAt: t }));
    serialized += due.length;
  }
  assert.equal(serialized, 6);
});
