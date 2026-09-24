import { ChevronRight, ChevronsDownUp, ChevronsUpDown, Ellipsis, LoaderCircle, RefreshCw } from "lucide-react";
import { Fragment, useEffect, useRef, useState } from "react";
import { FindToggles, flipOnKey } from "@/components/FindBox";
import { Tip } from "@/components/ui/tooltip";
import { api, errorMessage, SEARCH_CANCELLED, SEARCH_MAX_HITS, type SearchResult } from "@/lib/api";
import { useFind } from "@/lib/find";
import { findMatches, type FindOptions, NO_OPTIONS } from "@/lib/findQuery";
import { focusPanel } from "@/lib/panels";
import { revealInCode } from "@/lib/reveal";
import type { Selection } from "@/lib/selection";
import { useListNav } from "@/lib/useListNav";
import { cn } from "@/lib/utils";
import { FileIcon } from "./FileIcon";

const DEBOUNCE = 250;

interface Props {
  /** Whether the explorer panel shows this view (Find there focuses its box). */
  active: boolean;
  /** Find in Files asked for the box: `seed` is the code view's selection, if any. */
  ask: { id: number; seed: string };
  onOpen: (s: Selection, pin?: boolean) => void;
}

/**
 * Search in files, VS Code's: the worktree's text through `git grep` (grep.rs), matches grouped
 * by file. Picking one opens the file at its line with the match selected.
 */
