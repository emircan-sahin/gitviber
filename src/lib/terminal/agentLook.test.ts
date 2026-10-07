import assert from "node:assert/strict";
import { test } from "node:test";
import { isNews, lookLabel, LOOKS, lookOf, mostUrgent, paneLook, shownState } from "./agentLook.ts";
import { agentsWaiting, nextAgent, type PaneAgent } from "./agentState.ts";

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

test("every agent state, with and without unseen news, and no agent at all", () => {
  // [state file, needsYou, look]
  const table: [PaneAgent["state"] | "none", boolean, ReturnType<typeof paneLook>][] = [
    ["none", false, null],
    ["none", true, "unread"],
    [null, false, null],
    [null, true, "unread"],
    ["working", false, "working"],
    ["working", true, "working"],
    ["waiting", false, "needs"],
    ["waiting", true, "needs"],
    ["idle", false, "done"],
    ["idle", true, "unread"],
  ];
  for (const [s, needsYou, look] of table) {
    const pane = s === "none" ? { needsYou } : { agent: claude(s), needsYou };
    assert.equal(paneLook(pane), look, `${s} ${needsYou}`);
    // The agents list reads the same pane the same way.
    if (s !== "none") assert.equal(lookOf(shownState(s), needsYou), look, `list ${s} ${needsYou}`);
  }
});

test("the most urgent look is the first in LOOKS present, whatever the order or the gaps", () => {
  const all = [...LOOKS, null, undefined];
  // Every multiset of up to three looks, in every order.
  for (const a of all)
    for (const b of all)
      for (const c of all) {
        const looks = [a, b, c];
        const want = LOOKS.find((l) => looks.includes(l)) ?? null;
        assert.equal(mostUrgent(looks), want, String(looks));
        for (const l of looks) if (l) assert.ok(LOOKS.indexOf(mostUrgent(looks)!) <= LOOKS.indexOf(l));
      }
});

test("an agent its background shell wakes before its finish is looked at is counted as its dot shows", () => {
  // busy → shell (finished, marked unseen in a pane out of sight) → the shell exits and wakes it.
  const pane = { agent: claude("working"), needsYou: false };
  pane.agent = nextAgent(pane.agent, claude("idle"), true).agent!;
  pane.needsYou = true;
  pane.agent = nextAgent(pane.agent, claude("working"), true).agent!;
  assert.equal(paneLook(pane), "working");
  const entry = { pane: 1, name: "Claude Code", state: shownState(pane.agent.state), unseen: pane.needsYou, cwd: "/w", since: 0 };
  // The Dock badge, the Agents button's summary and the list's order all read this one.
  assert.equal(agentsWaiting([entry]), isNews(paneLook(pane)) ? 1 : 0);
});

test("a label names the agent only in the agent's own looks", () => {
  assert.equal(lookLabel("working", "Claude Code"), "Claude Code working");
  assert.equal(lookLabel("done", "Claude Code"), "Claude Code finished");
  assert.equal(lookLabel("unread", "Claude Code"), "Not viewed");
  assert.equal(lookLabel("needs", "Claude Code"), "Needs you");
  assert.equal(lookLabel("working"), "Agent working");
});
