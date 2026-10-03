import type { NotifyPermission } from "../api";

/** What to do about a permission: open System Settings, or ask macOS (the only two ways to change it). */
export type NotifyFix = "settings" | "ask";
export interface NotifyNotice {
  text: string;
  fix: NotifyFix | null;
}

const NOTICES: Partial<Record<NotifyPermission, NotifyNotice>> = {
  denied: { text: "macOS isn't allowing GitViber to show notifications. Turn them on in System Settings → Notifications → GitViber.", fix: "settings" },
  quiet: { text: "Banners are off: GitViber's alert style is None in System Settings → Notifications, so they only go to Notification Center.", fix: "settings" },
  prompt: { text: "macOS hasn't been asked yet. Ask it, then allow GitViber in its prompt.", fix: "ask" },
  unbundled: {
    text: "This development build couldn't become an app bundle (see the dev terminal), so macOS can't be asked: its notifications show as Terminal's, and a click opens Terminal.",
    fix: null,
  },
};

/** The line under the switch: only while it's on, and only when something is in the way. */
export function permissionNotice(on: boolean, state: NotifyPermission | null): NotifyNotice | null {
  return on && state ? (NOTICES[state] ?? null) : null;
}

/** What Send Test does: it sends where macOS can show it, and otherwise says why not. Never asks by itself. */
export function testPlan(state: NotifyPermission | null): { send: true; note?: string } | { send: false; title: string; fix: NotifyFix | null } {
  switch (state) {
    case "denied":
      return { send: false, title: "macOS isn't allowing GitViber to show notifications", fix: "settings" };
    case "prompt":
      return { send: false, title: "macOS hasn't been asked yet", fix: "ask" };
    case "quiet":
      return { send: true, note: "Banners are off, so it's in Notification Center only." };
    default:
      return { send: true };
  }
}
