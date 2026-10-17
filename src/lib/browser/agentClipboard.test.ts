import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { browser } from "./agentSource.test.ts";

// scripts/clipboard.js on stand-in pages: an agent's tab keeps its page's clipboard calls to one
// store, GitViber's, and lets them through once the tab is the user's; frames included.
const KEY = "KEY-5f3a9c-do-not-leak";
const ORIGIN = "https://a.test";
const script = readFileSync(new URL("scripts/clipboard.js", browser), "utf8").replace("__GV_KEY__", JSON.stringify(KEY));
const names = ["window", "document", "Document", "navigator", "Navigator", "addEventListener", "removeEventListener", "DataTransfer", "ClipboardEvent", "ClipboardItem", "Blob", "setTimeout", "console"];

type Listener = (e: FakeEvent) => void;

class FakeEvent {
  type: string;
  defaultPrevented = false;
  stopped = false;
  isTrusted = false;
  clipboardData?: FakeData;
  constructor(type: string, init: Record<string, unknown> = {}) {
    this.type = type;
    Object.assign(this, init);
  }
  preventDefault() {
    this.defaultPrevented = true;
  }
  stopImmediatePropagation() {
    this.stopped = true;
  }
}

class FakeData {
  data: Record<string, string> = {};
  setData(type: string, value: string) {
    this.data[type] = value;
  }
  getData(type: string) {
    return this.data[type] ?? "";
  }
}

/** What GitViber keeps for the tab, and what the user's own clipboard holds. */
interface Host {
  /** The copy of ORIGIN, and of each origin. */
  copied: string;
  copies: Record<string, string>;
  /** What the store refuses a copy with ("gv:size"), and a handler that's gone or never answers. */
  refuse?: string;
  gone?: boolean;
  silent?: boolean;
  guarded: boolean;
  /** Each answer waits for this when it's set. */
  hold?: Promise<void>;
  user: string;
  asked: string[];
}

class Element {
  localName: string;
  value = "";
  selectionStart: number | null = null;
  selectionEnd: number | null = null;
  type = "";
  contentDocument: Doc | null = null;
  listeners: Record<string, Listener[]> = {};
  focused = 0;
  removed = false;
  constructor(localName: string, value = "") {
    this.localName = localName;
    this.value = value;
  }
  addEventListener(type: string, fn: Listener) {
    (this.listeners[type] ??= []).push(fn);
  }
  dispatchEvent(e: FakeEvent) {
    for (const fn of this.listeners[e.type] ?? []) fn(e);
    return !e.defaultPrevented;
  }
  select() {
    this.selectionStart = 0;
    this.selectionEnd = this.value.length;
    doc().selected = this.value;
    doc().activeElement = this;
  }
  append(_child: Element) {}
  remove() {
    this.removed = true;
  }
  focus() {
    this.focused++;
    doc().activeElement = this;
  }
  get style() {
    return {} as Record<string, string>;
  }
  set style(_: unknown) {}
}

let current: Doc;
const doc = () => current;

class Doc {
  static real: Host;
  static refuse = false;
  activeElement: Element | null = null;
  body = new Element("body");
  selected = "";
  deleted = 0;
  listeners: Record<string, Listener[]> = {};
  window!: Win;
  getSelection() {
    const doc = this;
    return { toString: () => doc.selected, rangeCount: 0, removeAllRanges() {}, addRange() {} };
  }
  createElement(name: string) {
    return new Element(name);
  }
  get documentElement() {
    return this.body;
  }
  // WebKit's own: a copy goes to the user's clipboard, as the copy event lets the page fill it.
  execCommand(command: string) {
    if (command === "delete") return void this.deleted++, true;
    if (Doc.refuse) return false;
    const e = new FakeEvent("copy", { clipboardData: new FakeData() });
    for (const fn of this.window.listeners.copy ?? []) if (!e.stopped) fn(e);
    for (const fn of this.listeners.copy ?? []) if (!e.stopped) fn(e);
    Doc.real.user = e.clipboardData?.getData("text/plain") || this.selected;
    return true;
  }
}

interface Win {
  top: Win;
  length: number;
  webkit?: unknown;
  listeners: Record<string, Listener[]>;
  [key: string]: unknown;
}

