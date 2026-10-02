import { useEffect } from "react";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import type { NotifyPermission } from "@/lib/api";
import { enableNotifications, openNotificationSettings, refreshNotifyPermission, sendTestNotification, useAskingNotify, useNotifyPermission } from "@/lib/app/notify";
import { IS_MAC } from "@/lib/platform";
import { type NotifyEvent, updateSettings, useSettings } from "@/lib/settings";
import { Field, Group } from "@/features/settings/controls";

const EVENTS: [NotifyEvent, string, string][] = [
  ["notifyAgentDone", "An agent finishes", "Claude Code, or another agent that reports its state, stops working in a terminal."],
  ["notifyAgentWaiting", "An agent asks for you", "It waits for an answer or a permission before it goes on."],
  ["notifyTerminal", "A terminal rings or notifies", "Any other program ringing the bell or sending a notification (OSC 9, 777 or 99). An agent that reports its state goes by the two switches above."],
  ["notifyGit", "A git command ends", "A push, pull, fetch, clone or commit finishes or fails."],
];

const STATUS: Record<NotifyPermission, string> = {
  granted: "macOS allows them. How they show (banners, sound, in Focus) is up to System Settings → Notifications.",
  denied: "Turned off for GitViber in System Settings → Notifications. None can show until they're allowed there.",
  prompt: "macOS hasn't been asked yet: turning notifications on asks.",
  unbundled: "This development build isn't an app bundle, so macOS can't be asked: its notifications show as Terminal's, and a click opens Terminal.",
};

export function NotificationsSection() {
  const s = useSettings();
  const permission = useNotifyPermission();
  const asking = useAskingNotify();
  // The user may have just changed it in System Settings.
  useEffect(() => {
    refreshNotifyPermission();
    window.addEventListener("focus", refreshNotifyPermission);
    return () => window.removeEventListener("focus", refreshNotifyPermission);
  }, []);
  const canSend = permission === "granted" || permission === "unbundled";
  const status = !IS_MAC ? "Shows one now, to see how they look." : permission ? STATUS[permission] : "Checking with macOS…";
  return (
    <>
      <Group>
        <Field
          label="Desktop notifications"
          hint={asking ? "Waiting for your answer to macOS's prompt…" : "While GitViber isn't the app in front. Clicking one brings it back, at the terminal it's about."}
        >
          <Switch checked={s.notify} disabled={asking} onChange={(v) => void enableNotifications(v)} />
        </Field>
        <Field label="Test notification" hint={status}>
          <div className="flex gap-2">
            {IS_MAC && (permission === "granted" || permission === "denied") && (
              <Button variant="outline" size="sm" onClick={openNotificationSettings}>
                Open System Settings
              </Button>
            )}
            <Button variant="outline" size="sm" disabled={!canSend} onClick={sendTestNotification}>
              Send Test
            </Button>
          </div>
        </Field>
      </Group>
      <Group title="Notify when">
        {EVENTS.map(([key, label, hint]) => (
          <Field key={key} label={label} hint={hint}>
            <Switch checked={s[key]} disabled={!s.notify} onChange={(v) => updateSettings({ [key]: v })} />
          </Field>
        ))}
      </Group>
    </>
  );
}
