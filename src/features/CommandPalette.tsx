import { Dialog as DialogPrimitive } from "radix-ui";
import { type ReactNode, useDeferredValue, useEffect, useId, useMemo, useRef, useState, useSyncExternalStore } from "react";
import { Dialog, DialogContent } from "@/components/ui/dialog";
import { api, errorMessage } from "@/lib/api";
import { bindingsFor, COMMANDS, formatChord, isCommandId } from "@/lib/commands";
import { fuzzyMatch, type Match, matchPath, prepareQuery } from "@/lib/fuzzy";
import { type Action, hasHandler, MENU_ACTION_INFO, MENU_ACTIONS, matchesCommand, runCommand } from "@/lib/keybindings";
import { useSettings } from "@/lib/settings";
import { useTerminals } from "@/lib/terminals";
import { cn, splitPath } from "@/lib/utils";
import type { changeList } from "./ChangesPanel";
import { FileIcon } from "./FileIcon";
import { StatusLetter } from "./StatusBadge";

/**
 * ⇧⌘P runs any command, ⌘P opens a file, as in VS Code: one box, and a leading ">" in it means
 * commands. "Open Changed File" lists the changes instead, and opens their diffs.
 */

type Change = ReturnType<typeof changeList>[number];
type Mode = "files" | "changes";

/** What the open workspace gives quick open; none on the welcome screen. */
export interface QuickOpenSource {
  root: string;
  changes: Change[];
  openFile: (path: string) => void;
  openChange: (change: Change) => void;
}

let source: QuickOpenSource | null = null;

/** Workspace hands over its files and how to open them, for as long as it's mounted. */
export function useQuickOpenSource(src: QuickOpenSource) {
  useEffect(() => {
    source = src;
  });
  useEffect(
    () => () => {
      source = null;
    },
    [],
  );
}

let shown: { mode: Mode; query: string; id: number } | null = null;
const listeners = new Set<() => void>();
const set = (s: typeof shown) => {
  shown = s;
  listeners.forEach((l) => l());
};
let opens = 0;
// Where focus was, for Radix to put it back on close; a picked command runs after that.
let before: Element | null = null;
function show(mode: Mode, query: string) {
  if (!shown) before = document.activeElement;
  set({ mode, query, id: ++opens });
}
export const showCommands = () => show("files", ">");
export const showQuickOpen = (mode: Mode = "files") => show(mode, "");

// At most this many rows: the best matches are at the top, and a repo can have 100k files.
const LIMIT = 200;
const RECENT = 20;
const RECENT_COMMANDS = "gitviber.palette.commands";
const RECENT_FILES = "gitviber.palette.files";

function load<T>(key: string, fallback: T): T {
  try {
    return (JSON.parse(localStorage.getItem(key) ?? "null") as T) ?? fallback;
  } catch {
    return fallback;
  }
}

function save(key: string, value: unknown) {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    // Not critical.
  }
}

const strings = (v: unknown) => (Array.isArray(v) ? v.filter((x): x is string => typeof x === "string") : []);
const bump = (list: string[], item: string) => [item, ...list.filter((x) => x !== item)].slice(0, RECENT);

const recentCommands = () => strings(load(RECENT_COMMANDS, []));
// Per repository, by its worktree's path.
const recentFiles = (root: string) => strings(load<Record<string, unknown>>(RECENT_FILES, {})[root]);
function rememberFile(root: string, path: string) {
  save(RECENT_FILES, { ...load<Record<string, unknown>>(RECENT_FILES, {}), [root]: bump(recentFiles(root), path) });
}

// A repo's file list, kept between opens so the palette shows it at once while a new one loads.
let files: { root: string; list: string[] } | null = null;

interface Item {
  key: string;
  run: () => void;
  row: (hot: boolean) => ReactNode;
}

