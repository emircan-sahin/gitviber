import { ChevronDown } from "lucide-react";
import { useRef, useState } from "react";
import { DropdownMenu, DropdownMenuContent, DropdownMenuRadioGroup, DropdownMenuRadioItem, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { Input } from "@/components/ui/input";
import { github } from "@/lib/api";
import { updateSettings, useSettings } from "@/lib/settings";
import { ALL_MODELS, effortArg, GUIDE_LIMIT_KB, effortLevels, effortOf, leanFlags, modelOf, presetOf, PULL_PROMPT, SUGGEST_LIMIT_KB, SUGGEST_PRESETS, SUGGEST_PROMPT, type SuggestPreset as Preset } from "@/lib/git/suggest";
import { failed } from "@/lib/app/toast";
import { cn } from "@/lib/utils";
import { segmentClass } from "@/components/ui/segmented";
import { Switch } from "@/components/ui/switch";
import { Field, Group, OptionSelect } from "@/features/settings/controls";
import { GUIDE_PROMPT } from "@/lib/review/guide";

function ModelsLink({ label, url }: { label: string; url: string }) {
  return (
    <button type="button" onClick={() => github.openUrl(url).catch(failed("Could not open the link"))} className="text-foreground underline underline-offset-2 hover:text-primary">
      {label}
    </button>
  );
}

/** Segmented like the rest, with the less common CLIs under Others to keep it narrow. */
function AgentPicker({ value, onChange }: { value: Preset | "custom"; onChange: (v: Preset | "custom") => void }) {
  const presets = Object.keys(SUGGEST_PRESETS) as Preset[];
  const other = value !== "custom" && SUGGEST_PRESETS[value].other;
  return (
    <div className="flex h-7 overflow-hidden rounded-md border border-border-strong">
      {presets
        .filter((k) => !SUGGEST_PRESETS[k].other)
        .map((k) => (
          <button key={k} aria-pressed={k === value} onClick={() => onChange(k)} className={segmentClass("field", k === value)}>
            {SUGGEST_PRESETS[k].label}
          </button>
        ))}
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <button className={cn(segmentClass("field", other), "flex items-center gap-1")}>
            {other ? SUGGEST_PRESETS[value].label : "Others"}
            <ChevronDown className="size-3" />
          </button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="start" className="min-w-36">
          <DropdownMenuRadioGroup value={value} onValueChange={(v) => onChange(v as Preset)}>
            {presets
              .filter((k) => SUGGEST_PRESETS[k].other)
              .map((k) => (
                <DropdownMenuRadioItem key={k} value={k}>
                  {SUGGEST_PRESETS[k].label}
                </DropdownMenuRadioItem>
              ))}
          </DropdownMenuRadioGroup>
        </DropdownMenuContent>
      </DropdownMenu>
      <button aria-pressed={value === "custom"} onClick={() => onChange("custom")} className={segmentClass("field", value === "custom")}>
        Custom
      </button>
    </div>
  );
}

/** How long the model thinks, for a preset whose CLI has a flag for it. */
function EffortField({ preset }: { preset: Preset }) {
  const { suggestEfforts } = useSettings();
  const levels = effortLevels(preset);
  if (!levels.length) return null;
  const effort = effortOf(preset, suggestEfforts);
  return (
    <Field
      label="Effort"
      hint={
        <>
          Passed as <code className="font-mono text-foreground">{effortArg(preset, "<level>")}</code>
          {SUGGEST_PRESETS[preset].effort?.default ? `, ${SUGGEST_PRESETS[preset].effort?.default} until you pick another` : ""}; CLI default passes none, leaving it to the CLI's own settings. More
          effort writes more considered answers, slower and at a higher cost.{preset === "opencode" && " Levels are per model, and not every model has every one."}
        </>
      }
    >
      <OptionSelect
        className="w-80"
        value={effort}
        options={{ "": "CLI default", ...Object.fromEntries(levels.map((l) => [l, l])) }}
        onChange={(v) => updateSettings({ suggestEfforts: { ...suggestEfforts, [preset]: v } })}
      />
    </Field>
  );
}

export function CommitSection() {
  const s = useSettings();
  const preset = presetOf(s.suggestCommand);
  // Custom stays picked while its text happens to match a preset.
  const [custom, setCustom] = useState(false);
  const input = useRef<HTMLInputElement>(null);
  return (
    <>
      <Group title="Suggestions">
        <Field
          label="Suggest commit messages"
          hint="Adds a ✦ button to the commit box that asks your own agent CLI to write the message, one to New pull request for its title and description, and Explain Commit (History) and Guided Review (branch review) for a guided review of a change. GitViber sends nothing itself and keeps no keys: the command runs on this Mac, and it decides where the diff goes."
        >
          <Switch checked={s.suggestEnabled} onChange={(v) => updateSettings({ suggestEnabled: v })} />
        </Field>
        <Field
          label="Command"
          hint="Runs in the repository's folder, directly, not through a shell. The prompt and the diff arrive on stdin; put {prompt} in the command to pass the prompt as an argument instead. If it isn't found, give its full path (`which claude` in Terminal prints it)."
          commands={["git.suggestMessage"]}
        >
          <div className="flex w-80 flex-col gap-2">
            <AgentPicker
              value={custom || !preset ? "custom" : preset}
              onChange={(v) => {
                setCustom(v === "custom");
                if (v === "custom") input.current?.focus();
                else updateSettings({ suggestCommand: SUGGEST_PRESETS[v].command });
              }}
            />
            <Input ref={input} value={s.suggestCommand} onChange={(e) => updateSettings({ suggestCommand: e.target.value })} placeholder="claude -p" spellCheck={false} className="font-mono" />
          </div>
        </Field>
        {preset ? (
          <Field
            label="Model"
            hint={
              <>
                Passed as <code className="font-mono text-foreground">{`${SUGGEST_PRESETS[preset].modelFlag} ${SUGGEST_PRESETS[preset].model}`}</code>; empty uses the CLI's own default. New models come out often, and <ModelsLink {...SUGGEST_PRESETS[preset].models} /> has the current IDs.
                {leanFlags(preset, false) && (
                  <>
                    {" "}
                    Every run also gets <code className="font-mono text-foreground">{leanFlags(preset, false)}</code> (a guided review <code className="font-mono text-foreground">--tools Read,Grep,Glob</code>, to read
                    the patch): no MCP servers, skills, saved session or tools beyond reading files, so it starts and answers faster.
                  </>
                )}
              </>
            }
          >
            <Input
              value={modelOf(preset, s.suggestModels)}
              onChange={(e) => updateSettings({ suggestModels: { ...s.suggestModels, [preset]: e.target.value.replace(/\s/g, "") } })}
              placeholder="CLI default"
              spellCheck={false}
              className="w-80 font-mono"
            />
          </Field>
        ) : (
          <Field
            label="Model"
            hint={
              <>
                Goes in the command itself. <ModelsLink {...ALL_MODELS} /> lists every provider's current model IDs.
              </>
            }
          >
            {null}
          </Field>
        )}
        {preset && <EffortField preset={preset} />}
      </Group>
      <Group title="What the command gets">
        <div className="py-3.5">
          <div className="text-[11.5px] leading-relaxed text-muted-foreground">Only when you click ✦, Explain Commit or Guided Review, and nothing else from the app:</div>
          <pre className="mt-2 rounded-md border border-border bg-background px-3 py-2 font-mono text-[11.5px] leading-relaxed whitespace-pre-wrap text-muted-foreground">
            {SUGGEST_PROMPT}
            {"\n\n"}
            <span className="text-subtle">
              [the diff: the staged changes, or every change when nothing is staged (Commit all), or the whole commit when amending; up to {SUGGEST_LIMIT_KB} KB, with a note in the
              prompt when it's cut]
            </span>
          </pre>
          <div className="mt-2 text-[11.5px] leading-relaxed text-muted-foreground">For a pull request:</div>
          <pre className="mt-2 rounded-md border border-border bg-background px-3 py-2 font-mono text-[11.5px] leading-relaxed whitespace-pre-wrap text-muted-foreground">
            {PULL_PROMPT}
            {"\n\n"}
            <span className="text-subtle">
              [the subjects of the branch's commits, the repository's pull request template if it has one, and the branch's diff since it left the base; up to {SUGGEST_LIMIT_KB} KB in all]
            </span>
          </pre>
          <div className="mt-2 text-[11.5px] leading-relaxed text-muted-foreground">For a guided review:</div>
          <pre className="mt-2 rounded-md border border-border bg-background px-3 py-2 font-mono text-[11.5px] leading-relaxed whitespace-pre-wrap text-muted-foreground">
            {GUIDE_PROMPT}
            {"\n\n"}
            <span className="text-subtle">
              [a commit's message, or the subjects of the branch's commits since it left the base (not uncommitted changes); a list of every changed file; then as many whole diffs as fit
              in {GUIDE_LIMIT_KB} KB, lockfiles and generated files left out. When any diff is left out, the whole patch goes in a temporary file only you can read, named in the prompt and
              deleted when the run ends. Claude Code also gets the answer's shape as <span className="font-mono">--json-schema</span>, and it and opencode are let read that file.]
            </span>
          </pre>
        </div>
      </Group>
    </>
  );
}
