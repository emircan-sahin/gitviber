import { DIM_LEVELS, OPTION_KEYS, type OptionKey, updateSettings, useSettings } from "@/lib/settings";
import { Segmented } from "@/components/ui/segmented";
import { Switch } from "@/components/ui/switch";
import { Field } from "@/features/settings/controls";
import { IS_MAC } from "@/lib/platform";

export function TerminalSection() {
  const s = useSettings();
  return (
    <>
      <Field
        label="Shell integration"
        hint="Lets zsh and bash 4.4+ mark where each command starts and ends, without changing your dotfiles: a dot beside each command, red when it failed, keys to jump between them, and Copy Last Command Output on right-click. fish marks its own. Terminals opened after a change pick it up."
        commands={["terminal.prevCommand", "terminal.nextCommand"]}
      >
        <Switch checked={s.shellIntegration} onChange={(v) => updateSettings({ shellIntegration: v })} />
      </Field>
      <Field
        label="Dim unfocused panes"
        hint="In a split terminal, the panes other than the one you type into fade, so it's clear where the keys go."
        commands={["terminal.split", "terminal.splitDown"]}
      >
        <Segmented<string>
          value={String(s.terminalInactiveDim)}
          onChange={(v) => updateSettings({ terminalInactiveDim: Number(v) })}
          options={DIM_LEVELS.map((d) => ({ value: String(d), label: d ? `${d}%` : "Off" }))}
          variant="field"
        />
      </Field>
      {IS_MAC && (
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
      )}
    </>
  );
}