export function CommandPalette() {
  const state = useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    () => shown,
  );
  const [query, setQuery] = useState("");
  const [mode, setMode] = useState<Mode>("files");
  const [index, setIndex] = useState(0);
  const [list, setList] = useState<string[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  // What was picked runs once the palette has closed and its dialog no longer blocks commands.
  const picked = useRef<(() => void) | null>(null);
  const listRef = useRef<HTMLDivElement>(null);
  // WebKit sends a mousemove after every scroll, the pointer standing still: ↓ scrolling the
  // list then moved the highlight to whatever row slid under it. Only a real move counts.
  const pointer = useRef<{ x: number; y: number } | null>(null);
  const hover = (i: number) => (e: React.MouseEvent) => {
    const p = pointer.current;
    pointer.current = { x: e.screenX, y: e.screenY };
    if (p && (p.x !== e.screenX || p.y !== e.screenY)) setIndex(i);
  };
  const listId = useId();
  const { keybindings } = useSettings();
  const terminalOpen = useTerminals().open;

  const commands = query.startsWith(">");
  const root = source?.root ?? null;

  useEffect(() => {
    if (!state) return;
    setQuery(state.query);
    setMode(state.mode);
  }, [state]);

  const wantsFiles = !!state && !commands && mode === "files" && !!root;
  useEffect(() => {
    if (!wantsFiles || !root) return;
    let live = true;
    setList(files?.root === root ? files.list : null);
    setError(null);
    api.listFiles().then(
      (list) => {
        files = { root, list };
        if (live) setList(list);
      },
      (e) => live && setError(errorMessage(e)),
    );
    return () => {
      live = false;
    };
  }, [wantsFiles, root, state?.id]);

  const deferred = useDeferredValue(query);
  const items = useMemo((): Item[] => {
    if (!state) return [];
    const pick = (fn: () => void) => () => {
      picked.current = fn;
      set(null);
    };
    if (deferred.startsWith(">")) {
      const recent = recentCommands();
      const entries = [
        ...COMMANDS.map((c) => ({ id: c.id as Action, title: c.title, category: c.category })),
        ...MENU_ACTIONS.map((id) => ({ id: id as Action, ...MENU_ACTION_INFO[id] })),
      ].filter((c) => c.id !== "workbench.showCommands" && hasHandler(c.id));
      return rank(
        deferred.slice(1),
        entries,
        (c) => {
          const title = c.id === "terminal.toggle" ? (terminalOpen ? "Hide Terminal" : "Show Terminal") : c.title;
          return c.category === "General" ? title : `${c.category}: ${title}`;
        },
        fuzzyMatch,
        (c) => recent.indexOf(c.id),
      ).map(({ item: c, label, match }) => {
        const chord = isCommandId(c.id) ? bindingsFor(c.id, keybindings)[0] : MENU_ACTION_INFO[c.id as keyof typeof MENU_ACTION_INFO].key;
        return {
          key: c.id,
          run: pick(() => {
            save(RECENT_COMMANDS, bump(recentCommands(), c.id));
            runCommand(c.id);
          }),
          row: () => (
            <>
              <Highlight text={label} hits={match.hits} className="min-w-0 flex-1 truncate" />
              {recent.includes(c.id) && !deferred.slice(1).trim() && <span className="shrink-0 text-[10.5px] opacity-60">recently used</span>}
              {chord && <kbd className="shrink-0 font-mono text-[11px] opacity-70">{formatChord(chord)}</kbd>}
            </>
          ),
        };
      });
    }
    const src = source;
    if (!src) return [];
    if (mode === "changes") {
      return rank(deferred, src.changes, (c) => c.file.path, matchPath).map(({ item: c, label, match }) => ({
        key: `${c.kind}:${c.file.path}`,
        run: pick(() => src.openChange(c)),
        row: (hot: boolean) => (
          <>
            <PathRow path={label} hits={match.hits} />
            {c.kind !== "unstaged" && <span className="shrink-0 text-[10.5px] opacity-70">{c.kind === "staged" ? "Staged" : "Conflict"}</span>}
            <StatusLetter status={c.file.status} className={cn(hot && "text-current")} />
          </>
        ),
      }));
    }
    const recent = recentFiles(src.root);
    return rank(deferred, list ?? [], (p) => p, matchPath, (p) => recent.indexOf(p)).map(({ item: path, match }) => ({
      key: path,
      run: pick(() => {
        rememberFile(src.root, path);
        src.openFile(path);
      }),
      row: () => (
        <>
          <PathRow path={path} hits={match.hits} />
          {recent.includes(path) && !deferred.trim() && <span className="shrink-0 text-[10.5px] opacity-60">recently opened</span>}
        </>
      ),
    }));
  }, [state, deferred, mode, list, keybindings, terminalOpen]);

  useEffect(() => setIndex(0), [deferred, mode, state, list]);
  useEffect(() => {
    listRef.current?.querySelector(`[data-option="${index}"]`)?.scrollIntoView({ block: "nearest" });
  }, [index]);

  const onKeyDown = (e: React.KeyboardEvent) => {
    // The palette's own keys switch it over, as in VS Code.
    if (matchesCommand("workbench.showCommands", e.nativeEvent)) setQuery(">");
    else if (matchesCommand("workbench.quickOpen", e.nativeEvent) && root) {
      setMode("files");
      setQuery("");
    } else if (e.key === "ArrowDown") setIndex((i) => (i + 1 < items.length ? i + 1 : 0));
    else if (e.key === "ArrowUp") setIndex((i) => (i > 0 ? i - 1 : items.length - 1));
    else if (e.key === "PageDown") setIndex((i) => Math.min(items.length - 1, i + 10));
    else if (e.key === "PageUp") setIndex((i) => Math.max(0, i - 10));
    else if (e.key === "Enter" && !e.nativeEvent.isComposing) items[index]?.run();
    else return;
    e.preventDefault();
  };

  const empty = commands
    ? "No matching commands"
    : !root
      ? "Open a repository to find its files · type > for commands"
      : mode === "changes"
        ? source?.changes.length
          ? "No matching changes"
          : "No changes"
        : error
          ? `Could not list files: ${error}`
          : list === null
            ? "Listing files…"
            : "No matching files";

  return (
    <Dialog open={!!state} onOpenChange={(o) => !o && set(null)}>
      <DialogContent
        aria-describedby={undefined}
        className="top-[12%] flex max-w-xl flex-col overflow-hidden p-0"
        // Focus goes back where it was first, so a command that moves it (Focus Explorer) has the last word.
        onCloseAutoFocus={(e) => {
          const run = picked.current;
          picked.current = null;
          if (!run) return;
          e.preventDefault();
          if (before instanceof HTMLElement && before.isConnected) before.focus();
          run();
        }}
      >
        <DialogPrimitive.Title className="sr-only">{commands ? "Command palette" : mode === "changes" ? "Open changed file" : "Open file"}</DialogPrimitive.Title>
        <input
          autoFocus
          role="combobox"
          aria-expanded
          aria-controls={listId}
          aria-activedescendant={items[index] ? `${listId}-${index}` : undefined}
          aria-autocomplete="list"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={onKeyDown}
          spellCheck={false}
          placeholder={commands ? "Type a command" : mode === "changes" ? "Open a changed file's diff · type > for commands" : "Search files by name · type > for commands"}
          className="h-10 shrink-0 border-b border-border bg-transparent px-3 text-[13px] outline-none placeholder:text-subtle"
        />
        <div ref={listRef} id={listId} role="listbox" className="max-h-[min(420px,60vh)] min-h-0 overflow-x-hidden overflow-y-auto p-1">
          {items.length === 0 && <div className="px-2 py-3 text-center text-[12px] text-subtle">{empty}</div>}
          {items.map((it, i) => (
            <div
              key={it.key}
              id={`${listId}-${i}`}
              data-option={i}
              role="option"
              aria-selected={i === index}
              onMouseMove={hover(i)}
              // Keep the focus in the box.
              onMouseDown={(e) => e.preventDefault()}
              onClick={it.run}
              className={cn("flex h-7 cursor-pointer items-center gap-2 rounded-sm px-2 text-[12.5px]", i === index ? "bg-primary text-primary-foreground" : "text-foreground")}
            >
              {it.row(i === index)}
            </div>
          ))}
        </div>
      </DialogContent>
    </Dialog>
  );
}

