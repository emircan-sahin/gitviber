import assert from "node:assert/strict";
import { test } from "node:test";
import { buttonLost, untilReleased } from "./held.ts";

// The page's window and document, as far as untilReleased uses them.
const g = globalThis as Record<string, unknown>;
g.window = new EventTarget();
g.document = new EventTarget();
g.Element = class {};

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
  assert.equal(buttonLost(move(1)), false);
  assert.equal(buttonLost(move(0)), true);
});
