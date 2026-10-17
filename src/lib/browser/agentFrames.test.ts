import assert from "node:assert/strict";
import { test } from "node:test";
import { source } from "./agentSource.test.ts";

// scripts/agent/ on a page with frames: a same-origin frame's page read and acted in as part of
// the page, at the top viewport's coordinates; another origin's frame named, not read.
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

const valueSetter = {
  prototype: Object.defineProperty({}, "value", {
    set(this: Node, v: string) {
      this.value = v;
    },
  }),
};

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
  events: { type: string; view?: unknown }[];
  [key: string]: any;
}

const style = (s: Record<string, string> = {}) => ({ display: "block", visibility: "visible", cursor: "auto", ...s });

function el(tag: string, attrs: Record<string, string> = {}, children: (Node | string)[] = [], own: Record<string, unknown> = {}): Node {
  const kids = children.map((c) => (typeof c === "string" ? ({ nodeType: 3, nodeValue: c, textContent: c, childNodes: [] } as unknown as Node) : c));
  const node: Node = {
    nodeType: 1,
    localName: tag,
    childNodes: kids,
    parentNode: null,
    parentElement: null,
    style: style(),
    box: { x: 10, y: 20, width: 100, height: 30 },
    events: [],
    isConnected: true,
    get textContent(): string {
      return kids.map((c) => c.textContent).join("");
    },
    getAttribute: (k: string) => attrs[k] ?? null,
    hasAttribute: (k: string) => k in attrs,
    querySelector: () => null,
    matches: () => false,
    getClientRects: () => [node.box],
    getBoundingClientRect: () => node.box,
    scrollIntoView: () => {},
    focus: () => {},
    click: () => node.events.push({ type: "click" }),
    dispatchEvent: (e: FakeEvent & { view?: unknown }) => {
      node.events.push({ type: e.type, view: e.view });
      return true;
    },
    contains: (other: Node | null) => {
      for (let n = other; n; n = n.parentNode) if (n === node) return true;
      return false;
    },
    ...own,
  };
  for (const kid of kids) {
    kid.parentNode = node;
    kid.parentElement = node;
  }
  return node;
}

