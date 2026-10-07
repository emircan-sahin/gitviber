import assert from "node:assert/strict";
import { test } from "node:test";
import { releaseIfButtonLost, untilReleased } from "./held.ts";

// The page's window and document, as far as untilReleased uses them.
const g = globalThis as Record<string, unknown>;
// Counts what's attached, as EventTarget dedupes it: by type, listener and capture.
class Counted extends EventTarget {
  attached = new Map<string, Set<unknown>>();
  key = (type: string, o?: boolean | AddEventListenerOptions | EventListenerOptions) => `${type}:${typeof o === "boolean" ? o : !!o?.capture}`;
  override addEventListener(type: string, fn: EventListenerOrEventListenerObject | null, o?: boolean | AddEventListenerOptions) {
    const k = this.key(type, o);
    this.attached.set(k, (this.attached.get(k) ?? new Set()).add(fn));
    super.addEventListener(type, fn, o);
  }
  override removeEventListener(type: string, fn: EventListenerOrEventListenerObject | null, o?: boolean | EventListenerOptions) {
    this.attached.get(this.key(type, o))?.delete(fn);
    super.removeEventListener(type, fn, o);
  }
  get count() {
    return [...this.attached.values()].reduce((n, s) => n + s.size, 0);
  }
}
const win = new Counted();
const doc = new Counted();
g.window = win;
g.document = doc;
g.Element = class {};
const listening = () => win.count + doc.count;

const fire = (on: EventTarget, type: string, flags: Partial<Record<"metaKey" | "ctrlKey" | "altKey" | "shiftKey", boolean>> = {}) =>
  on.dispatchEvent(Object.assign(new Event(type), { metaKey: false, ctrlKey: false, altKey: false, shiftKey: false, ...flags }));

test("Mission Control: no keyup and no blur, the next move without ⌘ releases it", () => {
  let released = 0;
  untilReleased("Meta", () => released++);
  // Still held: moves, a scroll and other keys with ⌘ say so.
  fire(window, "mousemove", { metaKey: true });
  fire(window, "wheel", { metaKey: true });
  fire(window, "keydown", { metaKey: true });
  assert.equal(released, 0);
  // Back from Mission Control, ⌘ let go in there.
  fire(window, "mousemove");
  assert.equal(released, 1);
  // Once: the listeners are gone.
  fire(window, "mousemove");
  fire(window, "blur");
  assert.equal(released, 1);
});

test("held again after that, it waits again", () => {
  let released = 0;
  untilReleased("Meta", () => released++);
  fire(window, "mousemove");
  untilReleased("Meta", () => released++);
  fire(window, "mousemove", { metaKey: true });
  assert.equal(released, 1);
  fire(window, "keyup");
  assert.equal(released, 2);
});

test("its keyup, a key typed without it, the window losing focus or going hidden release it", () => {
  for (const [on, type] of [
    [window, "keyup"],
    [window, "keydown"],
    [window, "mousedown"],
    [window, "blur"],
    [document, "visibilitychange"],
  ] as const) {
    let released = 0;
    untilReleased("Meta", () => released++);
    fire(on, type);
    assert.equal(released, 1, type);
  }
});

test("only its own modifier counts", () => {
  let released = 0;
  const stop = untilReleased("Alt", () => released++);
  // ⌥ still down, ⌘ up.
  fire(window, "keyup", { altKey: true });
  assert.equal(released, 0);
  stop();
  fire(window, "keyup");
  assert.equal(released, 0);
});

test("a drag's move with no button down is a lost pointerup", () => {
  const move = (buttons: number) => Object.assign(new Event("pointermove"), { buttons, pointerId: 1 }) as unknown as PointerEvent;
  assert.equal(releaseIfButtonLost(move(1)), false);
  assert.equal(releaseIfButtonLost(move(0)), true);
});

const MODIFIERS = ["Meta", "Control", "Alt", "Shift"] as const;
const FLAGS = { Meta: "metaKey", Control: "ctrlKey", Alt: "altKey", Shift: "shiftKey" } as const;
const SIGNS = ["keydown", "keyup", "mousemove", "mousedown", "wheel"];

