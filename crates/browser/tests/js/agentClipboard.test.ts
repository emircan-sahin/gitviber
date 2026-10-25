import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { browser } from "./agentSource.ts";

// scripts/clipboard.js on stand-in pages: an agent's tab keeps its page's clipboard calls to one
// store, GitViber's, and lets them through once the tab is the user's; frames included.
const KEY = "KEY-5f3a9c-do-not-leak";
const ORIGIN = "https://a.test";
const script = readFileSync(new URL("scripts/clipboard.js", browser), "utf8").replace("__GV_KEY__", JSON.stringify(KEY));
const names = ["window", "document", "Document", "navigator", "Navigator", "addEventListener", "removeEventListener", "DataTransfer", "ClipboardEvent", "ClipboardItem", "Blob", "setTimeout", "console"];
const WORDS = ["on", "off"];

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
  /** The frames that said hello, as GitViber keeps them: where it pushes the guard. */
  frames: { win: Win; token: string }[];
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
  // It makes the command a string itself, as WebKit does: an object is asked again.
  execCommand(command: string) {
    const name = String(command);
    if (name === "delete") return void this.deleted++, true;
    if (name !== "copy" && name !== "cut") return true;
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
  token: string;
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
            if (op === "hello") {
              // GitViber keeps the frame, and tells it at once if the tab is the user's.
              host.frames.push({ win, token: text });
              if (!host.guarded) queueMicrotask(() => (win.__gvClipboard as (t: string, w: string) => unknown)(text, "off"));
            }
            return (async () => {
              await host.hold;
              if (op === "set") {
                if (host.refuse) throw new Error(host.refuse);
                host.copies[origin] = text;
              }
              return op === "get" ? (host.copies[origin] ?? "") : "";
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
    token: host.frames.find((f) => f.win === win)?.token ?? "",
    ask: (what) => {
      current = document;
      Doc.real = host;
      return (win.__gvClipboard as (k: string, w: string) => unknown)(tab.token, what);
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

/** GitViber's push of the guard to every frame that said hello (guard_clipboard). */
function push(host: Host, on: boolean) {
  host.guarded = on;
  for (const f of host.frames) (f.win.__gvClipboard as (t: string, w: string) => unknown)(f.token, on ? "on" : "off");
}

const fresh = (): Host => {
  const host: Host = {
    copies: {},
    guarded: true,
    user: "the user's own text",
    asked: [],
    frames: [],
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
  push(host, false);
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
  push(host, false);
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
  push(host, false);
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

test("a frame is let go by GitViber's push, to every frame that said hello and to one that says it later", async () => {
  const host = fresh();
  const top = page(host);
  const frame = page(host, { top: top.win, origin: "https://other.test" });
  frame.run();
  frame.doc.selected = "guarded at first";
  frame.doc.execCommand("copy");
  assert.equal(host.user, "the user's own text", "it begins guarded");
  push(host, false);
  frame.run();
  frame.doc.selected = "the user's press";
  frame.doc.execCommand("copy");
  assert.equal(host.user, "the user's press");
  // A frame made after the tab became the user's is told when it says hello.
  const later = page(host, { top: top.win, origin: "https://late.test" });
  await tick();
  later.run();
  later.doc.selected = "late";
  later.doc.execCommand("copy");
  assert.equal(host.user, "late");
  // And the agent's next command guards them all again.
  push(host, true);
  for (const f of [frame, later]) {
    f.run();
    f.doc.selected = "agent";
    f.doc.execCommand("copy");
  }
  assert.equal(host.user, "late");
});

test("a frame's own press or key does nothing to the guard: only GitViber's push does", async () => {
  const host = fresh();
  const top = page(host);
  const frame = page(host, { top: top.win });
  for (const type of ["pointerdown", "keydown", "mousedown", "click"]) frame.press(type);
  await tick();
  frame.run();
  frame.doc.selected = "agent's click";
  frame.doc.execCommand("copy");
  assert.equal(host.user, "the user's own text");
  assert.deepEqual(host.asked.filter((a) => a !== "hello" && a !== "set"), [], "it asks nothing of GitViber");
});

test("a tab GitViber no longer answers for keeps the guard it was pushed, and no handler at all is the user's", async () => {
  const host = fresh();
  const top = page(host).win;
  const frame = page(host, { top });
  host.gone = true;
  assert.equal(await failure(clip(frame).writeText("x")), "NotAllowedError", "its calls fail, they don't reach the clipboard");
  assert.equal(host.user, "the user's own text");

  // No handler at all: no agent to keep the guard for.
  const bare = fresh();
  const free = page(bare, { handler: false });
  free.doc.selected = "free";
  free.doc.execCommand("copy");
  assert.equal(bare.user, "free");
});

test("only a frame's own token moves its guard, and a frame born of the page's making is never given one", async () => {
  const host = fresh();
  const main = page(host);
  const child = page(host, { top: main.win });
  const caught: unknown[][] = [];
  const decoy = { __gvClipboard: (...args: unknown[]) => void caught.push(args) };
  Object.assign(main.win, { length: 2, 0: child.win, 1: decoy });
  const call = (w: Win, token: unknown, what: string) => (w.__gvClipboard as (t: unknown, w: string) => unknown)(token, what);
  for (const guess of [undefined, null, "", KEY, "off", main.token, 0]) for (const what of ["off", "on", "copy", "paste", "ask"]) call(child.win, guess, what);
  child.run();
  child.doc.selected = "forged";
  child.doc.execCommand("copy");
  assert.equal(host.user, "the user's own text");
  push(host, false);
  child.run();
  child.doc.selected = "pushed";
  child.doc.execCommand("copy");
  assert.equal(host.user, "pushed");
  assert.deepEqual(caught, [], "nothing is ever called with a token or the key but by GitViber");
});

/** Replaces what a hostile page can, running `body`; puts everything back. */
async function hostile(body: (forged: string[]) => Promise<void>) {
  const forged: string[] = [];
  const saved = {
    then: Promise.prototype.then,
    stringify: JSON.stringify,
    parse: JSON.parse,
    call: Function.prototype.call,
    apply: Function.prototype.apply,
    bind: Function.prototype.bind,
    reflect: Reflect.apply,
    from: Array.from,
    map: Array.prototype.map,
    some: Array.prototype.some,
    includes: Array.prototype.includes,
    join: Array.prototype.join,
    push: Array.prototype.push,
    define: Object.defineProperty,
  };
  // Every reply it can forge: the callbacks it is handed are called with a "0", a "1", the key, an
  // empty text, and an error, in turn.
  Promise.prototype.then = function (this: Promise<unknown>, a?: unknown, b?: unknown) {
    if (typeof a === "function") {
      const first = a as (v: unknown) => unknown;
      return saved.then.call(this, (v: unknown) => {
        for (const lie of ["0", "off", false, null, "", KEY]) {
          try {
            forged.push(String(lie));
            first(lie);
          } catch {}
        }
        return first(v);
      }, b as never);
    }
    return saved.then.call(this, a as never, b as never);
  } as typeof Promise.prototype.then;
  JSON.stringify = (() => "0") as typeof JSON.stringify;
  JSON.parse = (() => ({ guarded: false })) as typeof JSON.parse;
  Array.from = (() => []) as unknown as typeof Array.from;
  Array.prototype.map = function () { return []; } as typeof Array.prototype.map;
  Array.prototype.some = function () { return true; } as typeof Array.prototype.some;
  Array.prototype.includes = function () { return true; } as typeof Array.prototype.includes;
  Array.prototype.join = function () { return "0"; } as typeof Array.prototype.join;
  try {
    await body(forged);
  } finally {
    Promise.prototype.then = saved.then;
    JSON.stringify = saved.stringify;
    JSON.parse = saved.parse;
    Function.prototype.call = saved.call;
    Function.prototype.apply = saved.apply;
    Function.prototype.bind = saved.bind;
    Reflect.apply = saved.reflect;
    Array.from = saved.from;
    Array.prototype.map = saved.map;
    Array.prototype.some = saved.some;
    Array.prototype.includes = saved.includes;
    Array.prototype.join = saved.join;
    Array.prototype.push = saved.push;
    Object.defineProperty = saved.define;
  }
}

test("a hostile frame that replaces then, Array's methods, JSON and window.length, forging every reply, never turns its guard off", async () => {
  const host = fresh();
  const top = page(host);
  let frame!: Tab;
  await hostile(async (forged) => {
    // After the shim ran, and the page's first script before it, as another frame's would.
    frame = page(host, { top: top.win });
    Object.defineProperty(frame.win, "length", { get: () => 999 });
    for (const type of ["pointerdown", "keydown"]) frame.press(type);
    await clip(frame).writeText("a hostile frame's text").catch(() => {});
    await clip(frame).readText().catch(() => {});
    frame.run();
    frame.doc.selected = "PAGE-WROTE-THIS";
    frame.doc.execCommand("copy");
    await tick();
    assert.ok(forged.length > 0, "its hooks ran");
  });
  assert.equal(host.user, "the user's own text", "nothing reached the user's clipboard");
  // And the frame is still guarded, now the hooks are gone: it takes GitViber to let it go.
  frame.run();
  frame.doc.selected = "after";
  frame.doc.execCommand("copy");
  assert.equal(host.user, "the user's own text");
  push(host, false);
  frame.run();
  frame.doc.execCommand("copy");
  assert.equal(host.user, "after");
});

test("a frame whose page replaced then before the shim ran is still guarded", async () => {
  const host = fresh();
  const top = page(host);
  const real = Promise.prototype.then;
  // The reply a poc forged: any callback registered is called with a "0".
  Promise.prototype.then = function (this: Promise<unknown>, a?: unknown, b?: unknown) {
    if (typeof a === "function") return real.call(this, () => (a as (v: string) => unknown)("0"), b as never);
    return real.call(this, a as never, b as never);
  } as typeof Promise.prototype.then;
  let frame!: Tab;
  try {
    frame = page(host, { top: top.win });
    frame.win.__gvClipboard && (frame.win.__gvClipboard as (t: unknown, w: string) => unknown)(null, "ask");
    await new Promise((r) => setTimeout(r, 20));
  } finally {
    Promise.prototype.then = real;
  }
  frame.run();
  frame.doc.selected = "PAGE-WROTE-THIS";
  frame.doc.execCommand("copy");
  assert.equal(host.user, "the user's own text");
});

test("a page that hooks everything the script could use sees neither the key nor a token", async () => {
  const host = fresh();
  const seen: string[] = [];
  const spy = (v: unknown) => void seen.push(typeof v === "string" ? v : String(Object.prototype.toString.call(v)));
  const t = page(host);
  const frame = page(host, { top: t.win });
  const real = { stringify: JSON.stringify, define: Object.defineProperty, bind: Function.prototype.bind, then: Promise.prototype.then, reflect: Reflect.apply };
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
    JSON.stringify("the hook is in");
    for (const what of ["off", "on", "copy", "paste", "ask", "guard"]) (t.win.__gvClipboard as (k: unknown, w: string) => unknown)("guess", what);
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
  assert.ok(seen.includes("the hook is in"), "the hooks ran");
  const secrets = [KEY, t.token, frame.token];
  assert.ok(secrets.every((x) => x.length > 8));
  assert.ok(!seen.some((x) => secrets.some((secret) => x.includes(secret))), "no hook saw the key or a token");
  assert.ok(!String(t.win.__gvClipboard).includes(KEY) && !String(t.win.__gvClipboard).includes(t.token));
});

test("a copy longer than the cap is refused before it is sent", async () => {
  const host = fresh();
  const t = page(host);
  const sent = host.asked.length;
  assert.equal(await failure(clip(t).writeText("x".repeat((4 << 20) + 1))), "QuotaExceededError");
  assert.equal(host.asked.length, sent, "nothing went to GitViber");
});

test("where WebKit has the async API, its calls are the store's while guarded and WebKit's after", async () => {
  const host = fresh();
  const t = page(host, { native: true });
  await clip(t).writeText("kept");
  assert.equal(host.copied, "kept");
  assert.equal(host.user, "the user's own text");
  push(host, false);
  await clip(t).writeText("real");
  assert.equal(host.user, "real");
  assert.equal(await clip(t).readText(), "real");
});

// What a page can do to the shim's own call of WebKit's execCommand, each a way of making it copy for
// real inside an agent's click while the tab is guarded.
test("a command that is an object answering twice is classified and sent once", () => {
  const host = fresh();
  const t = page(host);
  t.doc.selected = "ATTACKER TEXT";
  Doc.real = host;
  let n = 0;
  t.doc.execCommand({ toString: () => (n++ === 0 ? "bold" : "copy") } as unknown as string);
  assert.equal(host.user, "the user's own text");
  assert.equal(n, 1, "it was asked once");
});

test("a page that hooks Function.prototype.call, apply or Reflect.apply is not handed WebKit's execCommand", () => {
  const host = fresh();
  const t = page(host);
  t.doc.selected = "ATTACKER TEXT 2";
  Doc.real = host;
  const orig = { call: Function.prototype.call, apply: Function.prototype.apply, reflect: Reflect.apply };
  let leaked: Function | undefined;
  const take = (self: unknown) => {
    if (typeof self === "function" && self.name === "execCommand") leaked = self;
  };
  (Function.prototype as any).call = function (this: any, ...a: unknown[]) {
    take(this);
    return orig.call.apply(this, a as never);
  };
  (Function.prototype as any).apply = function (this: any, ...a: unknown[]) {
    take(this);
    return orig.apply.apply(this, a as never);
  };
  try {
    t.doc.execCommand("bold");
    t.doc.execCommand("copy");
  } finally {
    Function.prototype.call = orig.call;
    Function.prototype.apply = orig.apply;
  }
  assert.equal(leaked, undefined, "nothing of WebKit's was handed to the hook");
  assert.equal(host.user, "the user's own text");
});

test("a page that swaps String.prototype.toLowerCase still has its copy kept", async () => {
  const host = fresh();
  const t = page(host);
  t.doc.selected = "ATTACKER TEXT 3";
  Doc.real = host;
  const orig = String.prototype.toLowerCase;
  String.prototype.toLowerCase = function () {
    return "x";
  };
  try {
    t.doc.execCommand("copy");
    t.doc.execCommand("COPY");
  } finally {
    String.prototype.toLowerCase = orig;
  }
  await tick();
  assert.equal(host.user, "the user's own text");
  assert.equal(host.copied, "ATTACKER TEXT 3");
});

test("a page brought back from the cache is guarded again and says hello again; a push is answered yes only by its own frame", () => {
  const host = fresh();
  const t = page(host);
  const call = (token: unknown, what: string) => (t.win.__gvClipboard as (t: unknown, w: string) => unknown)(token, what);
  assert.equal(call("not-its-token", "off"), null);
  assert.equal(call(t.token, "off"), "yes");
  Doc.real = host;
  t.doc.selected = "the user's";
  t.doc.execCommand("copy");
  assert.equal(host.user, "the user's");
  const hellos = host.asked.filter((a) => a === "hello").length;
  // Left for the cache: guarded. Brought back: guarded, and heard again; not by a page that was not kept.
  for (const fn of t.win.listeners.pagehide ?? []) fn(new FakeEvent("pagehide"));
  t.doc.selected = "while kept";
  t.doc.execCommand("copy");
  assert.equal(host.user, "the user's");
  call(t.token, "off");
  for (const fn of t.win.listeners.pageshow ?? []) fn(new FakeEvent("pageshow", { persisted: false }));
  assert.equal(host.asked.filter((a) => a === "hello").length, hellos, "a plain show is a new page's: it says hello itself");
  for (const fn of t.win.listeners.pageshow ?? []) fn(new FakeEvent("pageshow", { persisted: true }));
  assert.equal(host.asked.filter((a) => a === "hello").length, hellos + 1);
  t.doc.selected = "brought back";
  t.doc.execCommand("copy");
  assert.equal(host.user, "the user's", "guarded again");
});
