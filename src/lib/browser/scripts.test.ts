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
