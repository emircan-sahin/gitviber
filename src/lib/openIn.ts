import { useMemo, useSyncExternalStore } from "react";
import { api, errorMessage, type OpenInApp } from "./api";
import { getSettings, type Settings, updateSettings, useSettings } from "./settings";
import { toast } from "./toast";

/** A built-in app found on this machine, or one of the user's own (with its command). */
export type OpenApp = Omit<OpenInApp, "group"> & { group: OpenInApp["group"] | "custom"; command?: string };

/** What to open: a path in the worktree ("" for the worktree), and the line an editor should show. */
export interface OpenTarget {
  path: string;
  line?: number;
}

export const GROUPS: [OpenApp["group"], string][] = [
  ["editor", "Editors"],
  ["terminal", "Terminals"],
  ["other", "Git Clients"],
  ["custom", "Your Apps"],
];

// Detection asks LaunchServices about every known app, so the last answer is kept (across
// launches too) and shown at once while a fresh one is fetched.
const KEY = "gitviber.openInApps";
let installed: OpenInApp[] = (() => {
  try {
    const list: unknown = JSON.parse(localStorage.getItem(KEY) ?? "[]");
    return Array.isArray(list) ? list : [];
  } catch {
    return [];
  }
})();
const listeners = new Set<() => void>();

export function subscribeOpenApps(l: () => void) {
  listeners.add(l);
  return () => void listeners.delete(l);
}

export function refreshOpenApps() {
  api
    .openInApps()
    .then((list) => {
      const json = JSON.stringify(list);
      if (json === JSON.stringify(installed)) return;
      installed = list;
      try {
        localStorage.setItem(KEY, json);
      } catch {
        // Detected again next time.
      }
      listeners.forEach((l) => l());
    })
    .catch(() => {});
}

function listFor(s: Settings, found: OpenInApp[]): OpenApp[] {
  const custom = s.openInCustom.filter((c) => c.name.trim() && c.command.trim()).map((c) => ({ id: c.id, name: c.name.trim(), group: "custom" as const, command: c.command }));
  return [...(s.openInHideBuiltins ? [] : found), ...custom];
}

/** Every app "Open in" offers, in menu order. */
export function openApps(s: Settings = getSettings()): OpenApp[] {
  return listFor(s, installed);
}

/** The app a click runs, while it's still offered. */
export function lastOpenApp(s: Settings = getSettings()): OpenApp | undefined {
  return s.openInApp ? openApps(s).find((a) => a.id === s.openInApp) : undefined;
}

export function useOpenApps(): { apps: OpenApp[]; last: OpenApp | undefined } {
  const found = useSyncExternalStore(subscribeOpenApps, () => installed);
  const s = useSettings();
  return useMemo(() => {
    const apps = listFor(s, found);
    return { apps, last: apps.find((a) => a.id === s.openInApp) };
  }, [found, s.openInCustom, s.openInHideBuiltins, s.openInApp]);
}

/** Opens `target` in `app`, which becomes the one a click runs. */
export function openIn(app: OpenApp, target: OpenTarget) {
  updateSettings({ openInApp: app.id });
  const run = app.command ? api.openInCustom(app.command, target.path, target.line) : api.openIn(app.id, target.path, target.line);
  run.catch((e) => toast("error", `Could not open in ${app.name}`, errorMessage(e)));
}

refreshOpenApps();
