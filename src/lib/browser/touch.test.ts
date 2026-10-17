import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

// scripts/touch.js, what a touch device's tab runs in its pages, against a stand-in window made of
// Node's events: what the page can tell of a touch screen, and how the mouse reaches it.
const source = readFileSync(new URL("../../../src-tauri/src/browser/scripts/touch.js", import.meta.url), "utf8");

class UIEvent extends Event {
  view: unknown;
  constructor(type: string, init: Record<string, unknown> = {}) {
    super(type, init);
    this.view = init.view;
  }
}

class PointerEvent extends UIEvent {
  [key: string]: unknown;
  constructor(type: string, init: Record<string, unknown> = {}) {
    super(type, init);
    for (const [k, v] of Object.entries(init)) if (!["bubbles", "cancelable", "composed"].includes(k)) this[k] = v;
  }
}

interface Rule {
  media?: { mediaText: string };
  cssRules?: Rule[];
}

function page() {
  const asked: string[] = [];
  const inserted: string[] = [];
  class Navigator {}
  class Document extends EventTarget {
    createEvent(_kind: string): Event {
      throw new Error("not supported");
    }
  }
  class Element extends EventTarget {}
  class CSSStyleSheet {
    insertRule(text: string) {
      inserted.push(text);
      return 0;
    }
  }
  const observers: ((records: unknown[]) => void)[] = [];
  class MutationObserver {
    constructor(fn: (records: unknown[]) => void) {
      observers.push(fn);
    }
    observe() {}
  }
  const window = Object.assign(new EventTarget(), {
    Navigator,
    UIEvent,
    PointerEvent,
    Document,
    HTMLElement: Element,
    SVGElement: Element,
    CSSStyleSheet,
    MutationObserver,
    matchMedia: (query: string) => (asked.push(query), { matches: true, media: query }),
  }) as unknown as Record<string, any> & EventTarget;
  const document = Object.assign(new Document(), { styleSheets: [] as { media: { mediaText: string }; cssRules: Rule[] }[] });
  new Function("window", "document", source)(window, document);
  return { window, document, asked, inserted, observers, navigator: new Navigator() as Record<string, any> };
}

const mouse = (type: string, init: Record<string, unknown> = {}) => {
  const e = new PointerEvent(type, { bubbles: true, cancelable: true, pointerId: 1, pointerType: "mouse", button: 0, buttons: 0, clientX: 10, clientY: 20, ...init });
  Object.defineProperty(e, "isTrusted", { value: true });
  return e;
};

test("a touch device's page finds a touch screen: events, points, handlers", () => {
  const p = page();
  assert.equal(p.navigator.maxTouchPoints, 5);
  assert.equal("ontouchstart" in p.window, true);
  assert.equal(typeof p.window.TouchEvent, "function");
  assert.equal(p.window.ontouchstart, null);
  const e = new p.window.TouchEvent("touchstart", { touches: [new p.window.Touch({ identifier: 1 })] });
  assert.equal(e.touches.length, 1);
  assert.equal(e.touches.item(0).identifier, 1);
  assert.equal(e.changedTouches.length, 0);
  // `on…` handlers run, and a false answer cancels as an attribute's does.
  let ran = 0;
  p.window.ontouchstart = () => (ran++, false);
  const start = new p.window.TouchEvent("touchstart", { cancelable: true });
  p.window.dispatchEvent(start);
  assert.deepEqual([ran, start.defaultPrevented], [1, true]);
  p.window.ontouchstart = null;
  p.window.dispatchEvent(new p.window.TouchEvent("touchstart"));
  assert.equal(ran, 1);
  assert.equal(p.document.createEvent("TouchEvent").type, "");
});

test("a media query naming the pointer or hover is answered as a touch screen would", () => {
  const p = page();
  const ask = (q: string) => {
    p.window.matchMedia(q);
    return p.asked.at(-1);
  };
  assert.equal(ask("(pointer: coarse)"), "(min-width: 0px)");
  assert.equal(ask("(pointer: fine)"), "(min-width: 100000px)");
  assert.equal(ask("(any-pointer:coarse)"), "(min-width: 0px)");
  assert.equal(ask("(hover: hover)"), "(min-width: 100000px)");
  assert.equal(ask("(hover: none) and (min-width: 700px)"), "(min-width: 0px) and (min-width: 700px)");
  assert.equal(ask("not (hover)"), "not (min-width: 100000px)");
  assert.equal(ask("(pointer)"), "(min-width: 0px)");
  assert.equal(ask("(max-width: 600px)"), "(max-width: 600px)");
  // The page still reads what it asked.
  assert.equal(p.window.matchMedia("(hover: hover)").media, "(hover: hover)");
});

