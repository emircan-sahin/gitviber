import { Check, ChevronDown, Search, Tag } from "lucide-react";
import { useCallback, useRef, useState } from "react";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { errorMessage, type IssueLabel, issues, type Target } from "@/lib/api";
import { useGitHubData } from "@/lib/github/githubCache";
import { pointerMoved } from "@/lib/ui/pointer";
import { cn } from "@/lib/utils";
import { LabelDot } from "./IssueBadges";

/**
 * GitHub's label filter: any number of labels, and the list keeps issues carrying all of them.
 * Lists every label the repository defines (both, for a fork).
 */
export function LabelFilter({
  upstream,
  selected,
  onChange,
  counted,
}: {
  upstream: string | null;
  selected: IssueLabel[];
  onChange: (labels: IssueLabel[]) => void;
  /** The filters beside it show counts: its word goes first, as New's does. */
  counted: boolean;
}) {
  // Spelled out: Tailwind only finds whole class names.
  const hide = counted ? "@max-[380px]:hidden" : "@max-[300px]:hidden";
  return (
    <LabelPicker
      repos={upstream ? [null, upstream] : [null]}
      selected={selected}
      onChange={onChange}
      hint={selected.length > 1 ? "Issues with all of them" : undefined}
      align="end"
    >
      {/* Compact: the chosen labels show in a row of their own under the header. */}
      <button
        aria-label="Filter by label"
        className={cn(
          "flex h-6 shrink-0 items-center gap-1 rounded-md px-1.5 text-[11.5px]",
          counted ? "@max-[380px]:px-1" : "@max-[300px]:px-1",
          selected.length ? "bg-active text-foreground" : "text-muted-foreground hover:bg-hover focus-visible:bg-hover hover:text-foreground focus-visible:text-foreground data-[state=open]:bg-hover data-[state=open]:text-foreground",
        )}
      >
        <Tag className="size-3" />
        <span className={hide}>Label</span>
        {selected.length > 0 ? (
          <span className="rounded-full bg-primary/20 px-1 text-[10px] leading-3.5 font-medium text-primary">{selected.length}</span>
        ) : (
          <ChevronDown className={cn("size-3 opacity-70", hide)} />
        )}
      </button>
    </LabelPicker>
  );
}

/**
 * Picks any number of the labels `repos` define (one or two), with a search box and ↑↓ ↵.
 * They're loaded when it first opens; `children` is the trigger.
 */
