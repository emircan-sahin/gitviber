import assert from "node:assert/strict";
import { test } from "node:test";
import { source } from "./agentSource.test.ts";

// scripts/agent/ at the edges a real page has: a field's value set the way WebKit sets it (a
// maxlength holds for typing, not for a script; a datetime-local is normalized), a scroll that
// may be smooth, elements moved or put back, a page that draws no frames.
const AsyncFunction = Object.getPrototypeOf(async () => {}).constructor;
const globals = ["window", "document", "getComputedStyle", "Event", "InputEvent", "MouseEvent", "PointerEvent", "KeyboardEvent", "HTMLInputElement", "HTMLTextAreaElement", "MutationObserver", "location"];
const agent = new AsyncFunction("input", ...globals, source);

class FakeEvent {
  type: string;
  constructor(type: string, init: Record<string, unknown> = {}) {
    this.type = type;
    Object.assign(this, init);
  }
}

interface Box {
  x: number;
  y: number;
  width: number;
  height: number;
}

interface Node {
  localName: string;
  childNodes: Node[];
  parentNode: Node | null;
  parentElement: Node | null;
  style: Record<string, string>;
  box: Box;
  events: string[];
  [key: string]: any;
}

// A script's value goes through as given, sanitized as the type does (no maxlength: that holds
// for what a person types only).
const valueSetter = {
  prototype: Object.defineProperty({}, "value", {
    set(this: Node, v: string) {
      this._value = this.sanitize ? this.sanitize(v) : v;
    },
  }),
};

function el(tag: string, attrs: Record<string, string> = {}, children: (Node | string)[] = [], own: Record<string, unknown> = {}): Node {
  const kids = children.map((c) => (typeof c === "string" ? ({ nodeType: 3, nodeValue: c, textContent: c, childNodes: [] } as unknown as Node) : c));
  const node: Node = {
    nodeType: 1,
    localName: tag,
    childNodes: kids,
    parentNode: null,
    parentElement: null,
    style: { display: "block", visibility: "visible", cursor: "auto" },
    box: { x: 10, y: 20, width: 100, height: 30 },
    events: [],
    isConnected: true,
    get textContent(): string {
      return kids.map((c) => c.textContent).join("");
    },
    getAttribute: (k: string) => attrs[k] ?? null,
    hasAttribute: (k: string) => k in attrs,
    setAttribute: (k: string, v: string) => void (attrs[k] = v),
    matches: (css: string) => css === ":disabled" && node.disabled === true,
    getClientRects: () => (node.style.display === "none" ? [] : [node.box]),
    getBoundingClientRect: () => node.box,
    scrollIntoView: () => {},
    focus: () => {
      node.events.push("focus");
      node.page && (node.page.document.activeElement = node);
    },
    select: () => {},
    click: () => {
      node.events.push("click");
      node.onClick?.();
    },
    dispatchEvent: (e: FakeEvent) => {
      node.events.push(e.type);
      node.on?.[e.type]?.();
      return true;
    },
    contains: (other: Node | null) => {
      for (let n = other; n; n = n.parentNode) if (n === node) return true;
      return false;
    },
    ...own,
  };
  if (own.style) node.style = { display: "block", visibility: "visible", cursor: "auto", ...(own.style as object) };
  for (const kid of kids) {
    kid.parentNode = node;
    kid.parentElement = node;
  }
  return node;
}

/** An <input> whose value reads and sets as WebKit's does. */
function input(type: string, own: Record<string, unknown> = {}): Node {
  const node = el("input", { type }, [], { type, maxLength: -1, ...own });
  node._value = "";
  Object.defineProperty(node, "value", {
    get: () => node._value,
    set: (v: string) => (node._value = node.sanitize ? node.sanitize(v) : v),
  });
  return node;
}

function page(children: Node[]) {
  const body = el("body", {}, children);
  const p = {
    hit: null as Node | null,
    location: { href: "http://localhost:3000/form" },
    window: { innerWidth: 1280, innerHeight: 800, scrollY: 0, requestAnimationFrame: (f: () => void) => queueMicrotask(f) } as Record<string, any>,
    document: {
      body,
      documentElement: body,
      activeElement: null as Node | null,
      getElementById: () => null,
      querySelector: (css: string) => find(body, css),
      querySelectorAll: () => [],
      getElementsByTagName: () => all(body),
      // Typing, as the editor does it: into the focused field, its selection replaced, no more
      // than its maxlength.
      execCommand: (cmd: string, _ui: boolean, text: string) => {
        const field = p.document.activeElement;
        if (!field || field.localName !== "input" || cmd !== "insertText") return false;
        const room = field.maxLength >= 0 ? field.maxLength : Infinity;
        field.value = text.slice(0, room);
        field.dispatchEvent(new FakeEvent("input"));
        return true;
      },
      elementFromPoint: () => p.hit,
    },
    getComputedStyle: (e: Node) => e.style,
  };
  for (const n of all(body)) n.page = p;
  return p;
}