test("the page's own styles are read for those features, as are rules added later", () => {
  const p = page();
  const inner: Rule = { media: { mediaText: "(hover: hover)" } };
  const outer: Rule = { media: { mediaText: "screen and (pointer: fine)" }, cssRules: [inner] };
  const plain: Rule = { media: { mediaText: "(min-width: 700px)" } };
  p.document.styleSheets.push({ media: { mediaText: "all" }, cssRules: [outer, plain, {}] });
  p.document.dispatchEvent(new Event("DOMContentLoaded"));
  assert.equal(outer.media!.mediaText, "screen and (min-width: 100000px)");
  assert.equal(inner.media!.mediaText, "(min-width: 100000px)");
  assert.equal(plain.media!.mediaText, "(min-width: 700px)");
  // A sheet of another origin can't be read, and doesn't stop the rest.
  p.document.styleSheets.unshift({
    media: { mediaText: "all" },
    get cssRules(): Rule[] {
      throw new DOMException("blocked", "SecurityError");
    },
  });
  assert.doesNotThrow(() => p.document.dispatchEvent(new Event("DOMContentLoaded")));
  new p.window.CSSStyleSheet().insertRule("@media (hover: none) { a { color: red } }", 0);
  assert.deepEqual(p.inserted, ["@media (min-width: 0px) { a { color: red } }"]);
});

test("a <style> is read again when its text is put in or changed, and an @import when it arrives", () => {
  const p = page();
  const rule: Rule = { media: { mediaText: "(hover: hover)" } };
  const style: any = { nodeType: 1, localName: "style", sheet: { media: { mediaText: "all" }, cssRules: [rule] } };
  const text = { nodeType: 3, parentNode: style };
  const observe = (...records: unknown[]) => p.observers[0](records);
  // Its text set after the element came in: the element is the record's target.
  observe({ type: "childList", target: style, addedNodes: [text] });
  assert.equal(rule.media!.mediaText, "(min-width: 100000px)");
  // Edited in place (hot reload): a new sheet, read through the text's parent.
  const next: Rule = { media: { mediaText: "(hover: hover)" } };
  style.sheet = { media: { mediaText: "all" }, cssRules: [next] };
  observe({ type: "characterData", target: text, addedNodes: [] });
  assert.equal(next.media!.mediaText, "(min-width: 100000px)");
  // An element holding a <style> comes in; a text change elsewhere costs nothing.
  const late: Rule = { media: { mediaText: "(pointer: fine)" } };
  const holder = { nodeType: 1, localName: "div", firstElementChild: {}, querySelectorAll: () => [{ sheet: { media: { mediaText: "all" }, cssRules: [late] } }] };
  observe({ type: "childList", target: {}, addedNodes: [holder] }, { type: "characterData", target: { parentNode: { localName: "p" } }, addedNodes: [] });
  assert.equal(late.media!.mediaText, "(min-width: 100000px)");
  // A sheet is walked once; its @import is followed once it has loaded.
  const imported: Rule = { media: { mediaText: "(hover: hover)" } };
  const link: any = { media: { mediaText: "all" } };
  p.document.styleSheets.push({ media: { mediaText: "all" }, cssRules: [link, { selectorText: "a" }] });
  p.document.dispatchEvent(new Event("DOMContentLoaded"));
  link.styleSheet = { media: { mediaText: "all" }, cssRules: [imported] };
  p.document.dispatchEvent(new Event("DOMContentLoaded"));
  assert.equal(imported.media!.mediaText, "(min-width: 100000px)");
});

/** A page's listeners, logged by type. */
function watch(p: ReturnType<typeof page>) {
  const log: string[] = [];
  for (const type of ["pointerdown", "pointermove", "pointerup", "pointerover", "mousemove", "mousedown", "mouseup", "click", "touchstart", "touchmove", "touchend", "touchcancel", "pointercancel", "dragstart"]) {
    p.window.addEventListener(type, (e: any) => log.push(e.isTrusted ? `${type}!` : e.pointerType ? `${type}:${e.pointerType}` : type));
  }
  return log;
}