test("10,000 random sequences: it fires once iff a sign of release comes before stop, and leaves nothing attached", () => {
  // Seeded, so a failure replays.
  let seed = 0x9e3779b9;
  const rand = (n: number) => {
    seed = (Math.imul(seed ^ (seed >>> 15), 0x2c1b3c6d) + 0x6d2b79f5) >>> 0;
    return seed % n;
  };
  // Signs on their own target, the same on the wrong one, and events that say nothing about the keys.
  const kinds: [EventTarget, string][] = [
    ...SIGNS.map((t) => [win, t] as [EventTarget, string]),
    [win, "blur"],
    [doc, "visibilitychange"],
    [doc, "blur"],
    [win, "visibilitychange"],
    [win, "pointermove"],
    [win, "click"],
    [win, "focus"],
    [doc, "keyup"],
  ];
  for (let run = 0; run < 10_000; run++) {
    const key = MODIFIERS[rand(4)];
    let released = 0;
    const stop = untilReleased(key, () => released++);
    let expected = 0;
    let stopped = false;
    const steps = rand(12);
    for (let i = 0; i < steps; i++) {
      if (!stopped && rand(20) === 0) {
        stop();
        stopped = true;
        assert.equal(listening(), 0, `run ${run}: stop left listeners`);
        continue;
      }
      const [on, type] = kinds[rand(kinds.length)];
      const flags = { metaKey: !!rand(2), ctrlKey: !!rand(2), altKey: !!rand(2), shiftKey: !!rand(2) };
      // Nothing carries an event from the stub document up to the window, so only the right target counts.
      const sign = ((on === win && SIGNS.includes(type)) && !flags[FLAGS[key]]) || (on === win && type === "blur") || (on === doc && type === "visibilitychange");
      if (sign && !stopped && !expected) expected = 1;
      fire(on, type, flags);
      assert.equal(released, expected, `run ${run} step ${i}: ${type} on ${on === win ? "window" : "document"} for ${key}`);
      if (expected) assert.equal(listening(), 0, `run ${run}: listeners left after release`);
    }
    stop();
    assert.equal(released, expected);
    assert.equal(listening(), 0, `run ${run}: listeners left`);
  }
});

test("a release that starts a new wait isn't released by the same event", () => {
  const log: string[] = [];
  let stopNext = () => {};
  untilReleased("Meta", () => {
    log.push("first");
    stopNext = untilReleased("Meta", () => log.push("second"));
  });
  fire(win, "keyup");
  assert.deepEqual(log, ["first"]);
  fire(win, "mousemove", { metaKey: true });
  assert.deepEqual(log, ["first"]);
  fire(win, "mousemove");
  assert.deepEqual(log, ["first", "second"]);
  stopNext();
  assert.equal(listening(), 0);
});

test("stopped by an earlier listener of the same event, it doesn't fire", () => {
  let released = 0;
  let stop = () => {};
  const early = () => stop();
  win.addEventListener("keyup", early, true);
  stop = untilReleased("Meta", () => released++);
  fire(win, "keyup");
  win.removeEventListener("keyup", early, true);
  assert.equal(released, 0);
  assert.equal(listening(), 0);
});

test("off macOS the held key is Control: ⌘'s flag says nothing about it", () => {
  let released = 0;
  untilReleased("Control", () => released++);
  // A repeating Ctrl, then the Windows key going up with Ctrl still down.
  fire(win, "keydown", { ctrlKey: true });
  fire(win, "keydown", { ctrlKey: true });
  fire(win, "keyup", { ctrlKey: true });
  assert.equal(released, 0);
  fire(win, "keyup", { metaKey: true });
  assert.equal(released, 1);
  assert.equal(listening(), 0);
});

test("on Linux Ctrl's own keyup still says ctrlKey (GTK's flags are from before it): it releases anyway", () => {
  let released = 0;
  untilReleased("Control", () => released++);
  win.dispatchEvent(Object.assign(new Event("keyup"), { key: "Shift", ctrlKey: true }));
  assert.equal(released, 0);
  win.dispatchEvent(Object.assign(new Event("keyup"), { key: "Control", ctrlKey: true }));
  assert.equal(released, 1);
  assert.equal(listening(), 0);
});

test("a pen, an eraser or a finger on the glass isn't a lost button; a captured element lets go only when it holds the capture", () => {
  const released: number[] = [];
  class Captor extends (g.Element as new () => object) {
    held: boolean;
    constructor(held: boolean) {
      super();
      this.held = held;
    }
    hasPointerCapture = () => this.held;
    releasePointerCapture = (id: number) => void released.push(id);
  }
  const move = (buttons: number, target: unknown, pointerId = 7) => ({ buttons, target, pointerId }) as unknown as PointerEvent;
  // Pen tip and touch contact are 1, the eraser 32, a barrel button 2.
  for (const buttons of [1, 2, 32, 33]) assert.equal(releaseIfButtonLost(move(buttons, new Captor(true))), false, `buttons ${buttons}`);
  assert.deepEqual(released, []);
  assert.equal(releaseIfButtonLost(move(0, new Captor(false))), true);
  assert.deepEqual(released, []);
  assert.equal(releaseIfButtonLost(move(0, new Captor(true), 9)), true);
  assert.deepEqual(released, [9]);
  // Not an element (the document, the window): nothing to let go of.
  assert.equal(releaseIfButtonLost(move(0, doc)), true);
});