function all(root: Node): Node[] {
  const kids = (root.childNodes ?? []).filter((k) => k.nodeType === 1);
  return kids.flatMap((k) => [k, ...all(k)]);
}

function find(root: Node, css: string): Node | null {
  if (root.id && css === `#${root.id}`) return root;
  for (const kid of root.childNodes ?? []) {
    const found = kid.nodeType === 1 ? find(kid, css) : null;
    if (found) return found;
  }
  return null;
}

async function run(p: ReturnType<typeof page>, ask: Record<string, unknown>) {
  const args = [p.window, p.document, p.getComputedStyle, FakeEvent, FakeEvent, FakeEvent, FakeEvent, FakeEvent, valueSetter, valueSetter, Observer, p.location];
  return JSON.parse(await agent(JSON.stringify(ask), ...args));
}

/** The page's one MutationObserver. */
class Observer {
  static live: Observer | null = null;
  fn: (records: unknown[]) => void;
  constructor(fn: (records: unknown[]) => void) {
    this.fn = fn;
  }
  observe() {
    Observer.live = this;
  }
  disconnect() {
    if (Observer.live === this) Observer.live = null;
  }
  takeRecords() {
    return [];
  }
}

test("fill holds to a maxlength as typing does, rather than setting past it", async () => {
  const pin = input("text", { id: "pin", maxLength: 4 });
  const p = page([pin]);
  const reply = await run(p, { cmd: "fill", target: "#pin", text: "123456" });
  // A person typing 123456 gets 1234; fill says so instead of storing six digits.
  assert.equal(pin.value.length <= 4, true, `the field holds ${JSON.stringify(pin.value)}`);
  assert.match(reply.error ?? "", /maxlength 4/);
});

test("fill takes a datetime-local with seconds, which WebKit normalizes", async () => {
  // The HTML spec's sanitization: a valid value is kept in its shortest form.
  const normalize = (v: string) => (/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}(:\d{2}(\.\d{1,3})?)?$/.test(v) ? v.replace(/(:00)?(\.0+)?$/, "") : "");
  const when = input("datetime-local", { id: "when", sanitize: normalize });
  const p = page([when]);
  const reply = await run(p, { cmd: "fill", target: "#when", text: "2026-03-01T09:30:00" });
  assert.equal(when.value, "2026-03-01T09:30");
  assert.equal(reply.error, undefined);
  assert.equal(reply.out, "Filled textbox");
});

test("type goes into an email or number field, whose selection can't be set", async () => {
  for (const kind of ["email", "number"]) {
    const field = input(kind, {
      id: "f",
      // As WebKit does for these types.
      setSelectionRange: () => {
        throw new Error(`InvalidStateError: The input element's type ('${kind}') does not support selection.`);
      },
    });
    const p = page([field]);
    const reply = await run(p, { cmd: "type", target: "#f", text: "12" });
    assert.equal(reply.error, undefined, `${kind}: ${reply.error}`);
  }
});

test("a click by CSS on an SVG shape lands, though it has no click()", async () => {
  const shape = el("path", {}, [], { id: "close-icon", click: undefined });
  const p = page([el("button", {}, [el("svg", {}, [shape])])]);
  p.hit = shape;
  const reply = await run(p, { cmd: "click", target: "#close-icon" });
  assert.equal(reply.error, undefined, reply.error);
  assert.ok(shape.events.includes("click"), "a click event reached it");
});

test("an element's screenshot box is read after the scroll to it, on a page that scrolls smoothly", async () => {
  const far = el("section", {}, ["Pricing"], { id: "pricing", box: { x: 0, y: 3000, width: 600, height: 400 } });
  // CSS scroll-behavior: smooth, as many sites set on :root: the scroll lands a while later.
  far.scrollIntoView = (o: { behavior?: string }) => {
    const land = () => (far.box = { x: 0, y: 200, width: 600, height: 400 });
    if (o?.behavior === "instant") land();
    else setTimeout(land, 20);
  };
  const p = page([far]);
  const reply = await run(p, { cmd: "view", target: "#pricing" });
  assert.deepEqual(reply.rect, { x: 0, y: 200, w: 600, h: 400 });
});

