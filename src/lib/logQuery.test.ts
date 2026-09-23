import assert from "node:assert/strict";
import { test } from "node:test";
import { isEmptyFilter, parseLogQuery } from "./logQuery.ts";

test("words match the message, prefixes narrow", () => {
  const { filter } = parseLogQuery("  fix auth  author:ada path:./src/lib/ code:useState ");
  assert.deepEqual(filter, { grep: ["fix", "auth"], author: ["ada"], code: "useState", paths: ["src/lib"], follow: false });
});

test("quotes keep spaces, prefixes ignore case, empty values drop", () => {
  const { filter } = parseLogQuery('"fix login" Author:"Ada Lovelace" code:"a b" path: author:');
  assert.deepEqual(filter.grep, ["fix login"]);
  assert.deepEqual(filter.author, ["Ada Lovelace"]);
  assert.equal(filter.code, "a b");
  assert.deepEqual(filter.paths, []);
});

test("sha-looking words are also looked up", () => {
  assert.deepEqual(parseLogQuery("1a2b3c4 added deadbeefcafe path:abcdef0").shas, ["1a2b3c4", "deadbeefcafe"]);
  assert.deepEqual(parseLogQuery("1a2b3c4").filter.grep, ["1a2b3c4"]);
});

test("an empty or blank search is no filter", () => {
  assert.ok(isEmptyFilter(parseLogQuery("").filter));
  assert.ok(isEmptyFilter(parseLogQuery('  "" ').filter));
  assert.ok(!isEmptyFilter(parseLogQuery("x").filter));
});
