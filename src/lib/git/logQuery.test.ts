import assert from "node:assert/strict";
import { test } from "node:test";
import { isEmptyFilter, parseLogQuery, withAuthor } from "./logQuery.ts";

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

test("a clicked author replaces the author terms and keeps the rest", () => {
  assert.equal(withAuthor("", "ada"), "author:ada");
  assert.equal(withAuthor("fix path:src author:bob Author:\"Bo B\"", "Ada Lovelace"), 'fix path:src author:"Ada Lovelace"');
  // Clicking the one already there changes nothing.
  assert.equal(withAuthor("author:ada", "ada"), "author:ada");
  assert.equal(withAuthor(withAuthor('"fix login"', "Ada Lovelace"), "Ada Lovelace"), '"fix login" author:"Ada Lovelace"');
});

test("whatever the name, the parser reads it back as one author", () => {
  for (const name of ["ada", "Ada Lovelace", "  Grace  Hopper ", "Jean-Luc O'Brien", 'Kim "KJ" Lee', "Zoë Çelik", "path:src", "dev@example.com"]) {
    const { filter } = parseLogQuery(withAuthor("code:x", name));
    assert.deepEqual(filter.author, [name.replaceAll('"', "").trim()], name);
    assert.equal(filter.code, "x");
    assert.deepEqual(filter.grep, []);
  }
});
