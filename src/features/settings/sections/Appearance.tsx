import { IS_MAC } from "@/lib/platform";
import {
  type Appearance,
  LIGHT_SYNTAX_THEMES,
  type LightSyntaxTheme,
  type Settings,
  SYNTAX_THEMES,
  type SyntaxTheme,
  UI_FONTS,
  UI_SCALES,
  type UiFont,
  updateSettings,
  useSettings,
} from "@/lib/settings";
import { Segmented } from "@/components/ui/segmented";
import { Field, FontPicker, OptionSelect } from "@/features/settings/controls";

// What the System theme follows.
const DESKTOP = IS_MAC ? "macOS" : "your desktop";

export function AppearanceSection() {
  const s = useSettings();
  return (
    <>
      <Field label="Theme" hint={`System follows ${DESKTOP}. Dimmed is a softer, lighter dark.`}>
        <Segmented<Appearance>
          value={s.appearance}
          onChange={(v) => updateSettings({ appearance: v })}
          options={[
            { value: "system", label: "System" },
            { value: "light", label: "Light" },
            { value: "dark", label: "Dark" },
            { value: "dim", label: "Dimmed" },
          ]}
          variant="field"
        />
      </Field>
      {s.appearance === "system" && (
        <Field label="Dark variant" hint={`The dark theme System uses while ${DESKTOP} is dark.`}>
          <Segmented<Settings["darkVariant"]>
            value={s.darkVariant}
            onChange={(v) => updateSettings({ darkVariant: v })}
            options={[
              { value: "dark", label: "Dark" },
              { value: "dim", label: "Dimmed" },
            ]}
            variant="field"
          />
        </Field>
      )}
      {/* Each appearance keeps its own syntax theme, so switching back restores it. */}
      <Field label="Dark syntax theme" hint="Code colors while the app is dark.">
        <OptionSelect value={s.syntaxTheme} options={SYNTAX_THEMES} onChange={(v) => updateSettings({ syntaxTheme: v as SyntaxTheme })} />
      </Field>
      <Field label="Light syntax theme" hint="Code colors while the app is light.">
        <OptionSelect value={s.lightSyntaxTheme} options={LIGHT_SYNTAX_THEMES} onChange={(v) => updateSettings({ lightSyntaxTheme: v as LightSyntaxTheme })} />
      </Field>
      <Field label="Interface font" hint="The code font is under Editor.">
        <FontPicker
          fonts={Object.keys(UI_FONTS)}
          value={s.uiFont}
          custom={s.customUiFont}
          onChange={(uiFont, customUiFont) => updateSettings({ uiFont: uiFont as UiFont, customUiFont })}
        />
      </Field>
      <Field label="Interface scale" hint="Zooms the whole window. The code font size stays its own setting." commands={["view.zoomIn", "view.zoomOut", "view.zoomReset"]}>
        <Segmented<string>
          value={String(s.uiScale)}
          onChange={(v) => updateSettings({ uiScale: Number(v) })}
          options={UI_SCALES.map((z) => ({ value: String(z), label: `${Math.round(z * 100)}%` }))}
          variant="field"
        />
      </Field>
    </>
  );
}
