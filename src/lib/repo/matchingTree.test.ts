import assert from "node:assert/strict";
import { test } from "node:test";
import type { Entry } from "../api/types.ts";
import { MAX_MATCHES, matchingTree } from "./matchingTree.ts";

const file = (path: string, ignored = false): Entry => ({ name: path.slice(path.lastIndexOf("/") + 1), path, isDir: false, ignored });
const has = (needle: string) => (path: string) => path.includes(needle);

test("no match lists an empty root, not nothing", () => {
  const t = matchingTree([file("src/a.ts")], has("zzz"));
  assert.deepEqual(t.children, { "": [] });
  assert.equal(t.found, 0);
});

test("matches come with their folders, open, folders first", () => {
  const t = matchingTree([file("src/lib/env.ts"), file(".env", true), { name: "node_modules", path: "node_modules", isDir: true, ignored: true }, file("README.md")], (p) => /env|node/.test(p));
  assert.deepEqual(
    t.children[""].map((e) => [e.path, e.isDir, e.ignored]),
    [
      ["node_modules", true, true],
      ["src", true, false],
      [".env", false, true],
    ],
  );
  assert.deepEqual(t.children["src"].map((e) => e.path), ["src/lib"]);
  assert.deepEqual([...t.expanded].sort(), ["", "src", "src/lib"]);
  assert.equal(t.children["node_modules"], undefined, "an ignored folder stays closed");
});

test("capped at MAX_MATCHES", () => {
  const t = matchingTree(Array.from({ length: MAX_MATCHES + 5 }, (_, i) => file(`f${i}`)), () => true);
  assert.equal(t.found, MAX_MATCHES);
  assert.ok(t.capped);
  assert.equal(t.children[""].length, MAX_MATCHES);
});
