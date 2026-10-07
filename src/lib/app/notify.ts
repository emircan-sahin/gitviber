// Desktop notifications, when the user turned them on (Settings → Notifications): a network command
// that ends while the app is in the background says so, as GitHub Desktop does, and so does a
// terminal that rings or sends a notification (lib/terminal/needsYou), as Ghostty and iTerm2 do, or
// whose agent finishes or asks. notifications.rs posts them, asking macOS first.
import { api, errorMessage, type NotifyPermission } from "../api";
import { getSettings, type NotifyEvent, updateSettings } from "../settings";
import { createStore } from "../store";
import { listenHere } from "./settingsWindow";
import { testPlan } from "./notifyState";
import { failed, toast } from "./toast";

// Null until read: Settings reads when it shows, and every return to the window reads again, as the
// user may have changed it in System Settings.
const permission = createStore<NotifyPermission | null>(null);
export const useNotifyPermission = permission.use;

export function refreshNotifyPermission() {
  api.notificationPermission().then(permission.set, () => {});
}

// True while macOS's prompt is open.
const asking = createStore(false);
export const useAskingNotify = asking.use;
// Each flip of the switch; an answer to an older one is let go.
let turns = 0;

// The settings window in front is the app in front too (settings_window.rs says as it changes).
let settingsFocused = false;
listenHere<boolean>("settings-window-focused", ({ payload }) => (settingsFocused = payload)).catch(() => {});

/** Whether one can show now: granted (with banners or into Notification Center only), or a dev build that can't be asked. */
const allowed = (state: NotifyPermission | null) => state === "granted" || state === "quiet" || state === "unbundled";

export const openNotificationSettings = () => void api.notificationSettings().catch(failed("Could not open System Settings"));

/** Asks macOS, which prompts once; after that it answers as the user did. Only a switch turned on or "Ask macOS" gets here. */
export async function askNotifications() {
  const turn = turns;
  asking.set(true);
  try {
    const state = await api.requestNotifications();
    if (turn === turns) permission.set(state);
  } catch (e) {
    toast("error", "Could not ask macOS about notifications", errorMessage(e));
    refreshNotifyPermission();
  } finally {
    asking.set(false);
  }
}

/**
 * The switch is the user's wish, kept as they set it. Off touches nothing in macOS. On asks it if
 * it hasn't been; where that leaves them is said under the switch (Notifications.tsx), which stays
 * until the permission is given: each return to the window reads it again.
 */
export async function enableNotifications(on: boolean) {
  turns++;
  updateSettings({ notify: on });
  if (!on) return asking.set(false);
  if ((await api.notificationPermission().catch(() => null)) === "prompt") await askNotifications();
  else refreshNotifyPermission();
}

/**
 * Tells the user `title`, if they asked to be told of `event` and aren't looking at the app.
 * `target`: what a click on it shows ("pane:<run>:<id>", needsYou). True when it was sent.
 */
export function notifyIfAway(event: NotifyEvent, title: string, body?: string, target?: string) {
  const s = getSettings();
  const state = permission.get();
  if (!s.notify || !s[event] || document.hasFocus() || settingsFocused || (state !== null && !allowed(state))) return false;
  api.notify(title, body ?? "", target).catch(() => {});
  return true;
}

/** From Settings, so shown while the app is in front. It always answers: sent, or why it can't be. */
export async function sendTestNotification() {
  let state: NotifyPermission | null = null;
  try {
    state = await api.notificationPermission();
    permission.set(state);
  } catch (e) {
    toast("error", "Could not read macOS's notification settings", errorMessage(e));
    return;
  }
  const plan = testPlan(state);
  if (!plan.send) {
    const run = plan.fix === "ask" ? () => void askNotifications() : openNotificationSettings;
    toast("info", plan.title, "Nothing was sent.", { label: plan.fix === "ask" ? "Ask macOS" : "Open System Settings", run });
    return;
  }
  try {
    await api.notify("GitViber", "Notifications are on. Click one to come back here.");
    toast("success", "Test notification sent", plan.note);
  } catch (e) {
    toast("error", "Could not send a notification", errorMessage(e));
  }
}

// What macOS says now, at launch and on each return to the window: Settings shows it.
refreshNotifyPermission();
window.addEventListener("focus", refreshNotifyPermission);
