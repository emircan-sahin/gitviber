import { useEffect } from "react";
import { listenHere } from "../app/settingsWindow";
import type { Selection } from "../repo/selection";
import { createStore } from "../store";
import { paneLabel } from "../terminal/terminals";
import type { DeviceChoice } from "./devices";

// Tabs an agent opened with `gitviber browser` (browser/macos/agent.rs), and the devices it showed
// them as, for the workspace they belong to (`root`: the folder its pane opened in, as pty.rs
// keeps it): taken in once it's open, never made the active tab.

type BrowserSelection = Extract<Selection, { kind: "browser" }>;

const opened = createStore<readonly { root: string; sel: BrowserSelection }[]>([]);
// Whose each is, by tab id, for as long as this page lives: the backend sends a pane's commands
// to its tab until then (browser/control.rs Routes), and a reload starts them afresh.
const owners = createStore<ReadonlyMap<string, number>>(new Map());
listenHere<{ id: string; url: string; root: string; pty: number }>("browser-open", ({ payload: { id, url, root, pty } }) => {
  owners.set(new Map(owners.get()).set(id, pty));
  opened.set([...opened.get(), { root, sel: { kind: "browser", id, url } }]);
}).catch(() => {});

/** The terminal (pty) whose agent opened tab `id`; undefined for the user's own tabs. */
export const useAgentPty = (id: string) => owners.use().get(id);

const devices = createStore<ReadonlyMap<string, DeviceChoice | null>>(new Map());
listenHere<{ id: string; device: DeviceChoice | null }>("browser-device", ({ payload: { id, device } }) => devices.set(new Map(devices.get()).set(id, device))).catch(() => {});

/** `root`'s agent tabs added as they come, and its tabs' devices as their agents choose them. */
export function useAgentTabs(root: string, tabs: readonly { key: string; sel: Selection }[], add: (sel: Selection) => void, update: (key: string, sel: Selection) => void) {
  const waiting = opened.use();
  useEffect(() => {
    const mine = waiting.filter((o) => o.root === root);
    if (!mine.length) return;
    opened.set(opened.get().filter((o) => o.root !== root));
    for (const o of mine) add(o.sel);
  }, [waiting, root, add]);

  const chosen = devices.use();
  useEffect(() => {
    const next = new Map(chosen);
    for (const { key, sel } of tabs) {
      if (sel.kind !== "browser" || !next.has(sel.id)) continue;
      const device = next.get(sel.id);
      next.delete(sel.id);
      const { device: _, ...rest } = sel;
      update(key, device ? { ...rest, device } : rest);
    }
    if (next.size !== chosen.size) devices.set(next);
  }, [chosen, tabs, update]);
}

/** What an agent's tab says it is, by the pane whose agent opened it. */
export function agentTabTitle(pty: number, page: string): string {
  const pane = paneLabel(pty);
  return `${page}\nOpened by the agent in ${pane ? `“${pane}”` : "a terminal pane since closed"}`;
}
