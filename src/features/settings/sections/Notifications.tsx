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
  granted: "macOS shows them. How (banners, sound, in Focus) is up to System Settings → Notifications.",
  denied: "macOS doesn't show them: GitViber is off in System Settings → Notifications, or its alert style is None.",
  prompt: "macOS hasn't been asked yet: turning notifications on or sending a test asks.",
  unbundled: "This development build couldn't become an app bundle (see the dev terminal), so macOS can't be asked: its notifications show as Terminal's, and a click opens Terminal.",
};

export function NotificationsSection() {
  const s = useSettings();
  const permission = useNotifyPermission();
  const asking = useAskingNotify();
  // notify.ts asks again whenever the window comes back, from System Settings say.
  useEffect(refreshNotifyPermission, []);
  // On but not allowed: the wish stays, and so does the way to grant it.
  const needed = IS_MAC && s.notify && !asking && (permission === "denied" || permission === "prompt");
  const status = !IS_MAC ? "Shows one now, to see how they look." : permission ? STATUS[permission] : "Checking with macOS…";
  return (
    <>
      <Group>
        <Field
          label="Desktop notifications"
          hint={asking ? "Waiting for your answer to macOS's prompt…" : "While GitViber isn't the app in front. Clicking one brings it back, at the terminal it's about."}
        >
          <div className="flex items-center gap-3">
            {needed && (
              <>
                <span className="text-[11.5px] font-medium text-modified">Permission needed</span>
                {permission === "denied" ? (
                  <Button variant="outline" size="sm" onClick={openNotificationSettings}>
                    Open System Settings
                  </Button>
                ) : (
                  <Button variant="outline" size="sm" onClick={() => void enableNotifications(true)}>
                    Ask macOS
                  </Button>
                )}
              </>
            )}
            <Switch checked={s.notify} disabled={asking} onChange={(v) => void enableNotifications(v)} />
          </div>
        </Field>
        <Field label="Test notification" hint={status}>
          <div className="flex gap-2">
            {IS_MAC && !needed && (permission === "granted" || permission === "denied") && (
              <Button variant="outline" size="sm" onClick={openNotificationSettings}>
                Open System Settings
              </Button>
            )}
            <Button variant="outline" size="sm" disabled={asking} onClick={() => void sendTestNotification()}>
              Send Test
            </Button>
          </div>
        </Field>
      </Group>
      <Group title="Notify when">
        {EVENTS.map(([key, label, hint]) => (
          <Field key={key} label={label} hint={hint}>
            <Switch checked={s[key]} onChange={(v) => updateSettings({ [key]: v })} />
          </Field>
        ))}
      </Group>
    </>
  );
}
