import assert from "node:assert/strict";
import { test } from "node:test";
import { MINUTE, minuteTicker } from "./minute.ts";

function fake() {
  const env = { shown: true, timers: new Set<() => void>(), started: 0 };
  const ticker = minuteTicker({
    visible: () => env.shown,
    every: (fn, ms) => {
      assert.equal(ms, MINUTE);
      env.started++;
      env.timers.add(fn);
      return () => void env.timers.delete(fn);
    },
  });
  const fire = () => env.timers.forEach((f) => f());
  return { env, ticker, fire };
}

test("one timer for every subscriber, and none once the last one leaves", () => {
  const { env, ticker, fire } = fake();
  assert.equal(env.timers.size, 0);
  let a = 0;
  let b = 0;
  const offA = ticker.subscribe(() => a++);
  const offB = ticker.subscribe(() => b++);
  assert.equal(env.timers.size, 1);
  fire();
  assert.deepEqual([a, b], [1, 1]);
  offA();
  assert.equal(env.timers.size, 1);
  fire();
  assert.deepEqual([a, b], [1, 2]);
  offB();
  assert.equal(env.timers.size, 0);
  assert.equal(env.started, 1);
});

test("a hidden window stops the timer; shown again, it catches up at once and ticks on", () => {
  const { env, ticker } = fake();
  let n = 0;
  ticker.subscribe(() => n++);
  env.shown = false;
  ticker.visibilityChanged();
  assert.equal(env.timers.size, 0);
  assert.equal(n, 0);
  // Mounted while hidden: still no timer.
  ticker.subscribe(() => {});
  assert.equal(env.timers.size, 0);
  env.shown = true;
  ticker.visibilityChanged();
  assert.equal(n, 1);
  assert.equal(env.timers.size, 1);
});

test("no subscribers: showing the window starts nothing", () => {
  const { env, ticker } = fake();
  env.shown = false;
  ticker.visibilityChanged();
  env.shown = true;
  ticker.visibilityChanged();
  assert.equal(env.started, 0);
});
