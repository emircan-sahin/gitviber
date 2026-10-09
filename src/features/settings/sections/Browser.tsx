import { Button } from "@/components/ui/button";
import { Segmented } from "@/components/ui/segmented";
import { Switch } from "@/components/ui/switch";
import { Field, Group } from "@/features/settings/controls";
import { copyText } from "@/lib/app/clipboard";
import { clearBrowsingData } from "@/lib/browser/clearData";
import { IS_LINUX } from "@/lib/platform";
import { BROWSER_LIVE_HIDDEN, BROWSER_PARK_AFTER, updateSettings, useSettings } from "@/lib/settings";

const minutes = (n: number) => (n === 0 ? "Never" : n < 60 ? `${n} min` : `${n / 60} h`);

// For CLAUDE.md or AGENTS.md: GitViber writes no agent's files itself.
const AGENT_NOTE = `## Browser

You run in a GitViber terminal, which gives you a browser tab of your own: drive it with \`gitviber browser\`.

- \`gitviber browser open localhost:5173\` loads a page and waits for it
- \`gitviber browser snapshot -i\` lists what you can act on, each with a ref (e1, e2, ...)
- \`gitviber browser click e2\`, \`fill e3 "some text"\`, \`press Enter\`, \`wait --text "Saved"\`
- \`gitviber browser screenshot\` saves a PNG and prints its path; \`console --errors\` shows the page's errors
- \`gitviber browser help\` lists every command

Refs last until the page changes: take a snapshot again after a navigation.`;

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
      <Group title="Console">
        <Field label="Keep the page's console" hint="A browser tab keeps what its page logs as errors and warnings, shows how many, and sends the errors to the worktree's agent on request. Off, pages load without it.">
          <Switch checked={s.browserConsole} onChange={(v) => updateSettings({ browserConsole: v })} />
        </Field>
      </Group>
      <Group title="Agents">
        <Field
          label="Reload when an agent finishes"
          hint="When an agent in a terminal finishes its turn, the pages of its worktree load again. A page its dev server already reloads by itself (Vite, webpack, Next) is left as it is."
        >
          <Switch checked={s.browserReloadOnAgentDone} onChange={(v) => updateSettings({ browserReloadOnAgentDone: v })} />
        </Field>
        <Field
          label="Let agents use the browser"
          hint="An agent in a terminal can open pages, read them, click, type and take screenshots with the gitviber browser command. Each terminal pane gets a tab of its own, opened in the background without taking your active tab. Off, the command says so and does nothing."
        >
          <Switch checked={s.browserAgentControl} onChange={(v) => updateSettings({ browserAgentControl: v })} />
        </Field>
        {s.browserAgentControl && (
          <div className="border-b border-border py-3.5 last:border-0">
            <div className="flex items-center gap-2">
              <div className="flex-1 text-[11.5px] leading-relaxed text-muted-foreground">Agents learn of it from their instructions: add this to the project's CLAUDE.md or AGENTS.md.</div>
              <Button type="button" variant="outline" size="sm" onClick={() => void copyText(AGENT_NOTE, "Copied", "Paste it into CLAUDE.md or AGENTS.md.")}>
                Copy
              </Button>
            </div>
            <pre className="mt-2 max-h-48 overflow-auto rounded-md bg-panel p-2.5 font-mono text-[11px] leading-relaxed whitespace-pre-wrap text-muted-foreground select-text">{AGENT_NOTE}</pre>
          </div>
        )}
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