/** A document around `children`, its elements owned by it; `hit` is its top element anywhere. */
function documentOf(children: Node[], view: Record<string, unknown>) {
  const body = el("body", {}, children);
  const doc: Record<string, any> = {
    body,
    documentElement: body,
    defaultView: view,
    baseURI: "http://localhost:3000/shop",
    hit: null as Node | null,
    getElementById: () => null,
    querySelector: (css: string) => find(body, css),
    querySelectorAll: (css: string) => (css === "iframe" ? all(body).filter((n) => n.localName === "iframe") : [find(body, css)].filter(Boolean)),
    getElementsByTagName: () => all(body),
    execCommand: () => false,
    elementFromPoint: () => doc.hit,
  };
  for (const n of [body, ...all(body)]) n.ownerDocument = doc;
  return doc;
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

/**
 * A shop page: a cart frame at (100, 50) with a 2 px border, holding a pay frame at (10, 20)
 * whose Pay button sits at (5, 5); beside them an ad from another origin.
 */
function shop() {
  const pay = el("button", {}, ["Pay"], { id: "pay", box: { x: 5, y: 5, width: 40, height: 20 } });
  const payFrame = el("iframe", { title: "Payment" }, [], { box: { x: 10, y: 20, width: 300, height: 200 }, clientLeft: 0, clientTop: 0 });
  const payView = { innerWidth: 300, innerHeight: 200, frameElement: payFrame, getComputedStyle: (e: Node) => e.style };
  const payDoc = documentOf([el("h2", {}, ["Card"]), pay], payView);
  payFrame.contentDocument = payDoc;

  const coupon = el("a", { href: "/coupon" }, ["Use a coupon"]);
  const cartFrame = el("iframe", { title: "Cart" }, [], { box: { x: 100, y: 50, width: 600, height: 400 }, clientLeft: 2, clientTop: 2 });
  const cartView = { innerWidth: 600, innerHeight: 400, frameElement: cartFrame, getComputedStyle: (e: Node) => e.style };
  const cartDoc = documentOf([coupon, payFrame], cartView);
  cartFrame.contentDocument = cartDoc;

  const ad = el("iframe", { src: "https://ads.example.test/slot?id=1" }, [], { contentDocument: null });
  const window = { innerWidth: 1280, innerHeight: 800, requestAnimationFrame: (f: () => void) => queueMicrotask(f) };
  const top = documentOf([el("h1", {}, ["Shop"]), cartFrame, ad], window);
  return { window, top, cartDoc, payDoc, pay, payFrame, cartFrame, coupon };
}

async function run(p: ReturnType<typeof shop>, input: Record<string, unknown>) {
  const args = [p.window, p.top, (e: Node) => e.style, FakeEvent, FakeEvent, FakeEvent, FakeEvent, FakeEvent, valueSetter, valueSetter, class {}, { href: "http://localhost:3000/shop" }];
  return JSON.parse(await agent(JSON.stringify(input), ...args));
}

test("a same-origin frame's page reads as part of the tree, refs and all; another origin's is named, not read", async () => {
  const p = shop();
  const { out } = await run(p, { cmd: "snapshot" });
  assert.equal(
    out,
    [
      '- heading "Shop" [level=1]',
      '- iframe "Cart"',
      '  - link "Use a coupon" [ref=e1] -> /coupon',
      '  - iframe "Payment"',
      '    - heading "Card" [level=2]',
      '    - button "Pay" [ref=e2]',
      "- iframe (cross-origin frame https://ads.example.test: not readable from the page)",
    ].join("\n"),
  );
  // -i keeps what acts, and the frame it can't read.
  const interactive = (await run(p, { cmd: "snapshot", interactive: true })).out.split("\n");
  assert.deepEqual(interactive, ['- link "Use a coupon" [ref=e1] -> /coupon', '- button "Pay" [ref=e2]', "- iframe (cross-origin frame https://ads.example.test: not readable from the page)"]);
  assert.match((await run(p, { cmd: "find", pattern: "Pay" })).out, /button "Pay" \[ref=e2\]/);
});

test("an element two frames down is aimed at in the top viewport, and its box is told there too", async () => {
  const p = shop();
  await run(p, { cmd: "snapshot" });
  p.payDoc.hit = p.pay;
  p.cartDoc.hit = p.payFrame;
  p.top.hit = p.cartFrame;
  // The button's middle (25, 15) is (35, 35) in the cart, and (137, 87) past its border on top:
  // where a native press goes, as AppKit's events are the top view's.
  assert.deepEqual(await run(p, { cmd: "aim", target: "e2" }), { x: 137, y: 87, label: 'button "Pay"', picker: false, drag: false, scrolled: false, last: 2 });
  assert.equal((await run(p, { cmd: "get", what: "box", target: "e2" })).out, "117,77,40,20");
  // A click goes to the button with its own frame's coordinates and window.
  assert.match((await run(p, { cmd: "click", target: "e2" })).out, /Clicked button "Pay"/);
  const down = p.pay.events.find((e) => e.type === "mousedown");
  assert.equal(down?.view, p.payDoc.defaultView);
  assert.ok(p.pay.events.some((e) => e.type === "click"));
});

test("a frame covered on the page above covers what's in it", async () => {
  const p = shop();
  await run(p, { cmd: "snapshot" });
  p.payDoc.hit = p.pay;
  p.cartDoc.hit = p.payFrame;
  p.top.hit = el("div", {}, [], { id: "cookie-wall" });
  p.top.hit.parentElement = p.top.body;
  assert.equal((await run(p, { cmd: "click", target: "e2" })).error, 'button "Pay" is covered by <div#cookie-wall>');
  assert.deepEqual(p.pay.events, []);
});

test("a ref in a frame that navigated, or was taken out, is stale", async () => {
  const p = shop();
  await run(p, { cmd: "snapshot" });
  // A navigated frame's old document has no window any more; its elements are still "connected" to it.
  const view = p.payDoc.defaultView;
  p.payDoc.defaultView = null;
  p.payFrame.contentDocument = documentOf([el("button", {}, ["Done"])], view);
  assert.equal((await run(p, { cmd: "click", target: "e2" })).error, "e2 is stale: take a new snapshot");
  const again = await run(p, { cmd: "snapshot" });
  assert.deepEqual(again.removed, ["e2"]);
  assert.match(again.out, /button "Done" \[ref=e3\]/);
  assert.equal((await run(p, { cmd: "get", what: "box", target: "e1" })).out, "112,72,100,30", "the cart's own refs stay");
});

/** The shop with its frames and heading findable by id, and the text its pay frame shows. */
function named() {
  const p = shop();
  p.cartFrame.id = "cart";
  p.payFrame.id = "payment";
  p.top.body.childNodes[0].id = "title";
  p.top.body.childNodes[2].id = "ad";
  p.payDoc.body.innerText = "Card Pay";
  return p;
}

test("a CSS selector reaches into a same-origin frame with frame=<css> >> <css>, nested as the frames are", async () => {
  const p = named();
  const get = (target: string) => run(p, { cmd: "get", what: "text", target });
  assert.equal((await get("frame=#cart >> frame=#payment >> #pay")).out, "Pay");
  assert.equal((await get("frame=#cart >> #payment")).out, "");
  // It looks in the frame's own page, not the page above's.
  assert.equal((await get("frame=#cart >> #pay")).error, "No element matches frame=#cart >> #pay");
  assert.equal((await get("#pay")).error, "No element matches #pay");
  // A frame by its ref is the same, and a click lands as on any ref of a frame.
  p.payDoc.hit = p.pay;
  p.cartDoc.hit = p.payFrame;
  p.top.hit = p.cartFrame;
  assert.match((await run(p, { cmd: "click", target: "frame=#cart >> frame=#payment >> #pay" })).out, /Clicked button "Pay"/);
  assert.ok(p.pay.events.some((e) => e.type === "click"));
  assert.match((await run(p, { cmd: "snapshot", scope: "frame=#cart >> #payment" })).out, /heading "Card"/);
});

test("a frame's name that isn't a readable frame says why", async () => {
  const p = named();
  const text = (target: string) => run(p, { cmd: "get", what: "text", target });
  assert.equal((await text("frame=#nope >> #pay")).error, "No frame matches #nope");
  assert.equal((await text("frame=#title >> #pay")).error, "#title isn't a frame");
  assert.equal((await text("frame=#ad >> p")).error, "#ad is another origin's frame: its page can't be read");
  await run(p, { cmd: "snapshot" });
  assert.equal((await text("frame=e9 >> #pay")).error, "No ref e9 on this page: take a new snapshot");
  assert.equal((await text("frame=e1 >> #pay")).error, "e1 isn't a frame");
});

test("wait looks in a frame by its selector, and a frame not there yet is only not yet", async () => {
  const p = named();
  assert.deepEqual(await run(p, { cmd: "exists", css: "frame=#cart >> frame=#payment >> #pay" }), { found: true, count: 1, last: 0 });
  const shown = await run(p, { cmd: "exists", css: "frame=#cart >> #missing" });
  assert.deepEqual([shown.found, shown.count], [false, 0]);
  const late = await run(p, { cmd: "exists", css: "frame=#late >> #pay" });
  assert.deepEqual([late.found, late.count, late.note, late.invalid], [false, 0, "No frame matches #late", undefined]);
  const closed = await run(p, { cmd: "exists", css: "frame=#ad >> p" });
  assert.deepEqual([closed.found, closed.invalid, closed.note], [false, true, "#ad is another origin's frame: its page can't be read"]);
  assert.equal((await run(p, { cmd: "exists", css: "frame=#title >> p" })).note, "#title isn't a frame");
});

test("wait --text looks in the page's text, or a frame's when it names one", async () => {
  const p = named();
  const has = (text: string, frame?: string) => run(p, { cmd: "hasText", text, frame });
  assert.equal((await has("Card", "#cart >> frame=#payment")).found, true);
  assert.equal((await has("Nowhere", "#cart >> frame=#payment")).found, false);
  assert.equal((await has("Card")).found, false, "the page above doesn't have the frame's text");
  assert.equal((await has("Card", "#late")).note, "No frame matches #late");
  assert.equal((await has("Card", "#ad")).invalid, true);
});

test("a frame's selector is cut at the first ' >> ' outside quotes and brackets", async () => {
  const p = named();
  const note = async (css: string) => (await run(p, { cmd: "exists", css })).note;
  assert.equal(await note("frame=#late[title='a >> b'] >> #x"), "No frame matches #late[title='a >> b']");
  assert.equal(await note('frame=#late[title="a >> b"] >> frame=#y >> #x'), 'No frame matches #late[title="a >> b"]');
  assert.equal(await note("frame=#late:not(.a >> .b) >> #x"), "No frame matches #late:not(.a >> .b)");
  // The first frame is the one missing, whatever follows; a lone frame= is no step at all.
  assert.equal(await note("frame=#nope >> frame=#cart >> #x"), "No frame matches #nope");
  assert.equal((await run(p, { cmd: "exists", css: "frame=#cart" })).found, false);
});
