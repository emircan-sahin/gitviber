import assert from "node:assert/strict";
import { test } from "node:test";
import { filesIn } from "./drop.ts";

test("dropped files are opened by their path from the repo, others are not", () => {
  assert.deepEqual(filesIn(["/w/demo/src/a.ts", "/w/demo/README.md", "/w/other/b.ts", "/w/demo-2/c.ts"], "/w/demo"), {
    inside: ["src/a.ts", "README.md"],
    outside: ["/w/other/b.ts", "/w/demo-2/c.ts"],
  });
  // git prints a root with a trailing separator.
  assert.deepEqual(filesIn(["/w/demo/a b/ç ğ.ts"], "/w/demo/"), { inside: ["a b/ç ğ.ts"], outside: [] });
  // The repo folder itself isn't a file in it.
  assert.deepEqual(filesIn(["/w/demo"], "/w/demo"), { inside: [], outside: ["/w/demo"] });
  assert.deepEqual(filesIn(["C:\\code\\demo\\src\\a.ts", "D:\\x.ts"], "C:\\code\\demo"), { inside: ["src/a.ts"], outside: ["D:\\x.ts"] });
});