test("a click reaches an element far down a page that scrolls smoothly", async () => {
  const far = el("button", {}, ["Far"], { id: "far", box: { x: 10, y: 4000, width: 60, height: 20 } });
  // As WKWebView traced it: each smooth scrollIntoView starts the glide over, and a glide begun
  // anew each try closes only part of the way (y 4000 → 548 in 30 tries, aiming at 291).
  let gliding = false;
  far.scrollIntoView = (o: { behavior?: string }) => {
    if (o?.behavior === "instant") far.box = { x: 10, y: 390, width: 60, height: 20 };
    else gliding = true;
  };
  far.getBoundingClientRect = () => {
    if (gliding) far.box = { ...far.box, y: far.box.y - (far.box.y - 390) * 0.08 };
    return far.box;
  };
  const p = page([far]);
  p.hit = far;
  const reply = await run(p, { cmd: "click", target: "#far" });
  assert.equal(reply.error, undefined, reply.error);
});

test("select takes the same option named twice in a <select multiple>", async () => {
  const option = (value: string) => el("option", { value }, [value.toUpperCase()], { value, selected: false });
  const options = [option("a"), option("b")];
  const multi = el("select", {}, options, { id: "multi", options, multiple: true });
  const reply = await run(page([multi]), { cmd: "select", target: "#multi", values: ["a", "a"] });
  assert.equal(reply.error, undefined, reply.error);
});

test("a link inside a heading gets a ref", async () => {
  const link = el("a", { href: "/posts/1" }, ["First post"]);
  const p = page([el("h2", {}, [link])]);
  const { out } = await run(p, { cmd: "snapshot" });
  assert.match(out, /link "First post" \[ref=e\d+\]/);
  assert.match((await run(p, { cmd: "snapshot", interactive: true })).out, /link "First post"/);
});

test("an element moved elsewhere keeps its ref; a new one with the same text gets its own", async () => {
  const save = el("button", {}, ["Save"]);
  const left = el("div", {}, [save]);
  const right = el("div", {}, []);
  const p = page([left, right]);
  assert.match((await run(p, { cmd: "snapshot" })).out, /button "Save" \[ref=e1\]/);
  // Moved to another parent (a portal, a re-parent): the same element, the same ref.
  left.childNodes.splice(0);
  right.childNodes.push(save);
  save.parentNode = right;
  assert.match((await run(p, { cmd: "snapshot" })).out, /button "Save" \[ref=e1\]/);
  // Re-rendered as a new element: e1 is stale, and the new one is e2.
  save.isConnected = false;
  const fresh = el("button", {}, ["Save"]);
  right.childNodes.splice(0, 1, fresh);
  const again = await run(p, { cmd: "snapshot" });
  assert.match(again.out, /button "Save" \[ref=e2\]/);
  assert.deepEqual(again.removed, ["e1"]);
  assert.equal((await run(p, { cmd: "get", what: "box", target: "e1" })).error, "e1 is stale: take a new snapshot");
});

test("an element taken out and put back (a kept-alive tab panel) has its old ref again", async () => {
  const tab = el("button", {}, ["Settings"]);
  const panel = el("div", {}, [tab]);
  const p = page([panel]);
  await run(p, { cmd: "snapshot" });
  tab.isConnected = false;
  panel.childNodes.splice(0);
  assert.deepEqual((await run(p, { cmd: "snapshot" })).removed, ["e1"]);
  tab.isConnected = true;
  panel.childNodes.push(tab);
  const back = await run(p, { cmd: "snapshot" });
  assert.match(back.out, /button "Settings" \[ref=e1\]/);
  assert.equal(back.last, 1, "no number spent on it");
});

test("a page back from the back-forward cache keeps its refs, and numbers new ones past the tab's highest", async () => {
  const old = el("button", {}, ["Old"]);
  const p = page([old]);
  await run(p, { cmd: "snapshot", floor: 0 });
  // Another document handed out up to e40 meanwhile; this one's window (and refs) came back.
  p.document.body.childNodes.push(el("button", {}, ["New"]));
  const back = await run(p, { cmd: "snapshot", floor: 40 });
  assert.match(back.out, /button "Old" \[ref=e1\]/);
  assert.match(back.out, /button "New" \[ref=e41\]/);
  assert.equal((await run(p, { cmd: "get", what: "box", target: "e20", floor: 41 })).error, "e20 is stale: take a new snapshot");
  assert.equal((await run(p, { cmd: "get", what: "box", target: "e42", floor: 41 })).error, "No ref e42 on this page: take a new snapshot");
});

test("snapshots again and again spend no numbers on what already has a ref", async () => {
  const buttons = Array.from({ length: 50 }, (_, i) => el("button", {}, [`B${i}`]));
  const p = page(buttons);
  let last = 0;
  for (let i = 0; i < 20; i++) last = (await run(p, { cmd: "snapshot" })).last;
  assert.equal(last, 50);
  // find and a screenshot's labels use the same numbers.
  assert.equal((await run(p, { cmd: "find", pattern: "B7" })).last, 50);
  assert.equal((await run(p, { cmd: "view", refs: true })).last, 50);
});

