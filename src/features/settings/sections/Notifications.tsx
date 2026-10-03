import { Fragment, type ReactNode, useEffect } from "react";
import { Button } from "@/components/ui/button";
import { Segmented } from "@/components/ui/segmented";
import { Switch } from "@/components/ui/switch";
import { askNotifications, enableNotifications, openNotificationSettings, refreshNotifyPermission, sendTestNotification, useAskingNotify, useNotifyPermission } from "@/lib/app/notify";
import { permissionNotice } from "@/lib/app/notifyState";
import { IS_MAC } from "@/lib/platform";
import { formatDuration } from "@/lib/terminal/commandMarks";
import { LONG_COMMAND_SECONDS, type NotifyEvent, updateSettings, useSettings } from "@/lib/settings";
import { Field, Group } from "@/features/settings/controls";

const EVENTS: [NotifyEvent, string, string][] = [
  ["notifyAgentDone", "An agent finishes", "Claude Code, or another agent that reports its state, stops working in a terminal."],
  ["notifyAgentWaiting", "An agent asks for you", "It waits for an answer or a permission before it goes on."],
  ["notifyTerminal", "A terminal rings or notifies", "Any other program ringing the bell or sending a notification (OSC 9, 777 or 99). An agent that reports its state goes by the two switches above."],
  [
    "notifyLongCommand",
    "A long command finishes",
    "A command in a terminal you aren't looking at ends after running longer than the time below; its tab gets a dot too. Needs shell integration (zsh, bash 4.4+). An agent goes by the switches above.",
  ],
  ["notifyGit", "A git command ends", "A push, pull, fetch, clone or commit finishes or fails."],
];

export function NotificationsSection() {
  const s = useSettings();
  const permission = useNotifyPermission();
  const asking = useAskingNotify();
  // notify.ts reads it again whenever the window comes back, as from System Settings.
  useEffect(refreshNotifyPermission, []);
  const notice = IS_MAC ? permissionNotice(s.notify, permission) : null;
  // Rows that go with a switch, shown while it's on.
  const after: Partial<Record<NotifyEvent, ReactNode>> = {
    notifyLongCommand: (
      <Field label="Long means longer than">
        <Segmented<string>
          value={String(s.longCommandSeconds)}
          onChange={(v) => updateSettings({ longCommandSeconds: Number(v) })}
          options={LONG_COMMAND_SECONDS.map((n) => ({ value: String(n), label: formatDuration(n * 1000) }))}
          variant="field"
        />
      </Field>
    ),
  };
  return (
    <>
      <Group>
        <Field
          label="Desktop notifications"
          hint={
            <>
              While GitViber isn't the app in front. Clicking one brings it back, at the terminal it's about.
              {asking && " Waiting for your answer to macOS's prompt…"}
              {notice && (
                <span className="mt-1 flex flex-wrap items-center gap-x-3 gap-y-1.5 font-medium text-modified">
                  {notice.text}
                  {notice.fix && (
                    <Button variant="outline" size="sm" disabled={asking} onClick={notice.fix === "ask" ? () => void askNotifications() : openNotificationSettings}>
                      {notice.fix === "ask" ? "Ask macOS" : "Open System Settings"}
                    </Button>
                  )}
                </span>
              )}
            </>
          }
        >
          <Switch checked={s.notify} onChange={(v) => void enableNotifications(v)} />
        </Field>
        <Field label="Test notification" hint={!IS_MAC ? "Shows one now, to see how they look." : "Shows one now. If macOS won't show it, this says why."}>
          <Button variant="outline" size="sm" onClick={() => void sendTestNotification()}>
            Send Test
          </Button>
        </Field>
      </Group>
      <Group title="Notify when">
        {EVENTS.map(([key, label, hint]) => (
          <Fragment key={key}>
            <Field label={label} hint={hint}>
              <Switch checked={s[key]} onChange={(v) => updateSettings({ [key]: v })} />
            </Field>
            {s[key] && after[key]}
          </Fragment>
        ))}
      </Group>
    </>
  );
}
