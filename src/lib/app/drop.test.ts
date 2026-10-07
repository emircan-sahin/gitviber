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

test("a drop is matched to the repo by whole folder names, on any platform", () => {
  // A sibling whose name starts with the repo's, and the repo's parent, aren't in it.
  assert.deepEqual(filesIn(["/w/demo.old/a.ts", "/w/a.ts", "/w/dem"], "/w/demo"), { inside: [], outside: ["/w/demo.old/a.ts", "/w/a.ts", "/w/dem"] });
  // Nested folders keep their path; a Windows root with a trailing separator or forward slashes.
  assert.deepEqual(filesIn(["C:\\code\\demo\\a\\b\\c.ts"], "C:\\code\\demo\\"), { inside: ["a/b/c.ts"], outside: [] });
  assert.deepEqual(filesIn(["C:\\code\\demo\\x.ts"], "C:/code/demo"), { inside: ["x.ts"], outside: [] });
  assert.deepEqual(filesIn(["\\\\server\\share\\demo\\x.ts"], "\\\\server\\share\\demo"), { inside: ["x.ts"], outside: [] });
  // Outside paths come back as dropped, for the toast.
  assert.deepEqual(filesIn(["D:\\other\\y.ts"], "C:\\code\\demo").outside, ["D:\\other\\y.ts"]);
  assert.deepEqual(filesIn([], "/w/demo"), { inside: [], outside: [] });
});

// slashes() turns every "\" into "/", but outside Windows "\" is a letter of a name (path.ts).
test("a macOS name with a backslash stays one file", { todo: "filesIn splits it into folders" }, () => {
  assert.deepEqual(filesIn(["/w/demo/a\\b.ts"], "/w/demo"), { inside: ["a\\b.ts"], outside: [] });
});

// NTFS ignores case: `gitviber c:\code\demo` gives a root git keeps in the case it was typed.
test("a Windows drop matches the repo whatever the case", { todo: "isInside compares case-sensitively" }, () => {
  assert.deepEqual(filesIn(["C:\\Code\\Demo\\x.ts"], "c:/code/demo"), { inside: ["x.ts"], outside: [] });
});
