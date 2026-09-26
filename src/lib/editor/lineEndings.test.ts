import assert from "node:assert/strict";
import { test } from "node:test";
import { keepsLineEndings } from "./lineEndings.ts";

test("one kind of line ending survives Monaco", () => {
  assert.equal(keepsLineEndings("a\nb\n"), true);
  assert.equal(keepsLineEndings("a\r\nb\r\n"), true);
  assert.equal(keepsLineEndings(""), true);
  assert.equal(keepsLineEndings("no newline"), true);
});

test("mixed or lone CR endings don't", () => {
  assert.equal(keepsLineEndings("a\r\nb\n"), false);
  assert.equal(keepsLineEndings("\na\r\n"), false);
  assert.equal(keepsLineEndings("a\rb\r"), false);
  assert.equal(keepsLineEndings("a\r\nb\rc\r\n"), false);
});
