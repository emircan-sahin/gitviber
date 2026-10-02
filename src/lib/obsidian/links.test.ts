import assert from "node:assert/strict";
import { test } from "node:test";
import { linkIndex, resolveLink, splitTarget } from "./links.ts";

const files = ["Home.md", "Projects/Alpha.md", "Projects/Beta/Plan.md", "Archive/Plan.md", "Plan.md", "Inbox/Plan.md", "assets/diagram.png", "Daily/2026-01-02.md", "Café.md", "Notes/Note.v2.md"];
const index = linkIndex(files);

test("a name finds its note anywhere, without .md and regardless of case", () => {
  assert.equal(resolveLink(index, "alpha", "Home.md"), "Projects/Alpha.md");
  assert.equal(resolveLink(index, "Projects/ALPHA.md", "Home.md"), "Projects/Alpha.md");
  assert.equal(resolveLink(index, "diagram.png", "Daily/2026-01-02.md"), "assets/diagram.png");
  assert.equal(resolveLink(index, "Note.v2", "Home.md"), "Notes/Note.v2.md");
  assert.equal(resolveLink(index, "Missing", "Home.md"), null);
  assert.equal(resolveLink(index, "", "Daily/2026-01-02.md"), "Daily/2026-01-02.md");
});

test("of several that match: the path from the top, then the same folder, then the shortest", () => {
  assert.equal(resolveLink(index, "Plan", "Projects/Alpha.md"), "Plan.md");
  assert.equal(resolveLink(index, "Archive/Plan", "Home.md"), "Archive/Plan.md");
  assert.equal(resolveLink(index, "Beta/Plan", "Home.md"), "Projects/Beta/Plan.md");
  const noRoot = linkIndex(files.filter((f) => f !== "Plan.md"));
  assert.equal(resolveLink(noRoot, "Plan", "Archive/Other.md"), "Archive/Plan.md");
  assert.equal(resolveLink(noRoot, "Plan", "Home.md"), "Inbox/Plan.md");
});

test("relative paths and Unicode forms", () => {
  assert.equal(resolveLink(index, "../Home", "Projects/Alpha.md"), "Home.md");
  assert.equal(resolveLink(index, "./Beta/Plan.md", "Projects/Alpha.md"), "Projects/Beta/Plan.md");
  assert.equal(resolveLink(index, "Cafe\u0301", "Home.md"), "Café.md");
});

test("targets split at the first #", () => {
  assert.deepEqual(splitTarget("Note#Part#Sub"), { path: "Note", anchor: "Part#Sub" });
  assert.deepEqual(splitTarget("#^block"), { path: "", anchor: "^block" });
});
