import assert from "node:assert/strict";
import { test } from "node:test";
import { windowTitle } from "./windowTitle.ts";

test("the window is named after its repo and branch", () => {
  assert.equal(windowTitle(null, null, null), "GitViber");
  assert.equal(windowTitle("/Users/ada/code/demo-app", "main", "0123456789abcdef"), "demo-app — main");
  assert.equal(windowTitle("/Users/ada/code/demo-app/", "feature/login", null), "demo-app — feature/login");
  assert.equal(windowTitle("C:\\code\\demo-app", "main", null), "demo-app — main");
  // Detached: its commit.
  assert.equal(windowTitle("/w/demo-app", null, "0123456789abcdef"), "demo-app — 0123456");
  // Not read yet.
  assert.equal(windowTitle("/w/demo-app", null, null), "demo-app");
});
