import assert from "node:assert/strict";
import { test } from "node:test";
import { cloneFolderName, cloneUrl } from "./clone.ts";

test("owner/name is GitHub shorthand; anything else goes to git as typed", () => {
  assert.equal(cloneUrl(" emircan-sahin/gitviber "), "https://github.com/emircan-sahin/gitviber.git");
  assert.equal(cloneUrl("me/repo.js.git"), "https://github.com/me/repo.js.git");
  assert.equal(cloneUrl("https://gitlab.com/me/repo"), "https://gitlab.com/me/repo");
  assert.equal(cloneUrl("git@github.com:me/repo.git"), "git@github.com:me/repo.git");
  assert.equal(cloneUrl("/Users/me/origin.git"), "/Users/me/origin.git");
  assert.equal(cloneUrl("../x"), "../x");
  assert.equal(cloneUrl("a/b/c"), "a/b/c");
});

test("the folder is named like git clone names it", () => {
  assert.equal(cloneFolderName("https://github.com/me/repo.git"), "repo");
  assert.equal(cloneFolderName("https://github.com/me/repo/"), "repo");
  assert.equal(cloneFolderName("git@github.com:me/repo.git"), "repo");
  assert.equal(cloneFolderName("git@host:repo.git"), "repo");
  assert.equal(cloneFolderName("ssh://git@host:2222/me/my.repo.git"), "my.repo");
  assert.equal(cloneFolderName("file:///tmp/work/.git"), "work");
  assert.equal(cloneFolderName("https://host/me/repo.git?ref=x"), "repo");
  assert.equal(cloneFolderName(""), "");
  assert.equal(cloneFolderName("https://host/.."), "");
});
