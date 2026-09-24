// Desktop notifications, when the user turned them on (Settings → Git): a network command that
// ends while the app is in the background says so, as GitHub Desktop does.
import { isPermissionGranted, requestPermission, sendNotification } from "@tauri-apps/plugin-notification";
import { getSettings, updateSettings } from "./settings";
import { toast } from "./toast";

/** Turns them on, asking the OS first; stays off if it says no. */
export async function enableNotifications(on: boolean) {
  if (!on) return updateSettings({ notify: false });
  try {
    const granted = (await isPermissionGranted()) || (await requestPermission()) === "granted";
    if (granted) updateSettings({ notify: true });
    else toast("info", "Notifications are off for GitViber", "Allow them in System Settings → Notifications, then turn this on again.");
  } catch (e) {
    toast("error", "Could not turn on notifications", String(e));
  }
}

/** Tells the user `title`, if they asked to be told and aren't looking at the app. */
export function notifyIfAway(title: string, body?: string) {
  if (!getSettings().notify || document.hasFocus()) return;
  try {
    sendNotification({ title, body });
  } catch {
    // Permission taken back in the OS: nothing to show then.
  }
}
