import assert from "node:assert/strict";
import { test } from "node:test";
import { isNews, type Look, LOOKS, mostUrgent, paneLook, shownState } from "./agentLook.ts";
import { type AgentEntry, agentsWaiting, type AgentState, byUrgency, nextAgent, type PaneAgent } from "./agentState.ts";

// 10,000 random events across 50 panes in 10 tabs, applied as agents.ts and needsYou.ts apply
// them: an agent's state file changing, a bell, the pane looked at, the agent exiting or starting.

function rng(seed: number) {
  return () => {
    seed = (seed + 0x6d2b79f5) | 0;
    let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

interface Sim {
  id: number;
  group: number;
  cwd: string;
  agent?: PaneAgent;
  needsYou: boolean;
}

const STATES: (AgentState | null)[] = ["working", "waiting", "idle", null];
const agent = (state: AgentState | null): PaneAgent => ({ name: "Claude Code", command: "claude --resume a-1", session: "a-1", cwd: "/w", state });

/** agentList's entries, without its `since`. */
const entries = (panes: Sim[]): AgentEntry[] =>
  panes.filter((p) => p.agent).map((p) => ({ pane: p.id, name: p.agent!.name, state: shownState(p.agent!.state), unseen: p.needsYou, cwd: p.cwd, since: 0 }));

function run(seed: number, check: (panes: Sim[]) => void) {
  const r = rng(seed);
  const pick = <T>(xs: readonly T[]) => xs[Math.floor(r() * xs.length)];
  const panes: Sim[] = Array.from({ length: 50 }, (_, id) => ({ id, group: id % 10, cwd: `/repo/wt${id % 4}`, needsYou: false }));
  for (let step = 0; step < 10_000; step++) {
    const p = pick(panes);
    // Looked at while it happens: no mark (needsYou's focus check).
    const focused = r() < 0.2;
    const ev = r();
    if (ev < 0.5 && p.agent) {
      const { agent: next, note } = nextAgent(p.agent, { ...p.agent, state: pick(STATES) }, true);
      p.agent = next;
      if (note && !focused) p.needsYou = true;
    } else if (ev < 0.6) {
      if (!focused) p.needsYou = true;
    } else if (ev < 0.8) {
      p.needsYou = false;
    } else if (ev < 0.9) {
      p.agent = nextAgent(p.agent, null).agent;
    } else if (!p.agent) {
      p.agent = nextAgent(undefined, agent(pick(STATES))).agent;
    }
    check(panes);
  }
}

test("a tab's and a worktree's dot is never calmer than any of its panes", () => {
  run(1, (panes) => {
    const looks = panes.map(paneLook);
    for (let g = 0; g < 10; g++) {
      const mine = panes.filter((p) => p.group === g).map(paneLook);
      const tab = mostUrgent(mine);
      assert.equal(tab === null, mine.every((l) => l === null));
      for (const l of mine) if (l) assert.ok(LOOKS.indexOf(tab!) <= LOOKS.indexOf(l));
    }
    // The worktree picker folds pane by pane (looksIn): the same as all at once.
    const byWorktree = new Map<string, Look>();
    panes.forEach((p, i) => {
      if (looks[i]) byWorktree.set(p.cwd, mostUrgent([byWorktree.get(p.cwd), looks[i]])!);
    });
    for (const [cwd, l] of byWorktree) assert.equal(l, mostUrgent(panes.filter((p) => p.cwd === cwd).map(paneLook)));
  });
});

test("the Dock badge counts the agents whose dot is a question or news", () => {
  run(2, (panes) => {
    const shown = panes.filter((p) => p.agent && isNews(paneLook(p)));
    const counted = entries(panes).filter((e) => agentsWaiting([e]) === 1);
    assert.deepEqual(
      counted.map((e) => e.pane),
      shown.map((p) => p.id),
      `badge ${agentsWaiting(entries(panes))}, dots ${panes.filter((p) => p.agent).map((p) => `${p.id}:${p.agent!.state}/${p.needsYou}`).join(" ")}`,
    );
  });
});

test("the agents list puts every row with news above every row without", () => {
  run(3, (panes) => {
    const looks = new Map(panes.map((p) => [p.id, paneLook(p)]));
    const sorted = entries(panes).sort(byUrgency);
    const news = sorted.map((e) => isNews(looks.get(e.pane)!));
    assert.ok(news.indexOf(false) === -1 || news.lastIndexOf(true) < news.indexOf(false), sorted.map((e) => `${e.state}/${e.unseen}`).join(" "));
  });
});