interface Tab {
  win: Win;
  doc: Doc;
  host: Host;
  ask: (what: string) => unknown;
  press: (type: string, trusted?: boolean) => void;
  run: () => void;
}

/** A page running the script: `host` is shared between a tab's frames. */
function page(host: Host, options: { top?: Win; native?: boolean; origin?: string; handler?: boolean } = {}): Tab {
  // Its own class: the script wraps the one it's given.
  const Page = class extends Doc {};
  const document = new Page();
  Doc.real = host;
  current = document;
  const origin = options.origin ?? ORIGIN;
  const win: Win = { length: 0, listeners: {}, Navigator: class {}, origin } as unknown as Win;
  win.top = options.top ?? win;
  document.window = win;
  if (options.handler !== false) {
    win.webkit = {
      messageHandlers: {
        gvClipboard: {
          postMessage(body: string) {
            const [key, op, from, ...rest] = body.split("\n");
            const text = rest.join("\n");
            assert.equal(key, KEY);
            assert.equal(from, origin);
            host.asked.push(op);
            if (host.gone) throw new TypeError("Message handler gvClipboard was removed");
            if (host.silent) return new Promise(() => {});
            // Answered as things stand now, delivered when `hold` lets it.
            const guard = host.guarded ? "1" : "0";
            return (async () => {
              await host.hold;
              if (op === "set") {
                if (host.refuse) throw new Error(host.refuse);
                host.copies[origin] = text;
              }
              return op === "get" ? (host.copies[origin] ?? "") : op === "guard" ? guard : "";
            })();
          },
        },
      },
    };
  }
  const nav = new (win.Navigator as new () => Record<string, unknown>)();
  if (options.native) {
    win.Clipboard = class {
      async writeText(t: string) {
        host.user = t;
      }
      async readText() {
        return host.user;
      }
    };
  }
  const add = (type: string, fn: Listener) => (win.listeners[type] ??= []).push(fn);
  const remove = (type: string, fn: Listener) => (win.listeners[type] = (win.listeners[type] ?? []).filter((f) => f !== fn));
  if (options.native) Object.defineProperty(nav, "clipboard", { value: new (win.Clipboard as new () => object)() });
  // A slow handler is slow for 20 ms here, not two seconds.
  const quick = (fn: () => void, ms: number) => setTimeout(fn, Math.min(ms, 20));
  const env = [win, document, Page, nav, win.Navigator, add, remove, FakeData, FakeEvent, class {}, class {}, quick, console];
  new Function(...names, script)(...env);
  const tab: Tab = {
    win,
    doc: document,
    host,
    ask: (what) => {
      current = document;
      Doc.real = host;
      return (win.__gvClipboard as (k: string, w: string) => unknown)(KEY, what);
    },
    press: (type, trusted = true) => {
      for (const fn of win.listeners[type] ?? []) fn(new FakeEvent(type, { isTrusted: trusted }));
    },
    run: () => {
      current = document;
      Doc.real = host;
    },
  };
  Object.defineProperty(tab, "clipboard", { get: () => (nav as { clipboard?: unknown }).clipboard });
  return tab;
}

const fresh = (): Host => {
  const host: Host = {
    copies: {},
    guarded: true,
    user: "the user's own text",
    asked: [],
    get copied() {
      return this.copies[ORIGIN] ?? "";
    },
    set copied(text: string) {
      this.copies[ORIGIN] = text;
    },
  };
  return host;
};
const clip = (t: Tab) => (t as unknown as { clipboard: any }).clipboard;
const tick = () => new Promise((r) => setTimeout(r, 5));
const failure = async (p: Promise<unknown>) => p.then(() => "resolved", (e) => (e as Error).name);

test("a guarded tab's navigator.clipboard is its store: written, read back, and never the user's", async () => {
  const host = fresh();
  const t = page(host);
  await clip(t).writeText("from the page");
  assert.equal(host.copied, "from the page");
  assert.equal(await clip(t).readText(), "from the page");
  assert.equal(host.user, "the user's own text");
  assert.equal(Object.prototype.toString.call(clip(t)), "[object Clipboard]");
});

