import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { relativeTime } from "@/lib/format";
import { updateSettings, useSettings } from "@/lib/settings";
import { checkForUpdates, showUpdate, useUpdateMode, useUpdates } from "@/lib/app/updates";
import { useAbout } from "@/features/app/AboutDialog";
import { Field } from "@/features/settings/controls";

export function UpdatesSection() {
  const s = useSettings();
  const mode = useUpdateMode();
  const about = useAbout();
  const { release, checking, checkedAt } = useUpdates();
  const status = !mode
    ? "Updates are off in development builds."
    : checking
      ? "Checking…"
      : release
        ? `GitViber ${release.version} is available.`
        : checkedAt
          ? `Up to date, checked ${relativeTime(checkedAt / 1000)}.`
          : "Not checked yet.";
  return (
    <>
      <Field label="Check for updates automatically" hint="At launch and every few hours GitViber asks GitHub Releases whether a newer version is out. Nothing downloads until you choose to update.">
        <Switch checked={s.autoUpdate} onChange={(v) => updateSettings({ autoUpdate: v })} />
      </Field>
      <Field label={about ? `GitViber ${about.version}` : "GitViber"} hint={status}>
        {release ? (
          <Button variant="outline" size="sm" onClick={() => showUpdate()}>
            What's New…
          </Button>
        ) : (
          <Button variant="outline" size="sm" disabled={!mode || checking} onClick={() => void checkForUpdates(true)}>
            Check Now
          </Button>
        )}
      </Field>
    </>
  );
}