test("the mouse reaches a touch page as a finger: a tap, a drag, no hovering", () => {
  const p = page();
  const log = watch(p);
  const send = (e: Event) => p.window.dispatchEvent(e);
  send(mouse("pointerover"));
  send(mouse("pointermove"));
  send(mouse("mousemove"));
  assert.deepEqual(log, ["mousemove!"], "a hover is no touch, but the mouse's own compatibility events stay");
  log.length = 0;
  send(mouse("pointerdown"));
  send(mouse("mousedown"));
  send(mouse("pointermove", { buttons: 1 }));
  send(mouse("mousemove", { buttons: 1 }));
  send(mouse("pointerup"));
  send(mouse("mouseup"));
  send(mouse("click"));
  assert.deepEqual(log, [
    "pointerdown:touch",
    "touchstart",
    "mousedown!",
    "pointermove:touch",
    "touchmove",
    "pointerup:touch",
    "touchend",
    "mouseup!",
    "click!",
  ]);
});

test("a touch handler that cancels the tap cancels the mouse events after it, once", () => {
  const p = page();
  const log = watch(p);
  const cancelOn = (type: string) => p.window.addEventListener(type, (e: Event) => e.preventDefault(), { once: true });
  const tap = () => {
    const events = ["pointerdown", "mousedown", "pointerup", "mouseup", "click"].map((t) => mouse(t));
    for (const e of events) p.window.dispatchEvent(e);
    return events.map((e) => e.defaultPrevented);
  };
  // Cancelled at touchend: the press already reached the page, what follows doesn't.
  cancelOn("touchend");
  assert.deepEqual(tap(), [false, false, false, true, true]);
  assert.deepEqual(log.filter((t) => t.endsWith("!")), ["mousedown!"]);
  log.length = 0;
  // Cancelled at touchstart: none of the mouse events, and mousedown's default (focus) is cancelled.
  cancelOn("touchstart");
  assert.deepEqual(tap(), [false, true, false, true, true]);
  assert.deepEqual(log.filter((t) => t.endsWith("!")), []);
  // The next tap is judged on its own, and a click with no touch before it (a key's) is the page's.
  log.length = 0;
  assert.deepEqual(tap(), [false, false, false, false, false]);
  assert.deepEqual(log.filter((t) => t.endsWith("!")), ["mousedown!", "mouseup!", "click!"]);
  const key = mouse("click");
  p.window.dispatchEvent(key);
  assert.equal(key.defaultPrevented, false);
});

test("only the main button is a finger, and a press the page loses lifts it", () => {
  const p = page();
  const log = watch(p);
  const send = (e: Event) => p.window.dispatchEvent(e);
  // A right click is a mouse's alone.
  send(mouse("pointerdown", { button: 2, buttons: 2 }));
  send(mouse("pointerup", { button: 2 }));
  assert.deepEqual(log, ["pointerdown!", "pointerup!"]);
  log.length = 0;
  // A drag that starts is a touch cancelled; a move after it is no finger's.
  send(mouse("pointerdown", { button: 0 }));
  send(mouse("dragstart"));
  send(mouse("pointermove", { buttons: 1 }));
  assert.deepEqual(log, ["pointerdown:touch", "touchstart", "pointercancel:touch", "touchcancel", "dragstart!", "pointermove!"]);
  // A pointer cancelled is the same, and the next press starts afresh.
  log.length = 0;
  send(mouse("pointerdown"));
  send(mouse("pointercancel"));
  send(mouse("pointerup"));
  assert.deepEqual(log, ["pointerdown:touch", "touchstart", "pointercancel:touch", "touchcancel", "pointerup!"]);
});

test("the finger lands on what was hit inside a shadow tree, not its host", () => {
  const p = page();
  const inner = new EventTarget();
  const reached: string[] = [];
  for (const type of ["pointerdown", "touchstart", "touchend"]) inner.addEventListener(type, () => reached.push(type));
  const down: any = mouse("pointerdown");
  down.composedPath = () => [inner, p.window];
  const up: any = mouse("pointerup");
  up.composedPath = () => [inner, p.window];
  p.window.dispatchEvent(down);
  p.window.dispatchEvent(up);
  assert.deepEqual(reached, ["pointerdown", "touchstart", "touchend"]);
});
