import { useEffect } from "react";
import { Switch } from "@/components/ui/switch";
import { Field, Group } from "@/features/settings/controls";
import { refreshVaults, vaultList } from "@/lib/obsidian/vaultList";
import { IS_MAC, IS_WINDOWS } from "@/lib/platform";
import { updateSettings, useSettings } from "@/lib/settings";

// Where Obsidian keeps its vault list, as shown when it has none.
const LIST = IS_MAC ? "~/Library/Application Support/obsidian/obsidian.json" : IS_WINDOWS ? "%APPDATA%\\obsidian\\obsidian.json" : "~/.config/obsidian/obsidian.json";

export function ObsidianSection() {
  const s = useSettings();
  const vaults = vaultList.use();
  useEffect(() => void refreshVaults(), []);
  const show = (path: string, on: boolean) => updateSettings({ hiddenVaults: on ? s.hiddenVaults.filter((p) => p !== path) : [...s.hiddenVaults, path] });
  return (
    <>
      <Group>
        <Field label="Show vaults in the explorer" hint="A section under the files lists your Obsidian vault. Its notes open in a tab, rendered as Obsidian shows them.">
          <Switch checked={s.obsidian} onChange={(v) => updateSettings({ obsidian: v })} />
        </Field>
      </Group>
      {s.obsidian && (
        <Group title="Vaults">
          {vaults?.length ? (
            vaults.map((v) => (
              <Field key={v.path} label={v.name} hint={v.path}>
                <Switch checked={!s.hiddenVaults.includes(v.path)} onChange={(on) => show(v.path, on)} />
              </Field>
            ))
          ) : (
            <div className="py-3.5 text-[11.5px] leading-relaxed text-muted-foreground">
              {vaults ? `No vaults found. GitViber reads the ones Obsidian lists in ${LIST}.` : "Looking for vaults…"}
            </div>
          )}
        </Group>
      )}
    </>
  );
}
