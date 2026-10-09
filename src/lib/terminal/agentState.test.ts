import assert from "node:assert/strict";
import { test } from "node:test";
import { type AgentEntry, agentsWaiting, byUrgency, handoffTo, nextAgent, type PaneAgent, quitStops, restoredAgent, resumeOf, savedAgents } from "./agentState.ts";

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

test("the agents list puts the ones that need the user first, and counts them for the badge", () => {
  const e = (pane: number, state: AgentEntry["state"], since: number, unseen = false): AgentEntry => ({ pane, name: "Claude Code", state, unseen, cwd: "/w", since });
  const list = [e(1, "finished", 10), e(2, "working", 30), e(3, "finished", 50, true), e(4, "waiting", 40), e(5, "running", 20), e(6, "waiting", 5)];
  assert.deepEqual(
    [...list].sort(byUrgency).map((x) => x.pane),
    [6, 4, 3, 5, 2, 1],
  );
  assert.equal(agentsWaiting(list), 3);
  assert.equal(agentsWaiting([]), 0);
});

test("the badge counts each agent once, and drops to 0 as panes close", () => {
  const e = (pane: number, state: AgentEntry["state"], unseen = false): AgentEntry => ({ pane, name: "Claude Code", state, unseen, cwd: "/w", since: 0 });
  // A question not looked at yet is both waiting and unseen: one agent, one count.
  const all = [e(1, "waiting", true), e(2, "finished", true), e(3, "working"), e(4, "running", true), e(5, "finished")];
  assert.equal(agentsWaiting(all), 3);
  for (let n = all.length; n >= 0; n--) assert.ok(agentsWaiting(all.slice(0, n)) <= n);
  assert.equal(agentsWaiting(all.slice(2, 3)), 0);
  // Many agents across tabs and projects: still one count each.
  const many = Array.from({ length: 500 }, (_, i) => e(i, i % 2 ? "waiting" : "working", i % 3 === 0));
  // A working one's news is old: its dot is working, and it isn't counted.
  assert.equal(agentsWaiting(many), many.filter((x) => x.state === "waiting").length);
});

test("a quit asks about agents mid-turn and running commands, not about agents done with their turn", () => {
  assert.deepEqual(quitStops([]), []);
  // Shells at their prompt.
  assert.deepEqual(quitStops([{ busy: false }, { busy: false, agent: claude("working") }]), []);
  // Resumed on the next run.
  assert.deepEqual(quitStops([{ busy: true, agent: claude("idle") }]), []);
  assert.deepEqual(quitStops([{ busy: true, agent: claude("working") }]), ["1 agent working"]);
  assert.deepEqual(quitStops([{ busy: true, agent: claude("waiting") }, { busy: true, agent: claude("working") }]), ["2 agents working"]);
  assert.deepEqual(quitStops([{ busy: true }]), ["1 command running"]);
  // One that doesn't say its state may be mid-turn.
  assert.deepEqual(quitStops([{ busy: true, agent: claude(null) }, { busy: true }, { busy: true, agent: claude("working") }, { busy: true, agent: claude("idle") }]), ["1 agent working", "2 commands running"]);
});

test("a quit counts each pane once, agents before commands", () => {
  const quiet = { busy: false };
  assert.deepEqual(quitStops([quiet, quiet, { busy: false, agent: claude(null) }]), []);
  // An agent asking for permission is mid-turn: quitting stops it.
  assert.deepEqual(quitStops([{ busy: true, agent: claude("waiting") }]), ["1 agent working"]);
  assert.deepEqual(quitStops([{ busy: true }, { busy: true }, { busy: true, agent: claude("working") }, { busy: true, agent: claude("working") }, { busy: true, agent: claude("working") }]), ["3 agents working", "2 commands running"]);
});

test("text for a worktree's agent goes into a free one, waits for a busy one, else starts one", () => {
  const agent = (state: PaneAgent["state"]) => ({ name: "Claude Code", state });
  assert.deepEqual(handoffTo(undefined), { to: "start" });
  assert.deepEqual(handoffTo(agent("idle")), { to: "paste" });
  assert.deepEqual(handoffTo(agent(null)), { to: "paste" });
  assert.deepEqual(handoffTo(agent("working")), { to: "wait", name: "Claude Code", working: true });
  assert.deepEqual(handoffTo(agent("waiting")), { to: "wait", name: "Claude Code", working: false });
});
