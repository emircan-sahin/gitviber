import type { JournalEntry, RepoStatus } from "../api/types.ts";

type Step = Pick<JournalEntry, "switchTo">;

/** The branch undoing (or redoing) `steps` in order leaves checked out, if one of them switches. */
export const landing = (steps: Step[]) => steps.reduce<string | null>((at, e) => e.switchTo ?? at, null);

/** What a tooltip adds after the action's name: where it switches to, if it does. */
export const switchNote = (forward: boolean, to: string | null) => (to ? `: switches ${forward ? "" : "back "}to ${to}` : "");

/**
 * What to ask before undoing (or redoing) `steps`, or null to go ahead. Switching carries uncommitted
 * changes along, which ⌘Z pressed in passing shouldn't do unasked; anything else just goes.
 */
export function switchQuestion(forward: boolean, steps: Step[], status: Pick<RepoStatus, "staged" | "unstaged"> | null) {
  const to = landing(steps);
  const dirty = !!status && status.staged.length + status.unstaged.length > 0;
  if (!to || !dirty) return null;
  return `This ${forward ? "redo" : "undo"} switches ${forward ? "" : "back "}to ${to}, and your uncommitted changes go along to it.`;
}
