import assert from "node:assert/strict";
import { test } from "node:test";
import { findMatches, NO_OPTIONS, optionKey } from "./findQuery.ts";

const at = (text: string, query: string, o = {}) => findMatches(text, query, { ...NO_OPTIONS, ...o });

test("plain text matches any case, its regex characters literally", () => {
  assert.deepEqual(at("Foo foo FOO", "foo"), [
    [0, 3],
    [4, 7],
    [8, 11],
  ]);
  assert.deepEqual(at("a.b axb", "a.b"), [[0, 3]]);
  assert.deepEqual(at("Foo foo", "foo", { matchCase: true }), [[4, 7]]);
});

test("whole word skips matches inside words, and tries later starts", () => {
  assert.deepEqual(at("foobar foo _foo foo_", "foo", { wholeWord: true }), [[7, 10]]);
  // The first start is inside a word; the second fits.
  assert.deepEqual(at("xab ab", "ab", { wholeWord: true }), [[4, 6]]);
  assert.deepEqual(at("é-ab", "ab", { wholeWord: true }), [[2, 4]]);
});

test("regex: its own syntax, empty matches left out, a broken one is an error", () => {
  assert.deepEqual(at("a1 b22", "\\d+", { regex: true }), [
    [1, 2],
    [4, 6],
  ]);
  assert.deepEqual(at("abc", "x*", { regex: true }), []);
  assert.ok(at("abc", "(", { regex: true }) instanceof Error);
  assert.deepEqual(at("abc", ""), []);
});

test("the option keys are Monaco's", () => {
  const key = (code: string, mods: { alt?: boolean; meta?: boolean; shift?: boolean; ctrl?: boolean }, mac: boolean) =>
    optionKey({ code, altKey: !!mods.alt, metaKey: !!mods.meta, shiftKey: !!mods.shift, ctrlKey: !!mods.ctrl }, mac);
  assert.equal(key("KeyC", { alt: true, meta: true }, true), "matchCase");
  assert.equal(key("KeyW", { alt: true, meta: true }, true), "wholeWord");
  assert.equal(key("KeyR", { alt: true }, false), "regex");
  // ⌥C alone types ç on a Mac.
  assert.equal(key("KeyC", { alt: true }, true), null);
  assert.equal(key("KeyC", { alt: true, meta: true }, false), null);
  assert.equal(key("KeyX", { alt: true }, false), null);
});