/** Best matches first; with an empty query, recent ones first, then the list's own order. */
function rank<T>(query: string, list: T[], text: (t: T) => string, match: (q: string, s: string) => Match | null, recency: (t: T) => number = () => -1) {
  const q = prepareQuery(query);
  const out: { item: T; label: string; match: Match; recent: number; at: number }[] = [];
  list.forEach((item, at) => {
    const label = text(item);
    const m = match(q, label);
    const r = recency(item);
    if (m) out.push({ item, label, match: m, recent: r < 0 ? Infinity : r, at });
  });
  out.sort((a, b) => (q ? b.match.score - a.match.score : 0) || a.recent - b.recent || a.at - b.at);
  return out.slice(0, LIMIT);
}

function PathRow({ path, hits }: { path: string; hits: number[] }) {
  const { dir, name } = splitPath(path);
  return (
    <>
      <FileIcon path={path} />
      <Highlight text={name} hits={hits} offset={dir.length} className="shrink-0 truncate" />
      <Highlight text={dir.replace(/\/$/, "")} hits={hits} className="min-w-0 flex-1 truncate text-[11.5px] opacity-60" />
    </>
  );
}

/** `text` with the matched characters (`hits`, as indices into the text `offset` places it in) in bold. */
function Highlight({ text, hits, offset = 0, className }: { text: string; hits: number[]; offset?: number; className?: string }) {
  const marked = new Set(hits.map((h) => h - offset));
  const parts: ReactNode[] = [];
  let run = "";
  let bold = false;
  const flush = () => {
    if (run) parts.push(bold ? <b key={parts.length} className="font-semibold">{run}</b> : run);
    run = "";
  };
  for (let i = 0; i < text.length; i++) {
    if (marked.has(i) !== bold) {
      flush();
      bold = !bold;
    }
    run += text[i];
  }
  flush();
  return <span className={className}>{parts}</span>;
}
