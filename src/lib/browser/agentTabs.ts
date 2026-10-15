import { useEffect, useMemo } from "react";
import { listenHere } from "../app/settingsWindow";
import type { Selection } from "../repo/selection";
import { useSettings } from "../settings";
import { createStore } from "../store";
import { paneLabel } from "../terminal/terminals";
import type { DeviceChoice } from "./devices";
import { hiddenAgentIds } from "./agentShown";
import type { Scheme } from "./look";

// Tabs an agent opened with `gitviber browser` (browser/macos/agent.rs), and the devices it showed
// them as, for the workspace they belong to (`root`: the folder its pane opened in, as pty.rs
// keeps it): taken in once it's open, never made the active tab, unless it was opened to show
// the user (`tab new --show`).

type BrowserSelection = Extract<Selection, { kind: "browser" }>;
type Tab = { key: string; sel: Selection };
const isBrowser = (t: Tab): t is { key: string; sel: BrowserSelection } => t.sel.kind === "browser";

const opened = createStore<readonly { root: string; show: boolean; sel: BrowserSelection }[]>([]);
// Whose each is, by tab id, for as long as this page lives: the backend sends a pane's commands
// to its tabs until then (browser/control.rs Routes), and a reload starts them afresh.
const owners = createStore<ReadonlyMap<string, number>>(new Map());
// Tabs opened to show the user, by id, with the pty that did: the user's own until they give one
// back, and so no agent's here (no Bot, never hidden) until then.
const shown = createStore<ReadonlyMap<string, number>>(new Map());
listenHere<{ id: string; url: string; root: string; pty: number; show: boolean }>("browser-open", ({ payload: { id, url, root, pty, show } }) => {
  if (show) shown.set(new Map(shown.get()).set(id, pty));
  else owners.set(new Map(owners.get()).set(id, pty));
  opened.set([...opened.get(), { root, show, sel: { kind: "browser", id, url } }]);
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
  drop(shown);
  for (const ids of [taken, released]) {
    if (ids.get().has(id)) {
      const left = new Set(ids.get());
      left.delete(id);
      ids.set(left);
    }
  }
  if (shut.get().has(id)) {
    const left = new Set(shut.get());
    left.delete(id);
    shut.set(left);
  }
}

/** The terminal (pty) whose agent opened tab `id` for the user, while it is theirs alone; undefined for any other. */
export const useShownPty = (id: string) => shown.use().get(id);

/** The terminal (pty) whose agent opened tab `id`; undefined for the user's own tabs. */
export const useAgentPty = (id: string) => owners.use().get(id);

// Agents' tabs the user took with a press or a key of their own (browser/control.rs Owner): their
// agents only read them until the user gives them back.
const taken = createStore<ReadonlySet<string>>(new Set());
listenHere<{ id: string; user: boolean }>("browser-owner", ({ payload: { id, user } }) => {
  const next = new Set(taken.get());
  if (user) next.add(id);
  else next.delete(id);
  taken.set(next);
  // Given back, a tab shown to the user is an agent's like any other.
  const pty = shown.get().get(id);
  if (!user && pty !== undefined) {
    const left = new Map(shown.get());
    left.delete(id);
    shown.set(left);
    owners.set(new Map(owners.get()).set(id, pty));
  }
}).catch(() => {});

/** Whether the user has taken agent tab `id` from its agents. */
export const useTakenFromAgent = (id: string) => taken.use().has(id);

// Agents' tabs whose pane closed: the user's now, though they still say who opened them.
const released = createStore<ReadonlySet<string>>(new Set());
listenHere<{ ids: string[] }>("browser-released", ({ payload: { ids } }) => released.set(new Set([...released.get(), ...ids]))).catch(() => {});

/** The agent tabs the tab strip leaves out: all of them while Settings has them hidden, except those the user has. */
export function useHiddenAgentTabs(): ReadonlySet<string> {
  const show = useSettings().browserShowAgentTabs;
  const own = owners.use();
  const mine = taken.use();
  const freed = released.use();
  return useMemo(() => hiddenAgentIds(own, new Set([...mine, ...freed]), show), [own, mine, freed, show]);
}

// What agents changed of their tabs' looks, by tab id, until the tab takes it in: a device, or
// light or dark (null: back to none).
type Asked = { device?: DeviceChoice | null; scheme?: Scheme | null };
const asked = createStore<ReadonlyMap<string, Asked>>(new Map());
const ask = (id: string, change: Asked) => asked.set(new Map(asked.get()).set(id, { ...asked.get().get(id), ...change }));
listenHere<{ id: string; device: DeviceChoice | null }>("browser-device", ({ payload: { id, device } }) => ask(id, { device })).catch(() => {});
listenHere<{ id: string; dark: boolean | null }>("browser-appearance", ({ payload: { id, dark } }) => ask(id, { scheme: dark === null ? null : dark ? "dark" : "light" })).catch(() => {});

/**
 * `root`'s agent tabs added and closed as their agents say (`discard`: closed for good, no ⇧⌘T), and its tabs' devices and schemes as
 * they choose them. `add` leaves the active tab alone; `show` makes one opened for the user the active one.
 */
export function useAgentTabs(
  root: string,
  tabs: readonly Tab[],
  add: (sel: Selection) => void,
  show: (sel: Selection) => void,
  update: (key: string, sel: Selection) => void,
  discard: (keys: string[]) => void,
) {
  const waiting = opened.use();
  useEffect(() => {
    const mine = waiting.filter((o) => o.root === root);
    if (!mine.length) return;
    opened.set(opened.get().filter((o) => o.root !== root));
    // `show`: opened for the user, who sees it at once.
    for (const o of mine) (o.show ? show : add)(o.sel);
  }, [waiting, root, add, show]);

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

/** What a page opened for the user says it is, by the pane whose agent opened it. */
export function shownTabTitle(pty: number, page: string): string {
  const pane = paneLabel(pty);
  return `${page}\nOpened for you by the terminal in ${pane ? `“${pane}”` : "a pane since closed"}`;
}

/** What an agent's tab says it is, by the pane whose agent opened it. */
export function agentTabTitle(pty: number, page: string): string {
  const pane = paneLabel(pty);
  return `${page}\nOpened by the agent in ${pane ? `“${pane}”` : "a terminal pane since closed"}`;
}
