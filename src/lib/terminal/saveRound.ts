// Which panes' history the session save serializes in a round. Pure, so it runs under `node --test`.

/** How often the session save runs while something changed. */
export const SAVE_MS = 2000;
// Serializing a pane's history is 7-14 ms (#97), and an agent's spinner keeps its pane printing:
// saved every round, three agents stalled scrolling 20-40 ms every 2 s. A printing pane is now
// serialized once it stops, else every 30 s, and one pane a round, so a round stays under a frame.
const STALE_MS = 30_000;

export interface SaveState {
  /** Output or a resize since it was last serialized. */
  dirty: boolean;
  wroteAt: number;
  serializedAt: number;
}

/** The panes to serialize now: `all` (the window out of sight, where a stall goes unseen), or the stalest one due. */
export function dueForSave<T extends SaveState>(panes: Iterable<T>, now: number, all: boolean): T[] {
  const due = [...panes].filter((p) => p.dirty && (all || now - p.wroteAt >= SAVE_MS || now - p.serializedAt >= STALE_MS));
  return all ? due : due.sort((a, b) => a.serializedAt - b.serializedAt).slice(0, 1);
}
