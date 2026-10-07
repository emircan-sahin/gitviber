import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useMemo, useSyncExternalStore } from "react";
import { pty } from "../api";
import { enableNotifications } from "../app/notify";
import { toast } from "../app/toast";
import { getSettings } from "../settings";
import { readJson, writeJson } from "../storage";
import { type AgentEntry, agentsWaiting, type AgentState, byUrgency, nextAgent, type PaneAgent, shownState } from "./agentState";
import { type Look, paneLook } from "./agentLook";
import { needsYou } from "./needsYou";
import { panes, type Pane, state, subscribe, update } from "./terminals";

// The coding agents in the panes (agents.rs): looked up for the panes a session save round
// saves, at ⌘Q and after a prompt; their state comes as "agent-state" events from a watch on
// their state file, never polled. An agent that finishes or asks marks its pane as a bell would
// (needsYou).

const info = (p: Pane) => state.groups.flatMap((g) => g.panes).find((i) => i.id === p.id);

/** When each pane's agent was first seen or last changed state, for the agents list. */
const since = new Map<number, number>();

function apply(p: Pane, read: PaneAgent | null, live = false) {
  const i = info(p);
  if (!i) return;
  const { agent, note, first } = nextAgent(i.agent, read, live);
  if (!agent) since.delete(p.id);
  else {
    watchBadge();
    // Before the update: its listeners read it.
    if (agent.name !== i.agent?.name || agent.state !== i.agent.state) since.set(p.id, Date.now());
  }
  if (agent !== i.agent) update(p.id, (x) => ({ ...x, agent }));
  if (note) needsYou(p, { body: note }, agent?.state === "waiting" ? "notifyAgentWaiting" : "notifyAgentDone");
  if (first && agent) suggestNotifications(agent.name);
}

/** Looks up the agents `only` (or every pane) run now. */
export async function refreshAgents(only?: Pane[]) {
  for (const id of since.keys()) if (!panes.has(id)) since.delete(id);
  const list = (only ?? [...panes.values()]).filter((p) => p.pty !== null);
  if (!list.length) return;
  const running = await pty.agents(list.map((p) => p.pty!)).catch(() => null);
  if (!running) return;
  for (const p of list) if (panes.has(p.id) && p.pty !== null) apply(p, running[p.pty] ?? null);
}

/** The shell prompts again: the agent there exited, and isn't one to resume. */
export function agentPrompted(p: Pane) {
  if (!info(p)?.agent) return;
  apply(p, null);
  // So agents.rs stops watching its state file.
  void refreshAgents([p]);
}

try {
  listen<{ id: number; state: AgentState | null }>("agent-state", ({ payload }) => {
    const p = [...panes.values()].find((x) => x.pty === payload.id);
    const agent = p && info(p)?.agent;
    // Its state file gone: the agent with it.
    if (agent) apply(p, payload.state ? { ...agent, state: payload.state } : null, true);
  }).catch(() => {});
} catch {
  // Not in Tauri (the browser-only dev fixture).
}

const HINT_KEY = "gitviber.agentNotifyHint";

/** Once ever, and only while notifications are off: turning them on is what asks the OS. */
function suggestNotifications(name: string) {
  if (getSettings().notify || readJson(HINT_KEY, false)) return;
  writeJson(HINT_KEY, true);
  toast("info", `Get a notification when ${name} finishes?`, "While GitViber isn't the app in front. Turning them on asks your OS for permission.", {
    label: "Turn on",
    run: () => void enableNotifications(true),
  });
}

/** Each pane's look (agentLook) with its folder, for the marks outside the panel. */
const looks = () => JSON.stringify(state.groups.flatMap((g) => g.panes.map((p) => [p.cwd, paneLook(p)]).filter(([, look]) => look)));

/** [folder, look] of every pane that shows a dot, re-rendering only when one changes. */
export function usePaneLooks() {
  const key = useSyncExternalStore(subscribe, looks);
  return useMemo(() => JSON.parse(key) as [string, Look][], [key]);
}

/** Every pane's agent, in every tab: the ones that need the user first, longest waiting first within each. */
export function agentList(): AgentEntry[] {
  const list = state.groups.flatMap((g) =>
    g.panes.flatMap((p): AgentEntry[] => {
      if (!p.agent) return [];
      return [{ pane: p.id, name: p.agent.name, state: shownState(p.agent.state), unseen: !!p.needsYou, cwd: p.cwd, since: since.get(p.id) ?? 0 }];
    }),
  );
  return list.sort(byUrgency);
}

/** The pane whose agent is in conversation `id` now, if one is: resumed again, it would run twice. */
export const paneOfConversation = (id: string) => state.groups.flatMap((g) => g.panes).find((p) => p.agent?.session === id)?.id;

const listed = () => JSON.stringify(agentList());

/** agentList, re-rendering only when it changes (not on every title a program sets). */
export function useAgentList() {
  const key = useSyncExternalStore(subscribe, listed);
  return useMemo(() => JSON.parse(key) as AgentEntry[], [key]);
}

/** The Dock (and Linux launcher) badge. macOS shows a count of 0 as "0": no count clears it. */
function setBadge(n: number) {
  try {
    void getCurrentWindow()
      .setBadgeCount(n || undefined)
      .catch(() => {});
  } catch {
    // Not in Tauri (the browser-only dev fixture).
  }
}
// A reload (⇧⌘R) starts with no agent seen yet: the count the last page set mustn't stay.
setBadge(0);

let badge: number | null = null;
/** The badge counts the agents waiting for the user, from the first agent seen on. */
function watchBadge() {
  if (badge !== null) return;
  badge = 0;
  subscribe(() => {
    const n = agentsWaiting(agentList());
    if (n === badge) return;
    badge = n;
    setBadge(n);
  });
}
