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

test("a title keeps names as they are, whatever their letters or length", () => {
  assert.equal(windowTitle("/Users/ada/Projeler/çalışma 😀", "özellik/giriş", null), "çalışma 😀 — özellik/giriş");
  const long = "x".repeat(300);
  assert.equal(windowTitle(`/w/${long}`, long, null), `${long} — ${long}`);
  // A share on Windows, and a root git printed with a trailing separator.
  assert.equal(windowTitle("\\\\server\\share\\demo", "main", null), "demo — main");
  assert.equal(windowTitle("C:\\code\\demo\\", "main", null), "demo — main");
  // The branch wins over the commit; an unborn branch has a name and no commit yet.
  assert.equal(windowTitle("/w/demo", "main", "abcdef0"), "demo — main");
  assert.equal(windowTitle("/w/new", "main", null), "new — main");
});
