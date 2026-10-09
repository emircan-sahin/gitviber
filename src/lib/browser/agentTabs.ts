import { useEffect } from "react";
import { listenHere } from "../app/settingsWindow";
import type { Selection } from "../repo/selection";
import { createStore } from "../store";
import { paneLabel } from "../terminal/terminals";
import type { DeviceChoice } from "./devices";
import type { Scheme } from "./look";

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

// What agents changed of their tabs' looks, by tab id, until the tab takes it in: a device, or
// light or dark (null: back to none).
type Asked = { device?: DeviceChoice | null; scheme?: Scheme | null };
const asked = createStore<ReadonlyMap<string, Asked>>(new Map());
const ask = (id: string, change: Asked) => asked.set(new Map(asked.get()).set(id, { ...asked.get().get(id), ...change }));
listenHere<{ id: string; device: DeviceChoice | null }>("browser-device", ({ payload: { id, device } }) => ask(id, { device })).catch(() => {});
listenHere<{ id: string; dark: boolean | null }>("browser-appearance", ({ payload: { id, dark } }) => ask(id, { scheme: dark === null ? null : dark ? "dark" : "light" })).catch(() => {});

/** `root`'s agent tabs added as they come, and its tabs' devices and schemes as their agents choose them. */
export function useAgentTabs(root: string, tabs: readonly { key: string; sel: Selection }[], add: (sel: Selection) => void, update: (key: string, sel: Selection) => void) {
  const waiting = opened.use();
  useEffect(() => {
    const mine = waiting.filter((o) => o.root === root);
    if (!mine.length) return;
    opened.set(opened.get().filter((o) => o.root !== root));
    for (const o of mine) add(o.sel);
  }, [waiting, root, add]);

  const chosen = asked.use();
  useEffect(() => {
    const next = new Map(chosen);
    for (const { key, sel } of tabs) {
      const change = sel.kind === "browser" && next.get(sel.id);
      if (!change || sel.kind !== "browser") continue;
      next.delete(sel.id);
      const { device, scheme, ...rest } = { ...sel, ...change };
      update(key, { ...rest, ...(device && { device }), ...(scheme && { scheme }) });
    }
    if (next.size !== chosen.size) asked.set(next);
  }, [chosen, tabs, update]);
}

/** What an agent's tab says it is, by the pane whose agent opened it. */
export function agentTabTitle(pty: number, page: string): string {
  const pane = paneLabel(pty);
  return `${page}\nOpened by the agent in ${pane ? `“${pane}”` : "a terminal pane since closed"}`;
}
