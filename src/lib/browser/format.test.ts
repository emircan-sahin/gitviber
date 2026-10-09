import assert from "node:assert/strict";
import { test } from "node:test";
import type { BrowserPick, ConsoleEntry } from "../api/browser.ts";
import { formatErrors, formatPick } from "./format.ts";

const pick: BrowserPick = {
  selector: "#save",
  tag: "button",
  html: '<button id="save"\n  class="btn">Save</button>',
  text: "Save",
  box: { x: 10.4, y: 20.6, w: 80, h: 30 },
  styles: { display: "inline-flex", position: "static", color: "rgb(0, 0, 0)", margin: "0px", "font-weight": "600" },
  components: ["SaveButton", "Form", "App"],
  screenshot: "/tmp/picks/pick-1.png",
  url: "http://localhost:5173/settings",
};

test("a pick reads as where, what, how it looks, its picture, then the note", () => {
  assert.equal(
    formatPick(pick, "  make it blue  ", { name: "iPhone 16 Pro", viewport: { w: 402, h: 778 } }),
    [
      "Picked an element on http://localhost:5173/settings as iPhone 16 Pro (402×778)",
      "- selector: #save",
      "- React: SaveButton < Form < App",
      '- text: "Save"',
      "- box: 80×30 at 10,21",
      "- styles: display: inline-flex; position: static; color: rgb(0, 0, 0); font-weight: 600",
      '- html: <button id="save" class="btn">Save</button>',
      "- screenshot: /tmp/picks/pick-1.png",
      "Note: make it blue",
    ].join("\n"),
  );
});

test("what a pick doesn't have is left out, not written empty", () => {
  const bare = formatPick({ ...pick, components: [], text: "", screenshot: null, styles: {} }, " ");
  assert.equal(bare, ["Picked an element on http://localhost:5173/settings", "- selector: #save", "- box: 80×30 at 10,21", '- html: <button id="save" class="btn">Save</button>'].join("\n"));
});

const entry = (level: ConsoleEntry["level"], msg: string, stack = "", url = "http://localhost:5173/"): ConsoleEntry => ({ level, msg, stack, url, ts: 0 });

test("errors go once each with how often, the latest last, a little of each stack", () => {
  const text = formatErrors([
    entry("error", "TypeError: a is undefined", "TypeError: a is undefined\n    at f (app.js:1:2)\n    at g (app.js:3:4)\n    at h (app.js:5:6)\n    at i (app.js:7:8)"),
    entry("warn", "careful"),
    entry("load", ""),
    entry("error", "boom"),
    entry("error", "TypeError: a is undefined"),
  ]);
  assert.equal(text, ["Errors on http://localhost:5173/:", "- boom", "- (×2) TypeError: a is undefined"].join("\n"));
  assert.equal(formatErrors([entry("warn", "only a warning")]), "");
  const many = Array.from({ length: 30 }, (_, i) => entry("error", `e${i}`));
  const lines = formatErrors(many).split("\n");
  assert.equal(lines.length, 21);
  assert.equal(lines[1], "- e10");
  const two = formatErrors([entry("error", "a", "", "http://a.test/"), entry("error", "b", "  at x (b.js:1:1)", "http://b.test/")]);
  assert.equal(two, ["Errors from the page:", "- a (http://a.test/)", "- b (http://b.test/)", "    at x (b.js:1:1)"].join("\n"));
});