export function SearchView({ active, ask, onOpen }: Props) {
  const [text, setText] = useState("");
  const [options, setOptions] = useState<FindOptions>(NO_OPTIONS);
  const [include, setInclude] = useState("");
  const [exclude, setExclude] = useState("");
  const [globs, setGlobs] = useState(false);
  const [result, setResult] = useState<SearchResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState(false);
  const [again, setAgain] = useState(0);
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const [picked, setPicked] = useState<string | null>(null);
  const input = useRef<HTMLInputElement>(null);
  const nav = useListNav({ activeKey: picked });

  const focusBox = () => {
    input.current?.focus();
    input.current?.select();
  };
  useFind("explorer", active ? focusBox : null);
  useEffect(() => {
    if (!ask.id) return;
    if (ask.seed) setText(ask.seed);
    // After the render that shows the view.
    requestAnimationFrame(focusBox);
  }, [ask]);

  // Typing runs a search once it pauses; git stops the one before (grep.rs), and its late answer is dropped here.
  useEffect(() => {
    if (!text) {
      setResult(null);
      setError(null);
      setPending(false);
      return;
    }
    let live = true;
    // Sent and not answered: stopped in git, not just ignored here, when the query changes, clears or the view goes.
    let running = false;
    setPending(true);
    const t = setTimeout(() => {
      running = true;
      api.searchFiles({ text, ...options, include, exclude }).then(
        (r) => {
          running = false;
          if (!live) return;
          setResult(r);
          setError(null);
          setCollapsed(new Set());
          setPending(false);
        },
        (e) => {
          running = false;
          if (!live || errorMessage(e) === SEARCH_CANCELLED) return;
          setError(errorMessage(e));
          setResult(null);
          setPending(false);
        },
      );
    }, DEBOUNCE);
    return () => {
      live = false;
      clearTimeout(t);
      if (running) void api.cancelSearch().catch(() => {});
    };
  }, [text, options, include, exclude, again]);

  const open = (path: string, line: number, pin = false) => {
    revealInCode({ path, line, query: text, options });
    onOpen({ kind: "file", path }, pin);
  };
  const toggle = (path: string) =>
    setCollapsed((c) => {
      const next = new Set(c);
      if (!next.delete(path)) next.add(path);
      return next;
    });
  const allCollapsed = !!result?.files.length && result.files.every((f) => collapsed.has(f.path));

  const onBoxKey = (e: React.KeyboardEvent) => {
    if (flipOnKey(e, options, setOptions)) return;
    if (e.key === "ArrowDown") {
      if (!focusPanel("explorer")) return;
    } else if (e.key === "Enter") setAgain((n) => n + 1);
    else if (e.key === "Escape" && text) setText("");
    else return;
    e.preventDefault();
  };

  const files = result?.files.length ?? 0;
  const counted = result && (result.count ? `${result.count} ${result.count === 1 ? "result" : "results"} in ${files} ${files === 1 ? "file" : "files"}` : "No results found.");
  // \d, \w, \b and lookarounds don't mean the same there: say what ran.
  const status = error ?? (counted && result?.posix ? `${counted} · POSIX regex (git without PCRE)` : counted);

  return (
    <div className="flex h-full flex-col">
      <div className="shrink-0 space-y-1.5 border-b border-border p-2">
        <div className="flex items-center gap-1">
          <div className="flex min-w-0 flex-1 items-center rounded-sm border border-border-strong bg-background pr-0.5 pl-2 focus-within:ring-1 focus-within:ring-ring">
            <input
              ref={input}
              value={text}
              onChange={(e) => setText(e.target.value)}
              onKeyDown={onBoxKey}
              placeholder="Search"
              spellCheck={false}
              className="h-7 min-w-0 flex-1 bg-transparent text-[12px] outline-none placeholder:text-subtle"
            />
            <FindToggles options={options} onOptions={setOptions} />
          </div>
          <Tip label="Toggle search details">
            <button
              aria-label="Toggle search details"
              aria-pressed={globs}
              onClick={() => setGlobs(!globs)}
              className={cn("flex size-6 shrink-0 items-center justify-center rounded-sm hover:bg-hover hover:text-foreground", globs || include || exclude ? "text-foreground" : "text-subtle")}
            >
              <Ellipsis className="size-3.5" />
            </button>
          </Tip>
        </div>
        {(globs || include || exclude) && (
          <>
            <GlobInput label="files to include" value={include} onChange={setInclude} onKeyDown={onBoxKey} placeholder="e.g. *.ts, src/**" />
            <GlobInput label="files to exclude" value={exclude} onChange={setExclude} onKeyDown={onBoxKey} placeholder="e.g. *.test.ts, dist" />
          </>
        )}
      </div>
      {(status || pending) && (
        <div className="flex shrink-0 items-center gap-1.5 py-1 pr-1 pl-3 text-[11.5px]">
          {pending && <LoaderCircle className="size-3 shrink-0 animate-spin text-subtle" />}
          <span className={cn("min-w-0 flex-1 truncate", error ? "text-removed" : "text-muted-foreground")} title={status ?? undefined}>
            {pending && !result ? "Searching…" : status}
          </span>
          {result && result.count > 0 && (
            <>
              <Tip label="Refresh">
                <button aria-label="Refresh" onClick={() => setAgain((n) => n + 1)} className="flex size-5 items-center justify-center rounded-sm text-subtle hover:bg-hover hover:text-foreground">
                  <RefreshCw className="size-3" />
                </button>
              </Tip>
              <Tip label={allCollapsed ? "Expand all" : "Collapse all"}>
                <button
                  aria-label={allCollapsed ? "Expand all" : "Collapse all"}
                  onClick={() => setCollapsed(allCollapsed ? new Set() : new Set(result.files.map((f) => f.path)))}
                  className="flex size-5 items-center justify-center rounded-sm text-subtle hover:bg-hover hover:text-foreground"
                >
                  {allCollapsed ? <ChevronsUpDown className="size-3" /> : <ChevronsDownUp className="size-3" />}
                </button>
              </Tip>
            </>
          )}
        </div>
      )}
      <div className="min-h-0 flex-1 overflow-x-hidden overflow-y-auto pb-2">
        <div role="listbox" aria-label="Search results" {...nav}>
          {result?.files.map((f) => {
            const expanded = !collapsed.has(f.path);
            const slash = f.path.lastIndexOf("/");
            return (
              <Fragment key={f.path}>
                <div
                  role="option"
                  aria-selected={false}
                  aria-expanded={expanded}
                  aria-level={1}
                  tabIndex={-1}
                  data-row={`file:${f.path}`}
                  title={f.path}
                  onClick={() => toggle(f.path)}
                  className="flex h-6 cursor-pointer items-center gap-1.5 pr-2 pl-2 text-[12px] outline-none select-none hover:bg-hover focus:bg-hover focus-visible:ring-1 focus-visible:ring-ring focus-visible:ring-inset"
                >
                  <ChevronRight className={cn("size-3 shrink-0 text-subtle transition-transform duration-100", expanded && "rotate-90")} />
                  <FileIcon path={f.path} />
                  <span className="shrink-0 text-foreground/90">{f.path.slice(slash + 1)}</span>
                  <span className="min-w-0 truncate text-[11px] text-subtle">{f.path.slice(0, Math.max(0, slash))}</span>
                  <span className="ml-auto shrink-0 rounded-sm bg-elevated px-1 font-mono text-[10px] leading-4 text-muted-foreground">{f.hits.length}</span>
                </div>
                {expanded &&
                  f.hits.map((h) => {
                    const key = `hit:${f.path}:${h.line}`;
                    return (
                      <div
                        key={key}
                        role="option"
                        aria-selected={picked === key}
                        aria-level={2}
                        tabIndex={-1}
                        data-row={key}
                        onClick={() => {
                          setPicked(key);
                          open(f.path, h.line);
                        }}
                        onDoubleClick={() => open(f.path, h.line, true)}
                        className={cn(
                          "relative flex h-6 cursor-pointer items-center gap-2 pr-2 pl-8 text-[12px] outline-none hover:bg-hover focus:bg-hover focus-visible:ring-1 focus-visible:ring-ring focus-visible:ring-inset",
                          picked === key && "bg-primary/15",
                        )}
                      >
                        <span className="w-7 shrink-0 text-right font-mono text-[10.5px] text-subtle">{h.line}</span>
                        <Highlighted text={h.text.trimStart()} query={text} options={options} />
                      </div>
                    );
                  })}
              </Fragment>
            );
          })}
        </div>
        {result?.capped && <div className="px-4 py-2 text-center text-[11px] text-subtle">Showing the first {SEARCH_MAX_HITS} results. Narrow the search to see the rest.</div>}
        {result?.timedOut && <div className="px-4 py-2 text-center text-[11px] text-subtle">The search stopped after 10 seconds; these are the results by then.</div>}
      </div>
    </div>
  );
}

