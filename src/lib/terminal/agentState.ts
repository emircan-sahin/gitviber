// A pane's coding agent (agents.rs): what it is, how to resume it, and what a change of its state
// is worth telling the user. Pure, so it runs under `node --test`.
import { plural } from "../format.ts";
import { isNews, lookOf, LOOKS } from "./agentLook.ts";

/** "working" is Claude Code's busy; "idle" covers its shell (the turn is over, a background shell runs on); "waiting" is its asking. */
export type AgentState = "working" | "waiting" | "idle";

export interface PaneAgent {
  /** What the user calls it ("Claude Code"). */
  name: string;
  /** What resumes its conversation; null while that can't be told. */
  command: string | null;
  /** The conversation's id, when it's read. */
  session: string | null;
  /** The folder it runs in. */
  cwd: string | null;
  /** Null for an agent that doesn't report one. */
  state: AgentState | null;
}

/**
 * What text for a worktree's agent does with the one running there (terminal/handoff.ts): goes in,
 * waits while it works or asks something (a paste would land in its dialog or the user's draft),
 * or a new session starts. One that doesn't report a state is taken as free.
 */
export type Handoff = { to: "paste" } | { to: "wait"; name: string; working: boolean } | { to: "start" };

export function handoffTo(agent: Pick<PaneAgent, "name" | "state"> | undefined): Handoff {
  if (!agent) return { to: "start" };
  if (agent.state === "working" || agent.state === "waiting") return { to: "wait", name: agent.name, working: agent.state === "working" };
  return { to: "paste" };
}

/** What the session save keeps of a pane's agent: one that can be resumed, and where it ran. */
export interface SavedAgent {
  name: string;
  command: string;
  cwd: string;
}

/** Settings → Terminal: on a restore, type the resume command at the prompt, run it, or neither. */
export type ResumeMode = "type" | "run" | "off";

/**
 * The pane's agent once `read` (null: none runs there now) is taken in, and what that's worth:
 * a `note` when it finished or stopped to ask, `first` when it's newly seen and reports its state.
 * `live`: `read` is the state file's change as it happened. A lookup's state only starts the
 * agent off: answered after a change that came since, it would undo it, and the next change
 * would be told twice. The same object back when nothing changed, so no re-render follows.
 */
export function nextAgent(prev: PaneAgent | undefined, read: PaneAgent | null, live = false): { agent: PaneAgent | undefined; note: string | null; first: boolean } {
  if (!read) return { agent: undefined, note: null, first: false };
  if (!live && prev?.name === read.name && prev.state !== null) read = { ...read, state: prev.state };
  const same = prev && prev.name === read.name && prev.command === read.command && prev.session === read.session && prev.cwd === read.cwd && prev.state === read.state;
  const agent = same ? prev : read;
  let note: string | null = null;
  if (read.state === "waiting" && prev?.state !== "waiting") note = `${read.name} is waiting for you`;
  else if (read.state === "idle" && prev?.state === "working") note = `${read.name} finished`;
  return { agent, note, first: !prev && read.state !== null };
}

/** The agents the save keeps, pane by pane: resumable ones, and of two that read the same conversation, the first. */
export function savedAgents(agents: (PaneAgent | undefined)[]): (SavedAgent | undefined)[] {
  const taken = new Set<string>();
  return agents.map((a) => {
    // Without a session id the agent continues its folder's last conversation: one pane per folder.
    const key = a?.session ?? `${a?.name}\0${a?.cwd}`;
    if (!a?.command || !a.cwd || taken.has(key)) return undefined;
    taken.add(key);
    return { name: a.name, command: a.command, cwd: a.cwd };
  });
}

/** A saved agent as read back from storage, which an older or broken save may hold anything in. */
export function restoredAgent(raw: unknown): SavedAgent | null {
  if (!raw || typeof raw !== "object") return null;
  const { name, command, cwd } = raw as Record<string, unknown>;
  return typeof name === "string" && typeof command === "string" && typeof cwd === "string" && command && !/[\x00-\x1f\x7f]/.test(command) ? { name, command, cwd } : null;
}

/** The dim line a restored pane shows under its history, and what's typed at its first prompt. */
export function resumeOf(agent: SavedAgent, mode: Exclude<ResumeMode, "off">) {
  return mode === "run"
    ? { hint: `Resuming ${agent.name}, which was running here.`, run: `${agent.command}\r` }
    : { hint: `${agent.name} was running here. Press Enter to resume.`, run: agent.command };
}

/** An agent in the agents list: `waiting` it asked, `finished` it's done (idle), `running` it doesn't say. */
export interface AgentEntry {
  pane: number;
  name: string;
  state: "working" | "waiting" | "finished" | "running";
  /** Its pane needs the user (needsYou): a finish or a question not looked at yet. */
  unseen: boolean;
  /** Where its pane opened, which decides its worktree. */
  cwd: string;
  /** Since when it's been in `state` (ms); 0 when not known. */
  since: number;
}

// In the order of their dots (agentLook), so the list, its button and the badge can't disagree.
// An agent that doesn't say its state ranks with the working ones.
const rank = (e: AgentEntry) => LOOKS.indexOf(lookOf(e.state, e.unseen) ?? "working");

/** The agents that need the user first, then the working ones, then the finished; the longest in its state first. */
export const byUrgency = (a: AgentEntry, b: AgentEntry) => rank(a) - rank(b) || a.since - b.since;

/** How many agents wait for the user, for the Dock badge: the ones whose dot is a question or news. */
export const agentsWaiting = (list: AgentEntry[]) => list.filter((e) => isNews(lookOf(e.state, e.unseen))).length;

/**
 * What quitting stops that's worth asking about, from each pane's `busy` (a program in the
 * foreground, pty.busy) and agent: an agent mid-turn, or a command. Not an agent done with its
 * turn, which the next run resumes; one that doesn't say its state counts as a command.
 */
export function quitStops(panes: { busy: boolean; agent?: PaneAgent }[]): string[] {
  let agents = 0;
  let commands = 0;
  for (const { busy, agent } of panes) {
    if (!busy || agent?.state === "idle") continue;
    if (agent?.state) agents++;
    else commands++;
  }
  const stops: string[] = [];
  if (agents) stops.push(`${plural(agents, "agent")} working`);
  if (commands) stops.push(`${plural(commands, "command")} running`);
  return stops;
}
