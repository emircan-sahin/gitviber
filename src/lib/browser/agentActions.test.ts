import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

// scripts/agent.js acting on a stand-in page: an action happens only on an element a person
// could act on, and fill, select and check read back what they did.
const source = readFileSync(new URL("../../../src-tauri/src/browser/scripts/agent.js", import.meta.url), "utf8");
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

// The value setter fill goes through, as a page's framework sees it; `accept` stands for what the
// page or the browser keeps of it (a maxlength, a mask).
const valueSetter = {
  prototype: Object.defineProperty({}, "value", {
    set(this: Node, v: string) {
      this.value = this.accept ? this.accept(v) : v;
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
  events: string[];
  [key: string]: any;
}

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
    focus: () => node.events.push("focus"),
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

type Mutation = { type: string; addedNodes?: { nodeType: number }[]; removedNodes?: { nodeType: number }[] };

/** The page's one MutationObserver, which a test's page fires as it changes. */
class Observer {
  static live: Observer | null = null;
  pending: Mutation[] = [];
  fn: (records: Mutation[]) => void;
  constructor(fn: (records: Mutation[]) => void) {
    this.fn = fn;
  }
  observe() {
    Observer.live = this;
  }
  disconnect() {
    if (Observer.live === this) Observer.live = null;
  }
  takeRecords() {
    return this.pending.splice(0);
  }
  static fire(given: Mutation[], later = false) {
    const o = Observer.live;
    if (!o) return;
    // As a MutationRecord has them: both lists, empty or not.
    const records = given.map((r) => ({ addedNodes: [], removedNodes: [], ...r }));
    if (later) o.pending.push(...records);
    else o.fn(records);
  }
}

/** A page around `children`, whose top element anywhere is `hit` (the element acted on unless set). */
function page(children: Node[]) {
  const body = el("body", {}, children);
  const p = {
    hit: null as Node | null,
    location: { href: "http://localhost:3000/join" },
    window: { innerWidth: 1280, innerHeight: 800, requestAnimationFrame: (f: () => void) => queueMicrotask(f) },
    document: {
      body,
      documentElement: body,
      activeElement: null,
      getElementById: () => null,
      querySelector: (css: string) => find(body, css),
      querySelectorAll: (css: string) => (css.startsWith("dialog") ? all(body).filter((n) => n.getAttribute("role") === "dialog") : []),
      getElementsByTagName: () => all(body),
      execCommand: () => false,
      elementFromPoint: () => p.hit,
    },
    getComputedStyle: (e: Node) => e.style,
  };
  return p;
}

/** Every element under `root`. */
function all(root: Node): Node[] {
  const kids = (root.childNodes ?? []).filter((k) => k.nodeType === 1);
  return kids.flatMap((k) => [k, ...all(k)]);
}

/** `#id` in the stand-in page. */
function find(root: Node, css: string): Node | null {
  if (root.id && css === `#${root.id}`) return root;
  for (const kid of root.childNodes ?? []) {
    const found = kid.nodeType === 1 ? find(kid, css) : null;
    if (found) return found;
  }
  return null;
}

async function run(p: ReturnType<typeof page>, input: Record<string, unknown>) {
  const args = [p.window, p.document, p.getComputedStyle, FakeEvent, FakeEvent, FakeEvent, FakeEvent, FakeEvent, valueSetter, valueSetter, Observer, p.location];
  return JSON.parse(await agent(JSON.stringify(input), ...args));
}

test("a click lands only on what shows, is enabled, holds still and is on top", async () => {
  const pay = el("button", {}, ["Pay"], { id: "pay" });
  const p = page([pay]);
  p.hit = pay;
  assert.equal((await run(p, { cmd: "click", target: "#pay" })).out, 'Clicked button "Pay"');
  assert.deepEqual(pay.events, ["pointerover", "mouseover", "pointermove", "mousemove", "pointerdown", "mousedown", "focus", "pointerup", "mouseup", "click"]);
  // Native input clicks where this says: the middle of its box.
  const at = await run(p, { cmd: "point", target: "#pay" });
  assert.deepEqual([at.x, at.y], [60, 35]);

  pay.events.length = 0;
  pay.disabled = true;
  assert.equal((await run(p, { cmd: "click", target: "#pay" })).error, 'button "Pay" is disabled');
  pay.disabled = false;
  pay.style.display = "none";
  assert.equal((await run(p, { cmd: "click", target: "#pay" })).error, 'button "Pay" isn\'t visible (display: none, here or above)');
  pay.style.display = "block";
  pay.style.visibility = "hidden";
  assert.match((await run(p, { cmd: "click", target: "#pay" })).error, /isn't visible \(visibility: hidden\)/);
  pay.style.visibility = "visible";
  pay.box = { x: 10, y: 20, width: 0, height: 0 };
  assert.match((await run(p, { cmd: "click", target: "#pay" })).error, /isn't visible \(no size\)/);
  assert.deepEqual(pay.events, [], "nothing reached it");
});

test("a covered element names its cover, and a click inside it or on its label still counts", async () => {
  const pay = el("button", {}, [el("span", {}, ["Pay"])], { id: "pay" });
  const line = el("p", {}, ["We use cookies"]);
  const consent = el("div", {}, [line], { id: "consent", className: "banner fixed  shadow" });
  const p = page([pay, consent]);
  p.hit = line;
  assert.equal((await run(p, { cmd: "click", target: "#pay" })).error, 'button "Pay" is covered by <div#consent.banner.fixed>');
  assert.deepEqual(pay.events, []);
  p.hit = pay.childNodes[0];
  assert.equal((await run(p, { cmd: "click", target: "#pay" })).out, 'Clicked button "Pay"');
  // pointer-events: none hands the point to what's under it, its parent here.
  p.hit = p.document.body;
  assert.match((await run(p, { cmd: "click", target: "#pay" })).error, /doesn't take the pointer/);
  p.hit = null;
  assert.match((await run(p, { cmd: "click", target: "#pay" })).error, /is outside the viewport/);

  const label = el("label", {}, ["Remember me"]);
  const box = el("input", { type: "checkbox" }, [], { id: "remember", type: "checkbox", checked: false, labels: [label] });
  box.onClick = () => (box.checked = !box.checked);
  const q = page([label, box]);
  q.hit = label;
  assert.equal((await run(q, { cmd: "check", target: "#remember", on: true })).out, 'Checked checkbox "Remember me"');
});

test("an element still moving is no target, and a hover needs no enabled element", async () => {
  const menu = el("button", {}, ["Menu"], { id: "menu", disabled: true });
  let x = 0;
  menu.getBoundingClientRect = () => ({ x: (x += 5), y: 20, width: 100, height: 30 });
  const p = page([menu]);
  p.hit = menu;
  assert.equal((await run(p, { cmd: "hover", target: "#menu" })).error, 'button "Menu" is still moving');
  menu.getBoundingClientRect = () => menu.box;
  assert.equal((await run(p, { cmd: "hover", target: "#menu" })).out, 'Hovered button "Menu"');
});

test("fill takes text fields only, and says so for the rest", async () => {
  const fields = [
    el("input", { type: "checkbox" }, [], { id: "agree", type: "checkbox" }),
    el("input", { type: "radio" }, [], { id: "plan", type: "radio" }),
    el("input", { type: "file" }, [], { id: "cv", type: "file" }),
    el("input", { type: "submit" }, [], { id: "go", type: "submit", value: "Go" }),
    el("input", {}, [], { id: "code", type: "text", value: "A1", readOnly: true }),
    el("select", {}, [], { id: "size" }),
    el("div", {}, ["Notes"], { id: "notes" }),
  ];
  const p = page(fields);
  const error = async (id: string) => (await run(p, { cmd: "fill", target: `#${id}`, text: "x" })).error;
  assert.equal(await error("agree"), "checkbox is a checkbox: use check or uncheck");
  assert.equal(await error("plan"), "radio is a radio: use check or uncheck");
  assert.equal(await error("cv"), "button is a file input: fill can't choose files");
  assert.equal(await error("go"), 'button "Go" is an <input type=submit>: it takes no text');
  assert.equal(await error("code"), "textbox is read-only");
  assert.equal(await error("size"), "combobox is a <select>: use select");
  assert.match(await error("notes"), /isn't a text field/);
});

test("fill checks a date's or a color's shape, and reads every value back", async () => {
  const when = el("input", { type: "date" }, [], { id: "when", type: "date", value: "" });
  const color = el("input", { type: "color" }, [], { id: "tint", type: "color", value: "#000000", accept: (v: string) => v.toLowerCase() });
  const pin = el("input", {}, [], { id: "pin", type: "text", value: "", maxLength: 4, accept: (v: string) => v.slice(0, 4) });
  const pass = el("input", { type: "password" }, [], { id: "pass", type: "password", value: "", accept: (v: string) => v.slice(0, 3) });
  const name = el("textarea", {}, [], { id: "bio", value: "" });
  const p = page([when, color, pin, pass, name]);
  assert.equal(
    (await run(p, { cmd: "fill", target: "#when", text: "12/31/2026" })).error,
    'Malformed value for textbox (a date input): "12/31/2026" isn\'t YYYY-MM-DD',
  );
  assert.equal(when.value, "", "a malformed value never goes in");
  assert.equal((await run(p, { cmd: "fill", target: "#when", text: "2026-12-31" })).out, "Filled textbox");
  assert.equal(when.value, "2026-12-31");
  assert.ok(when.events.includes("input") && when.events.includes("change"));
  assert.equal((await run(p, { cmd: "fill", target: "#tint", text: "#FFAA00" })).out, "Filled button");
  assert.equal((await run(p, { cmd: "fill", target: "#pin", text: "123456" })).error, 'textbox didn\'t take "123456": it holds "1234" (maxlength 4)');
  // A secret is never said back, nor what of it went in.
  const secret = (await run(p, { cmd: "fill", target: "#pass", text: "hunter22", secret: true })).error;
  assert.equal(secret, "textbox didn't take the secret: it holds something else");
  assert.equal((await run(p, { cmd: "fill", target: "#bio", text: "Hello" })).out, "Filled textbox");
  assert.equal(name.value, "Hello");
});

test("check and uncheck read the state back, and a radio can't be unchecked", async () => {
  const agree = el("input", { type: "checkbox" }, [], { id: "agree", type: "checkbox", checked: false });
  agree.onClick = () => (agree.checked = !agree.checked);
  const stuck = el("input", { type: "checkbox" }, [], { id: "stuck", type: "checkbox", checked: false });
  const plan = el("input", { type: "radio" }, [], { id: "plan", type: "radio", checked: true });
  const toggle = el("div", { role: "switch", "aria-checked": "false" }, ["Dark mode"], { id: "dark" });
  toggle.onClick = () => toggle.setAttribute("aria-checked", "true");
  const p = page([agree, stuck, plan, toggle]);
  p.hit = agree;
  assert.equal((await run(p, { cmd: "check", target: "#agree", on: true })).out, "Checked checkbox");
  assert.equal(agree.checked, true);
  assert.equal((await run(p, { cmd: "check", target: "#agree", on: true })).out, "checkbox was checked already");
  assert.equal((await run(p, { cmd: "check", target: "#agree", on: false })).out, "Unchecked checkbox");
  p.hit = stuck;
  assert.equal((await run(p, { cmd: "check", target: "#stuck", on: true })).error, "Clicked checkbox, but it's still unchecked");
  assert.match((await run(p, { cmd: "check", target: "#plan", on: false })).error, /radio button: check another of its group/);
  p.hit = toggle;
  assert.equal((await run(p, { cmd: "check", target: "#dark", on: true })).out, 'Checked switch "Dark mode"');
  p.hit = null;
  const text = el("input", {}, [], { id: "q", type: "text" });
  assert.match((await run(page([text]), { cmd: "check", target: "#q", on: true })).error, /isn't a checkbox, radio or switch/);
});

test("a styled checkbox whose input hides is checked through its label", async () => {
  const label = el("label", {}, ["Subscribe"]);
  const input = el("input", { type: "checkbox" }, [], { id: "sub", type: "checkbox", checked: false, labels: [label], style: { display: "none" } });
  label.onClick = () => (input.checked = !input.checked);
  const p = page([label, input]);
  p.hit = label;
  assert.equal((await run(p, { cmd: "check", target: "#sub", on: true })).out, 'Checked checkbox "Subscribe"');
  assert.ok(label.events.includes("click"));
});

test("select reads the choice back, and refuses a disabled option", async () => {
  const option = (value: string, text: string, own: Record<string, unknown> = {}) => el("option", { value }, [text], { value, selected: false, ...own });
  const options = [option("s", "Small"), option("m", "Medium"), option("l", "Large", { disabled: true })];
  const size = el("select", {}, options, { id: "size", options, multiple: false });
  const p = page([size]);
  assert.equal((await run(p, { cmd: "select", target: "#size", values: ["Medium"] })).out, 'Selected "Medium" in combobox');
  assert.equal((await run(p, { cmd: "select", target: "#size", values: ["l"] })).error, 'Option "Large" in combobox is disabled');
  // The page's change handler puts it back.
  size.on = { change: () => options.forEach((o, i) => (o.selected = i === 0)) };
  assert.equal((await run(p, { cmd: "select", target: "#size", values: ["m"] })).error, 'combobox didn\'t keep the choice: it shows "Small"');
});

test("an action's changes are counted from just before it until the page holds still", async () => {
  const dialog = el("div", { role: "dialog" }, ["Joined"], { style: { display: "none" } });
  const join = el("button", {}, ["Join"], { id: "join" });
  const p = page([join, dialog]);
  p.hit = join;
  join.onClick = () => {
    dialog.style.display = "block";
    Observer.fire([{ type: "childList", addedNodes: [{ nodeType: 1 }, { nodeType: 1 }, { nodeType: 3 }], removedNodes: [{ nodeType: 1 }] }, { type: "attributes" }]);
    // One more lands after the click returns, before the page goes still.
    Observer.fire([{ type: "childList", addedNodes: [{ nodeType: 1 }] }], true);
    p.location.href = "http://localhost:3000/lobby";
  };
  assert.equal((await run(p, { cmd: "click", target: "#join", watch: true })).out, 'Clicked button "Join"');
  const seen = await run(p, { cmd: "changes", quiet: 30, most: 200 });
  assert.deepEqual(
    { added: seen.added, removed: seen.removed, texts: seen.texts, attributes: seen.attributes, dialogs: seen.dialogs },
    { added: 3, removed: 1, texts: 1, attributes: 1, dialogs: 1 },
  );
  assert.match(seen.state, /^http:\/\/localhost:3000\/lobby\|/);
  assert.equal(Observer.live, null, "the watch ends as it's read");
  // Read again, or on a page that never watched (another document): none of this page's.
  assert.equal((await run(p, { cmd: "changes" })).gone, true);
});

test("a page that never holds still is read at the most, and a failed action leaves no watch", async () => {
  const busy = el("button", {}, ["Spin"], { id: "spin" });
  const p = page([busy]);
  p.hit = busy;
  await run(p, { cmd: "click", target: "#spin", watch: true });
  const timer = setInterval(() => Observer.fire([{ type: "attributes" }]), 5);
  const started = Date.now();
  const seen = await run(p, { cmd: "changes", quiet: 50, most: 120 });
  clearInterval(timer);
  assert.ok(Date.now() - started < 400, "capped");
  assert.ok(seen.attributes > 0);
  busy.disabled = true;
  assert.match((await run(p, { cmd: "click", target: "#spin", watch: true })).error, /is disabled/);
  assert.equal(Observer.live, null);
  // A still page's state is the same each time: what the loop nudge compares.
  busy.disabled = false;
  await run(p, { cmd: "click", target: "#spin", watch: true });
  const a = (await run(p, { cmd: "changes", quiet: 0, most: 0 })).state;
  await run(p, { cmd: "click", target: "#spin", watch: true });
  assert.equal((await run(p, { cmd: "changes", quiet: 0, most: 0 })).state, a);
});
