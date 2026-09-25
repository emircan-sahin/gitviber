import { Plus, X } from "lucide-react";
import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Tip } from "@/components/ui/tooltip";
import { refreshOpenApps, useOpenApps } from "@/lib/app/openIn";
import { type CustomApp, updateSettings, useSettings } from "@/lib/settings";
import { Switch } from "@/components/ui/switch";
import { Kbd } from "@/components/ui/kbd";
import { Field, OptionSelect } from "@/features/settings/controls";

export function OpenInSection() {
  const s = useSettings();
  const { apps } = useOpenApps();
  useEffect(refreshOpenApps, []);
  const setCustom = (list: CustomApp[]) => updateSettings({ openInCustom: list });
  return (
    <>
      <Field label="Default app" hint="What a click on Open in (in the status bar) runs. Picking an app from its list makes that one the default." commands={["file.openIn"]}>
        <OptionSelect value={s.openInApp} options={{ "": "None: show the list", ...Object.fromEntries(apps.map((a) => [a.id, a.name])) }} onChange={(v) => updateSettings({ openInApp: v })} />
      </Field>
      <Field label="Show detected apps" hint="Editors, terminals and git clients found on this Mac. Turn off to list only your own apps.">
        <Switch checked={!s.openInHideBuiltins} onChange={(v) => updateSettings({ openInHideBuiltins: !v })} />
      </Field>
      <div className="py-3.5">
        <div className="text-[12.5px] font-medium">Your apps</div>
        <div className="mt-0.5 text-[11.5px] leading-relaxed text-muted-foreground">
          A command run directly, not through a shell. <Kbd>{"{path}"}</Kbd> is the worktree, <Kbd>{"{file}"}</Kbd> the open file (the worktree when none is), <Kbd>{"{line}"}</Kbd> the line in
          view. With none of them, the file or folder goes last. Example: <Kbd>{"code -g {file}:{line}"}</Kbd>
        </div>
        <div className="mt-3 flex flex-col gap-1.5">
          {s.openInCustom.map((app, i) => (
            <CustomAppRow
              key={app.id}
              app={app}
              onChange={(next) => setCustom(s.openInCustom.map((c, j) => (j === i ? next : c)))}
              onRemove={() => setCustom(s.openInCustom.filter((_, j) => j !== i))}
            />
          ))}
        </div>
        <Button variant="secondary" size="sm" className="mt-2" onClick={() => setCustom([...s.openInCustom, { id: `custom:${Date.now().toString(36)}`, name: "", command: "" }])}>
          <Plus /> Add app
        </Button>
      </div>
    </>
  );
}

/** Saved on leaving a field: every keystroke would re-render everything that reads settings. */
function CustomAppRow({ app, onChange, onRemove }: { app: CustomApp; onChange: (a: CustomApp) => void; onRemove: () => void }) {
  const [draft, setDraft] = useState(app);
  const commit = () => (draft.name !== app.name || draft.command !== app.command) && onChange(draft);
  const field = (key: "name" | "command") => ({
    value: draft[key],
    onChange: (e: React.ChangeEvent<HTMLInputElement>) => setDraft({ ...draft, [key]: e.target.value }),
    onBlur: commit,
    onKeyDown: (e: React.KeyboardEvent) => e.key === "Enter" && commit(),
  });
  return (
    <div className="flex items-center gap-1.5">
      <Input {...field("name")} placeholder="Name" className="w-36" />
      <Input {...field("command")} placeholder="nvim-qt {file}" className="flex-1 font-mono" spellCheck={false} />
      <Tip label="Remove">
        <Button variant="ghost" size="icon-sm" onClick={onRemove}>
          <X />
        </Button>
      </Tip>
    </div>
  );
}
