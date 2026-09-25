import assert from "node:assert/strict";
import { test } from "node:test";
import { indentUnit, narrow, widen, widenColumn } from "./indent.ts";

const ts = "function a() {\n  /**\n   * Doc.\n   */\n  if (x) {\n    y();\n  }\n}\n";

test("two-space code is widened, four-space code isn't", () => {
  assert.equal(indentUnit(ts), 2);
  assert.equal(indentUnit("fn a() {\n    b();\n        c();\n}\n"), 0);
});

test("a file with tabs is left alone", () => {
  assert.equal(indentUnit("a\n  b\n\tc\n"), 0);
  assert.equal(indentUnit("a\n  b\n", "x\n\ty\n"), 0);
});

test("nothing indented, nothing to widen", () => {
  assert.equal(indentUnit("a\nb\n"), 0);
  assert.equal(indentUnit(null, ""), 0);
});

test("widening keeps a JSDoc's star aligned and turns back exactly", () => {
  const wide = widen(ts, 2);
  assert.equal(wide, "function a() {\n\t/**\n\t * Doc.\n\t */\n\tif (x) {\n\t\ty();\n\t}\n}\n");
  assert.equal(narrow(wide, 2), ts);
});

test("columns follow the widened indentation", () => {
  // "    y();": y at 4 → after two tabs at 2; inside the indentation clamps to its end.
  assert.equal(widenColumn("    y();", 4, 2), 2);
  assert.equal(widenColumn("    y();", 6, 2), 4);
  assert.equal(widenColumn("    y();", 3, 2), 2);
  assert.equal(widenColumn("   * Doc", 3, 2), 2);
  assert.equal(widenColumn("y();", 2, 2), 2);
});
