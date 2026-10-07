// The dot a pane, a tab, a worktree or a row of the agents list shows (StatusDot): one mapping, so
// every place reads the same. Pure, so it runs under `node --test`.
import type { AgentEntry, AgentState, PaneAgent } from "./agentState.ts";

/** Most urgent first: it asked, news not looked at, working, done and looked at. */
export const LOOKS = ["needs", "unread", "working", "done"] as const;
export type Look = (typeof LOOKS)[number];

/** For a screen reader and a tooltip: the color is never the only telling. */
export const LOOK_LABEL: Record<Look, string> = { needs: "Needs you", unread: "Not viewed", working: "Agent working", done: "Agent finished" };

/** LOOK_LABEL with the agent's name in its own looks: "Claude Code working". */
export const lookLabel = (look: Look, agent?: string) => LOOK_LABEL[look].replace(/^Agent/, agent ?? "Agent");

/**
 * `state` as the agents list says it (undefined: no agent), `unseen` its pane's needsYou. A
 * question outranks the news, and new work makes it old; news from a terminal with no agent, or
 * one that doesn't say its state, can't be told apart from a finish.
 */
export function lookOf(state: AgentEntry["state"] | undefined, unseen: boolean): Look | null {
  if (state === "waiting") return "needs";
  if (state === "working") return "working";
  if (unseen) return "unread";
  return state === "finished" ? "done" : null;
}

/** A state as the agents list says it: idle is finished, an agent that doesn't say is running. */
export const shownState = (s: AgentState | null): AgentEntry["state"] => (s === "idle" ? "finished" : (s ?? "running"));

/** A pane's look, from its agent's state file and its needsYou. */
export const paneLook = (p: { agent?: PaneAgent; needsYou?: boolean }) => lookOf(p.agent && shownState(p.agent.state), !!p.needsYou);

/** The most urgent of several panes' looks: a tab's, a worktree's. */
export const mostUrgent = (looks: (Look | null | undefined)[]): Look | null => LOOKS.find((l) => looks.includes(l)) ?? null;

/** News for the user: what a closed panel's button, or another worktree's mark, tells. */
export const isNews = (look: Look | null): look is "needs" | "unread" => look === "needs" || look === "unread";
