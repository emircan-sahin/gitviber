import { Dialog as DialogPrimitive } from "radix-ui";
import { type ReactNode, useEffect, useId, useMemo, useRef, useState } from "react";
import { Dialog, DialogContent } from "@/components/ui/dialog";
import { api, type Conversation, errorMessage, pty } from "@/lib/api";
import { bindingsFor, COMMANDS, formatChord, isCommandId } from "@/lib/commands/commands";
import { fuzzyMatch, type Match, matchPath, prepareQuery } from "@/lib/ui/fuzzy";
import { type Action, hasHandler, MENU_ACTION_INFO, MENU_ACTIONS, matchesCommand, runCommand } from "@/lib/commands/keybindings";
import { pointerMoved } from "@/lib/ui/pointer";
import { useSettings } from "@/lib/settings";
import { openTerminal, revealPane, useTerminalsMaximized, useTerminalsOpen } from "@/lib/terminal/terminals";
import { paneOfConversation } from "@/lib/terminal/agents";
import { cn } from "@/lib/utils";
import { folderName, splitPath } from "@/lib/path";
import { relativeTime } from "@/lib/format";
import { readJson, stringList, writeJson } from "@/lib/storage";
import { createStore } from "@/lib/store";
import type { Change } from "@/features/changes/changeList";
import { FileIcon } from "@/components/FileIcon";
import { StatusLetter } from "@/components/StatusBadge";
import { usePickerIndex } from "@/hooks/usePickerIndex";
import { ProjectTile } from "@/features/projects/ProjectList";

/**
 * ⇧⌘P runs any command, ⌘P opens a file, as in VS Code: one box, and a leading ">" in it means
 * commands. "Open Changed File" lists the changes instead, and opens their diffs; "New Terminal in
 * Project…" the other saved projects, to start a terminal in without switching to them; "Resume a
 * Conversation…" an agent's past conversations in a folder, to pick up in a new terminal there.
 */

type Mode = "files" | "changes" | "projects" | "conversations";

