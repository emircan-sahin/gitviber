import { OPTION_KEYS, type OptionKey, updateSettings, useSettings } from "@/lib/settings";
import { Segmented } from "@/components/ui/segmented";
import { Field } from "@/features/settings/controls";

export function TerminalSection() {
  const s = useSettings();
  return (
    <Field
      label="Option as Meta"
      hint="Off, ⌥ types characters like @ or œ. As Meta, ⌥ keys go to the shell and agents instead (Claude Code's ⌥P, readline's ⌥B and ⌥F). Left ⌥ keeps the right one for the characters your layout needs."
    >
      <Segmented<string>
        value={s.optionAsMeta}
        onChange={(v) => updateSettings({ optionAsMeta: v as OptionKey })}
        options={Object.entries(OPTION_KEYS).map(([value, label]) => ({ value, label }))}
        variant="field"
      />
    </Field>
  );
}
