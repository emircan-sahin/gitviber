import { useEffect } from "react";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { relativeTime } from "@/lib/format";
import { getSettings, updateSettings, useSettings } from "@/lib/settings";
import { IN_SETTINGS_WINDOW, runInMain } from "@/lib/app/settingsWindow";
import { checkForUpdates, mirrorUpdates, showUpdate, useSharedUpdates, useUpdateMode, useUpdates } from "@/lib/app/updates";
import { runInWorkspace } from "@/lib/commands/keybindings";
import { useAbout } from "@/features/app/AboutDialog";
import { Field, Group } from "@/features/settings/controls";

export function UpdatesSection() {
  const s = useSettings();
  const about = useAbout();
  // The main window checks and keeps the updater's state; the settings window mirrors it.
  const here = { state: useUpdates(), mode: useUpdateMode() };
  const mirrored = useSharedUpdates();
  const shared = IN_SETTINGS_WINDOW ? mirrored : here;
  const mode = shared?.mode ?? null;
  const { release, checking, checkedAt } = shared?.state ?? { release: null, checking: true, checkedAt: null };

  // Opening the section checks, quietly: what it finds shows below, and Check Now says how it went.
  useEffect(() => {
    const check = getSettings().autoUpdate;
    if (IN_SETTINGS_WINDOW) return mirrorUpdates(check);
    if (check) void checkForUpdates(false);
  }, []);

  const status =
    shared && !mode
      ? "Updates are off in development builds."
      : checking
        ? "Checking…"
        : release
          ? `GitViber ${release.version} is available.`
          : checkedAt
            ? `Up to date, checked ${relativeTime(checkedAt / 1000)}.`
            : "Not checked yet.";
  return (
    <Group>
      <Field label="Check for updates automatically" hint="At launch, every few hours and when this page opens, GitViber asks GitHub Releases whether a newer version is out. Nothing downloads until you choose to update.">
        <Switch checked={s.autoUpdate} onChange={(v) => updateSettings({ autoUpdate: v })} />
      </Field>
      <Field label={about ? `GitViber ${about.version}` : "GitViber"} hint={status}>
        {release ? (
          // The dialog is the main window's, brought forward from here.
          <Button variant="outline" size="sm" onClick={() => (IN_SETTINGS_WINDOW ? runInMain("app.checkForUpdates", true) : showUpdate())}>
            What's New…
          </Button>
        ) : (
          <Button variant="outline" size="sm" disabled={!mode || checking} onClick={() => runInWorkspace("app.checkForUpdates", true)}>
            Check Now
          </Button>
        )}
      </Field>
    </Group>
  );
}
