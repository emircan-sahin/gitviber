import { useMemo } from "react";
import { api, type OpenInApp } from "../api";
import { REVEAL_FAILED } from "../platform";
import { getSettings, type Settings, updateSettings, useSettings } from "../settings";
import { readJson, writeJson } from "../storage";
import { createStore } from "../store";
import { failed } from "./toast";

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
const installed = createStore(readJson<OpenInApp[]>(KEY, [], Array.isArray));
export const subscribeOpenApps = installed.subscribe;

export function refreshOpenApps() {
  api
    .openInApps()
    .then((list) => {
      if (JSON.stringify(list) === JSON.stringify(installed.get())) return;
      // Not kept when storage fails: detected again next time.
      writeJson(KEY, list);
      installed.set(list);
    })
    .catch(() => {});
}

function listFor(s: Settings, found: OpenInApp[]): OpenApp[] {
  const custom = s.openInCustom.filter((c) => c.name.trim() && c.command.trim()).map((c) => ({ id: c.id, name: c.name.trim(), group: "custom" as const, command: c.command }));
  return [...(s.openInHideBuiltins ? [] : found), ...custom];
}

/** Every app "Open in" offers, in menu order. */
function openApps(s: Settings = getSettings()): OpenApp[] {
  return listFor(s, installed.get());
}

/** The app a click runs, while it's still offered. */
export function lastOpenApp(s: Settings = getSettings()): OpenApp | undefined {
  return s.openInApp ? openApps(s).find((a) => a.id === s.openInApp) : undefined;
}

export function useOpenApps(): { apps: OpenApp[]; last: OpenApp | undefined } {
  const found = installed.use();
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
  run.catch(failed(`Could not open in ${app.name}`));
}

/** Shows a file of the open repo (`path` in it) in Finder, Explorer or the file manager. */
export const revealPath = (path: string) => api.revealPath(path).catch(failed(REVEAL_FAILED));

/** Shows a project's folder (its absolute path) in Finder, Explorer or the file manager. */
export const revealProject = (path: string) => api.revealProject(path).catch(failed(REVEAL_FAILED));

refreshOpenApps();
