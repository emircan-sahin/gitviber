import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

// The scripts the browser tab's view runs in a page (src-tauri/src/browser/scripts): WebKit
// reports a syntax error in one only as the feature quietly not working.
const script = (name: string) => readFileSync(new URL(`../../../src-tauri/src/browser/scripts/${name}`, import.meta.url), "utf8");

test("the page scripts parse, the component walk as the function body WebKit runs it as", () => {
  for (const name of ["picker.js", "console.js"]) assert.doesNotThrow(() => new Function(script(name)), name);
  const AsyncFunction = Object.getPrototypeOf(async () => {}).constructor;
  assert.doesNotThrow(() => new AsyncFunction("nonce", script("components.js")));
});

test("the component walk names React's components nearest first, skips host elements, and untags the element", async () => {
  const AsyncFunction = Object.getPrototypeOf(async () => {}).constructor;
  const walk = new AsyncFunction("nonce", "document", "CSS", script("components.js"));
  function SaveButton() {}
  function Form() {}
  const memo = { type: function Card() {} };
  const fiber = { type: "button", return: { type: SaveButton, return: { type: memo, return: { type: Form, return: { type: Form, return: null } } } } };
  const removed: string[] = [];
  const el = { "__reactFiber$abc": fiber, removeAttribute: (a: string) => removed.push(a) };
  const doc = { querySelector: (s: string) => (s === '[data-gv-pick="n1"]' ? el : null) };
  const names = JSON.parse(await walk("n1", doc, { escape: (s: string) => s }));
  assert.deepEqual(names, ["SaveButton", "Card", "Form"]);
  assert.deepEqual(removed, ["data-gv-pick"]);
  assert.equal(await walk("missing", doc, { escape: (s: string) => s }), "[]");
});

/** console.js run against a stand-in page: its console, its listeners, and what it posts. */
function consolePage(handler: { postMessage: (s: string) => void } | null = { postMessage: () => {} }) {
  const logged: unknown[][] = [];
  const listeners: Record<string, (e: unknown) => void> = {};
  const page = {
    webkit: handler && { messageHandlers: { gvConsole: handler } },
    addEventListener: (type: string, fn: (e: unknown) => void) => void (listeners[type] = fn),
  } as Record<string, unknown>;
  const pageConsole = {
    error: (...args: unknown[]) => void logged.push(["error", ...args]),
    warn: (...args: unknown[]) => void logged.push(["warn", ...args]),
    log: () => {},
  };
  new Function("window", "console", "location", script("console.js"))(page, pageConsole, { href: "http://localhost:5173/" });
  return { page, pageConsole, logged, listeners };
}

test("the console still logs everything, sends each error once, and survives what a page logs", () => {
  const sent: Record<string, unknown>[] = [];
  const { pageConsole, logged, listeners } = consolePage({ postMessage: (s) => void sent.push(JSON.parse(s)) });
  const cyclic: Record<string, unknown> = {};
  cyclic.self = cyclic;
  const hostile = { toString: () => { throw new Error("no"); }, toJSON: () => { throw new Error("no"); } };
  const bare = Object.create(null);
  pageConsole.error("plain", 42, null, undefined, cyclic, 10n, Symbol("s"));
  pageConsole.warn(hostile);
  pageConsole.warn(bare, "after a prototype-less object");
  pageConsole.error(new TypeError("bad"));
  pageConsole.error("x".repeat(100_000));
  assert.equal(logged.length, 5, "the page's own console got every call");
  assert.equal(sent[0].msg, "plain 42 null undefined [object Object] 10 Symbol(s)");
  assert.equal(sent[0].level, "error");
  // The hostile one made no line, and broke nothing after it.
  assert.deepEqual(sent.slice(1).map((s) => s.level), ["warn", "error", "error"]);
  assert.equal(sent[2].msg, "TypeError: bad");
  assert.match(String(sent[2].stack), /TypeError: bad/);
  assert.ok(String(sent[3].msg).length <= 2049, "cut");
  // Uncaught errors and rejections, whatever was thrown.
  listeners.error({ error: undefined, message: "Script error.", filename: "", lineno: 0, colno: 0 });
  listeners.unhandledrejection({ reason: { code: 7 } });
  listeners.unhandledrejection({ reason: undefined });
  assert.deepEqual(sent.slice(4).map((s) => s.msg), ["Script error.", 'Unhandled rejection: {"code":7}', "Unhandled rejection: undefined"]);
});

test("a handler that throws, or none, leaves the page's console as it was", () => {
  const { pageConsole, logged } = consolePage({
    postMessage: () => {
      throw new Error("gone");
    },
  });
  assert.doesNotThrow(() => pageConsole.error("still logs"));
  assert.equal(logged.length, 1);
  // Without the handler (another window, or the console turned off), nothing is wrapped.
  const none = consolePage(null);
  assert.equal(none.page.__gvConsole, undefined);
  // Run twice in one page (a frame re-injected), errors still go once.
  const sent: string[] = [];
  const twice = consolePage({ postMessage: (s) => void sent.push(s) });
  new Function("window", "console", "location", script("console.js"))(twice.page, twice.pageConsole, { href: "http://localhost/" });
  twice.pageConsole.error("once");
  assert.equal(sent.length, 1);
});

test("the component walk ends on any fiber chain and names wrappers by what they wrap", async () => {
  const AsyncFunction = Object.getPrototypeOf(async () => {}).constructor;
  const walk = new AsyncFunction("nonce", "document", "CSS", script("components.js"));
  const on = (fiber: unknown, key = "__reactFiber$x") => ({ querySelector: () => ({ [key]: fiber, removeAttribute: () => {} }) });
  const css = { escape: (s: string) => s };
  function Inner() {}
  const Named = Object.assign(function Anon() {}, { displayName: "Shown" });
  const memoOfRef = { type: { render: Inner } };
  const anonymous = [() => {}][0];
  const chain = { type: Named, return: { type: { render: Inner }, return: { type: memoOfRef, return: { type: anonymous, return: { type: class Page {}, return: null } } } } };
  // forwardRef by its render; memo(forwardRef) is a wrapper of a wrapper; anonymous arrows skipped.
  assert.deepEqual(JSON.parse(await walk("n", on(chain), css)), ["Shown", "Inner", "Page"]);
  // React 16's key; and no React at all.
  assert.deepEqual(JSON.parse(await walk("n", on({ type: Inner, return: null }, "__reactInternalInstance$y"), css)), ["Inner"]);
  assert.deepEqual(JSON.parse(await walk("n", on(undefined, "unrelated"), css)), []);
  // Deep trees stop at five names.
  let deep: unknown = null;
  for (let i = 0; i < 10_000; i++) deep = { type: Object.defineProperty(function () {}, "name", { value: `C${i}` }), return: deep };
  assert.equal(JSON.parse(await walk("n", on(deep), css)).length, 5);
});
