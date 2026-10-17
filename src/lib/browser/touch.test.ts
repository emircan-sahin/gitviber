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
  const e = new PointerEvent(type, { bubbles: true, cancelable: true, pointerId: 1, pointerType: "mouse", clientX: 10, clientY: 20, ...init });
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

/** A page's listeners, logged by type. */
function watch(p: ReturnType<typeof page>) {
  const log: string[] = [];
  for (const type of ["pointerdown", "pointermove", "pointerup", "pointerover", "mousemove", "mousedown", "mouseup", "click", "touchstart", "touchmove", "touchend"]) {
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

test("a touch handler that cancels the tap cancels its click too, once", () => {
  const p = page();
  const log = watch(p);
  p.window.addEventListener("touchend", (e: Event) => e.preventDefault());
  const tap = () => {
    p.window.dispatchEvent(mouse("pointerdown"));
    p.window.dispatchEvent(mouse("pointerup"));
    const click = mouse("click");
    p.window.dispatchEvent(click);
    return click.defaultPrevented;
  };
  assert.equal(tap(), true);
  assert.equal(log.filter((t) => t === "click!").length, 0, "the click never reached the page");
  // The next tap is judged on its own.
  assert.equal(tap(), true);
  // A click with no touch before it (a key's) is the page's.
  const key = mouse("click");
  p.window.dispatchEvent(key);
  assert.equal(key.defaultPrevented, false);
  assert.equal(log.filter((t) => t === "click!").length, 1);
});
