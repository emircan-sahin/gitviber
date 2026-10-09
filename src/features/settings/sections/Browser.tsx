import { Button } from "@/components/ui/button";
import { Segmented } from "@/components/ui/segmented";
import { Switch } from "@/components/ui/switch";
import { Field, Group } from "@/features/settings/controls";
import { clearBrowsingData } from "@/lib/browser/clearData";
import { IS_LINUX } from "@/lib/platform";
import { BROWSER_LIVE_HIDDEN, BROWSER_PARK_AFTER, updateSettings, useSettings } from "@/lib/settings";

const minutes = (n: number) => (n === 0 ? "Never" : n < 60 ? `${n} min` : `${n / 60} h`);

export function BrowserSection() {
  const s = useSettings();
  if (IS_LINUX) {
    return (
      <Group>
        <div className="py-3.5 text-[11.5px] leading-relaxed text-muted-foreground">
          On Linux a browser tab opens localhost and 127.0.0.1 only, in a frame of the app's own page, with nothing of its own to keep or clear. The settings here are for macOS, where each tab has a browser of its own.
        </div>
      </Group>
    );
  }
  return (
    <>
      <Group title="Memory">
        <Field label="Pages kept alive out of sight" hint="Each open page runs as a process of its own: about 20 MB for a plain one, 150 MB or more for a large app. Past this many, the one out of sight longest closes, and loads again from its address when its tab shows.">
          <Segmented<string>
            value={String(s.browserLiveHidden)}
            onChange={(v) => updateSettings({ browserLiveHidden: Number(v) })}
            options={BROWSER_LIVE_HIDDEN.map((n) => ({ value: String(n), label: String(n) }))}
            variant="field"
          />
        </Field>
        <Field label="Close a page out of sight after" hint="However many are kept alive. Never leaves it to the count above.">
          <Segmented<string>
            value={String(s.browserParkAfterMin)}
            onChange={(v) => updateSettings({ browserParkAfterMin: Number(v) })}
            options={BROWSER_PARK_AFTER.map((n) => ({ value: String(n), label: minutes(n) }))}
            variant="field"
          />
        </Field>
      </Group>
      <Group title="Agents">
        <Field
          label="Reload when an agent finishes"
          hint="When an agent in a terminal finishes its turn, the pages of its worktree load again. A page its dev server already reloads by itself (Vite, webpack, Next) is left as it is."
        >
          <Switch checked={s.browserReloadOnAgentDone} onChange={(v) => updateSettings({ browserReloadOnAgentDone: v })} />
        </Field>
      </Group>
      <Group title="Data">
        <Field
          label="Browsing data"
          hint="The browser tabs keep their own cookies, site storage and cache, apart from GitViber's settings. Chrome's sign-ins and cookies don't carry over: the tabs are WebKit, as Safari is. On macOS 13 they're kept only until GitViber quits."
        >
          <Button type="button" variant="outline" size="sm" onClick={() => void clearBrowsingData()}>
            Clear Browsing Data…
          </Button>
        </Field>
      </Group>
    </>
  );
}
