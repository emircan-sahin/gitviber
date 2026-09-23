/// <reference lib="dom" />
import assert from "node:assert/strict";
import { test } from "node:test";

// A stand-in worker: records what it's asked to tokenize and answers when told to.
type Msg = { id: number; code: string };
const sent: Msg[] = [];
let fake: { onmessage?: (e: { data: unknown }) => void } | null = null;
globalThis.Worker = class {
  onmessage?: (e: { data: unknown }) => void;
  constructor() {
    fake = this;
  }
  postMessage(m: Msg) {
    sent.push(m);
  }
  terminate() {}
} as unknown as typeof Worker;

const { highlight } = await import("./highlight.ts");

const reply = () => {
  const m = sent[sent.length - 1];
  fake!.onmessage!({ data: { id: m.id, lines: [[[m.code, "#fff", 0]]], fg: "#fff" } });
};
const codes = () => sent.map((m) => m.code);

test("a view that's gone before its turn is never tokenized", async () => {
  sent.length = 0;
  highlight("busy", "ts", "t");
  const stale = highlight("old revision", "ts", "t");
  highlight("new revision", "ts", "t");
  stale.release();
  assert.equal(await stale.promise, null);
  reply();
  reply();
  assert.deepEqual(codes(), ["busy", "new revision"]);
});

test("a finished result is served from the cache", async () => {
  sent.length = 0;
  const first = highlight("cached", "ts", "t");
  reply();
  await first.promise;
  const again = highlight("cached", "ts", "t");
  assert.equal((await again.promise)?.lines[0][0][0], "cached");
  assert.deepEqual(codes(), ["cached"]);
});
