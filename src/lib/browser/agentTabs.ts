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
type Tab = { key: string; sel: Selection };
const isBrowser = (t: Tab): t is { key: string; sel: BrowserSelection } => t.sel.kind === "browser";

const opened = createStore<readonly { root: string; sel: BrowserSelection }[]>([]);
// Whose each is, by tab id, for as long as this page lives: the backend sends a pane's commands
// to its tabs until then (browser/control.rs Routes), and a reload starts them afresh.
const owners = createStore<ReadonlyMap<string, number>>(new Map());
listenHere<{ id: string; url: string; root: string; pty: number }>("browser-open", ({ payload: { id, url, root, pty } }) => {
  owners.set(new Map(owners.get()).set(id, pty));
  opened.set([...opened.get(), { root, sel: { kind: "browser", id, url } }]);
}).catch(() => {});

// Tabs their agents closed (`tab close`, or unused for long), by id, until their workspace closes
// them too; one not taken in yet just isn't.
const shut = createStore<ReadonlySet<string>>(new Set());
listenHere<{ id: string }>("browser-closed", ({ payload: { id } }) => {
  const waiting = opened.get();
  if (waiting.some((o) => o.sel.id === id)) {
    opened.set(waiting.filter((o) => o.sel.id !== id));
    forgetAgentTab(id);
  } else shut.set(new Set(shut.get()).add(id));
}).catch(() => {});

/** A closed tab is no agent's: reopened, it's the user's (the backend forgets it too). */
export function forgetAgentTab(id: string) {
  const drop = <V>(store: ReturnType<typeof createStore<ReadonlyMap<string, V>>>) => {
    const next = new Map(store.get());
    if (next.delete(id)) store.set(next);
  };
  drop(owners);
  drop(asked);
  if (shut.get().has(id)) {
    const left = new Set(shut.get());
    left.delete(id);
    shut.set(left);
  }
}

/** The terminal (pty) whose agent opened tab `id`; undefined for the user's own tabs. */
export const useAgentPty = (id: string) => owners.use().get(id);

// What agents changed of their tabs' looks, by tab id, until the tab takes it in: a device, or
// light or dark (null: back to none).
type Asked = { device?: DeviceChoice | null; scheme?: Scheme | null };
const asked = createStore<ReadonlyMap<string, Asked>>(new Map());
const ask = (id: string, change: Asked) => asked.set(new Map(asked.get()).set(id, { ...asked.get().get(id), ...change }));
listenHere<{ id: string; device: DeviceChoice | null }>("browser-device", ({ payload: { id, device } }) => ask(id, { device })).catch(() => {});
listenHere<{ id: string; dark: boolean | null }>("browser-appearance", ({ payload: { id, dark } }) => ask(id, { scheme: dark === null ? null : dark ? "dark" : "light" })).catch(() => {});

/** `root`'s agent tabs added and closed as their agents say (`discard`: closed for good, no ⇧⌘T), and its tabs' devices and schemes as they choose them. */
export function useAgentTabs(root: string, tabs: readonly Tab[], add: (sel: Selection) => void, update: (key: string, sel: Selection) => void, discard: (keys: string[]) => void) {
  const waiting = opened.use();
  useEffect(() => {
    const mine = waiting.filter((o) => o.root === root);
    if (!mine.length) return;
    opened.set(opened.get().filter((o) => o.root !== root));
    for (const o of mine) add(o.sel);
  }, [waiting, root, add]);

  const closing = shut.use();
  useEffect(() => {
    const gone = tabs.filter(isBrowser).filter((t) => closing.has(t.sel.id));
    if (gone.length) discard(gone.map((t) => t.key));
  }, [closing, tabs, discard]);

  const chosen = asked.use();
  useEffect(() => {
    const next = new Map(chosen);
    for (const { key, sel } of tabs.filter(isBrowser)) {
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
