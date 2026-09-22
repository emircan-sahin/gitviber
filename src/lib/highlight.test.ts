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

const { highlight, prefetchHighlight } = await import("./highlight.ts");

const reply = () => {
  const m = sent[sent.length - 1];
  fake!.onmessage!({ data: { id: m.id, lines: [[[m.code, "#fff", 0]]], fg: "#fff" } });
};
const codes = () => sent.map((m) => m.code);

test("the file on screen goes ahead of queued prefetches", async () => {
  sent.length = 0;
  prefetchHighlight("p1", "ts", "t");
  prefetchHighlight("p2", "ts", "t");
  const view = highlight("v1", "ts", "t", true);
  reply();
  assert.deepEqual(codes(), ["p1", "v1"]);
  reply();
  assert.equal((await view.promise)?.lines[0][0][0], "v1");
  reply(); // p2
});

test("a view that's gone before its turn is never tokenized", async () => {
  sent.length = 0;
  prefetchHighlight("busy", "ts", "t");
  const stale = highlight("old revision", "ts", "t", true);
  highlight("new revision", "ts", "t", true);
  stale.release();
  assert.equal(await stale.promise, null);
  reply();
  reply();
  assert.deepEqual(codes(), ["busy", "new revision"]);
});

test("only the newest prefetches stay queued", () => {
  sent.length = 0;
  for (const c of ["a", "b", "c", "d", "e", "f", "g"]) prefetchHighlight(c, "ts", "t");
  for (let i = 0; i < 5; i++) reply();
  // "a" was already running; of the rest, the oldest ones beyond four were dropped, newest first.
  assert.deepEqual(codes(), ["a", "g", "f", "e", "d"]);
});

test("a finished result is served from the cache", async () => {
  sent.length = 0;
  const first = highlight("cached", "ts", "t", true);
  reply();
  await first.promise;
  const again = highlight("cached", "ts", "t", true);
  assert.equal((await again.promise)?.lines[0][0][0], "cached");
  assert.deepEqual(codes(), ["cached"]);
});
