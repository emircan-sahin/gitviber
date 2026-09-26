import { getVersion } from "@tauri-apps/api/app";
import { ask } from "@tauri-apps/plugin-dialog";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { api, errorMessage, pty } from "../api";
import { plural } from "../format";
import { netActivity } from "../repo/netActivity";
import { getSettings } from "../settings";
import { createStore } from "../store";
import { logError } from "./errorLog";
import { toast } from "./toast";
import { checked, downloaded, due, INITIAL, isExpectedFailure, percent, type UpdateState } from "./updateState";

/**
 * GitViber's own updates (tauri-plugin-updater, set up in updates.rs): checked quietly at launch
 * and every few hours, downloaded when the user asks, installed on "Restart to Update".
 */

// A launch checks at once; if that failed (the network wasn't up yet, say), again this soon after.
const RETRY_MS = 10_000;
// Asleep, a timer doesn't run: a short tick finds a check overdue soon after waking.
const TICK_MS = 30 * 60_000;
const CHECK_EVERY_MS = 4 * 3600_000;

export const RELEASES_URL = "https://github.com/emircan-sahin/gitviber/releases/latest";

/** "install": the app replaces itself; "download": from the Releases page (.deb, .rpm); null: off (dev builds). */
export type UpdateMode = "install" | "download" | null;

const state = createStore<UpdateState>(INITIAL);
const set = (patch: Partial<UpdateState>) => state.set({ ...state.get(), ...patch });
export const useUpdates = state.use;

const mode = createStore<UpdateMode>(null);
export const useUpdateMode = mode.use;

const shown = createStore(false);
export const useUpdateShown = shown.use;
export const showUpdate = (open = true) => shown.set(open);

/** The plugin's handle on the release in `state`: what downloads and installs. */
let pending: Update | null = null;
let inflight: Promise<string | null> | null = null;

/** Resolves to the error, if any. */
async function runCheck(): Promise<string | null> {
  set({ checking: true });
  try {
    const update = await check({ timeout: 30_000 });
    const s = state.get();
    if (s.download === null) {
      pending?.close().catch(() => {});
      pending = update;
    } else update?.close().catch(() => {});
    state.set(checked(s, update && { version: update.version, notes: update.body ?? "", date: update.date ?? null }, Date.now()));
    return null;
  } catch (e) {
    set({ checking: false });
    return errorMessage(e);
  }
}

/** `manual` (the menu, Settings) says how it went; an automatic check only logs what's unexpected. */
export async function checkForUpdates(manual: boolean) {
  if (!mode.get()) return;
  inflight ??= runCheck().finally(() => (inflight = null));
  const error = await inflight;
  if (!manual) {
    if (error && !isExpectedFailure(error)) logError("updater", error);
  } else if (error) {
    toast("error", "Could not check for updates", error);
  } else if (state.get().release) {
    showUpdate();
  } else {
    const version = await getVersion().catch(() => "");
    toast("success", "You're up to date", version ? `GitViber ${version} is the newest version.` : undefined);
  }
}

export async function downloadUpdate() {
  const update = pending;
  if (!update || state.get().download !== null) return;
  set({ download: { received: 0, total: null } });
  // Events come per network chunk; the store (and the status bar) only hears of a new percent.
  let progress = state.get();
  try {
    await update.download((e) => {
      const before = percent(progress.download);
      progress = downloaded(progress, e);
      if (percent(progress.download) !== before) set({ download: progress.download });
    });
    set({ download: "ready" });
  } catch (e) {
    set({ download: null });
    toast("error", "Could not download the update", errorMessage(e));
  }
}

/** The downloaded update is in place; only the relaunch is left. */
let installed = false;

/** Installs the downloaded update and restarts into it, once the user agrees to stop what's running. */
export async function restartToUpdate() {
  const update = pending;
  // Set before the first await: a second click would install twice.
  if (!update || state.get().download !== "ready" || state.get().restarting) return;
  set({ restarting: true });
  try {
    if (!(await restartAsked())) return;
    if (!installed) {
      try {
        await update.install();
      } catch (e) {
        // The downloaded bytes stay with the update, so another click tries again.
        return toast("error", "Could not install the update", errorMessage(e));
      }
      installed = true;
    }
    await relaunch().catch((e) => toast("error", "Could not restart GitViber", `The update is installed: quit and reopen GitViber to start it.\n${errorMessage(e)}`));
  } finally {
    set({ restarting: false });
  }
}

async function restartAsked() {
  const net = netActivity();
  const terminals = await pty.busy().catch(() => 0);
  const running = [net?.label, terminals ? `${plural(terminals, "terminal")} running a command` : null].filter(Boolean);
  if (!running.length) return true;
  return ask(`Restarting GitViber stops what's still running: ${running.join(", ")}.`, { title: "Restart to update", kind: "warning", okLabel: "Restart" });
}

/** Asks how this install updates, then checks at launch and every few hours while Settings allows. */
export function startUpdates() {
  let live = true;
  let first: ReturnType<typeof setTimeout> | undefined;
  let timer: ReturnType<typeof setInterval> | undefined;
  // A failed check leaves checkedAt alone, so the next tick tries again; after one that worked,
  // ticks do nothing until the next check is due.
  const tick = () => {
    if (getSettings().autoUpdate && due(state.get().checkedAt, Date.now(), CHECK_EVERY_MS)) void checkForUpdates(false);
  };
  api.updateMode().then((m) => {
    if (!live || !m) return;
    mode.set(m);
    tick();
    first = setTimeout(tick, RETRY_MS);
    timer = setInterval(tick, TICK_MS);
  }, () => {});
  return () => {
    live = false;
    clearTimeout(first);
    clearInterval(timer);
  };
}
