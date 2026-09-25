import { useMemo, useState } from "react";
import { Input } from "@/components/ui/input";
import { bindingsFor, type CommandId, formatChord } from "@/lib/commands/commands";
import { cleanFontName, useSettings } from "@/lib/settings";
import { Select } from "@/components/ui/select";
import { Kbd } from "@/components/ui/kbd";

export function Field({ label, hint, commands, children }: { label: string; hint?: React.ReactNode; commands?: CommandId[]; children: React.ReactNode }) {
  const { keybindings } = useSettings();
  const keys = commands?.map((id) => bindingsFor(id, keybindings)[0]).filter(Boolean) ?? [];
  return (
    <div className="flex flex-wrap items-center gap-x-6 gap-y-2 border-b border-border py-3.5 last:border-0">
      <div className="min-w-48 flex-1">
        <div className="text-[12.5px] font-medium">{label}</div>
        {hint && <div className="mt-0.5 text-[11.5px] leading-relaxed text-muted-foreground">{hint}</div>}
        {keys.length > 0 && (
          <div className="mt-1.5 flex gap-1">
            {keys.map((k) => (
              <Kbd key={k}>{formatChord(k)}</Kbd>
            ))}
          </div>
        )}
      </div>
      <div className="shrink-0">{children}</div>
    </div>
  );
}

export function OptionSelect({ value, options, onChange }: { value: string; options: Record<string, string>; onChange: (v: string) => void }) {
  return (
    <Select value={value} onChange={(e) => onChange(e.target.value)} className="w-52">
      {Object.entries(options).map(([id, label]) => (
        <option key={id} value={id}>
          {label}
        </option>
      ))}
    </Select>
  );
}

/** A preset font, or Custom with a field for any installed font's name. */
export function FontPicker({ fonts, value, custom, onChange }: { fonts: string[]; value: string; custom: string; onChange: (font: string, custom: string) => void }) {
  const [draft, setDraft] = useState(custom);
  const name = cleanFontName(draft);
  const missing = useMemo(() => !!name && !fontInstalled(name), [name]);
  // Applied on Enter or leaving the field: every partial name on the way would re-lay out the code view.
  const commit = () => name !== custom && onChange("Custom", name);
  return (
    <div className="flex w-52 flex-col gap-1.5">
      <OptionSelect value={value} options={Object.fromEntries([...fonts, "Custom"].map((f) => [f, f]))} onChange={(v) => onChange(v, custom)} />
      {value === "Custom" && (
        <>
          <Input value={draft} placeholder="Installed font name" onChange={(e) => setDraft(e.target.value)} onBlur={commit} onKeyDown={(e) => e.key === "Enter" && commit()} />
          {missing && <div className="text-[11px] leading-snug text-modified">Not installed: the default font shows instead.</div>}
        </>
      )}
    </div>
  );
}

/** Whether a font is installed: text set in it measures differently from every generic fallback. */
function fontInstalled(name: string) {
  const ctx = document.createElement("canvas").getContext("2d")!;
  const width = (font: string) => {
    ctx.font = `40px ${font}`;
    return ctx.measureText("mmmwwwiiilll0O@").width;
  };
  return ["monospace", "serif", "sans-serif"].some((generic) => width(`"${name}", ${generic}`) !== width(generic));
}
