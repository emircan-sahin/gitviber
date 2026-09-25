import { ChevronDown, Crosshair, GitCompareArrows, GitGraph, History } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { DropdownMenu, DropdownMenuCheckboxItem, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { Input } from "@/components/ui/input";
import { Popover, PopoverAnchor, PopoverContent } from "@/components/ui/popover";
import { Tip } from "@/components/ui/tooltip";
import { api, type Branch, type GraphRefs } from "@/lib/api";
import { shortRef } from "@/lib/git/refs";
import { pointerMoved } from "@/lib/ui/pointer";
import { toast } from "@/lib/app/toast";
import { cn } from "@/lib/utils";
import { usePickerIndex } from "@/hooks/usePickerIndex";
import { ReflogDialog } from "./ReflogDialog";
import { EVERY_REF, fullRef } from "./useGraphRefs";

type Picking = "goto" | "compare";

/**
 * The all-branches toggle, and next to it what to show, where to go and what to compare with.
 * `onGoTo` gets a commit's SHA, and is null while the graph isn't what's listed; `onCompare`
 * gets a branch's full ref.
 */
export function GraphMenu({
  all,
  setAll,
  refs,
  setRefs,
  branches,
  current,
  onGoToHead,
  onGoTo,
  onCompare,
}: {
  all: boolean;
  setAll: (on: boolean) => void;
  refs: GraphRefs;
  setRefs: (refs: GraphRefs) => void;
  branches: Branch[];
  current: string | null;
  onGoToHead: (() => void) | null;
  onGoTo: ((sha: string, name: string) => void) | null;
  onCompare: (ref: string) => void;
}) {
  const [picking, setPicking] = useState<Picking | null>(null);
  const [reflog, setReflog] = useState(false);
  // Set by the items that open the picker: focus going back to the menu button would close it.
  const opening = useRef(false);
  const pick = (p: Picking) => {
    opening.current = true;
    setPicking(p);
  };
  const choose = async (b: Branch) => {
    const ref = fullRef(b);
    if (picking === "compare") return onCompare(ref);
    const tip = await api.findCommit(ref).catch(() => null);
    if (tip) onGoTo?.(tip.sha, b.name);
    else toast("error", `Could not find ${b.name}`);
  };
  const toggle = (k: "local" | "remote" | "tags") => setRefs({ ...refs, [k]: !refs[k], only: null });

  return (
    <RefPicker
      open={picking !== null}
      onOpenChange={(o) => !o && setPicking(null)}
      title={picking === "compare" ? "Compare with…" : "Go to branch…"}
      // Comparing a branch with itself shows nothing.
      branches={picking === "compare" ? branches.filter((b) => b.remote || b.name !== current) : branches}
      onPick={choose}
    >
      <div className="flex shrink-0 items-center">
        <Tip label={all ? "Showing all branches" : "Show all branches"}>
          <button
            aria-label="All branches"
            aria-pressed={all}
            onClick={() => setAll(!all)}
            className={cn(
              "flex size-4 shrink-0 items-center justify-center rounded-sm outline-none hover:text-foreground focus-visible:text-foreground focus-visible:ring-1 focus-visible:ring-ring",
              all ? "text-primary" : "text-subtle",
            )}
          >
            <GitGraph className="size-3" />
          </button>
        </Tip>
        <DropdownMenu>
          <Tip label="Graph options">
            <DropdownMenuTrigger asChild>
              <button
                aria-label="Graph options"
                className="flex h-4 w-3 shrink-0 items-center justify-center rounded-sm text-subtle outline-none hover:text-foreground focus-visible:text-foreground focus-visible:ring-1 focus-visible:ring-ring"
              >
                <ChevronDown className="size-2.5" />
              </button>
            </DropdownMenuTrigger>
          </Tip>
          <DropdownMenuContent
            align="end"
            className="w-56"
            onCloseAutoFocus={(e) => {
              if (opening.current) e.preventDefault();
              opening.current = false;
            }}
          >
            {all && (
              <>
                <DropdownMenuCheckboxItem checked={refs.local} onCheckedChange={() => toggle("local")}>
                  Local branches
                </DropdownMenuCheckboxItem>
                <DropdownMenuCheckboxItem checked={refs.remote} onCheckedChange={() => toggle("remote")}>
                  Remote branches
                </DropdownMenuCheckboxItem>
                <DropdownMenuCheckboxItem checked={refs.tags} onCheckedChange={() => toggle("tags")}>
                  Tags
                </DropdownMenuCheckboxItem>
                <DropdownMenuSeparator />
                {onGoToHead && onGoTo && (
                  <>
                    <DropdownMenuItem onSelect={onGoToHead}>
                      <Crosshair /> Go to HEAD
                    </DropdownMenuItem>
                    <DropdownMenuItem onSelect={() => pick("goto")}>
                      <Crosshair /> Go to branch…
                    </DropdownMenuItem>
                    <DropdownMenuSeparator />
                  </>
                )}
              </>
            )}
            <DropdownMenuItem onSelect={() => pick("compare")}>
              <GitCompareArrows /> Compare with…
            </DropdownMenuItem>
            <DropdownMenuItem onSelect={() => setReflog(true)}>
              <History /> Reflog…
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
        {reflog && <ReflogDialog onClose={() => setReflog(false)} />}
      </div>
    </RefPicker>
  );
}

/** Type to filter branches, ↑/↓ + Enter or click to pick one. */
function RefPicker({
  open,
  onOpenChange,
  title,
  branches,
  onPick,
  children,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: string;
  branches: Branch[];
  onPick: (b: Branch) => void;
  children: React.ReactNode;
}) {
  const [query, setQuery] = useState("");

  const q = query.trim().toLowerCase();
  // Local branches first, then remote ones; each newest first.
  const options = branches
    .filter((b) => b.name.toLowerCase().includes(q) && !b.name.endsWith("/HEAD"))
    .sort((a, b) => Number(a.remote) - Number(b.remote) || b.timestamp - a.timestamp);
  const { index, setIndex, move } = usePickerIndex(options.length);
  useEffect(() => {
    if (!open) return;
    setQuery("");
    setIndex(0);
  }, [open]);
  const choose = (b: Branch) => {
    onOpenChange(false);
    onPick(b);
  };
  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "ArrowDown") move(1);
    else if (e.key === "ArrowUp") move(-1);
    else if (e.key === "Enter" && options[index]) choose(options[index]);
    else return;
    e.preventDefault();
  };

  return (
    <Popover open={open} onOpenChange={onOpenChange}>
      <PopoverAnchor className="flex">{children}</PopoverAnchor>
      <PopoverContent align="end" className="flex w-72 flex-col p-1" onKeyDown={onKeyDown}>
        <Input
          autoFocus
          placeholder={title}
          value={query}
          onChange={(e) => {
            setQuery(e.target.value);
            setIndex(0);
          }}
          spellCheck={false}
        />
        <div role="listbox" aria-label={title} className="mt-1 max-h-64 overflow-y-auto" onMouseLeave={(e) => pointerMoved(e) && setIndex(-1)}>
          {options.map((b, i) => (
            <div
              key={`${b.remote}:${b.name}`}
              role="option"
              aria-selected={i === index}
              onMouseMove={(e) => pointerMoved(e) && setIndex(i)}
              onClick={() => choose(b)}
              className={cn("flex h-7 cursor-pointer items-center gap-2 rounded-sm px-2 text-[12px] select-none", i === index && "bg-primary text-primary-foreground")}
            >
              <span className="truncate font-mono">{b.name}</span>
              {b.current && <span className="ml-auto shrink-0 text-[11px] opacity-70">current</span>}
            </div>
          ))}
          {!options.length && <div className="px-2 py-2 text-[11.5px] text-subtle">No branch matches.</div>}
        </div>
      </PopoverContent>
    </Popover>
  );
}

/** What the all-branches graph leaves out, so nothing is missing without a word, and the way back. */
export function GraphNotice({ refs, setRefs }: { refs: GraphRefs; setRefs: (refs: GraphRefs) => void }) {
  const off = [!refs.local && "local branches", !refs.remote && "remote branches", !refs.tags && "tags"].filter(Boolean);
  const parts = refs.only
    ? [`Only ${shortRef(refs.only)}`]
    : [off.length ? `No ${off.join(", ")}` : null, refs.hidden.length ? `${refs.hidden.length} hidden` : null].filter(Boolean);
  if (!parts.length) return null;
  return (
    <div className="flex shrink-0 items-center gap-1.5 border-b border-border px-2.5 py-1 text-[11px] text-muted-foreground">
      <span className="min-w-0 truncate" title={refs.hidden.map(shortRef).join("\n") || undefined}>
        {parts.join(" · ")}
      </span>
      <button onClick={() => setRefs(EVERY_REF)} className="shrink-0 rounded-sm px-1 text-primary outline-none hover:underline focus-visible:ring-1 focus-visible:ring-ring">
        Show all
      </button>
    </div>
  );
}
