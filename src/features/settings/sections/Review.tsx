import { useState } from "react";
import { Input } from "@/components/ui/input";
import { effortLevels, effortOf, GUIDE_LIMIT_KB, modelOf, presetOf, programOf, reviewAgent, SUGGEST_PRESETS, type SuggestPreset as Preset } from "@/lib/git/suggest";
import { guidePrompt } from "@/lib/review/guide";
import { updateSettings, useSettings } from "@/lib/settings";
import { Field, Group, OptionSelect } from "@/features/settings/controls";
import { CommandPicker, CustomModelField, ModelsLink } from "./Commit";

/** Languages by the name the prompt gives, each shown in its own. */
const LANGUAGES: Record<string, string> = {
  English: "English",
  Turkish: "Türkçe",
  German: "Deutsch",
  French: "Français",
  Spanish: "Español",
  Portuguese: "Português",
  Italian: "Italiano",
  Japanese: "日本語",
  Chinese: "中文",
  Korean: "한국어",
  Russian: "Русский",
};
const CUSTOM = "custom";
// Not an effort level of any preset.
const SAME = "same";

// The last command of its own, back when "Same as" is undone, until GitViber quits.
let lastOwn: string | null = null;

/** A preset's entry with `key` taken out: Commit Messages' then applies again. */
const without = (entries: Partial<Record<Preset, string>>, key: Preset) => Object.fromEntries(Object.entries(entries).filter(([k]) => k !== key));

export function ReviewSection() {
  const s = useSettings();
  const { command, models } = reviewAgent(s);
  const preset = presetOf(command);
  const levels = preset ? effortLevels(preset) : [];
  const same = `Same as Commit Messages (${programOf(s.suggestCommand) || "none"})`;
  // Custom stays picked while its text happens to name a listed language.
  const [picked, setPicked] = useState(false);
  const customLanguage = picked || !Object.hasOwn(LANGUAGES, s.reviewLanguage);
  return (
    <>
      <Group title="Agent">
        <Field
          label="Run with"
          hint={`The agent CLI that writes Explain Commit (History) and Guided Review (branch review).${s.suggestEnabled ? "" : " Both are off while Suggest commit messages is, under Commit Messages."}`}
        >
          <OptionSelect
            className="w-80"
            value={s.reviewCommand === null ? SAME : "own"}
            options={{ [SAME]: same, own: "Its own command" }}
            // Its own starts from the one it had, else Commit Messages' command, to change from there.
            onChange={(v) => {
              if (s.reviewCommand !== null) lastOwn = s.reviewCommand;
              updateSettings({ reviewCommand: v === SAME ? null : (lastOwn ?? s.suggestCommand) });
            }}
          />
        </Field>
        {s.reviewCommand !== null && (
          <Field label="Command" hint="Runs as Commit Messages' does: in the repository's folder, directly, not through a shell.">
            <CommandPicker command={s.reviewCommand} onChange={(reviewCommand) => updateSettings({ reviewCommand })} />
          </Field>
        )}
        {preset ? (
          <Field
            label="Model"
            hint={
              <>
                Empty runs Commit Messages' model for {SUGGEST_PRESETS[preset].label}. <ModelsLink {...SUGGEST_PRESETS[preset].models} /> has the current IDs.
              </>
            }
          >
            <Input
              aria-label="Model"
              value={s.reviewModels[preset] ?? ""}
              onChange={(e) => {
                const model = e.target.value.replace(/\s/g, "");
                updateSettings({ reviewModels: model ? { ...s.reviewModels, [preset]: model } : without(s.reviewModels, preset) });
              }}
              placeholder={`Same as Commit Messages: ${modelOf(preset, models) || "CLI default"}`}
              spellCheck={false}
              className="w-80 font-mono"
            />
          </Field>
        ) : (
          <CustomModelField />
        )}
        {preset && levels.length > 0 && (
          <Field label="Effort" hint="A guided review reads the whole change: more effort can catch more, slower and at a higher cost.">
            <OptionSelect
              className="w-80"
              value={s.reviewEfforts[preset] ?? SAME}
              options={{ [SAME]: `Same as Commit Messages (${effortOf(preset, s.suggestEfforts) || "CLI default"})`, "": "CLI default", ...Object.fromEntries(levels.map((l) => [l, l])) }}
              onChange={(v) => updateSettings({ reviewEfforts: v === SAME ? without(s.reviewEfforts, preset) : { ...s.reviewEfforts, [preset]: v } })}
            />
          </Field>
        )}
      </Group>
      <Group title="Writing">
        <Field label="Language" hint="The guide's prose is written in it; code, paths and names stay as they are. Commit messages and pull requests stay in English.">
          <OptionSelect
            className="w-80"
            value={customLanguage ? CUSTOM : s.reviewLanguage}
            options={{ ...LANGUAGES, [CUSTOM]: "Custom…" }}
            onChange={(v) => {
              setPicked(v === CUSTOM);
              if (v !== CUSTOM) updateSettings({ reviewLanguage: v });
            }}
          />
        </Field>
        {customLanguage && (
          <Field label="Custom language" hint="Its name, as you'd say it to the agent: Ukrainian, Brazilian Portuguese, Bahasa Indonesia.">
            <Input aria-label="Custom language" value={s.reviewLanguage} onChange={(e) => updateSettings({ reviewLanguage: e.target.value })} placeholder="English" maxLength={40} className="w-80" />
          </Field>
        )}
      </Group>
      <Group title="What the command gets">
        <div className="py-3.5">
          <div className="text-[11.5px] leading-relaxed text-muted-foreground">Only when you click Explain Commit or Guided Review, and nothing else from the app:</div>
          <pre className="mt-2 rounded-md border border-border bg-background px-3 py-2 font-mono text-[11.5px] leading-relaxed whitespace-pre-wrap text-muted-foreground">
            {guidePrompt(s.reviewLanguage)}
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
