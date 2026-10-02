// Desktop notifications, when the user turned them on (Settings → Notifications): a network command
// that ends while the app is in the background says so, as GitHub Desktop does, and so does a
// terminal that rings or sends a notification (lib/terminal/needsYou), as Ghostty and iTerm2 do, or
// whose agent finishes or asks. notifications.rs posts them, asking macOS first.
import { api, errorMessage, type NotifyPermission } from "../api";
import { getSettings, type NotifyEvent, updateSettings } from "../settings";
import { createStore } from "../store";
import { failed, toast } from "./toast";

// Null until asked: Settings asks each time it shows, as the user may have changed it in the OS.
const permission = createStore<NotifyPermission | null>(null);
export const useNotifyPermission = permission.use;

export function refreshNotifyPermission() {
  api.notificationPermission().then(permission.set, () => {});
}

// True while macOS's prompt is open: the switch waits for the answer.
const asking = createStore(false);
export const useAskingNotify = asking.use;
// Each turn of the switch; an answer to an older one is let go.
let turns = 0;

export const openNotificationSettings = () => void api.notificationSettings().catch(failed("Could not open System Settings"));

const DENIED = "Allow them in System Settings → Notifications, then turn this on again.";
const openSettingsAction = { label: "Open System Settings", run: openNotificationSettings };

/** Turns them on, asking the OS first; stays off if it says no. */
export async function enableNotifications(on: boolean) {
  const turn = ++turns;
  if (!on) {
    asking.set(false);
    return updateSettings({ notify: false });
  }
  asking.set(true);
  try {
    let state = await api.notificationPermission();
    if (state === "prompt") state = await api.requestNotifications();
    if (turn !== turns) return;
    permission.set(state);
    if (state === "granted" || state === "unbundled") updateSettings({ notify: true });
    else if (state === "denied") toast("info", "Notifications are off for GitViber", DENIED, openSettingsAction);
    else toast("info", "macOS is still asking", "Answer its prompt, then turn this on again.");
  } catch (e) {
    if (turn === turns) toast("error", "Could not turn on notifications", errorMessage(e));
  } finally {
    if (turn === turns) asking.set(false);
  }
}
/**
 * Tells the user `title`, if they asked to be told of `event` and aren't looking at the app.
 * `target`: what a click on it shows ("pane:<id>", needsYou). True when it was sent.
 */
export function notifyIfAway(event: NotifyEvent, title: string, body?: string, target?: string) {
  const s = getSettings();
  if (!s.notify || !s[event] || document.hasFocus()) return false;
  // Permission taken back in the OS: nothing to show then.
  api.notify(title, body ?? "", target).catch(() => {});
  return true;
}

/** From Settings, so shown while the app is in front. */
export function sendTestNotification() {
  api.notify("GitViber", "Notifications are on. Click one to come back here.").catch(failed("Could not send a notification"));
}

let toldDenied = false;

/**
 * With the switch on, what macOS says now. Up to 0.1.7 it was never asked, so a switch turned on
 * then has no permission behind it: off it goes, with a way back that asks. One taken back in
 * System Settings would drop every notification unseen, so that's said once.
 */
function checkPermission() {
  if (!getSettings().notify) return;
  api
    .notificationPermission()
    .then((state) => {
      permission.set(state);
      if (state === "prompt") {
        updateSettings({ notify: false });
        toast("info", "Notifications need your permission", "GitViber now asks macOS before it shows any.", { label: "Turn on", run: () => void enableNotifications(true) });
      } else if (state === "denied" && !toldDenied) {
        toldDenied = true;
        toast("info", "Notifications are off for GitViber", "They were turned off in System Settings → Notifications, so none can show.", openSettingsAction);
      }
    })
    .catch(() => {});
}

checkPermission();
window.addEventListener("focus", checkPermission);