export function LabelPicker({
  repos,
  selected,
  onChange,
  onOpenChange,
  hint,
  align = "start",
  children,
}: {
  repos: Target[];
  selected: IssueLabel[];
  onChange: (labels: IssueLabel[]) => void;
  onOpenChange?: (open: boolean) => void;
  hint?: string;
  align?: "start" | "end";
  children: React.ReactNode;
}) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [index, setIndex] = useState(0);
  // Selected labels go first, as of opening: reordering under the pointer would move the row just clicked.
  const [first, setFirst] = useState<IssueLabel[]>([]);
  const listRef = useRef<HTMLDivElement>(null);
  const [a, b] = [repos[0], repos.length > 1 ? repos[1] : undefined];
  const own = useGitHubData(open ? `issues:labels:${a ?? "origin"}` : null, useCallback(() => issues.labels(a), [a]), 600_000);
  const up = useGitHubData(open && b !== undefined ? `issues:labels:${b ?? "origin"}` : null, useCallback(() => issues.labels(b ?? null), [b]), 600_000);
  const failure = own.error ?? up.error;

  const defined = [...(own.data ?? []), ...(up.data ?? [])].filter((l, i, all) => all.findIndex((m) => m.name === l.name) === i);
  const isFirst = (l: IssueLabel) => first.some((m) => m.name === l.name);
  // One renamed or deleted since it was picked still shows, so it can be taken off.
  const gone = first.filter((l) => !defined.some((m) => m.name === l.name));
  const q = query.trim().toLowerCase();
  const shown = [...gone, ...defined]
    .sort((a, b) => Number(isFirst(b)) - Number(isFirst(a)))
    .filter((l) => !q || l.name.toLowerCase().includes(q) || l.description.toLowerCase().includes(q));
  const loaded = own.data !== undefined && (b === undefined || up.data !== undefined);

  const isOn = (l: IssueLabel) => selected.some((m) => m.name === l.name);
  const toggle = (l: IssueLabel) => onChange(isOn(l) ? selected.filter((m) => m.name !== l.name) : [...selected, l]);
  const move = (i: number) => {
    setIndex(i);
    listRef.current?.children[i]?.scrollIntoView({ block: "nearest" });
  };

  return (
    <Popover
      open={open}
      onOpenChange={(o) => {
        setOpen(o);
        if (o) {
          setFirst(selected);
          setQuery("");
          setIndex(0);
        }
        onOpenChange?.(o);
      }}
    >
      <PopoverTrigger asChild>{children}</PopoverTrigger>
      <PopoverContent align={align} className="flex w-72 flex-col overflow-hidden">
        <div className="flex h-9 shrink-0 items-center gap-2 border-b border-border px-2.5">
          <Search className="size-3.5 shrink-0 text-subtle" />
          <input
            autoFocus
            value={query}
            onChange={(e) => {
              setQuery(e.target.value);
              setIndex(0);
            }}
            onKeyDown={(e) => {
              if (e.key === "ArrowDown" || e.key === "ArrowUp") {
                e.preventDefault();
                if (shown.length) move((index + (e.key === "ArrowDown" ? 1 : shown.length - 1)) % shown.length);
              } else if (e.key === "Enter" && shown[index]) {
                e.preventDefault();
                toggle(shown[index]);
              }
            }}
            placeholder="Filter labels…"
            className="h-full min-w-0 flex-1 bg-transparent text-[12px] outline-none placeholder:text-subtle"
          />
        </div>
        <div ref={listRef} className="max-h-80 min-h-0 flex-1 overflow-x-hidden overflow-y-auto p-1">
          {failure !== undefined && !loaded ? (
            <div className="px-2 py-3 text-center text-[12px] text-muted-foreground">{errorMessage(failure)}</div>
          ) : !loaded && shown.length === 0 ? (
            <div className="px-2 py-3 text-center text-[12px] text-subtle">Loading labels…</div>
          ) : shown.length === 0 ? (
            <div className="px-2 py-3 text-center text-[12px] text-subtle">{q ? "No labels match" : "This repository has no labels"}</div>
          ) : (
            shown.map((l, i) => {
              const on = isOn(l);
              return (
                <button
                  key={l.name}
                  // Keep the focus in the search box; ↑↓ there walk these.
                  tabIndex={-1}
                  onMouseDown={(e) => e.preventDefault()}
                  onMouseMove={(e) => pointerMoved(e) && setIndex(i)}
                  onClick={() => toggle(l)}
                  className={cn("flex w-full items-start gap-2 rounded-sm px-2 py-1.5 text-left", i === index && "bg-hover")}
                >
                  <Check className={cn("mt-0.5 size-3.5 shrink-0 text-primary", !on && "invisible")} />
                  <LabelDot label={l} className="mt-1 size-2.5" />
                  <span className="min-w-0 flex-1">
                    <span className="block truncate text-[12px] leading-4">{l.name}</span>
                    {l.description && <span className="block truncate text-[10.5px] leading-4 text-subtle">{l.description}</span>}
                  </span>
                </button>
              );
            })
          )}
        </div>
        <div className="flex shrink-0 items-center gap-2 border-t border-border px-2.5 py-1.5 text-[10.5px] text-subtle">
          <span className="min-w-0 flex-1 truncate">{hint ?? "↑↓ navigate · ↵ select"}</span>
          {selected.length > 0 && (
            <button onClick={() => onChange([])} className="shrink-0 rounded-sm px-1.5 py-0.5 hover:bg-hover focus-visible:bg-hover hover:text-foreground focus-visible:text-foreground">
              Clear
            </button>
          )}
        </div>
      </PopoverContent>
    </Popover>
  );
}
