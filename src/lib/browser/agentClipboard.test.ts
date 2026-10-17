import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { browser } from "./agentSource.test.ts";

// scripts/clipboard.js on stand-in pages: an agent's tab keeps its page's clipboard calls to one
// store, GitViber's, and lets them through once the tab is the user's; frames included.
const script = readFileSync(new URL("scripts/clipboard.js", browser), "utf8").replace("__GV_KEY__", '"k"');
const names = ["window", "document", "Document", "navigator", "Navigator", "addEventListener", "removeEventListener", "DataTransfer", "ClipboardEvent", "ClipboardItem", "Blob", "DOMException"];

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
  copied: string;
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
function page(host: Host, options: { top?: Win; native?: boolean } = {}): Tab {
  // Its own class: the script wraps the one it's given.
  const Page = class extends Doc {};
  const document = new Page();
  Doc.real = host;
  current = document;
  const win: Win = { length: 0, listeners: {}, Navigator: class {} } as unknown as Win;
  win.top = options.top ?? win;
  document.window = win;
  win.webkit = {
    messageHandlers: {
      gvClipboard: {
        async postMessage(body: string) {
          const { key, op, text } = JSON.parse(body);
          assert.equal(key, "k");
          host.asked.push(op);
          // Answered as things stand now, delivered when `hold` lets it.
          const guard = host.guarded ? "1" : "0";
          await host.hold;
          if (op === "set") host.copied = text;
          return op === "get" ? host.copied : op === "guard" ? guard : "";
        },
      },
    },
  };
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
  const env = [win, document, Page, nav, win.Navigator, add, remove, FakeData, FakeEvent, class {}, class {}, DOMException];
  new Function(...names, script)(...env);
  const tab: Tab = {
    win,
    doc: document,
    host,
    ask: (what) => {
      current = document;
      Doc.real = host;
      return (win.__gvClipboard as (k: string, w: string) => unknown)("k", what);
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

const fresh = (): Host => ({ copied: "", guarded: true, user: "the user's own text", asked: [] });
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

test("an agent's copy inside a frame of the page's origin reads that frame; another origin's is left alone", async () => {
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
  frame.contentDocument = null;
  inner.selected = "unreachable";
  assert.equal(t.ask("copy"), null);
  assert.equal(await (t.ask("paste") as Promise<string>), "inside the frame", "the text still goes to the frame natively");
  assert.equal(host.copied, "inside the frame");
});

test("only the tab's key switches the guard or asks for a copy", async () => {
  const host = fresh();
  const t = page(host);
  const ask = t.win.__gvClipboard as (k: string, w: string) => unknown;
  assert.equal(ask("not-k", "off"), null);
  assert.equal(await clip(t).writeText("x"), undefined);
  assert.equal(host.user, "the user's own text");
  assert.throws(() => Object.defineProperty(t.win, "__gvClipboard", { value: () => 0 }));
});

test("a frame's first press asks whether it is the user's, and only the latest answer counts", async () => {
  const host = fresh();
  const top = page(host).win;
  const frame = page(host, { top });
  host.guarded = false;
  frame.press("pointerdown");
  await clip(frame).writeText("held until answered");
  assert.equal(host.copied, "held until answered", "guarded until GitViber answers");
  await tick();
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

test("the main frame's switch reaches the frames of its origin", () => {
  const host = fresh();
  const main = page(host);
  const child = page(host, { top: main.win });
  main.win.length = 1;
  (main.win as unknown as Record<number, unknown>)[0] = { __gvClipboard: child.win.__gvClipboard };
  main.ask("off");
  child.run();
  child.doc.selected = "child";
  child.doc.execCommand("copy");
  assert.equal(host.user, "child");
  main.ask("on");
  child.run();
  child.doc.selected = "guarded again";
  child.doc.execCommand("copy");
  assert.equal(host.user, "child");
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
