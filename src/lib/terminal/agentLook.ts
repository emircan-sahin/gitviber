// The dot a pane, a tab, a worktree or a row of the agents list shows (StatusDot): one mapping, so
// every place reads the same. Pure, so it runs under `node --test`.
import { type AgentEntry, type PaneAgent, shownState } from "./agentState.ts";

/** Most urgent first: it asked, news not looked at, working, done and looked at. */
export const LOOKS = ["needs", "unread", "working", "done"] as const;
export type Look = (typeof LOOKS)[number];

/** For a screen reader and a tooltip: the color is never the only telling. */
export const LOOK_LABEL: Record<Look, string> = { needs: "Needs you", unread: "Finished, not viewed", working: "Agent working", done: "Agent finished" };

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

/** A pane's look, from its agent's state file and its needsYou. */
export const paneLook = (p: { agent?: PaneAgent; needsYou?: boolean }) => lookOf(p.agent && shownState(p.agent.state), !!p.needsYou);

/** The most urgent of several panes' looks: a tab's, a worktree's. */
export const mostUrgent = (looks: (Look | null | undefined)[]): Look | null => LOOKS.find((l) => looks.includes(l)) ?? null;

/** News for the user: what a closed panel's button, or another worktree's mark, tells. */
export const isNews = (look: Look | null): look is "needs" | "unread" => look === "needs" || look === "unread";
