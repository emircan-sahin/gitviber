import assert from "node:assert/strict";
import { test } from "node:test";
import { nextAgent, type PaneAgent, restoredAgent, resumeOf, savedAgent } from "./agentState.ts";

const claude = (state: PaneAgent["state"], command: string | null = "claude --resume a-1"): PaneAgent => ({ name: "Claude Code", command, state });

test("an agent that finishes or stops to ask is worth a note, once", () => {
  assert.equal(nextAgent(claude("working"), claude("idle")).note, "Claude Code finished");
  assert.equal(nextAgent(claude("working"), claude("waiting")).note, "Claude Code is waiting for you");
  assert.equal(nextAgent(claude("idle"), claude("waiting")).note, "Claude Code is waiting for you");
  // Read again by the next save round: nothing new.
  assert.equal(nextAgent(claude("waiting"), claude("waiting")).note, null);
  assert.equal(nextAgent(claude("idle"), claude("idle")).note, null);
  // Answered, then idle: the user was there.
  assert.equal(nextAgent(claude("waiting"), claude("idle")).note, null);
  // Seen first when already idle (a restore, a pane opened on it): it didn't just finish.
  assert.equal(nextAgent(undefined, claude("idle")).note, null);
  assert.equal(nextAgent(claude("working"), claude("working")).note, null);
});

test("a prompt (or nothing found) clears the agent, without a note", () => {
  assert.deepEqual(nextAgent(claude("working"), null), { agent: undefined, note: null, first: false });
});

test("an unchanged read keeps the same object; a new id or state replaces it", () => {
  const prev = claude("working");
  assert.equal(nextAgent(prev, claude("working")).agent, prev);
  assert.deepEqual(nextAgent(prev, claude("working", "claude --resume b-2")).agent, claude("working", "claude --resume b-2"));
  assert.deepEqual(nextAgent(prev, claude("idle")).agent, claude("idle"));
});

test("first: a newly seen agent that reports its state", () => {
  assert.equal(nextAgent(undefined, claude("working")).first, true);
  assert.equal(nextAgent(claude("idle"), claude("working")).first, false);
  assert.equal(nextAgent(undefined, { name: "opencode", command: "opencode --continue", state: null }).first, false);
});

test("the save keeps a resumable agent, and the restore reads it back", () => {
  const saved = savedAgent(claude("working"));
  assert.deepEqual(saved, { name: "Claude Code", command: "claude --resume a-1" });
  assert.deepEqual(restoredAgent(JSON.parse(JSON.stringify(saved))), saved);
  assert.equal(savedAgent(claude("working", null)), undefined, "its id unread yet");
  assert.equal(savedAgent(undefined), undefined);
  assert.equal(restoredAgent(undefined), null);
  assert.equal(restoredAgent({ name: "x" }), null);
  assert.equal(restoredAgent({ name: "x", command: "" }), null);
  assert.equal(restoredAgent({ name: "x", command: "rm -rf ~\rclaude" }), null, "a key in the command");
});

test("a restored agent's command is typed for Enter, or run", () => {
  const agent = { name: "Claude Code", command: "claude --resume a-1" };
  assert.deepEqual(resumeOf(agent, "type"), { hint: "Claude Code was running here. Press Enter to resume.", run: "claude --resume a-1" });
  assert.deepEqual(resumeOf(agent, "run"), { hint: "Resuming Claude Code, which was running here.", run: "claude --resume a-1\r" });
});