/** What the open workspace gives quick open; none on the welcome screen. */
export interface QuickOpenSource {
  root: string;
  changes: Change[];
  /** The saved projects other than this one. */
  projects: string[];
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

/** `cwd`: the folder whose conversations "conversations" lists. */
const shown = createStore<{ mode: Mode; query: string; id: number; cwd?: string } | null>(null);
const set = shown.set;
let opens = 0;
// Where focus was, for Radix to put it back on close; a picked command runs after that.
let before: Element | null = null;
function show(mode: Mode, query: string, cwd?: string) {
  if (!shown.get()) before = document.activeElement;
  set({ mode, query, id: ++opens, cwd });
}
export const showCommands = () => show("files", ">");
export const showQuickOpen = (mode: Exclude<Mode, "conversations"> = "files") => show(mode, "");
/** The agents' past conversations in `cwd`; the one picked resumes in a new terminal there. */
export const showConversations = (cwd: string) => show("conversations", "", cwd);

// At most this many rows: the best matches are at the top, and a repo can have 100k files.
const LIMIT = 200;
const RECENT = 20;
const RECENT_COMMANDS = "gitviber.palette.commands";
const RECENT_FILES = "gitviber.palette.files";

const bump = (list: string[], item: string) => [item, ...list.filter((x) => x !== item)].slice(0, RECENT);

const recentCommands = () => stringList(readJson<unknown>(RECENT_COMMANDS, []));
// Per repository, by its worktree's path.
const recentFiles = (root: string) => stringList(readJson<Record<string, unknown>>(RECENT_FILES, {})[root]);
function rememberFile(root: string, path: string) {
  writeJson(RECENT_FILES, { ...readJson<Record<string, unknown>>(RECENT_FILES, {}), [root]: bump(recentFiles(root), path) });
}

// A repo's file list, kept between opens so the palette shows it at once while a new one loads.
let files: { root: string; list: string[] } | null = null;

interface Item {
  key: string;
  run: () => void;
  row: (hot: boolean) => ReactNode;
}

export function CommandPalette() {
  const state = shown.use();
  const [query, setQuery] = useState("");
  const [mode, setMode] = useState<Mode>("files");
  const [list, setList] = useState<string[] | null>(null);
  const [conversations, setConversations] = useState<Conversation[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  // What was picked runs once the palette has closed and its dialog no longer blocks commands.
  const picked = useRef<(() => void) | null>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const listId = useId();
  const { keybindings } = useSettings();
  const terminalOpen = useTerminalsOpen();
  const terminalMaximized = useTerminalsMaximized();

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

  // Read each time it's asked for: a conversation started a minute ago is the likely one.
  const cwd = state?.mode === "conversations" ? state.cwd : undefined;
  useEffect(() => {
    if (!cwd) return;
    let live = true;
    setConversations(null);
    setError(null);
    pty.conversations(cwd).then(
      (list) => live && setConversations(list),
      (e) => live && setError(errorMessage(e)),
    );
    return () => {
      live = false;
    };
  }, [cwd, state?.id]);

  // Straight from the box, not useDeferredValue: ↵ must run what it says now, not what an older render listed.
  const items = useMemo((): Item[] => {
    if (!state) return [];
    const pick = (fn: () => void) => () => {
      picked.current = fn;
      set(null);
    };
    if (query.startsWith(">")) {
      const recent = recentCommands();
      const entries = [
        ...COMMANDS.map((c) => ({ id: c.id as Action, title: c.title, category: c.category })),
        ...MENU_ACTIONS.map((id) => ({ id: id as Action, ...MENU_ACTION_INFO[id] })),
      ].filter((c) => c.id !== "workbench.showCommands" && hasHandler(c.id));
      return rank(
        query.slice(1),
        entries,
        (c) => {
          let title: string = c.title;
          if (c.id === "terminal.toggle") title = terminalOpen ? "Hide Terminal" : "Show Terminal";
          else if (c.id === "terminal.toggleMaximize" && terminalMaximized) title = "Exit Maximized Terminal";
          return c.category === "General" ? title : `${c.category}: ${title}`;
        },
        fuzzyMatch,
        (c) => recent.indexOf(c.id),
      ).map(({ item: c, label, match }) => {
        const chord = isCommandId(c.id) ? bindingsFor(c.id, keybindings)[0] : undefined;
        return {
          key: c.id,
          run: pick(() => {
            writeJson(RECENT_COMMANDS, bump(recentCommands(), c.id));
            runCommand(c.id);
          }),
          row: () => (
            <>
              <Highlight text={label} hits={match.hits} className="min-w-0 flex-1 truncate" />
              {recent.includes(c.id) && !query.slice(1).trim() && <span className="shrink-0 text-[10.5px] text-muted-foreground in-aria-selected:text-primary-foreground/90">recently used</span>}
              {chord && <kbd className="shrink-0 font-mono text-[11px] opacity-70">{formatChord(chord)}</kbd>}
            </>
          ),
        };
      });
    }
    if (mode === "conversations" && cwd) {
      return rank(query, conversations ?? [], (c) => c.title, fuzzyMatch).map(({ item: c, label, match }) => {
        const pane = paneOfConversation(c.id);
        return {
          key: c.id,
          run: pick(() => (pane === undefined ? openTerminal(cwd, c.command) : revealPane(pane))),
          row: () => (
            <>
              <Highlight text={label} hits={match.hits} className="min-w-0 flex-1 truncate" />
              {pane !== undefined && <span className="shrink-0 text-[10.5px] text-muted-foreground in-aria-selected:text-primary-foreground/90">open in a terminal</span>}
              {c.branch && <span className="max-w-32 shrink-0 truncate font-mono text-[10.5px] text-muted-foreground in-aria-selected:text-primary-foreground/90">{c.branch}</span>}
              <span className="shrink-0 text-[10.5px] text-muted-foreground in-aria-selected:text-primary-foreground/90">{relativeTime(c.modified)}</span>
            </>
          ),
        };
      });
    }
    const src = source;
    if (!src) return [];
    if (mode === "projects") {
      return rank(query, src.projects, (p) => p, matchPath).map(({ item: path, match }) => ({
        key: path,
        run: pick(() => openTerminal(path)),
        row: () => <PathRow path={path} hits={match.hits} icon={<ProjectTile name={folderName(path)} />} />,
      }));
    }
    if (mode === "changes") {
      return rank(query, src.changes, (c) => c.file.path, matchPath).map(({ item: c, label, match }) => ({
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
    return rank(query, list ?? [], (p) => p, matchPath, (p) => recent.indexOf(p)).map(({ item: path, match }) => ({
      key: path,
      run: pick(() => {
        rememberFile(src.root, path);
        src.openFile(path);
      }),
      row: () => (
        <>
          <PathRow path={path} hits={match.hits} />
          {recent.includes(path) && !query.trim() && <span className="shrink-0 text-[10.5px] text-muted-foreground in-aria-selected:text-primary-foreground/90">recently opened</span>}
        </>
      ),
    }));
  }, [state, query, mode, list, conversations, keybindings, terminalOpen, terminalMaximized]);
  const { index, setIndex, move } = usePickerIndex(items.length, { wrap: true });

  useEffect(() => setIndex(0), [query, mode, state, list, conversations]);
  useEffect(() => {
    listRef.current?.querySelector(`[data-option="${index}"]`)?.scrollIntoView({ block: "nearest" });
  }, [index]);

  const onKeyDown = (e: React.KeyboardEvent) => {
    // The palette's own keys switch it over, as in VS Code.
    if (matchesCommand("workbench.showCommands", e.nativeEvent)) setQuery(">");
    else if (matchesCommand("workbench.quickOpen", e.nativeEvent) && root) {
      setMode("files");
      setQuery("");
    } else if (e.key === "ArrowDown") move(1);
    else if (e.key === "ArrowUp") move(-1);
    else if (e.key === "PageDown") move(10);
    else if (e.key === "PageUp") move(-10);
    else if (e.key === "Enter" && !e.nativeEvent.isComposing) items[index]?.run();
    else return;
    e.preventDefault();
  };

  const empty = () => {
    if (commands) return "No matching commands";
    if (mode === "conversations") {
      if (error) return `Could not read the conversations: ${error}`;
      if (conversations === null) return "Looking for conversations…";
      return conversations.length ? "No matching conversations" : `No Claude Code conversations in ${folderName(cwd ?? "") || cwd} yet`;
    }
    if (!root) return "Open a repository to find its files · type > for commands";
    if (mode === "changes") return source?.changes.length ? "No matching changes" : "No changes";
    if (mode === "projects") return "No matching projects";
    if (error) return `Could not list files: ${error}`;
    return list === null ? "Listing files…" : "No matching files";
  };

  return (
    <Dialog open={!!state} onOpenChange={(o) => !o && set(null)}>
      <DialogContent
        aria-describedby={undefined}
        className="top-[12%] flex max-w-xl flex-col overflow-hidden p-0"
        // Focus goes back where it was first, so a command that moves it (Focus Explorer) has the last word.
        onCloseAutoFocus={(e) => {
          const run = picked.current;
          picked.current = null;
          // Held past the close, it kept a closed editor's whole DOM alive.
          const back = before;
          before = null;
          if (!run) return;
          e.preventDefault();
          if (back instanceof HTMLElement && back.isConnected) back.focus();
          run();
        }}
      >
        <DialogPrimitive.Title className="sr-only">{commands ? "Command palette" : mode === "changes" ? "Open changed file" : mode === "projects" ? "New terminal in project" : mode === "conversations" ? "Resume a conversation" : "Open file"}</DialogPrimitive.Title>
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
          placeholder={
            commands
              ? "Type a command"
              : mode === "changes"
                ? "Open a changed file's diff · type > for commands"
                : mode === "projects"
                  ? "Open a terminal in a project · type > for commands"
                  : mode === "conversations"
                    ? `Resume a conversation in ${folderName(cwd ?? "")} · type > for commands`
                    : "Search files by name · type > for commands"
          }
          className="h-10 shrink-0 border-b border-border bg-transparent px-3 text-[13px] outline-none placeholder:text-subtle"
        />
        <div ref={listRef} id={listId} role="listbox" className="max-h-[min(420px,60vh)] min-h-0 overflow-x-hidden overflow-y-auto p-1">
          {items.length === 0 && <div className="px-2 py-3 text-center text-[12px] text-subtle">{empty()}</div>}
          {items.map((it, i) => (
            <div
              key={it.key}
              id={`${listId}-${i}`}
              data-option={i}
              role="option"
              aria-selected={i === index}
              onMouseMove={(e) => pointerMoved(e) && i !== index && setIndex(i)}
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

function PathRow({ path, hits, icon }: { path: string; hits: number[]; icon?: ReactNode }) {
  const { dir, name } = splitPath(path);
  return (
    <>
      {icon ?? <FileIcon path={path} />}
      <Highlight text={name} hits={hits} offset={dir.length} className="shrink-0 truncate" />
      <Highlight text={dir.replace(/\/$/, "")} hits={hits} className="min-w-0 flex-1 truncate text-[11.5px] text-muted-foreground in-aria-selected:text-primary-foreground/90" />
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
