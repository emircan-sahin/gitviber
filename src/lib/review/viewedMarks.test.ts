import assert from "node:assert/strict";
import { test } from "node:test";
import { pruned } from "./viewedMarks.ts";

test("only the oldest marks of commits' and ranges' files are given up, never a working-tree one", () => {
  const marks = new Map([
    ["unstaged:a.ts", "M:1"],
    ["commit:aaa:a.ts", "M:1"],
    ["pr-file:5@b..c:a.ts", "M:2"],
    ["branch:base:a.ts", "M:3"],
    ["commit:bbb:a.ts", "A:4"],
  ]);
  assert.deepEqual([...pruned(new Map(marks), 2).keys()], ["unstaged:a.ts", "pr-file:5@b..c:a.ts", "branch:base:a.ts", "commit:bbb:a.ts"]);
  // Under the limit: all stay.
  assert.equal(pruned(new Map(marks), 10).size, 5);
  assert.equal(pruned(new Map(marks), 0).size, 2);
});
