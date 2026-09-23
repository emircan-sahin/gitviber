import assert from "node:assert/strict";
import { test } from "node:test";
import { FITS, lineWidth, wrapLine } from "./wrap.ts";

const pieces = (text: string, cols: number) => {
  const { at } = wrapLine(text, cols);
  return [0, ...at].map((a, i) => text.slice(a, at[i] ?? text.length));
};

test("a line that fits isn't wrapped", () => {
  assert.equal(wrapLine("hello", 5), FITS);
  assert.equal(wrapLine("", 5), FITS);
});

test("breaks after whitespace, keeping it on the line it ends", () => {
  assert.deepEqual(pieces("aaa bbb ccc", 8), ["aaa bbb ", "ccc"]);
  assert.deepEqual(pieces("aaa bbb ccc", 6), ["aaa ", "bbb ", "ccc"]);
});

test("whitespace hangs instead of starting a line", () => {
  assert.deepEqual(pieces("aaaa     bbbb", 4), ["aaaa     ", "bbbb"]);
});

test("a word longer than the line is cut", () => {
  assert.deepEqual(pieces("abcdefghij", 4), ["abcd", "efgh", "ij"]);
  assert.deepEqual(pieces("ab cdefghij", 4), ["ab ", "cdef", "ghij"]);
});

test("continuation lines keep the indentation", () => {
  const w = wrapLine("    foo bar baz", 11);
  assert.equal(w.indent, 4);
  assert.deepEqual(pieces("    foo bar baz", 11), ["    foo bar ", "baz"]);
  // Room on continuation lines is what's left after the indent.
  assert.deepEqual(pieces("  abcdefgh", 6), ["  abcd", "efgh"]);
});

test("the indentation alone never becomes a line", () => {
  assert.deepEqual(pieces("        abcdefgh", 10), ["        ab", "cdefgh"]);
});

test("deep indentation isn't repeated", () => {
  assert.equal(wrapLine("        abcdefgh", 10).indent, 0);
});

test("tabs advance to the next stop", () => {
  // "\t" is 4 columns, "a\t" is 4 too.
  assert.deepEqual(pieces("a\tb\tcdef", 8), ["a\tb\t", "cdef"]);
});

test("wide characters take two columns", () => {
  assert.deepEqual(pieces("日本語テキスト", 6), ["日本語", "テキス", "ト"]);
  assert.deepEqual(pieces("ab😀cd", 4), ["ab😀", "cd"]);
});

test("combining marks take none", () => {
  assert.equal(wrapLine("ééé", 3), FITS);
});

test("a line's width counts tab stops and wide characters", () => {
  assert.equal(lineWidth("abc"), 3);
  assert.equal(lineWidth("a\tb"), 5);
  assert.equal(lineWidth("日本"), 4);
});

test("an emoji sequence or a flag is never split", () => {
  assert.deepEqual(pieces("ab👨‍👩‍👧cd", 4), ["ab👨‍👩‍👧", "cd"]);
  assert.deepEqual(pieces("🇹🇷🇹🇷🇹🇷", 4), ["🇹🇷🇹🇷", "🇹🇷"]);
});

test("emoji take two columns, spacing marks one", () => {
  assert.equal(lineWidth("✅🚀"), 4);
  assert.equal(lineWidth("❤️"), 2);
  assert.equal(lineWidth("का"), 1);
});
