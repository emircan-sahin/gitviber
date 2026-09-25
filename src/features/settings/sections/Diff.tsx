import type { Whitespace } from "@/lib/api";
import { updateSettings, useSettings } from "@/lib/settings";
import { Segmented } from "@/components/ui/segmented";
import { Switch } from "@/components/ui/switch";
import { Field } from "@/features/settings/controls";

export function DiffSection() {
  const s = useSettings();
  return (
    <>
      <Field label="Layout" hint="Unified shows one column, split shows before and after side by side." commands={["diff.toggleSplit"]}>
        <Segmented<string>
          value={s.sideBySide ? "split" : "unified"}
          onChange={(v) => updateSettings({ sideBySide: v === "split" })}
          options={[
            { value: "unified", label: "Unified" },
            { value: "split", label: "Split" },
          ]}
          variant="field"
        />
      </Field>
      <Field label="Collapse unchanged lines" hint="Fold long runs of unchanged code between changes." commands={["diff.toggleCollapse"]}>
        <Switch checked={s.hideUnchanged} onChange={(v) => updateSettings({ hideUnchanged: v })} />
      </Field>
      <Field
        label="Ignore whitespace"
        hint="Changes hides re-indented lines and trailing spaces, like git diff -b. All ignores every space, like git diff -w."
        commands={["diff.toggleWhitespace"]}
      >
        <Segmented<string>
          value={s.ignoreWhitespace ? s.whitespaceMode : "off"}
          onChange={(v) => updateSettings(v === "off" ? { ignoreWhitespace: false } : { ignoreWhitespace: true, whitespaceMode: v as Whitespace })}
          options={[
            { value: "off", label: "Off" },
            { value: "amount", label: "Changes" },
            { value: "all", label: "All" },
          ]}
          variant="field"
        />
      </Field>
    </>
  );
}
