import { ChevronDown } from "lucide-react";
import { useRef, useState } from "react";
import { DropdownMenu, DropdownMenuContent, DropdownMenuRadioGroup, DropdownMenuRadioItem, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { Input } from "@/components/ui/input";
import { github } from "@/lib/api";
import { updateSettings, useSettings } from "@/lib/settings";
import { ALL_MODELS, modelOf, presetOf, SUGGEST_LIMIT_KB, SUGGEST_PRESETS, SUGGEST_PROMPT, type SuggestPreset as Preset } from "@/lib/git/suggest";
import { failed } from "@/lib/app/toast";
import { cn } from "@/lib/utils";
import { segmentClass } from "@/components/ui/segmented";
import { Switch } from "@/components/ui/switch";
import { Field } from "@/features/settings/controls";

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
          <button key={k} onClick={() => onChange(k)} className={segmentClass("field", k === value)}>
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
      <button onClick={() => onChange("custom")} className={segmentClass("field", value === "custom")}>
        Custom
      </button>
    </div>
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
      <Field
        label="Suggest commit messages"
        hint="Adds a ✦ button to the commit box that asks your own agent CLI to write the message. GitViber sends nothing itself and keeps no keys: the command runs on this Mac, and it decides where the diff goes."
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
      <div className="py-3.5">
        <div className="text-[12.5px] font-medium">What the command gets</div>
        <div className="mt-0.5 text-[11.5px] leading-relaxed text-muted-foreground">Only when you click ✦, and nothing else from the app:</div>
        <pre className="mt-2 rounded-md border border-border bg-background px-3 py-2 font-mono text-[11.5px] leading-relaxed whitespace-pre-wrap text-muted-foreground">
          {SUGGEST_PROMPT}
          {"\n\n"}
          <span className="text-subtle">
            [the diff: the staged changes, or every change when nothing is staged (Commit all), or the whole commit when amending; up to {SUGGEST_LIMIT_KB} KB, with a note in the
            prompt when it's cut]
          </span>
        </pre>
      </div>
    </>
  );
}
