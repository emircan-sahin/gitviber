import assert from "node:assert/strict";
import { test } from "node:test";
import { foldersOf, pathTree, treeOrder } from "./pathTree.ts";

const id = (s: string) => s;
const shape = (rows: ReturnType<typeof pathTree<string>>) => rows.map((r) => `${"  ".repeat(r.depth)}${r.kind === "folder" ? `${r.label}/ (${r.items.length})` : r.name}`);

test("folders come first, by name in natural order, and a folder holding only a folder shares its row", () => {
  const rows = pathTree(["src/lib/ui/a.ts", "src/lib/ui/b.ts", "README.md", "src/main.ts", "docs/v10.md", "docs/v9.md", "app/x/y/z.ts"], id);
  assert.deepEqual(shape(rows), [
    "app/x/y/ (1)",
    "  z.ts",
    "docs/ (2)",
    "  v9.md",
    "  v10.md",
    "src/ (3)",
    "  lib/ui/ (2)",
    "    a.ts",
    "    b.ts",
    "  main.ts",
    "README.md",
  ]);
  // A compacted row acts for its deepest folder.
  const lib = rows.find((r) => r.kind === "folder" && r.label === "lib/ui");
  assert.equal(lib?.kind === "folder" && lib.path, "src/lib/ui");
});

test("a closed folder hides what's in it but still covers it", () => {
  const rows = pathTree(["a/b/c.ts", "a/d.ts", "e.ts"], id, { closed: (p) => p === "a" });
  assert.deepEqual(shape(rows), ["a/ (2)", "e.ts"]);
  assert.deepEqual(rows[0].kind === "folder" && rows[0].items, ["a/b/c.ts", "a/d.ts"]);
  assert.equal(rows[0].kind === "folder" && rows[0].open, false);
});

test("ranked, newest first: folders by their newest file", () => {
  const time: Record<string, number> = { "old/a.ts": 1, "new/b.ts": 5, "new/c.ts": 3, "top.ts": 9, "mid.ts": 4 };
  const rows = pathTree(Object.keys(time), id, { rank: (p) => time[p] });
  assert.deepEqual(shape(rows), ["new/ (2)", "  b.ts", "  c.ts", "old/ (1)", "  a.ts", "top.ts", "mid.ts"]);
  assert.deepEqual(treeOrder(Object.keys(time), id, (p) => time[p]), ["new/b.ts", "new/c.ts", "old/a.ts", "top.ts", "mid.ts"]);
});

test("a nested repository's folder path is a leaf", () => {
  assert.deepEqual(shape(pathTree(["vendor/lib/", "vendor/x.ts"], id)), ["vendor/ (2)", "  lib", "  x.ts"]);
});

test("the folders a path sits in", () => {
  assert.deepEqual(foldersOf("a/b/c.ts"), ["a", "a/b"]);
  assert.deepEqual(foldersOf("c.ts"), []);
});

test("a chain with a file at each level keeps a row for every folder", () => {
  assert.deepEqual(shape(pathTree(["a/b/c/d.ts", "a/x.ts", "a/b/y.ts", "a/b/c/e.ts"], id)), [
    "a/ (4)",
    "  b/ (3)",
    "    c/ (2)",
    "      d.ts",
    "      e.ts",
    "    y.ts",
    "  x.ts",
  ]);
});

test("only root files: no folder rows, and a closed folder that isn't there changes nothing", () => {
  const rows = pathTree(["b.ts", "a.ts"], id, { closed: () => true });
  assert.deepEqual(shape(rows), ["a.ts", "b.ts"]);
});

test("a closed compacted row hides its whole chain", () => {
  const rows = pathTree(["a/b/c/d.ts", "z.ts"], id, { closed: (p) => p === "a/b/c" });
  assert.deepEqual(shape(rows), ["a/b/c/ (1)", "z.ts"]);
});

test("files that all have no time (deleted) fall back to name order, and renamed ones rank by their new path", () => {
  const time = new Map<string, number>([["new/name.ts", 7]]);
  const rank = (p: string) => time.get(p) ?? -Infinity;
  assert.deepEqual(shape(pathTree(["b/gone.ts", "a/gone.ts", "new/name.ts"], id, { rank })), ["new/ (1)", "  name.ts", "a/ (1)", "  gone.ts", "b/ (1)", "  gone.ts"]);
});

test("hundreds of folders stay quick and keep their order", () => {
  const paths = Array.from({ length: 800 }, (_, i) => `d${i}/sub/f${i}.ts`);
  const start = performance.now();
  const rows = pathTree(paths, id);
  assert.equal(rows.length, 800 * 2);
  assert.ok(performance.now() - start < 1000);
  assert.equal(rows[0].kind === "folder" && rows[0].label, "d0/sub");
  assert.equal(rows[2].kind === "folder" && rows[2].label, "d1/sub");
});

test("a nested repository's path with a trailing slash sits in its folders", () => {
  assert.deepEqual(foldersOf("vendor/lib/"), ["vendor"]);
});