function GlobInput({ label, value, onChange, onKeyDown, placeholder }: { label: string; value: string; onChange: (v: string) => void; onKeyDown: (e: React.KeyboardEvent) => void; placeholder: string }) {
  return (
    <label className="block">
      <span className="mb-0.5 block text-[10.5px] text-subtle">{label}</span>
      <input
        value={value}
        onChange={(e) => onChange(e.target.value)}
        onKeyDown={onKeyDown}
        placeholder={placeholder}
        spellCheck={false}
        className="h-6 w-full rounded-sm border border-border-strong bg-background px-2 text-[12px] outline-none placeholder:text-subtle focus:ring-1 focus:ring-ring"
      />
    </label>
  );
}

/** A result's line with its matches marked, as found again here (git's regex dialect aside, the same). */
function Highlighted({ text, query, options }: { text: string; query: string; options: FindOptions }) {
  const found = findMatches(text, query, options, 50);
  const parts: React.ReactNode[] = [];
  let at = 0;
  for (const [start, end] of found instanceof Error ? [] : found) {
    parts.push(text.slice(at, start), <mark key={start} className="rounded-[2px] bg-(--find-match) text-foreground">{text.slice(start, end)}</mark>);
    at = end;
  }
  parts.push(text.slice(at));
  return <span className="min-w-0 truncate font-mono text-[11.5px] text-foreground/80">{parts}</span>;
}