test("once the tab is the user's the page's writeText is a real copy, whatever its copy listeners do", async () => {
  const host = fresh();
  const t = page(host);
  let pageSaw = 0;
  t.doc.listeners.copy = [() => void pageSaw++];
  t.ask("off");
  const field = new Element("input");
  t.doc.activeElement = field;
  await clip(t).writeText("Copied!");
  assert.equal(host.user, "Copied!");
  assert.equal(host.copied, "");
  assert.equal(pageSaw, 0, "the page's own copy listener didn't change it");
  assert.equal(field.focused, 1, "the page's focus is back where it was");
  assert.equal(await failure(clip(t).readText()), "NotAllowedError");
});

test("a real copy the engine refuses is a NotAllowedError, as the API's", async () => {
  const host = fresh();
  const t = page(host);
  t.ask("off");
  Doc.refuse = true;
  try {
    assert.equal(await failure(clip(t).writeText("x")), "NotAllowedError");
  } finally {
    Doc.refuse = false;
  }
});

test("execCommand's copy and cut go to the store while guarded and to the user's clipboard after", () => {
  const host = fresh();
  const t = page(host);
  const area = new Element("textarea", "hello world");
  area.selectionStart = 0;
  area.selectionEnd = 5;
  t.doc.activeElement = area;
  assert.equal(t.doc.execCommand("cut"), true);
  assert.equal(t.doc.deleted, 1);
  assert.equal(host.user, "the user's own text");
  t.ask("off");
  Doc.real = host;
  t.doc.selected = "chosen";
  t.doc.execCommand("copy");
  assert.equal(host.user, "chosen");
  return tick().then(() => assert.equal(host.copied, "hello"));
});

test("a page that fills the copy event gets its text kept, and one that cancels a cut keeps its text", async () => {
  const host = fresh();
  const t = page(host);
  const area = new Element("textarea", "abc");
  area.selectionStart = 0;
  area.selectionEnd = 3;
  area.addEventListener("cut", (e) => {
    e.preventDefault();
    e.clipboardData?.setData("text/plain", "custom");
  });
  t.doc.activeElement = area;
  t.doc.execCommand("cut");
  await tick();
  assert.equal(host.copied, "custom");
  assert.equal(t.doc.deleted, 0);
});

test("a password field's copy keeps nothing of it", async () => {
  const host = fresh();
  const t = page(host);
  host.copied = "earlier";
  const pw = new Element("input", "hunter2");
  pw.type = "password";
  pw.selectionStart = 0;
  pw.selectionEnd = 7;
  t.doc.activeElement = pw;
  t.doc.execCommand("copy");
  await tick();
  assert.equal(host.copied, "");
});

test("an agent's paste: the store's text in a paste event, put in unless the page took it", async () => {
  const host = fresh();
  const t = page(host);
  host.copied = "stored";
  const field = new Element("input");
  const seen: string[] = [];
  field.addEventListener("paste", (e) => void seen.push(e.clipboardData?.getData("text/plain") ?? ""));
  t.doc.activeElement = field;
  assert.equal(await (t.ask("paste") as Promise<string>), "stored");
  assert.deepEqual(seen, ["stored"]);
  field.addEventListener("paste", (e) => e.preventDefault());
  assert.equal(await (t.ask("paste") as Promise<string>), "");
  assert.equal(t.doc.execCommand("paste"), false);
});

test("an agent's copy inside a frame of the page's origin reads that frame; in another origin's it does nothing at all", async () => {
  const host = fresh();
  const t = page(host);
  host.copied = "before";
  const inner = new Doc();
  const frame = new Element("iframe");
  frame.contentDocument = inner;
  inner.selected = "inside the frame";
  inner.activeElement = new Element("div");
  t.doc.activeElement = frame;
  await (t.ask("copy") as Promise<unknown>);
  assert.equal(host.copied, "inside the frame");
  // Another origin's frame is out of reach: nothing is copied from it, and its page's own
  // origin has the store it never gets from here, so the paste puts nothing in either.
  frame.contentDocument = null;
  inner.selected = "unreachable";
  assert.equal(t.ask("copy"), null);
  assert.equal(host.copied, "inside the frame");
  assert.equal(await (t.ask("paste") as Promise<string>), "");
});

