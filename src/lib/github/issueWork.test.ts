import assert from "node:assert/strict";
import { test } from "node:test";
import { closingLine, issueBranchName, withClosing, withIssue } from "./issueWork.ts";

test("an issue's branch: its number and title, lowercase, cut at a word", () => {
  assert.equal(issueBranchName(12, "Fix the login bug!"), "12-fix-the-login-bug");
  assert.equal(issueBranchName(7, "Worktree picker: show each worktree's pull request and CI state"), "7-worktree-picker-show-each-worktrees-pull");
  assert.equal(issueBranchName(3, "Türkçe başlık"), "3-turkce-baslık");
  assert.equal(issueBranchName(4, "  ...  "), "issue-4");
  assert.equal(issueBranchName(5, "a".repeat(60)), `5-${"a".repeat(40)}`);
});

test("{issue} in the Run command is the number", () => {
  assert.equal(withIssue('claude "Fix #{issue}"', 12), 'claude "Fix #12"');
  assert.equal(withIssue("claude", 12), "claude");
});

test("the closing line names the repository only when the PR goes elsewhere", () => {
  assert.equal(closingLine("https://github.com/o/r/issues/12", "O/R"), "Closes #12");
  assert.equal(closingLine("https://github.com/up/r/issues/12", "me/r"), "Closes up/r#12");
  assert.equal(closingLine("https://github.com/o/r/pull/12", "o/r"), null);
});

test("the closing line goes after the body, once", () => {
  assert.equal(withClosing("", "Closes #12"), "Closes #12");
  assert.equal(withClosing("Body\n", "Closes #12"), "Body\n\nCloses #12");
  assert.equal(withClosing("Fixes #12 too", "Closes #12"), "Fixes #12 too");
  assert.equal(withClosing("Fixes #123", "Closes #12"), "Fixes #123\n\nCloses #12");
  assert.equal(withClosing("resolves up/r#12", "Closes up/r#12"), "resolves up/r#12");
});
