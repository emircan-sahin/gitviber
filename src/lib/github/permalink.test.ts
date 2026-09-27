import assert from "node:assert/strict";
import { test } from "node:test";
import { permalinkUrl, repoOfCommitUrl } from "./permalink.ts";

const WEB = "https://github.com/o/r";

test("a file links to its commit, with the lines picked", () => {
  assert.equal(permalinkUrl(WEB, "abc", "src/a.ts"), `${WEB}/blob/abc/src/a.ts`);
  assert.equal(permalinkUrl(WEB, "abc", "src/a.ts", { lines: [10, 24] }), `${WEB}/blob/abc/src/a.ts#L10-L24`);
  assert.equal(permalinkUrl(WEB, "abc", "src/a.ts", { lines: [7, 7] }), `${WEB}/blob/abc/src/a.ts#L7`);
});

test("folders are trees, and names are escaped per segment", () => {
  assert.equal(permalinkUrl(WEB, "abc", "", { tree: true }), `${WEB}/tree/abc`);
  assert.equal(permalinkUrl(WEB, "abc", "app/[id] #1/p?.tsx"), `${WEB}/blob/abc/app/%5Bid%5D%20%231/p%3F.tsx`);
  assert.equal(permalinkUrl(WEB, "abc", "docs", { tree: true }), `${WEB}/tree/abc/docs`);
});

test("a commit page's repo", () => {
  assert.equal(repoOfCommitUrl(`${WEB}/commit/abc`), WEB);
});
