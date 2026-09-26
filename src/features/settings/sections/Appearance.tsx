import { IS_MAC } from "@/lib/platform";
import {
  type Appearance,
  DARK_THEMES,
  type DarkTheme,
  LIGHT_SYNTAX_THEMES,
  LIGHT_THEMES,
  type LightSyntaxTheme,
  type LightTheme,
  SYNTAX_THEMES,
  type SyntaxTheme,
  THEMES,
  UI_SCALES,
  type UiFont,
  uiFontChoices,
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
      <Field label="Theme" hint={`System follows ${DESKTOP} between the light and dark themes below.`}>
        <Segmented<Appearance>
          value={s.appearance}
          onChange={(v) => updateSettings({ appearance: v })}
          options={[
            { value: "system", label: "System" },
            { value: "light", label: "Light" },
            { value: "dark", label: "Dark" },
          ]}
          variant="field"
        />
      </Field>
      {/* Picking a theme picks its own syntax theme too; either can be changed after. */}
      {s.appearance !== "light" && (
        <Field label="Dark theme" hint="The whole app's colors while it's dark. Dimmed is a softer, lighter dark.">
          <OptionSelect value={s.darkTheme} options={DARK_THEMES} onChange={(v) => updateSettings({ darkTheme: v as DarkTheme, syntaxTheme: THEMES[v as DarkTheme].syntax })} />
        </Field>
      )}
      {s.appearance !== "dark" && (
        <Field label="Light theme" hint="The whole app's colors while it's light.">
          <OptionSelect
            value={s.lightTheme}
            options={LIGHT_THEMES}
            onChange={(v) => updateSettings({ lightTheme: v as LightTheme, lightSyntaxTheme: THEMES[v as LightTheme].syntax })}
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
          fonts={uiFontChoices}
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
