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
// to its tabs until then (browser/control.rs Routes), and a reload starts them afresh.
const owners = createStore<ReadonlyMap<string, number>>(new Map());
listenHere<{ id: string; url: string; root: string; pty: number }>("browser-open", ({ payload: { id, url, root, pty } }) => {
  owners.set(new Map(owners.get()).set(id, pty));
  opened.set([...opened.get(), { root, sel: { kind: "browser", id, url } }]);
}).catch(() => {});

// Tabs their agents closed (`tab close`), by id, until their workspace closes them too.
const shut = createStore<ReadonlySet<string>>(new Set());
listenHere<{ id: string }>("browser-closed", ({ payload: { id } }) => {
  opened.set(opened.get().filter((o) => o.sel.id !== id));
  shut.set(new Set(shut.get()).add(id));
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

/** `root`'s agent tabs added and closed as their agents say, and its tabs' devices and schemes as they choose them. */
export function useAgentTabs(root: string, tabs: readonly { key: string; sel: Selection }[], add: (sel: Selection) => void, update: (key: string, sel: Selection) => void, close: (keys: string[]) => void) {
  const waiting = opened.use();
  useEffect(() => {
    const mine = waiting.filter((o) => o.root === root);
    if (!mine.length) return;
    opened.set(opened.get().filter((o) => o.root !== root));
    for (const o of mine) add(o.sel);
  }, [waiting, root, add]);

  const closing = shut.use();
  useEffect(() => {
    const gone = tabs.filter((t) => t.sel.kind === "browser" && closing.has(t.sel.id));
    if (!gone.length) return;
    const left = new Set(closing);
    for (const t of gone) if (t.sel.kind === "browser") left.delete(t.sel.id);
    shut.set(left);
    close(gone.map((t) => t.key));
  }, [closing, tabs, close]);

  const chosen = asked.use();
  useEffect(() => {
    const next = new Map(chosen);
    for (const { key, sel } of tabs) {
      if (sel.kind !== "browser") continue;
      const change = next.get(sel.id);
      if (!change) continue;
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
