/// <reference lib="dom" />
import assert from "node:assert/strict";
import { mock, test } from "node:test";

// The idle shutdown runs on the mock clock, so the run doesn't wait five minutes for it.
mock.timers.enable({ apis: ["setTimeout"] });

// A stand-in worker: records what it's asked to tokenize and answers when told to.
type Msg = { id: number; code: string };
const sent: Msg[] = [];
let fake: { onmessage?: (e: { data: unknown }) => void; terminated: boolean } | null = null;
globalThis.Worker = class {
  onmessage?: (e: { data: unknown }) => void;
  terminated = false;
  constructor() {
    fake = this;
  }
  postMessage(m: Msg) {
    sent.push(m);
  }
  terminate() {
    this.terminated = true;
  }
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

test("an idle worker stops, and the next job starts a new one", async () => {
  sent.length = 0;
  const job = highlight("before idle", "ts", "t");
  reply();
  await job.promise;
  const idle = fake!;
  mock.timers.tick(5 * 60_000);
  assert.ok(idle.terminated);
  const next = highlight("after idle", "ts", "t");
  assert.notEqual(fake, idle);
  reply();
  await next.promise;
  assert.deepEqual(codes(), ["before idle", "after idle"]);
});

test("the cache drops the oldest big text past its budget", async () => {
  sent.length = 0;
  const [a, b] = ["a".repeat(1_200_000), "b".repeat(1_200_000)];
  for (const code of [a, b]) {
    const job = highlight(code, "ts", "t");
    reply();
    await job.promise;
  }
  highlight(b, "ts", "t");
  highlight(a, "ts", "t");
  assert.deepEqual(codes(), [a, b, a]);
});
