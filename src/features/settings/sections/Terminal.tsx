import {
  DEFAULT_TERMINAL_FONT_SIZE,
  DIM_LEVELS,
  OPTION_KEYS,
  type OptionKey,
  SCROLLBACK_LINES,
  TERMINAL_CURSORS,
  TERMINAL_FONT_MAX,
  TERMINAL_FONT_MIN,
  TERMINAL_LINE_HEIGHTS,
  type TerminalCursor,
  type TerminalFont,
  terminalFontChoices,
  terminalFontFamily,
  updateSettings,
  useSettings,
} from "@/lib/settings";
import { Segmented } from "@/components/ui/segmented";
import { Switch } from "@/components/ui/switch";
import { Field, FontPicker, Group, OptionSelect, SizeStepper } from "@/features/settings/controls";
import { IS_MAC } from "@/lib/platform";

export function TerminalSection() {
  const s = useSettings();
  return (
    <>
      <pre
        className="mt-4 overflow-hidden rounded-md border border-border bg-background px-3 py-2 whitespace-pre text-muted-foreground"
        style={{ fontFamily: terminalFontFamily(s), fontSize: s.terminalFontSize, fontWeight: s.codeFontWeight, lineHeight: s.terminalLineHeight }}
      >
        <span className="text-primary">~/project</span> <span className="text-added">main</span> ❯ git status --short{"\n"} M src/app.ts{"\n"}?? notes.md
      </pre>
      <Group title="Font">
        <Field label="Font" hint="Same as editor follows the code font. The font weight is the editor's.">
          <FontPicker
            fonts={terminalFontChoices}
            labels={{ Editor: "Same as editor" }}
            value={s.terminalFont}
            custom={s.customTerminalFont}
            onChange={(terminalFont, customTerminalFont) => updateSettings({ terminalFont: terminalFont as TerminalFont, customTerminalFont })}
          />
        </Field>
        <Field
          label="Font size"
          hint="Separate from the code font size. With the terminal focused, the zoom keys, a pinch or Ctrl+scroll change it too."
          commands={["terminal.fontZoomIn", "terminal.fontZoomOut", "terminal.fontZoomReset"]}
        >
          <SizeStepper
            value={s.terminalFontSize}
            step={1}
            fallback={DEFAULT_TERMINAL_FONT_SIZE}
            min={TERMINAL_FONT_MIN}
            max={TERMINAL_FONT_MAX}
            onChange={(v) => updateSettings({ terminalFontSize: v })}
          />
        </Field>
        <Field label="Line height" hint="Taller lines fit fewer rows in a pane.">
          <Segmented<string>
            value={String(s.terminalLineHeight)}
            onChange={(v) => updateSettings({ terminalLineHeight: Number(v) })}
            options={TERMINAL_LINE_HEIGHTS.map((h) => ({ value: String(h), label: h.toFixed(1) }))}
            variant="field"
          />
        </Field>
      </Group>
      <Group title="Cursor">
        <Field label="Style">
          <Segmented<TerminalCursor>
            value={s.terminalCursor}
            onChange={(v) => updateSettings({ terminalCursor: v })}
            options={Object.entries(TERMINAL_CURSORS).map(([value, label]) => ({ value: value as TerminalCursor, label }))}
            variant="field"
          />
        </Field>
        <Field label="Blink">
          <Switch checked={s.terminalCursorBlink} onChange={(v) => updateSettings({ terminalCursorBlink: v })} />
        </Field>
      </Group>
      <Group title="Panes">
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
        <Field label="Scrollback" hint="Lines of history each terminal keeps.">
          <OptionSelect
            value={String(s.terminalScrollback)}
            options={Object.fromEntries(SCROLLBACK_LINES.map((n) => [n, `${n.toLocaleString("en-US")} lines`]))}
            onChange={(v) => updateSettings({ terminalScrollback: Number(v) })}
          />
        </Field>
      </Group>
      <Group title="Shell">
        <Field
          label="Shell integration"
          hint="Lets zsh and bash 4.4+ mark where each command starts and ends, without changing your dotfiles: a dot beside each command, red when it failed, keys to jump between them, and Copy Last Command Output on right-click. fish marks its own. Terminals opened after a change pick it up."
          commands={["terminal.prevCommand", "terminal.nextCommand"]}
        >
          <Switch checked={s.shellIntegration} onChange={(v) => updateSettings({ shellIntegration: v })} />
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
      </Group>
    </>
  );
}
