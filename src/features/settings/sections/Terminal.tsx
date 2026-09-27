import { OPTION_KEYS, type OptionKey, updateSettings, useSettings } from "@/lib/settings";
import { Field, OptionSelect } from "@/features/settings/controls";

/** macOS only: Linux sends Alt as Meta already. */
export function TerminalSection() {
  const s = useSettings();
  return (
    <Field
      label="Option key"
      hint="As Meta, ⌥ keys go to the shell and agents (Claude Code's ⌥P, readline's ⌥B and ⌥F) instead of typing characters like @ or œ. Left only keeps the right ⌥ for the characters your layout needs."
    >
      <OptionSelect value={s.optionAsMeta} options={OPTION_KEYS} onChange={(v) => updateSettings({ optionAsMeta: v as OptionKey })} />
    </Field>
  );
}
