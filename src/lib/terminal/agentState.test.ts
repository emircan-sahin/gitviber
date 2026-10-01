import assert from "node:assert/strict";
import { test } from "node:test";
import { nextAgent, type PaneAgent, restoredAgent, resumeOf, savedAgents } from "./agentState.ts";

const claude = (state: PaneAgent["state"], command: string | null = "claude --resume a-1", session: string | null = "a-1"): PaneAgent => ({ name: "Claude Code", command, session, cwd: "/w", state });
/** A change of state as the state file's watch tells it. */
const live = (prev: PaneAgent | undefined, read: PaneAgent | null) => nextAgent(prev, read, true);

test("an agent that finishes or stops to ask is worth a note, once", () => {
  assert.equal(live(claude("working"), claude("idle")).note, "Claude Code finished");
  assert.equal(live(claude("working"), claude("waiting")).note, "Claude Code is waiting for you");
  assert.equal(live(claude("idle"), claude("waiting")).note, "Claude Code is waiting for you");
  assert.equal(live(claude("waiting"), claude("waiting")).note, null);
  assert.equal(live(claude("idle"), claude("idle")).note, null);
  // Answered, then idle: the user was there.
  assert.equal(live(claude("waiting"), claude("idle")).note, null);
  // Seen first when already idle (a restore, a pane opened on it): it didn't just finish.
  assert.equal(nextAgent(undefined, claude("idle")).note, null);
  assert.equal(live(claude("working"), claude("working")).note, null);
});

test("a lookup answered after a change doesn't undo it, so the next change isn't told twice", () => {
  const idle = live(claude("working"), claude("idle")).agent;
  const stale = nextAgent(idle, claude("working"));
  assert.equal(stale.agent, idle);
  assert.equal(stale.note, null);
  assert.equal(live(stale.agent, claude("idle")).note, null);
  // Its id moving on still comes through a lookup.
  assert.deepEqual(nextAgent(idle, claude("working", "claude --resume b-2", "b-2")).agent, claude("idle", "claude --resume b-2", "b-2"));
});

test("a prompt (or nothing found) clears the agent, without a note", () => {
  assert.deepEqual(nextAgent(claude("working"), null), { agent: undefined, note: null, first: false });
});

test("an unchanged read keeps the same object", () => {
  const prev = claude("working");
  assert.equal(nextAgent(prev, claude("working")).agent, prev);
  assert.equal(live(prev, claude("working")).agent, prev);
  assert.deepEqual(live(prev, claude("idle")).agent, claude("idle"));
});

test("first: a newly seen agent that reports its state", () => {
  assert.equal(nextAgent(undefined, claude("working")).first, true);
  assert.equal(nextAgent(claude("idle"), claude("working")).first, false);
  assert.equal(nextAgent(undefined, { name: "opencode", command: "opencode --continue", session: null, cwd: "/w", state: null }).first, false);
});

test("the save keeps resumable agents, one per conversation, and the restore reads them back", () => {
  const [saved, twin, other, unread, none] = savedAgents([claude("working"), claude("idle"), claude("idle", "claude --resume b-2", "b-2"), claude("working", null, null), undefined]);
  assert.deepEqual(saved, { name: "Claude Code", command: "claude --resume a-1", cwd: "/w" });
  assert.equal(twin, undefined, "the same conversation in a second pane");
  assert.equal(other?.command, "claude --resume b-2");
  assert.equal(unread, undefined, "its id unread yet");
  assert.equal(none, undefined);
  // Continued rather than read: both would continue the folder's last conversation, so one per folder.
  const opencode = { name: "opencode", command: "opencode --continue", session: null, cwd: "/w", state: null };
  assert.equal(savedAgents([opencode, opencode]).filter(Boolean).length, 1);
  assert.equal(savedAgents([opencode, { ...opencode, cwd: "/v" }]).filter(Boolean).length, 2);
  assert.deepEqual(restoredAgent(JSON.parse(JSON.stringify(saved))), saved);
  assert.equal(restoredAgent(undefined), null);
  assert.equal(restoredAgent({ name: "x", command: "x" }), null, "a save from before cwd was kept");
  assert.equal(restoredAgent({ name: "x", command: "", cwd: "/w" }), null);
  assert.equal(restoredAgent({ name: "x", command: "rm -rf ~\rclaude", cwd: "/w" }), null, "a key in the command");
});

test("a restored agent's command is typed for Enter, or run", () => {
  const agent = { name: "Claude Code", command: "claude --resume a-1", cwd: "/w" };
  assert.deepEqual(resumeOf(agent, "type"), { hint: "Claude Code was running here. Press Enter to resume.", run: "claude --resume a-1" });
  assert.deepEqual(resumeOf(agent, "run"), { hint: "Resuming Claude Code, which was running here.", run: "claude --resume a-1\r" });
});
