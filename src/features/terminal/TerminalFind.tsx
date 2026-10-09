import { SquareTerminal } from "lucide-react";
import { FindBox, useFindBox } from "@/components/FindBox";
import { type FindOptions, NO_OPTIONS } from "@/lib/ui/findQuery";
import { Button } from "@/components/ui/button";
import { useEffect, useState } from "react";
import { clearFind, dismissRestore, endFind, findInTerminal, restoreSession, resumable, useTerminals } from "@/lib/terminal/terminals";
import { useSettings } from "@/lib/settings";
import { folderName } from "@/lib/path";
import { plural } from "@/lib/format";
import { overlaysChanged } from "@/lib/ui/overlays";

/** Find (⌘F with focus in the terminal): the focused pane's text, its scrollback included. */
export function TerminalFind() {
  const box = useFindBox("terminal");
  const [query, setQuery] = useState("");
  const [options, setOptions] = useState(NO_OPTIONS);
  const [at, setAt] = useState<{ index: number; total: number } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const search = (q: string, o: FindOptions, step: 0 | 1 | -1) => setError(findInTerminal(q, o, step, setAt));
  // Opened again: the last query's matches come back.
  useEffect(() => {
    if (box.asked && query) search(query, options, 0);
    // Only when Find asks.
  }, [box.asked]);
  // Another tab or pane: the box searches that one (the last one's marks go with it).
  const { groups, active } = useTerminals();
  const pane = groups.find((g) => g.id === active)?.focused;
  useEffect(() => {
    if (box.open && query) search(query, options, 0);
    else clearFind();
    // Only when the pane changes.
  }, [pane]);
  // The panel hidden: no marks left behind, nothing reporting to this box.
  useEffect(() => clearFind, []);
  useEffect(overlaysChanged, [box.open]);
  if (!box.open) return null;
  return (
    <div data-overlay className="absolute top-2 right-5 z-10">
      <FindBox
        query={query}
        onQuery={(q) => {
          setQuery(q);
          search(q, options, 0);
        }}
        options={options}
        onOptions={(o) => {
          setOptions(o);
          search(query, o, 0);
        }}
        at={at}
        error={error}
        onStep={(dir) => search(query, options, dir)}
        onClose={() => {
          box.close();
          endFind();
        }}
        focus={box.asked}
      />
    </div>
  );
}

/** Offers last run's terminals. Asked, not automatic: each one starts a shell. */
export function TerminalRestoreOffer() {
  const { restorable } = useTerminals();
  // What the restore does with agents follows the setting.
  useSettings();
  useEffect(overlaysChanged, [restorable]);
  if (!restorable) return null;
  const cwds = restorable.groups.flatMap((g) => g.panes.map((p) => p.cwd));
  const agents = resumable(restorable).size;
  const folders = [...new Set(cwds.map((c) => folderName(c) || c))];
  return (
    <div data-overlay className="pointer-events-auto fixed right-4 bottom-10 z-50 flex w-96 gap-3 rounded-md border border-border-strong bg-elevated p-3 shadow-lg shadow-black/50 animate-in fade-in-0 slide-in-from-bottom-2">
      <SquareTerminal className="mt-0.5 size-4 shrink-0 text-primary" />
      <div className="min-w-0 flex-1">
        <div className="text-[12px] font-medium">
          Restore {plural(cwds.length, "terminal")} from last session{agents > 0 && <> and resume {plural(agents, "agent")}</>}?
        </div>
        <div className="mt-1 truncate text-[11.5px] text-muted-foreground">{folders.join(", ")}</div>
        <div className="mt-2.5 flex gap-2">
          <Button size="sm" onClick={() => void restoreSession()}>
            Restore
          </Button>
          <Button size="sm" variant="secondary" onClick={dismissRestore}>
            Dismiss
          </Button>
        </div>
      </div>
    </div>
  );
}
