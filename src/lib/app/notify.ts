// Desktop notifications, when the user turned them on (Settings → Notifications): a network command
// that ends while the app is in the background says so, as GitHub Desktop does, and so does a
// terminal that rings or sends a notification (lib/terminal/needsYou), as Ghostty and iTerm2 do, or
// whose agent finishes or asks. notifications.rs posts them, asking macOS first.
import { api, errorMessage, type NotifyPermission } from "../api";
import { getSettings, type NotifyEvent, updateSettings } from "../settings";
import { createStore } from "../store";
import { failed, toast } from "./toast";

// Null until asked: Settings asks when it shows, and every return to the window asks again, as the
// user may have changed it in System Settings.
const permission = createStore<NotifyPermission | null>(null);
export const useNotifyPermission = permission.use;

export function refreshNotifyPermission() {
  api.notificationPermission().then(permission.set, () => {});
}

// True while macOS's prompt is open: the switch waits for the answer.
const asking = createStore(false);
export const useAskingNotify = asking.use;
// Each flip of the switch, or Send Test; an answer to an older one is let go.
let turns = 0;
// The toast about a permission taken back, once.
let toldDenied = false;

export const openNotificationSettings = () => void api.notificationSettings().catch(failed("Could not open System Settings"));
const openSettingsAction = { label: "Open System Settings", run: openNotificationSettings };

/** Whether one can show now: granted, or a dev build that can't be asked. */
const allowed = (state: NotifyPermission | null) => state === "granted" || state === "unbundled";

/**
 * Whether macOS lets them show, asking it first when it hasn't been. When it says no, System
 * Settings opens at GitViber's notifications, the one place that can change that (as MonoCode).
 */
async function ensurePermission(turn: number) {
  try {
    let state = await api.notificationPermission();
    if (state === "prompt" && turn === turns) {
      asking.set(true);
      state = await api.requestNotifications();
    }
    if (turn !== turns) return false;
    permission.set(state);
    if (state === "denied") {
      // Sent there just now: coming back isn't news.
      toldDenied = true;
      openNotificationSettings();
    }
    else if (state === "prompt") toast("info", "macOS is still asking", "Answer its prompt, then try again.");
    return allowed(state);
  } catch (e) {
    if (turn === turns) toast("error", "Could not ask macOS about notifications", errorMessage(e));
    return false;
  } finally {
    if (turn === turns) asking.set(false);
  }
}

/**
 * The switch is the user's wish, kept as they set it: on, macOS is asked if it hasn't been, and
 * a "no" opens System Settings, with "Permission needed" beside the switch until it's given. Each
 * return to the window asks again (checkPermission), so allowing it there is all it takes.
 */
export async function enableNotifications(on: boolean) {
  const turn = ++turns;
  updateSettings({ notify: on });
  if (on) await ensurePermission(turn);
  else asking.set(false);
}

/**
 * Tells the user `title`, if they asked to be told of `event` and aren't looking at the app.
 * `target`: what a click on it shows ("pane:<id>", needsYou). True when it was sent.
 */
export function notifyIfAway(event: NotifyEvent, title: string, body?: string, target?: string) {
  const s = getSettings();
  const state = permission.get();
  if (!s.notify || !s[event] || document.hasFocus() || (state !== null && !allowed(state))) return false;
  // Permission taken back in the OS: nothing to show then.
  api.notify(title, body ?? "", target).catch(() => {});
  return true;
}

/** From Settings, so shown while the app is in front. Without permission it goes the switch's way: ask, or System Settings. */
export async function sendTestNotification() {
  if (!(await ensurePermission(++turns))) return;
  api.notify("GitViber", "Notifications are on. Click one to come back here.").catch(failed("Could not send a notification"));
}

/**
 * What macOS says now, at launch and on each return to the window. With the switch on, a
 * permission taken back in System Settings would drop every notification unseen: that's said,
 * once. At launch only (a prompt open now is the switch's own): up to 0.1.7 macOS was never
 * asked, so a switch turned on then has no permission behind it; off it goes, with a way back.
 */
function checkPermission(launch = false) {
  api
    .notificationPermission()
    .then((state) => {
      permission.set(state);
      if (!getSettings().notify) return;
      if (state === "prompt" && launch) {
        updateSettings({ notify: false });
        toast("info", "Notifications need your permission", "GitViber now asks macOS before it shows any.", { label: "Turn on", run: () => void enableNotifications(true) });
      } else if (state === "denied" && !toldDenied) {
        toldDenied = true;
        toast("info", "Notifications are off for GitViber", "System Settings → Notifications doesn't let them show.", openSettingsAction);
      }
    })
    .catch(() => {});
}

checkPermission(true);
window.addEventListener("focus", () => checkPermission());
