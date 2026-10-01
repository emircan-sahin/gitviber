// A pane's coding agent (agents.rs): what it is, how to resume it, and what a change of its state
// is worth telling the user. Pure, so it runs under `node --test`.

/** "working" covers Claude Code's busy and its running a shell command; "waiting" is its asking. */
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
    if (!a?.command || !a.cwd || (a.session && taken.has(a.session))) return undefined;
    if (a.session) taken.add(a.session);
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
