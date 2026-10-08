import { browserApi, type ListeningPort } from "../api";
import { createStore } from "../store";
import { onCommandStarted } from "../terminal/terminals";

// What the terminals' programs listen on (browser/ports.rs), by terminal session: read when a
// ports menu opens, and a little after a pane starts a command, as a dev server takes a moment
// to listen. Never on a timer of its own.

const found = createStore<ReadonlyMap<number, ListeningPort[]>>(new Map());

/** Reads again what the sessions `ptys` listen on. */
export async function scanPorts(ptys: number[]) {
  if (!ptys.length) return;
  // A read a session: each keeps its own, for the menu of its pane.
  const read = await Promise.all(ptys.map((pty) => browserApi.ports([pty]).catch(() => null)));
  const next = new Map(found.get());
  ptys.forEach((pty, i) => read[i] && next.set(pty, read[i]));
  found.set(next);
}

/** What the sessions `ptys` were last seen listening on, each port once, the lowest first. */
export function usePorts(ptys: number[]): ListeningPort[] {
  const all = found.use();
  const ports = new Map<number, ListeningPort>();
  for (const pty of ptys) for (const p of all.get(pty) ?? []) ports.set(p.port, p);
  return [...ports.values()].sort((a, b) => a.port - b.port);
}

/** A port's page on this machine. */
export const portUrl = (port: number) => `http://localhost:${port}/`;

/** After a command starts: a dev server is usually listening within seconds. */
const AFTER_START_MS = [3000, 10_000];

onCommandStarted((pty) => {
  for (const ms of AFTER_START_MS) setTimeout(() => void scanPorts([pty]), ms);
});
