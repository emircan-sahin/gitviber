import assert from "node:assert/strict";
import { test } from "node:test";
import { authorTerm, isEmptyFilter, parseLogQuery, withAuthor } from "./logQuery.ts";

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
  for (const name of ["ada", "Ada Lovelace", "  Grace  Hopper ", "Jean-Luc O'Brien", "Zoë Çelik", "path:src", "dev@example.com"]) {
    const { filter } = parseLogQuery(withAuthor("code:x", name));
    assert.deepEqual(filter.author, [name.trim()], name);
    assert.equal(filter.code, "x");
    assert.deepEqual(filter.grep, []);
  }
});

test("a clicked author keeps every other kind of term, in order", () => {
  const before = 'fix "login flow" path:"src/a b" code:useState abc1234 AUTHOR:old author:"Old Name"';
  const after = withAuthor(before, "Ada");
  assert.equal(after, 'fix "login flow" path:"src/a b" code:useState abc1234 author:Ada');
  const { filter, shas } = parseLogQuery(after);
  assert.deepEqual(filter.grep, ["fix", "login flow", "abc1234"]);
  assert.deepEqual(filter.paths, ["src/a b"]);
  assert.equal(filter.code, "useState");
  assert.deepEqual(filter.author, ["Ada"]);
  assert.deepEqual(shas, ["abc1234"]);
});

test("names made of syntax read back as one author, never as another term", () => {
  const names = [
    "author:mallory",
    "path:src code:x",
    "-n",
    "--all",
    "a.b*",
    "Ada (bot)",
    "[^a]+$",
    "C:\\Users\\ada",
    "\\",
    "!@#$%^&*()[]{}|;:',.<>?/~`=+",
    "🦀 Ferris the Crab",
    "李 小龍",
    "Ada\tLovelace",
    "x".repeat(10_000),
    "Ada Lovelace ".repeat(1_000).trim(),
  ];
  for (const name of names) {
    const { filter, shas } = parseLogQuery(withAuthor("fix", name));
    assert.deepEqual(filter.author, [name], name.slice(0, 40));
    assert.deepEqual(filter.grep, ["fix"], name.slice(0, 40));
    assert.deepEqual(filter.paths, []);
    assert.equal(filter.code, null);
    assert.deepEqual(shas, []);
  }
});

test("clicking authors one after another keeps one author term", () => {
  let q = "fix";
  for (let i = 0; i < 200; i++) q = withAuthor(q, i % 2 ? `Dev ${i}` : `dev${i}`);
  assert.equal(q, 'fix author:"Dev 199"');
});

test("a long pathological search parses quickly", () => {
  const start = performance.now();
  for (const text of ['"'.repeat(20_000), 'a"'.repeat(10_000), "a ".repeat(10_000), `author:"${"x ".repeat(10_000)}`]) {
    parseLogQuery(withAuthor(text, "Ada"));
  }
  assert.ok(performance.now() - start < 1_000);
});

// Bug repro: an unclosed quote in the box swallows the author term appended after it.
test("an unclosed quote before a clicked author doesn't swallow it", () => {
  const { filter } = parseLogQuery(withAuthor('"fix login', "Ada"));
  assert.deepEqual(filter.author, ["Ada"]);
});

// Bug repro: git matches --author as a fixed substring of "Name <email>"; a name that loses its
// quotes ("Kim KJ Lee") is no longer in "Kim \"KJ\" Lee <...>", so the list goes empty.
test("a name holding quotes still matches its own commits", () => {
  for (const name of ['Kim "KJ" Lee', 'O"Neil']) {
    const [author] = parseLogQuery(withAuthor("", name)).filter.author;
    assert.ok(name.includes(author), `${name} -> ${author}`);
  }
});

test("with the email, the author term is that one person", () => {
  assert.equal(withAuthor("fix", "Ada Lovelace", "ada@example.com"), 'fix author:"Ada Lovelace <ada@example.com>"');
  assert.deepEqual(parseLogQuery(withAuthor("", "Ada", "ada@example.com")).filter.author, ["Ada <ada@example.com>"]);
  // A quote in the name: what's after it still holds the email.
  assert.equal(authorTerm('Kim "KJ" Lee', "kim@example.com"), "Lee <kim@example.com>");
  assert.equal(authorTerm("", "ada@example.com"), "<ada@example.com>");
});
