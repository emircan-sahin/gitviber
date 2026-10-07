import assert from "node:assert/strict";
import { test } from "node:test";
import { isNews, lookOf, mostUrgent, paneLook } from "./agentLook.ts";
import type { PaneAgent } from "./agentState.ts";

const claude = (state: PaneAgent["state"]): PaneAgent => ({ name: "Claude Code", command: "claude --resume a-1", session: "a-1", cwd: "/w", state });

test("each state has its look, and a finish not looked at is told apart from one that was", () => {
  assert.equal(lookOf("working", false), "working");
  assert.equal(lookOf("waiting", false), "needs");
  assert.equal(lookOf("finished", true), "unread");
  assert.equal(lookOf("finished", false), "done");
  // An agent that doesn't say its state shows nothing until it sends news.
  assert.equal(lookOf("running", false), null);
  assert.equal(lookOf("running", true), "unread");
  assert.equal(lookOf(undefined, false), null);
});

test("a question outranks news, and new work makes the news old", () => {
  assert.equal(lookOf("waiting", true), "needs");
  assert.equal(lookOf("working", true), "working");
});

test("a pane reads its agent's state file: idle is finished", () => {
  assert.equal(paneLook({ agent: claude("idle"), needsYou: true }), "unread");
  assert.equal(paneLook({ agent: claude("idle") }), "done");
  assert.equal(paneLook({ agent: claude("working"), needsYou: false }), "working");
  assert.equal(paneLook({ agent: claude(null) }), null);
  // A bell or a long command in a terminal with no agent.
  assert.equal(paneLook({ needsYou: true }), "unread");
  assert.equal(paneLook({}), null);
});

test("a tab or a worktree shows its most urgent pane", () => {
  assert.equal(mostUrgent(["done", "working", null]), "working");
  assert.equal(mostUrgent(["working", "unread"]), "unread");
  assert.equal(mostUrgent(["unread", "needs", "done"]), "needs");
  assert.equal(mostUrgent([null, undefined]), null);
  assert.equal(mostUrgent([]), null);
});

test("only a question or unseen news is news", () => {
  assert.deepEqual(
    (["needs", "unread", "working", "done", null] as const).map(isNews),
    [true, true, false, false, false],
  );
});
