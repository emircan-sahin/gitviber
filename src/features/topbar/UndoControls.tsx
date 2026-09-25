import { ChevronDown, Redo2, TriangleAlert, Undo2 } from "lucide-react";
import { useState } from "react";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Tip, Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import type { JournalEntry } from "@/lib/api";
import { useCommands, useShortcut } from "@/lib/commands/keybindings";
import { toast } from "@/lib/app/toast";
import { travel } from "@/lib/repo/undo";
import type { RepoData } from "@/lib/repo/useRepo";
import { cn } from "@/lib/utils";
import { relativeTime } from "@/lib/format";

/** What the undo history covers; the empty history and the "Nothing to undo" toast both say it. */
const UNDOABLE = "Commits, merges, pulls, discards, and branch and tag changes made in GitViber";

/**
 * Undo and redo for the git actions taken in the app, with ⌘Z / ⇧⌘Z, and their history:
 * picking an entry undoes it and everything after it (or redoes up to it).
 */
export function UndoControls({ repo, disabled }: { repo: RepoData; disabled: boolean }) {
  const [moving, setMoving] = useState(false);
  const undoKey = useShortcut("git.undo");
  const redoKey = useShortcut("git.redo");
  const journal = repo.journal;
  const undos = journal?.undo ?? [];
  const redos = journal?.redo ?? [];
  const off = disabled || moving;

  const go = async (forward: boolean, ids: number[]) => {
    setMoving(true);
    try {
      await travel(forward, ids, repo.refresh);
    } finally {
      setMoving(false);
    }
  };
  // The shortcut says why nothing happened; the buttons are disabled instead.
  const next = (forward: boolean) => {
    const e = forward ? redos[0] : undos[0];
    const blocked = forward ? journal?.redoBlocked : journal?.undoBlocked;
    const verb = forward ? "redo" : "undo";
    if (off) return;
    if (!e) toast("info", `Nothing to ${verb}`, `${UNDOABLE} can be undone.`);
    else if (blocked) toast("error", `Can't ${verb} ${e.label}`, blocked);
    else void go(forward, [e.id]);
  };
  useCommands({ "git.undo": () => next(false), "git.redo": () => next(true) });

  const button = (forward: boolean) => {
    const e = forward ? redos[0] : undos[0];
    const blocked = forward ? journal?.redoBlocked : journal?.undoBlocked;
    const verb = forward ? "Redo" : "Undo";
    return (
      <Tooltip>
        <TooltipTrigger asChild>
          <span>
            <Button variant="ghost" size="icon" aria-label={verb} disabled={off || !e || !!blocked} onClick={() => e && go(forward, [e.id])}>
              {forward ? <Redo2 /> : <Undo2 />}
            </Button>
          </span>
        </TooltipTrigger>
        <TooltipContent className="max-w-80">
          {!e ? `Nothing to ${verb.toLowerCase()}` : blocked ? `Can't ${verb.toLowerCase()} ${e.label}. ${blocked}` : `${verb} ${e.label}`}
          <span className="ml-2 font-mono text-[11px] text-subtle">{forward ? redoKey : undoKey}</span>
        </TooltipContent>
      </Tooltip>
    );
  };

  const row = (e: JournalEntry, forward: boolean, ids: number[], blocked: boolean) => (
    <DropdownMenuItem
      key={e.id}
      disabled={blocked}
      onSelect={() => go(forward, ids)}
      title={forward ? `Redo up to ${e.label}` : `Undo back to before ${e.label}`}
      className={cn(forward && "text-subtle")}
    >
      {forward ? <Redo2 /> : <Undo2 />}
      <span className="min-w-0 flex-1 truncate">{e.label}</span>
      <span className="shrink-0 text-[11px] opacity-70">{relativeTime(e.time)}</span>
    </DropdownMenuItem>
  );
  const blocked = journal?.undoBlocked ?? journal?.redoBlocked;

  return (
    <div className="flex shrink-0 items-center">
      {button(false)}
      {button(true)}
      <DropdownMenu>
        <Tip label="Undo history">
          <DropdownMenuTrigger asChild>
            <Button variant="ghost" className="w-5 px-0" disabled={off} aria-label="Undo history">
              <ChevronDown className="size-3" />
            </Button>
          </DropdownMenuTrigger>
        </Tip>
        <DropdownMenuContent align="end" className="w-80">
          <DropdownMenuLabel>Undo history</DropdownMenuLabel>
          {!undos.length && !redos.length && (
            <div className="px-2 py-1.5 text-[12px] text-muted-foreground">{UNDOABLE} show up here, to undo and redo.</div>
          )}
          {/* Furthest redo on top, so the list reads newest to oldest. */}
          {redos
            .map((e, i) => row(e, true, redos.slice(0, i + 1).map((x) => x.id), !!journal?.redoBlocked))
            .reverse()}
          {redos.length > 0 && undos.length > 0 && (
            <div className="flex items-center gap-2 px-2 py-0.5 text-[10.5px] tracking-wide text-subtle uppercase select-none">
              <span className="h-px flex-1 bg-border" /> Now <span className="h-px flex-1 bg-border" />
            </div>
          )}
          {undos.map((e, i) =>
            row(
              e,
              false,
              undos.slice(0, i + 1).map((x) => x.id),
              !!journal?.undoBlocked,
            ),
          )}
          {blocked && (
            <>
              <DropdownMenuSeparator />
              <div className="flex gap-2 px-2 py-1.5 text-[11.5px] text-muted-foreground">
                <TriangleAlert className="mt-0.5 size-3.5 shrink-0 text-conflict" />
                {blocked}
              </div>
            </>
          )}
        </DropdownMenuContent>
      </DropdownMenu>
    </div>
  );
}
