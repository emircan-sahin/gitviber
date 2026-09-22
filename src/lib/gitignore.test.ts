import assert from "node:assert/strict";
import { test } from "node:test";
import { ignorePattern } from "./gitignore.ts";

test("plain paths are anchored", () => {
  assert.equal(ignorePattern("dist/out.js"), "/dist/out.js");
  assert.equal(ignorePattern("#notes.md"), "/#notes.md");
  assert.equal(ignorePattern("!keep"), "/!keep");
});

test("glob characters are escaped", () => {
  assert.equal(ignorePattern("app/[id].tsx"), "/app/\\[id\\].tsx");
  assert.equal(ignorePattern("a*b?.txt"), "/a\\*b\\?.txt");
  assert.equal(ignorePattern("back\\slash"), "/back\\\\slash");
});

test("trailing spaces are kept", () => {
  assert.equal(ignorePattern("name  "), "/name\\ \\ ");
  assert.equal(ignorePattern("a b"), "/a b");
});
