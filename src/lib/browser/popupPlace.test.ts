import assert from "node:assert/strict";
import { test } from "node:test";
import { insertBeside, openerOf } from "./popupPlace.ts";

const keys = (tabs: { key: string }[]) => tabs.map((t) => t.key);
const t = (key: string) => ({ key });

test("a popup's tab goes right after its opener's, after the popups it already opened", () => {
  const tabs = [t("a"), t("b"), t("c")];
  assert.deepEqual(keys(insertBeside(tabs, "a", t("p1"), new Set())), ["a", "p1", "b", "c"]);
  assert.deepEqual(keys(insertBeside([t("a"), t("p1"), t("b")], "a", t("p2"), new Set(["p1"]))), ["a", "p1", "p2", "b"]);
  // Another opener's popup in between isn't kin.
  assert.deepEqual(keys(insertBeside([t("a"), t("q1"), t("b")], "a", t("p1"), new Set())), ["a", "p1", "q1", "b"]);
});

test("a popup whose opener is gone goes to the end", () => {
  assert.deepEqual(keys(insertBeside([t("a")], "zzz", t("p"), new Set())), ["a", "p"]);
});

test("a closed popup returns to its opener, while that is open", () => {
  const popups = new Map([["p", "a"]]);
  assert.equal(openerOf("p", [t("a"), t("b")], popups), "a");
  assert.equal(openerOf("p", [t("b")], popups), null, "its opener closed first");
  assert.equal(openerOf("p", [t("a"), t("p")], popups), null, "still open");
  assert.equal(openerOf("b", [t("a")], popups), null, "not a popup");
  assert.equal(openerOf(null, [t("a")], popups), null);
});
