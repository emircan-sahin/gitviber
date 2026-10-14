import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { Segmented } from "@/components/ui/segmented";
import { Switch } from "@/components/ui/switch";
import { Field, Group } from "@/features/settings/controls";
import { browserApi, type Skill } from "@/lib/api";
import { copyText } from "@/lib/app/clipboard";
import { listenHere } from "@/lib/app/settingsWindow";
import { clearBrowsingData } from "@/lib/browser/clearData";
import { skillLines, skillWanted } from "@/lib/browser/skill";
import { IS_LINUX } from "@/lib/platform";
import { BROWSER_LIVE_HIDDEN, BROWSER_PARK_AFTER, updateSettings, useSettings } from "@/lib/settings";

const minutes = (n: number) => (n === 0 ? "Never" : n < 60 ? `${n} min` : `${n / 60} h`);

// What the skill teaches, for the agents without skills: built by browser/skill.rs, as the skill is.
function useAgentNote(on: boolean) {
  const [note, setNote] = useState("");
  useEffect(() => {
    if (on && !note) void browserApi.agentNote().then(setNote, () => {});
  }, [on, note]);
  return note;
}

// Where each agent's skill stands. Only the main window writes it (store.ts), and says so after.
function useSkill(shown: boolean) {
  const [skill, setSkill] = useState<Skill | null>(null);
  useEffect(() => {
    if (!shown) return;
    let live = true;
    void browserApi.skillStatus().then((s) => live && setSkill(s), () => {});
    const unlisten = listenHere<Skill>("browser-skill", ({ payload }) => live && setSkill(payload));
    return () => {
      live = false;
      void unlisten.then((u) => u()).catch(() => {});
    };
  }, [shown]);
  return skill;
}

export function BrowserSection() {
  const s = useSettings();
  const note = useAgentNote(s.browserAgentControl && !IS_LINUX);
  const skillOn = skillWanted(s);
  const skill = useSkill(s.browserAgentControl && !IS_LINUX);
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
          hint="An agent in a terminal can open pages, read them, click, type and take screenshots with the gitviber browser command. A terminal pane's agents get tabs of their own, opened in the background without taking your active tab, each signed in apart from your browsing unless the agent asks for yours. Off, the command says so and does nothing."
        >
          <Switch checked={s.browserAgentControl} onChange={(v) => updateSettings({ browserAgentControl: v })} />
        </Field>
        {s.browserAgentControl && (
          <Field
            label="Teach Claude Code and Codex (skill)"
            hint={
              <>
                A skill on when and how to use the browser, rewritten as GitViber updates and deleted when this or agent control is turned off. A file there of your own is never touched.
                {[...skillLines(skill?.targets ?? [], skillOn), ...(skill?.why ? [skill.why] : [])].map((line) => (
                  <span key={line} className="mt-0.5 block font-mono text-[11px] break-all">
                    {line}
                  </span>
                ))}
              </>
            }
          >
            <Switch checked={s.browserAgentSkill} onChange={(v) => updateSettings({ browserAgentSkill: v })} />
          </Field>
        )}
        {s.browserAgentControl && note && (
          <div className="border-b border-border py-3.5 last:border-0">
            <div className="flex items-center gap-2">
              <div className="flex-1 text-[11.5px] leading-relaxed text-muted-foreground">Other agents learn of it from their instructions: add this to the project's CLAUDE.md or AGENTS.md.</div>
              <Button type="button" variant="outline" size="sm" onClick={() => void copyText(note, "Copied", "Paste it into CLAUDE.md or AGENTS.md.")}>
                Copy
              </Button>
            </div>
            <pre className="mt-2 max-h-48 overflow-auto rounded-md bg-panel p-2.5 font-mono text-[11px] leading-relaxed whitespace-pre-wrap text-muted-foreground select-text">{note}</pre>
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