test("a page that draws no frames still gets its answer within seconds", async () => {
  const hidden = el("button", {}, ["Later"], { id: "later", style: { display: "none" } });
  const p = page([hidden]);
  p.window.requestAnimationFrame = () => {};
  const started = Date.now();
  const reply = await run(p, { cmd: "click", target: "#later" });
  const took = Date.now() - started;
  assert.match(reply.error, /isn't visible/);
  assert.ok(took >= 1000 && took < 3000, `took ${took} ms`);
  delete p.window.requestAnimationFrame;
  assert.match((await run(p, { cmd: "click", target: "#later" })).error, /isn't visible/);
});

test("a full page is scrolled through with its fixed and sticky parts shown once, and put back", async () => {
  // A style that keeps inline properties as a page's does: a priority comes with the value.
  const pinned = (position: string, y: number, edges: Record<string, string> = { top: "0px", bottom: "auto" }) => {
    const inline = new Map<string, [string, string]>();
    return el("div", {}, [], {
      box: { x: 0, y, width: 1280, height: 60 },
      style: {
        position,
        ...edges,
        display: "block",
        visibility: "visible",
        getPropertyValue: (k: string) => inline.get(k)?.[0] ?? "",
        getPropertyPriority: (k: string) => inline.get(k)?.[1] ?? "",
        setProperty: (k: string, v: string, priority = "") => (v ? inline.set(k, [v, priority]) : inline.delete(k)),
        shown: () => !inline.get("visibility")?.[0],
      },
    });
  };
  const header = pinned("fixed", 0);
  const footer = pinned("fixed", 740);
  const bar = pinned("sticky", 0);
  const plain = pinned("static", 300);
  // Sticky in the flow of the page (a section's header), and one stuck to the bottom edge.
  const section = pinned("sticky", 300);
  const sticker = pinned("sticky", 740, { top: "auto", bottom: "0px" });
  const body = el("body", {}, [header, footer, bar, plain, section, sticker], { scrollHeight: 2400 });
  const p = page([]);
  Object.assign(p.document, { body, documentElement: body, querySelectorAll: () => [body, header, footer, bar, plain, section, sticker] });
  p.window.scrollX = 0;
  p.window.scrollY = 500;
  p.window.scrollTo = ({ top }: { top: number }) => (p.window.scrollY = top);
  const shown = () => [header, footer, bar, plain, section, sticker].map((n) => (n.style as any).shown());

  const begun = await run(p, { cmd: "tile", step: "begin" });
  assert.deepEqual([begun.height, begun.w, begun.h, p.window.scrollY], [2400, 1280, 800, 0]);
  // The first tile has the header and the bar stuck at the top; the last has the footer and the bar stuck at the bottom; a sticky one in the page's flow is always there.
  assert.equal((await run(p, { cmd: "tile", step: "to", y: 0, first: true, last: false })).y, 0);
  assert.deepEqual(shown(), [true, false, true, true, true, false]);
  assert.equal((await run(p, { cmd: "tile", step: "to", y: 800, first: false, last: false })).y, 800);
  assert.deepEqual(shown(), [false, false, false, true, true, false]);
  await run(p, { cmd: "tile", step: "to", y: 1600, first: false, last: true });
  assert.deepEqual(shown(), [false, true, false, true, true, true]);
  // A page of one tile shows all of it.
  await run(p, { cmd: "tile", step: "to", y: 0, first: true, last: true });
  assert.deepEqual(shown(), [true, true, true, true, true, true]);
  // Done: everything shown and the page where the person had it.
  await run(p, { cmd: "tile", step: "to", y: 800, first: false, last: false });
  await run(p, { cmd: "tile", step: "end" });
  assert.deepEqual([...shown(), p.window.scrollY], [true, true, true, true, true, true, 500]);
  assert.match((await run(p, { cmd: "tile", step: "to", y: 0 })).error, /No full-page screenshot/);
});

test("a full page's tile waits for the images in view that are still coming, but not for long", async () => {
  const body = el("body", {}, [], { scrollHeight: 2400 });
  const p = page([]);
  let asked = 0;
  // In view, and loaded on its fifth look; another far below, never loaded.
  const coming = { getBoundingClientRect: () => ({ x: 0, y: 100, width: 50, height: 50 }), get complete() { return ++asked >= 5; } };
  const below = { getBoundingClientRect: () => ({ x: 0, y: 3000, width: 50, height: 50 }), complete: false };
  Object.assign(p.document, { body, documentElement: body, querySelectorAll: () => [], images: [coming, below] });
  p.window.scrollX = 0;
  p.window.scrollTo = ({ top }: { top: number }) => (p.window.scrollY = top);
  await run(p, { cmd: "tile", step: "begin" });
  await run(p, { cmd: "tile", step: "to", y: 0, first: true, last: true });
  assert.equal(asked, 5, "waited for the one in view, and for it alone");
  await run(p, { cmd: "tile", step: "end" });
});
