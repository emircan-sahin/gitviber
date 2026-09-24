import { ChevronDown, Crosshair, GitCompareArrows, GitGraph, X } from "lucide-react";
import { type ComponentProps, useCallback, useEffect, useRef, useState } from "react";
import { DropdownMenu, DropdownMenuCheckboxItem, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { Input } from "@/components/ui/input";
import { Popover, PopoverAnchor, PopoverContent } from "@/components/ui/popover";
import { Tip } from "@/components/ui/tooltip";
import { api, type Branch, type Commit, errorMessage, type GraphRefs } from "@/lib/api";
import { pointerMoved } from "@/lib/pointer";
import { toast } from "@/lib/toast";
import { cn } from "@/lib/utils";
import { HistoryPanel } from "./HistoryPanel";
import { RepoPanes } from "./RepoPanes";

const PAGE = 200;
// Going to a commit loads pages until it's listed, but not the whole history of a huge repo.
const MAX_REACH = 20;
const ALL_KEY = "gitviber.history.allBranches";
const REFS_KEY = "gitviber.history.graphRefs";

export const EVERY_REF: GraphRefs = { local: true, remote: true, tags: true, hidden: [], only: null };

/** A branch as the graph's refs name it. */
export const fullRef = (b: Branch) => `${b.remote ? "refs/remotes/" : "refs/heads/"}${b.name}`;
/** refs/heads/main → main, refs/remotes/origin/main → origin/main. */
export const shortRef = (ref: string) => ref.replace(/^refs\/(heads|remotes|tags)\//, "");

function read<T>(key: string, fallback: T): T {
  try {
    const v = localStorage.getItem(key);
    return v === null ? fallback : (JSON.parse(v) as T);
  } catch {
    return fallback;
  }
}

function write(key: string, value: unknown) {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    // Not critical: History starts as it does by default next time.
  }
}

/** Whether History lists every branch: one choice, for every repo. */
export function useAllBranchesSetting() {
  const [on, setOn] = useState(() => read<unknown>(ALL_KEY, false) === true);
  const set = (next: boolean) => {
    setOn(next);
    write(ALL_KEY, next);
  };
  return [on, set] as const;
}

/** Which refs the all-branches graph walks, per repo: hidden branches are that repo's own. */
export function useGraphRefs(root: string | undefined) {
  const load = (r: string | undefined): GraphRefs => {
    const v = r ? read<Record<string, Partial<GraphRefs>>>(REFS_KEY, {})[r] : undefined;
    if (!v || typeof v !== "object") return EVERY_REF;
    return {
      local: v.local !== false,
      remote: v.remote !== false,
      tags: v.tags !== false,
      hidden: Array.isArray(v.hidden) ? v.hidden.filter((h) => typeof h === "string") : [],
      only: typeof v.only === "string" ? v.only : null,
    };
  };
  const [state, setState] = useState(() => ({ root, refs: load(root) }));
  const refs = state.root === root ? state.refs : load(root);
  const set = (next: GraphRefs) => {
    setState({ root, refs: next });
    if (!root) return;
    const { [root]: _, ...rest } = read<Record<string, GraphRefs>>(REFS_KEY, {});
    write(REFS_KEY, JSON.stringify(next) === JSON.stringify(EVERY_REF) ? rest : { ...rest, [root]: next });
  };
  return [refs, set] as const;
}

/**
 * Every branch's history as `refs` lets through, for as long as `on`. It's read again with
 * HEAD's (`ours`, a new list whenever the repo's refs change), keeping the pages loaded so far.
 * `reach` loads pages until a commit is listed, for going to it.
 */
export function useAllBranches(on: boolean, ours: Commit[], refs: GraphRefs) {
  const [log, setLog] = useState<{ commits: Commit[]; hasMore: boolean; error: string | null } | null>(null);
  const seq = useRef(0);
  const current = useRef(log);
  current.current = log;
  const key = JSON.stringify(refs);
  const latest = useRef(refs);
  latest.current = refs;

  useEffect(() => {
    const id = ++seq.current;
    if (!on) {
      setLog(null);
      return;
    }
    const limit = Math.max(PAGE, current.current?.commits.length ?? 0);
    api.log(0, limit, null, null, latest.current).then(
      (commits) => id === seq.current && setLog({ commits, hasMore: commits.length === limit, error: null }),
      (e) => id === seq.current && setLog({ commits: [], hasMore: false, error: errorMessage(e) }),
    );
  }, [on, ours, key]);

  // Appends a page; false when there's no more, or the list changed meanwhile.
  const page = useCallback(async () => {
    const l = current.current;
    if (!l?.hasMore) return false;
    const id = seq.current;
    const more = await api.log(l.commits.length, PAGE, null, null, latest.current);
    if (id !== seq.current) return false;
    const seen = new Set(l.commits.map((c) => c.sha));
    const next = { ...l, commits: [...l.commits, ...more.filter((c) => !seen.has(c.sha))], hasMore: more.length === PAGE };
    current.current = next;
    setLog(next);
    return true;
  }, []);

  const loadMore = useCallback(async () => void (await page()), [page]);

  const reach = useCallback(
    async (sha: string) => {
      for (let i = 0; i < MAX_REACH; i++) {
        if (current.current?.commits.some((c) => c.sha === sha)) return true;
        if (!(await page())) break;
      }
      return !!current.current?.commits.some((c) => c.sha === sha);
    },
    [page],
  );

  return { commits: log?.commits ?? null, hasMore: !!log?.hasMore, error: log?.error ?? null, loadMore, reach };
}

type Picking = "goto" | "compare";

/**
 * The all-branches toggle, and next to it what to show, where to go and what to compare with.
 * `onGoTo` gets a commit's SHA; `onCompare` a branch's full ref.
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
  onGoToHead: () => void;
  onGoTo: (sha: string, name: string) => void;
  onCompare: (ref: string) => void;
}) {
  const [picking, setPicking] = useState<Picking | null>(null);
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
    if (tip) onGoTo(tip.sha, b.name);
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
                <DropdownMenuItem onSelect={onGoToHead}>
                  <Crosshair /> Go to HEAD
                </DropdownMenuItem>
                <DropdownMenuItem onSelect={() => pick("goto")}>
                  <Crosshair /> Go to branch…
                </DropdownMenuItem>
                <DropdownMenuSeparator />
              </>
            )}
            <DropdownMenuItem onSelect={() => pick("compare")}>
              <GitCompareArrows /> Compare with…
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
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
  const [index, setIndex] = useState(0);
  useEffect(() => {
    if (!open) return;
    setQuery("");
    setIndex(0);
  }, [open]);

  const q = query.trim().toLowerCase();
  // Local branches first, then remote ones; each newest first.
  const options = branches
    .filter((b) => b.name.toLowerCase().includes(q) && !b.name.endsWith("/HEAD"))
    .sort((a, b) => Number(a.remote) - Number(b.remote) || b.timestamp - a.timestamp);
  const choose = (b: Branch) => {
    onOpenChange(false);
    onPick(b);
  };
  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "ArrowDown") setIndex((i) => Math.min(options.length - 1, i + 1));
    else if (e.key === "ArrowUp") setIndex((i) => Math.max(0, i - 1));
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

/** The graph's refs with these hidden too. Hiding the solo'd branch shows everything else. */
export function hideRefs(refs: GraphRefs, hide: string[]): GraphRefs {
  return { ...refs, only: null, hidden: [...new Set([...refs.hidden, ...hide])] };
}

type ListProps = Omit<ComponentProps<typeof HistoryPanel>, "commits" | "hasMore" | "loadMore">;

/**
 * GitHub Desktop's Compare: what `with` (a full ref) has that HEAD doesn't, and the other way
 * round, each its own list. Read again whenever HEAD's history changes.
 */
export function CompareHistory({ with: ref, current, ours, onClose, ...props }: ListProps & { with: string; current: string; ours: Commit[]; onClose: () => void }) {
  const [counts, setCounts] = useState<[number, number] | null>(null);
  useEffect(() => {
    let alive = true;
    api.compareCounts(ref).then(
      (c) => alive && setCounts(c),
      () => alive && setCounts(null),
    );
    return () => {
      alive = false;
    };
  }, [ref, ours]);
  const name = shortRef(ref);
  const list = (incoming: boolean) => <CompareList key={`${ref}:${incoming}`} {...props} with={ref} incoming={incoming} ours={ours} />;

  return (
    <div className="flex h-full flex-col">
      <div className="flex shrink-0 items-center gap-1.5 border-b border-border px-2.5 py-1 text-[11px] text-muted-foreground">
        <GitCompareArrows className="size-3 shrink-0 text-subtle" />
        <span className="min-w-0 truncate">
          <span className="font-mono text-foreground">{current}</span> compared with <span className="font-mono text-foreground">{name}</span>
        </span>
        <button
          aria-label="Stop comparing"
          onClick={onClose}
          className="ml-auto flex size-4 shrink-0 items-center justify-center rounded-sm text-subtle hover:bg-hover hover:text-foreground focus-visible:bg-hover focus-visible:text-foreground"
        >
          <X className="size-3" />
        </button>
      </div>
      <div className="min-h-0 flex-1">
        <RepoPanes
          id="compare"
          panes={[
            { id: "incoming", title: "Behind", detail: `in ${name}, not ${current}`, badge: counts?.[1], scrolls: true, children: list(true) },
            { id: "outgoing", title: "Ahead", detail: `in ${current}, not ${name}`, badge: counts?.[0], scrolls: true, children: list(false) },
          ]}
        />
      </div>
    </div>
  );
}

function CompareList({ with: ref, incoming, ours, ...props }: ListProps & { with: string; incoming: boolean; ours: Commit[] }) {
  const [log, setLog] = useState<{ commits: Commit[]; hasMore: boolean; error: string | null } | null>(null);
  const current = useRef(log);
  current.current = log;

  useEffect(() => {
    let alive = true;
    const limit = Math.max(PAGE, current.current?.commits.length ?? 0);
    api.logCompare(ref, incoming, 0, limit).then(
      (commits) => alive && setLog({ commits, hasMore: commits.length === limit, error: null }),
      (e) => alive && setLog({ commits: [], hasMore: false, error: errorMessage(e) }),
    );
    return () => {
      alive = false;
    };
  }, [ref, incoming, ours]);

  const loadMore = async () => {
    const l = current.current;
    if (!l) return;
    const more = await api.logCompare(ref, incoming, l.commits.length, PAGE);
    setLog((x) => {
      if (!x) return x;
      const seen = new Set(x.commits.map((c) => c.sha));
      return { ...x, commits: [...x.commits, ...more.filter((c) => !seen.has(c.sha))], hasMore: more.length === PAGE };
    });
  };

  if (log?.error) return <div className="px-4 py-6 text-center text-[12px] text-muted-foreground">{log.error}</div>;
  return (
    <HistoryPanel
      {...props}
      commits={log?.commits ?? []}
      hasMore={!!log?.hasMore}
      loadMore={loadMore}
      empty={log ? (incoming ? "Nothing here that your branch lacks." : "Nothing here that it lacks.") : "Loading…"}
      graph={false}
    />
  );
}
