import { AlertTriangle, CircleX, Send, Trash2, X } from "lucide-react";
import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { Tip } from "@/components/ui/tooltip";
import { browserApi, type ConsoleEntry } from "@/lib/api";
import { failed } from "@/lib/app/toast";
import { formatErrors, type ShownAs } from "@/lib/browser/format";
import { useLogged } from "@/lib/browser/store";
import { pageLabel } from "@/lib/browser/url";
import { cn } from "@/lib/utils";
import { type AskAbout, AskPopover, errorsAbout } from "./AskPopover";

/** The console's badge in the address bar: errors (else warnings) since it was last cleared. */
export function ConsoleBadge({ id, open, onToggle }: { id: string; open: boolean; onToggle: () => void }) {
  const { errors, warnings } = useLogged(id);
  const count = errors || warnings;
  return (
    <Tip label={errors ? `${errors} errors on the page` : warnings ? `${warnings} warnings on the page` : "The page's console"}>
      <Button type="button" variant="ghost" size="sm" aria-pressed={open} onClick={onToggle} className={cn("h-6 gap-1 px-1.5", open && "bg-active text-foreground")}>
        {errors ? <CircleX className="text-destructive" /> : <AlertTriangle className={warnings ? "text-modified" : undefined} />}
        {count > 0 && <span className="font-mono text-[11px]">{count > 99 ? "99+" : count}</span>}
      </Button>
    </Tip>
  );
}

/**
 * What the page logged, under the address: the latest errors and warnings, each load marked; its
 * errors go to the worktree's agent with a question, asked beside Send Errors (`onAsk` as it opens).
 * A panel, not a popover, which would have the page step aside.
 */
export function ConsolePanel({
  id,
  root,
  page,
  onAsk,
  onClose,
}: {
  id: string;
  root: string;
  /** The page as it shows now, which the errors name. */
  page: { url: string; device: ShownAs | null };
  onAsk: () => void;
  onClose: () => void;
}) {
  const [asking, setAsking] = useState<AskAbout | null>(null);
  const counts = useLogged(id);
  const [entries, setEntries] = useState<ConsoleEntry[]>([]);
  // Read again as lines come: the badge's counts change with each.
  useEffect(() => {
    void browserApi.console(id).then(setEntries, failed("Could not read the page's console"));
  }, [id, counts.errors, counts.warnings]);
  const errors = formatErrors(entries, page);
  const shown = entries.slice(-100);
  return (
    <div className="flex max-h-56 shrink-0 flex-col border-b border-border text-[11.5px]">
      <div className="flex h-8 shrink-0 items-center gap-1 px-2">
        <span className="mr-auto font-medium">Console</span>
        <AskPopover root={root} about={asking} onClose={() => setAsking(null)}>
          <Button
            type="button"
            variant="ghost"
            size="sm"
            disabled={!errors}
            onClick={() => {
              onAsk();
              setAsking(errorsAbout(errors));
            }}
          >
            <Send /> Send Errors to Agent
          </Button>
        </AskPopover>
        <Button type="button" variant="ghost" size="sm" disabled={!entries.length} onClick={() => void browserApi.consoleClear(id).then(() => setEntries([]), failed("Could not clear the console"))}>
          <Trash2 /> Clear
        </Button>
        <Tip label="Close">
          <Button type="button" variant="ghost" size="icon-sm" aria-label="Close" onClick={onClose}>
            <X />
          </Button>
        </Tip>
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto px-2 pb-1.5 font-mono">
        {shown.length === 0 && <div className="py-2 text-subtle">Nothing logged since the console was last cleared.</div>}
        {shown.map((e, i) =>
          e.level === "load" ? (
            <div key={i} className="mt-1 border-t border-border pt-1 text-[10.5px] text-subtle">
              {pageLabel(e.url)}
            </div>
          ) : (
            <div key={i} className={cn("flex gap-1.5 py-0.5 select-text", e.level === "error" ? "text-destructive" : "text-modified")} title={e.stack || undefined}>
              {e.level === "error" ? <CircleX className="mt-0.5 size-3 shrink-0" /> : <AlertTriangle className="mt-0.5 size-3 shrink-0" />}
              <span className="min-w-0 break-words whitespace-pre-wrap">{e.msg}</span>
            </div>
          ),
        )}
      </div>
    </div>
  );
}
