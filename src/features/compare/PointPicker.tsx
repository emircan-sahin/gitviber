import { Check, ChevronsUpDown, Cloud, GitBranch, GitCommitHorizontal, Search, Tag } from "lucide-react";
import { useEffect, useId, useMemo, useRef, useState } from "react";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { api, type Branch, type Commit } from "@/lib/api";
import { branchPoint, commitPoint, HEAD_POINT, type PointKind, pointKind, tagPoint } from "@/lib/git/comparePoints";
import type { ComparePoint } from "@/lib/repo/selection";
import { pointerMoved } from "@/lib/ui/pointer";
import { cn } from "@/lib/utils";
import { useAsyncValue } from "@/hooks/useAsyncValue";
import { usePickerIndex } from "@/hooks/usePickerIndex";

/** A repo can have thousands of tags: the rest wait for a longer search. */
const SHOWN = 80;
const SHA = /^[0-9a-f]{4,40}$/i;

const ICONS: Record<PointKind, typeof GitBranch> = { head: GitCommitHorizontal, branch: GitBranch, remote: Cloud, tag: Tag, commit: GitCommitHorizontal };

type Row = { key: string; group: string; point: ComparePoint; note?: string; current?: boolean };

/**
 * One side of a comparison: a branch, remote branch or tag from the list, or a commit by its id
 * typed or pasted. Type to narrow, ↑/↓ and Enter to pick.
 */
export function PointPicker({ side, value, branches, current, onPick }: { side: string; value: ComparePoint; branches: Branch[]; current: string | null; onPick: (p: ComparePoint) => void }) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const listId = useId();
  const listRef = useRef<HTMLDivElement>(null);
  // Asked on each open: a tag made in the terminal is there.
  const tags = useAsyncValue(open ? () => api.tags() : null, [open], [] as string[]);
  const q = query.trim();
  const asId = SHA.test(q);
  const commit = useAsyncValue<{ q: string; commit: Commit | null } | null>(open && asId ? () => api.findCommit(q).then((c) => ({ q, commit: c }), () => ({ q, commit: null })) : null, [open, q], null);

  const rows = useMemo<Row[]>(() => {
    const lower = q.toLowerCase();
    const match = (name: string) => name.toLowerCase().includes(lower);
    const heads = branches.filter((b) => !b.name.endsWith("/HEAD") && match(b.name)).sort((a, b) => Number(b.current) - Number(a.current) || b.timestamp - a.timestamp);
    const out: Row[] = [];
    if (current === null && match("HEAD")) out.push({ key: "HEAD", group: "Checked out", point: HEAD_POINT, note: "detached" });
    const add = (list: Row[]) => out.push(...list.slice(0, SHOWN));
    add(heads.filter((b) => !b.remote).map((b) => ({ key: `h:${b.name}`, group: "Branches", point: branchPoint(b), current: b.current })));
    add(heads.filter((b) => b.remote).map((b) => ({ key: `r:${b.name}`, group: "Remote branches", point: branchPoint(b) })));
    add(tags.filter(match).map((t) => ({ key: `t:${t}`, group: "Tags", point: tagPoint(t) })));
    if (commit?.q === q && commit.commit) out.push({ key: `c:${commit.commit.sha}`, group: "Commit", point: commitPoint(commit.commit.sha), note: commit.commit.subject });
    return out;
  }, [branches, tags, q, commit, current]);
  const { index, setIndex, move } = usePickerIndex(rows.length);
  useEffect(() => setIndex(0), [q, open]);
  useEffect(() => {
    listRef.current?.querySelector(`[data-option="${index}"]`)?.scrollIntoView({ block: "nearest" });
  }, [index]);

  const choose = (row: Row | undefined) => {
    if (!row) return;
    setOpen(false);
    setQuery("");
    onPick(row.point);
  };
  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "ArrowDown") move(1);
    else if (e.key === "ArrowUp") move(-1);
    else if (e.key === "Enter") choose(rows[index]);
    else return;
    e.preventDefault();
  };

  const Icon = ICONS[pointKind(value.ref)];
  return (
    <Popover
      open={open}
      onOpenChange={(o) => {
        setOpen(o);
        if (!o) setQuery("");
      }}
    >
      <PopoverTrigger asChild>
        <button aria-label={`${side}: ${value.label}`} className="flex h-7 max-w-80 min-w-0 items-center gap-1.5 rounded-md border border-border-strong px-2 text-left hover:bg-hover focus-visible:bg-hover data-[state=open]:bg-active">
          <span className="shrink-0 text-[10.5px] font-semibold tracking-[0.08em] text-subtle uppercase">{side}</span>
          <Icon className="size-3.5 shrink-0 text-primary" />
          <span className="truncate font-mono text-[12px]">{value.label}</span>
          <ChevronsUpDown className="size-3 shrink-0 text-subtle" />
        </button>
      </PopoverTrigger>
      <PopoverContent align="start" className="flex w-80 flex-col overflow-hidden">
        <div className="flex h-9 shrink-0 items-center gap-2 border-b border-border px-2.5">
          <Search className="size-3.5 shrink-0 text-subtle" />
          <input
            autoFocus
            role="combobox"
            aria-expanded
            aria-controls={listId}
            aria-activedescendant={index >= 0 && rows[index] ? `${listId}-${index}` : undefined}
            aria-autocomplete="list"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={onKeyDown}
            placeholder="Branch, tag or commit id…"
            spellCheck={false}
            className="h-full min-w-0 flex-1 bg-transparent font-mono text-[12px] outline-none placeholder:font-sans placeholder:text-subtle"
          />
        </div>
        <div ref={listRef} id={listId} role="listbox" aria-label={side} onMouseLeave={(e) => pointerMoved(e) && setIndex(-1)} className="max-h-[320px] min-h-0 flex-1 overflow-x-hidden overflow-y-auto p-1">
          {rows.map((row, i) => {
            const Kind = ICONS[pointKind(row.point.ref)];
            const hot = i === index;
            return (
              <div key={row.key}>
                {rows[i - 1]?.group !== row.group && <div className="px-1 pt-2 pb-1 text-[10.5px] font-semibold tracking-[0.08em] text-subtle uppercase">{row.group}</div>}
                <div
                  id={`${listId}-${i}`}
                  data-option={i}
                  role="option"
                  aria-selected={hot}
                  onMouseMove={(e) => pointerMoved(e) && setIndex(i)}
                  onClick={() => choose(row)}
                  className={cn("flex h-7 cursor-pointer items-center gap-2 rounded-sm px-2 text-[12px]", hot && "bg-primary text-primary-foreground")}
                >
                  {row.point.ref === value.ref ? <Check className="size-3.5 shrink-0" /> : <Kind className="size-3.5 shrink-0 opacity-60" />}
                  <span className="shrink-0 truncate font-mono text-[11.5px]">{row.point.label}</span>
                  {row.current && <span className="ml-auto shrink-0 text-[11px] opacity-70">current</span>}
                  {row.note && <span className="min-w-0 flex-1 truncate text-[11px] opacity-70">{row.note}</span>}
                </div>
              </div>
            );
          })}
          {!rows.length && <div className="px-2 py-3 text-center text-[12px] text-subtle">{asId ? "No commit with that id." : "Nothing matches."}</div>}
        </div>
        <div className="shrink-0 border-t border-border px-2.5 py-1.5 text-[10.5px] text-subtle">↑↓ navigate · ↵ pick · paste a commit id to compare a commit</div>
      </PopoverContent>
    </Popover>
  );
}
