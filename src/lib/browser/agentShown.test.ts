import assert from "node:assert/strict";
import { test } from "node:test";
import type { Selection } from "../repo/selection.ts";
import { hiddenAgentIds, nearShown, shownIndexToAll, shownTabs } from "./agentShown.ts";

const file = (key: string) => ({ key, sel: { kind: "file", path: key } satisfies Selection });
const page = (id: string) => ({ key: `browser:${id}`, sel: { kind: "browser", id, url: "http://a.test/" } satisfies Selection });

test("hidden, every agent tab but the user's own is left out; shown, none", () => {
  const owners = new Map([
    ["a", 1],
    ["b", 1],
    ["c", 2],
  ]);
  // b was taken; c's pane closed.
  assert.deepEqual([...hiddenAgentIds(owners, new Set(["b", "c"]), false)], ["a"]);
  assert.equal(hiddenAgentIds(owners, new Set(), true).size, 0);
});

test("the shown tabs are the user's own and the agent tabs not hidden, in order", () => {
  const tabs = [file("x"), page("a"), page("mine"), page("b")];
  assert.deepEqual(
    shownTabs(tabs, new Set(["a", "b"])).map((t) => t.key),
    ["x", "browser:mine"],
  );
  assert.equal(shownTabs(tabs, new Set()), tabs, "the same list when nothing is hidden");
  assert.equal(shownTabs(tabs, new Set(["zzz"])), tabs, "and when none of its tabs is");
});

test("a tab going out of the strip hands over to the next shown one to the right, else the left", () => {
  const tabs = [file("x"), page("a"), page("b"), file("y")];
  const hidden = new Set(["a", "b"]);
  assert.equal(nearShown(tabs, hidden, "browser:a"), "y");
  assert.equal(nearShown([file("x"), page("a"), page("b")], hidden, "browser:b"), "x");
  assert.equal(nearShown([page("a"), page("b")], hidden, "browser:a"), null);
});

test("a closed tab's neighbour skips the other tabs closing with it", () => {
  const tabs = [file("x"), file("y"), file("z")];
  assert.equal(nearShown(tabs, new Set(), "y", new Set(["y", "z"])), "x");
  assert.equal(nearShown(tabs, new Set(), "y", new Set(["y"])), "z");
  assert.equal(nearShown(tabs, new Set(), "y", new Set(["x", "y", "z"])), null);
});

test("an index among the shown tabs is the tab's place among all of them", () => {
  const tabs = [page("a"), file("x"), page("b"), file("y")];
  const hidden = new Set(["a", "b"]);
  assert.equal(shownIndexToAll(tabs, hidden, 0), 1);
  assert.equal(shownIndexToAll(tabs, hidden, 1), 3);
  assert.equal(shownIndexToAll(tabs, hidden, 2), -1);
  assert.equal(shownIndexToAll(tabs, new Set(), 2), 2);
});
