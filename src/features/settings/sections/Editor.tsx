import { CODE_FONT_WEIGHTS, type CodeFont, type CodeFontWeight, codeFontChoices, codeFontFamily, DEFAULT_FONT_SIZE, updateSettings, useSettings } from "@/lib/settings";
import { Segmented } from "@/components/ui/segmented";
import { Switch } from "@/components/ui/switch";
import { Field, FontPicker, Group, SizeStepper } from "@/features/settings/controls";

const SAMPLE = `const total = items.filter((x) => x.price >= 10)
  .reduce((sum, x) => sum + x.price, 0); // != === =>`;

export function EditorSection() {
  const s = useSettings();
  return (
    <>
      <pre
        className="mt-4 overflow-hidden rounded-md border border-border bg-background px-3 py-2 whitespace-pre text-muted-foreground"
        style={{
          fontFamily: codeFontFamily(s),
          fontSize: s.codeFontSize,
          fontWeight: s.codeFontWeight,
          lineHeight: `${Math.round(s.codeFontSize * s.lineHeight)}px`,
          fontVariantLigatures: s.ligatures ? "normal" : "none",
        }}
      >
        {SAMPLE}
      </pre>
      <Group title="Font">
        <Field label="Font">
          <FontPicker
            fonts={codeFontChoices}
            value={s.codeFont}
            custom={s.customCodeFont}
            onChange={(codeFont, customCodeFont) => updateSettings({ codeFont: codeFont as CodeFont, customCodeFont })}
          />
        </Field>
        <Field label="Font size" commands={["editor.fontZoomIn", "editor.fontZoomOut", "editor.fontZoomReset"]}>
          <SizeStepper value={s.codeFontSize} step={0.5} fallback={DEFAULT_FONT_SIZE} onChange={(v) => updateSettings({ codeFontSize: v })} />
        </Field>
        <Field label="Font weight" hint="The code view, diffs and the terminal. Bold text stays bolder.">
          <Segmented<string>
            value={String(s.codeFontWeight)}
            onChange={(v) => updateSettings({ codeFontWeight: Number(v) as CodeFontWeight })}
            options={Object.entries(CODE_FONT_WEIGHTS).map(([value, label]) => ({ value, label }))}
            variant="field"
          />
        </Field>
        <Field label="Line height">
          <Segmented<string>
            value={String(s.lineHeight)}
            onChange={(v) => updateSettings({ lineHeight: Number(v) })}
            options={[
              { value: "1.4", label: "Compact" },
              { value: "1.6", label: "Comfortable" },
              { value: "1.8", label: "Relaxed" },
            ]}
            variant="field"
          />
        </Field>
        <Field label="Font ligatures" hint="Joins pairs like => and != in fonts that have them (JetBrains Mono).">
          <Switch checked={s.ligatures} onChange={(v) => updateSettings({ ligatures: v })} />
        </Field>
      </Group>
      <Group title="Behavior">
        <Field label="Word wrap" hint="Wrap long lines instead of scrolling sideways." commands={["editor.toggleWrap"]}>
          <Switch checked={s.wordWrap} onChange={(v) => updateSettings({ wordWrap: v })} />
        </Field>
        <Field label="Open Markdown as preview" hint="Opening a .md file shows it rendered. Diffs of Markdown files always start on the diff.">
          <Switch checked={s.markdownPreview} onChange={(v) => updateSettings({ markdownPreview: v })} />
        </Field>
      </Group>
    </>
  );
}