test("a copy is the origin's that made it: a frame of another origin neither reads nor replaces it", async () => {
  const host = fresh();
  const top = page(host);
  const other = page(host, { top: top.win, origin: "https://evil.test" });
  await clip(top).writeText("site a's secret");
  other.run();
  assert.equal(await clip(other).readText(), "", "it reads its own, which is empty");
  await clip(other).writeText("planted");
  assert.equal(host.copies[ORIGIN], "site a's secret");
  assert.equal(host.copies["https://evil.test"], "planted");
  top.run();
  assert.equal(await clip(top).readText(), "site a's secret");
});

test("a copy the store refuses is an error the page and the agent see, not a success", async () => {
  const host = fresh();
  const t = page(host);
  const seen: string[] = [];
  const log = console.error;
  console.error = (m: string) => void seen.push(m);
  try {
    host.refuse = "gv:size";
    assert.equal(await failure(clip(t).writeText("x".repeat(10))), "QuotaExceededError");
    host.refuse = "gv:origin";
    assert.equal(await failure(clip(t).writeText("x")), "NotAllowedError");
    // execCommand can't say so to the page, but the console the agent reads does.
    t.doc.selected = "y";
    assert.equal(t.doc.execCommand("copy"), true);
    await tick();
  } finally {
    console.error = log;
  }
  assert.equal(seen.length, 3);
  assert.match(seen[0], /nothing was copied: The tab's clipboard holds 4 MiB at most/);
});

test("a frame's first press asks whether it is the user's, and only the latest answer counts", async () => {
  const host = fresh();
  const top = page(host).win;
  const frame = page(host, { top });
  host.guarded = false;
  frame.press("pointerdown");
  await clip(frame).writeText("held until answered");
  // The answer was "the user's", and the copy waited for it: a real one.
  assert.equal(host.user, "held until answered");
  frame.run();
  frame.doc.selected = "frame copy";
  frame.doc.execCommand("copy");
  assert.equal(host.user, "frame copy", "the user's own press in it releases it");

  // The agent clicks right after: its answer is the guarded one, and the user's late one is stale.
  let release!: () => void;
  host.hold = new Promise((r) => (release = r));
  host.guarded = false;
  frame.press("keydown");
  host.guarded = true;
  frame.press("pointerdown");
  release();
  host.hold = undefined;
  await tick();
  frame.run();
  frame.doc.selected = "agent copy";
  frame.doc.execCommand("copy");
  assert.notEqual(host.user, "agent copy");
  frame.press("keydown", false);
  assert.equal(host.asked.filter((a) => a === "guard").length, 3, "an untrusted event asks nothing");
});

test("until the answer is in a frame's guard is on: the user's Enter lands in the store, never an agent's click in the clipboard", async () => {
  const host = fresh();
  const top = page(host).win;
  const frame = page(host, { top });
  host.hold = new Promise(() => {});
  frame.press("keydown");
  frame.run();
  frame.doc.selected = "pressed";
  frame.doc.execCommand("copy");
  assert.equal(host.user, "the user's own text", "a copy that can't wait is kept, not made");
  assert.equal(host.copies[ORIGIN], undefined, "(the store is asked to hold it)");
});

test("a tab GitViber no longer answers for is the user's, and one that is only slow is asked again", async () => {
  const host = fresh();
  const top = page(host).win;
  const frame = page(host, { top });
  host.gone = true;
  frame.press("pointerdown");
  await tick();
  frame.run();
  frame.doc.selected = "after the pane closed";
  frame.doc.execCommand("copy");
  assert.equal(host.user, "after the pane closed", "its page's Copy is theirs");

  const slow = fresh();
  slow.silent = true;
  const waiting = page(slow, { top: page(slow).win });
  waiting.press("pointerdown");
  await new Promise((r) => setTimeout(r, 60));
  waiting.run();
  waiting.doc.selected = "slow";
  waiting.doc.execCommand("copy");
  assert.equal(slow.user, "the user's own text", "slow isn't gone: still guarded");

  // No handler at all: no agent to keep the guard for.
  const bare = fresh();
  const free = page(bare, { handler: false });
  free.doc.selected = "free";
  free.doc.execCommand("copy");
  assert.equal(bare.user, "free");
});

test("the main frame's switch makes its frames ask, and gives them no key", async () => {
  const host = fresh();
  const main = page(host);
  const child = page(host, { top: main.win });
  // A frame the page made itself, with a function of the same name to catch what it is called with.
  const caught: unknown[][] = [];
  const decoy = { __gvClipboard: (...args: unknown[]) => void caught.push(args) };
  main.win.length = 2;
  Object.assign(main.win, { 0: { __gvClipboard: child.win.__gvClipboard }, 1: decoy });
  host.guarded = false;
  main.ask("off");
  await tick();
  child.run();
  child.doc.selected = "child";
  child.doc.execCommand("copy");
  assert.equal(host.user, "child");
  host.guarded = true;
  main.ask("on");
  await tick();
  child.run();
  child.doc.selected = "guarded again";
  child.doc.execCommand("copy");
  assert.equal(host.user, "child");
  assert.ok(caught.length >= 2);
  assert.ok(!JSON.stringify(caught).includes(KEY), "the decoy never saw the key");
});

test("a page that hooks everything the script could use gets neither the key nor the guard", async () => {
  const host = fresh();
  const seen: string[] = [];
  const spy = (v: unknown) => void seen.push(typeof v === "string" ? v : String(Object.prototype.toString.call(v)));
  const t = page(host);
  const frame = page(host, { top: t.win });
  // What a page does as its first script: swap what a script might call with a key.
  const real = { stringify: JSON.stringify, define: Object.defineProperty, bind: Function.prototype.bind, call: Function.prototype.call, apply: Function.prototype.apply, then: Promise.prototype.then, reflect: Reflect.apply };
  const handler = (t.win.webkit as { messageHandlers: { gvClipboard: { postMessage: unknown } } }).messageHandlers.gvClipboard;
  const post = handler.postMessage;
  JSON.stringify = ((v: unknown, ...r: unknown[]) => (spy(v), real.stringify(v, ...(r as [])))) as typeof JSON.stringify;
  Object.defineProperty = ((o: object, k: PropertyKey, d: PropertyDescriptor) => (spy(k), real.define(o, k, d))) as typeof Object.defineProperty;
  Function.prototype.bind = function (this: Function, ...a: unknown[]) {
    a.forEach(spy);
    return real.bind.apply(this, a as [unknown]);
  } as typeof Function.prototype.bind;
  Reflect.apply = ((f: Function, th: unknown, a: unknown[]) => (a.forEach(spy), real.reflect(f, th, a))) as typeof Reflect.apply;
  handler.postMessage = (body: string) => (spy(body), (post as (b: string) => unknown)(body));
  Promise.prototype.then = function (this: Promise<unknown>, a?: unknown, b?: unknown) {
    spy(a);
    return real.then.call(this, a as never, b as never);
  } as typeof Promise.prototype.then;
  try {
    // Guessing: every call a page can make, with the wrong key.
    for (const what of ["off", "on", "copy", "paste", "ask", "guard"]) (t.win.__gvClipboard as (k: unknown, w: string) => unknown)("guess", what);
    for (const what of ["off", "ask"]) (frame.win.__gvClipboard as (k: unknown, w: string) => unknown)(undefined, what);
    // What the script does on its own meanwhile.
    await clip(t).writeText("kept from a hostile page");
    await clip(t).readText();
    t.doc.selected = "x";
    t.doc.execCommand("copy");
    frame.press("pointerdown");
    await tick();
    assert.equal(host.user, "the user's own text");
    assert.equal(host.copied, "x");
  } finally {
    JSON.stringify = real.stringify;
    Object.defineProperty = real.define;
    Function.prototype.bind = real.bind;
    Reflect.apply = real.reflect;
    Promise.prototype.then = real.then;
  }
  assert.ok(seen.length > 0, "the hooks ran");
  assert.ok(!seen.some((s) => s.includes(KEY)), "no hook saw the key");
  assert.ok(!String(t.win.__gvClipboard).includes(KEY) && !String(Function.prototype.toString.call(t.win.__gvClipboard)).includes(KEY));
  assert.ok(!JSON.stringify(Object.getOwnPropertyNames(t.win).map((k) => String(t.win[k]))).includes(KEY));
});

test("where WebKit has the async API, its calls are the store's while guarded and WebKit's after", async () => {
  const host = fresh();
  const t = page(host, { native: true });
  await clip(t).writeText("kept");
  assert.equal(host.copied, "kept");
  assert.equal(host.user, "the user's own text");
  t.ask("off");
  await clip(t).writeText("real");
  assert.equal(host.user, "real");
  assert.equal(await clip(t).readText(), "real");
});
