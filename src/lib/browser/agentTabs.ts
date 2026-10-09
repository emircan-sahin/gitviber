import { useEffect } from "react";
import { listenHere } from "../app/settingsWindow";
import type { Selection } from "../repo/selection";
import { createStore } from "../store";
import { paneOfPty } from "../terminal/terminals";
import type { DeviceChoice } from "./devices";

// Tabs an agent opened with `gitviber browser` (browser/macos/agent.rs), and the devices it showed
// them as, for the workspace they belong to: taken in once it's open, never made the active tab.

type BrowserSelection = Extract<Selection, { kind: "browser" }>;

const opened = createStore<readonly { root: string; sel: BrowserSelection }[]>([]);
listenHere<{ id: string; url: string; root: string; pty: number }>("browser-open", ({ payload: { id, url, root, pty } }) => {
  // The pane's own folder is the one its workspace goes by; the backend's is found from the command's.
  const at = paneOfPty(pty)?.cwd ?? root;
  opened.set([...opened.get(), { root: at, sel: { kind: "browser", id, url, agent: { pty } } }]);
}).catch(() => {});

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
  const pane = paneOfPty(pty);
  return `${page}\nOpened by the agent in ${pane ? `“${pane.label}”` : "a terminal pane since closed"}`;
}
