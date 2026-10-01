import { listen } from "@tauri-apps/api/event";
import { useMemo, useSyncExternalStore } from "react";
import { pty } from "../api";
import { enableNotifications } from "../app/notify";
import { toast } from "../app/toast";
import { getSettings } from "../settings";
import { readJson, writeJson } from "../storage";
import { type AgentState, nextAgent, type PaneAgent } from "./agentState";
import { needsYou } from "./needsYou";
import { panes, type Pane, state, subscribe, update } from "./terminals";

// The coding agents in the panes (agents.rs): looked up for the panes a session save round
// saves, at ⌘Q and after a prompt; their state comes as "agent-state" events from a watch on
// their state file, never polled. An agent that finishes or asks marks its pane as a bell would
// (needsYou).

const info = (p: Pane) => state.groups.flatMap((g) => g.panes).find((i) => i.id === p.id);

function apply(p: Pane, read: PaneAgent | null, live = false) {
  const i = info(p);
  if (!i) return;
  const { agent, note, first } = nextAgent(i.agent, read, live);
  if (agent !== i.agent) update(p.id, (x) => ({ ...x, agent }));
  if (note) needsYou(p, { body: note });
  if (first && agent) suggestNotifications(agent.name);
}

/** Looks up the agents `only` (or every pane) run now. */
export async function refreshAgents(only?: Pane[]) {
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

/** Folders of the panes whose agent is working, "\0"-joined, as useNeedsYou's. */
const working = () =>
  state.groups
    .flatMap((g) => g.panes.filter((p) => p.agent?.state === "working").map((p) => p.cwd))
    .sort()
    .join("\0");

/** The folders of panes whose agent is working, for the worktree picker. */
export function useAgentsWorking() {
  const key = useSyncExternalStore(subscribe, working);
  return useMemo(() => (key ? key.split("\0") : []), [key]);
}
