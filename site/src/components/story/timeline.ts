// What the window shows at each moment of each chapter, as pure functions of the time since the
// chapter became active. The desktop window and the phone panels render from the same states.
import type { FileLine } from "../app/Diff.tsx";
import type { ChangeFile, CommitState, TerminalTab } from "../app/parts.tsx";
import { SCENES, type DiffLine, type Scene } from "../demo-data.ts";

export const TYPE_MS = 90;

type KeyEvent = [at: number, key: string];

/** The key shown in the corner: the last one pressed, for a moment after it's pressed. */
function keyAt(events: KeyEvent[], t: number) {
  const last = events.filter(([at]) => at <= t).pop();
  return last && t - last[0] < 650 ? last[1] : undefined;
}

const typed = (text: string, t: number, from: number, perChar: number) =>
  t < from ? "" : text.slice(0, Math.floor((t - from) / perChar));

/** Worktrees: the switcher is open and the highlight walks down the agents' worktrees. */
export function worktreesAt(t: number) {
  const step = Math.floor(t / 1100);
  return { highlighted: 1 + (step % 3), key: step > 0 ? keyAt([[step * 1100, "↓"]], t) : undefined };
}

/** An agent at work: how many of its diff lines have landed. */
export const landedAt = (lines: number, t: number) => Math.min(lines, Math.floor(t / TYPE_MS));

// The review chapter walks the fuzzy-search worktree's files.
const FUZZY = SCENES[0];

export const REVIEW_DIFFS: Record<string, { start: [number, number]; lines: DiffLine[] }> = {
  "src/palette/QuickOpen.tsx": {
    start: [1, 1],
    lines: [
      { kind: " ", text: 'import { useState } from "react";' },
      { kind: "-", text: 'import { byName } from "../search/byName";' },
      { kind: "+", text: 'import { score } from "../search/fuzzy";' },
      { kind: " ", text: "" },
      { kind: " ", text: "export function QuickOpen({ files }: { files: string[] }) {" },
      { kind: " ", text: '  const [query, setQuery] = useState("");' },
      { kind: "-", text: "  const hits = files.filter((f) => byName(f, query));" },
      { kind: "+", text: "  const hits = files" },
      { kind: "+", text: "    .map((path) => ({ path, rank: score(query, path) }))" },
      { kind: "+", text: "    .filter((h) => h.rank >= 0)" },
      { kind: "+", text: "    .sort((a, b) => b.rank - a.rank);" },
      { kind: "-", text: "  return <Palette items={hits} onQuery={setQuery} />;" },
      { kind: "+", text: "  return <Palette items={hits.map((h) => h.path)} onQuery={setQuery} />;" },
      { kind: " ", text: "}" },
    ],
  },
  "src/search/fuzzy.ts": { start: FUZZY.start, lines: FUZZY.lines },
  "src/search/fuzzy.test.ts": {
    start: [0, 1],
    lines: `import assert from "node:assert/strict";
import { test } from "node:test";
import { score } from "./fuzzy.ts";

test("score ranks word starts first", () => {
  assert.ok(score("qo", "src/quick/open.ts") > score("qo", "src/aqo.ts"));
});

test("score returns -1 on a miss", () => {
  assert.equal(score("xyz", "src/search/fuzzy.ts"), -1);
});`
      .split("\n")
      .map((text) => ({ kind: "+" as const, text })),
  },
};

const REVIEW_ORDER = ["src/palette/QuickOpen.tsx", "src/search/fuzzy.ts", "src/search/fuzzy.test.ts"];
const byPath = (path: string) => FUZZY.files.find((f) => f.path === path)!;

/**
 * Review: J walks down the files and V marks each viewed; then the agent saves fuzzy.ts again, so
 * its mark clears and its count grows.
 */
export function reviewAt(t: number) {
  const events: KeyEvent[] = [
    [700, "V"],
    [1400, "J"],
    [2100, "V"],
    [2800, "J"],
    [3500, "V"],
  ];
  const pressed = (key: string) => events.filter(([at, k]) => k === key && at <= t).length;
  const cursor = Math.min(2, pressed("J"));
  const touched = t >= 4700;
  const files: ChangeFile[] = REVIEW_ORDER.map((path, i) => {
    const file = byPath(path);
    const viewed = pressed("V") > i && !(touched && i === 1);
    return { ...file, add: touched && i === 1 ? file.add + 3 : file.add, viewed };
  });
  return { files, cursor, touched, key: keyAt(events, t) };
}

export interface Pane {
  tab: TerminalTab;
  command: string;
  /** How much of the command has been typed, and how many output lines have printed. */
  typed: number;
  printed: number;
  output: "test" | "auth" | "dev";
}

export const OUTPUT_LINES = { test: 7, auth: 3, dev: 6 };

const FUZZY_TAB: TerminalTab = { folder: FUZZY.folder, branch: FUZZY.branch };
const AUTH_TAB: TerminalTab = { folder: SCENES[1].folder, branch: SCENES[1].branch };

function pane(tab: TerminalTab, command: string, output: Pane["output"], t: number, from: number): Pane | undefined {
  if (t < from) return undefined;
  const typedChars = Math.min(command.length, Math.floor((t - from) / 45));
  const doneTyping = from + command.length * 45 + 250;
  const printed = t < doneTyping ? 0 : Math.min(OUTPUT_LINES[output], 1 + Math.floor((t - doneTyping) / 110));
  return { tab, command, typed: typedChars, printed, output };
}

/**
 * Terminal: ⌘J opens it on this worktree and the tests run; then a second tab opens on another
 * agent's worktree from the + menu, and ⌘D splits it to start the dev server.
 */
export function terminalAt(t: number) {
  const events: KeyEvent[] = [
    [0, "⌘J"],
    [5300, "⌘D"],
  ];
  const secondTab = t >= 3300;
  const split = t >= 5300;
  const panes = [
    secondTab ? pane(AUTH_TAB, "pnpm test src/auth", "auth", t, 3500) : pane(FUZZY_TAB, "pnpm test", "test", t, 500),
    split ? pane(AUTH_TAB, "pnpm dev", "dev", t, 5600) : undefined,
  ].filter((p): p is Pane => !!p);
  const tabs: TerminalTab[] = secondTab ? [FUZZY_TAB, { ...AUTH_TAB, panes: split ? 2 : undefined }] : [FUZZY_TAB];
  return {
    tabs,
    active: secondTab ? 1 : 0,
    fresh: secondTab ? 1 : undefined,
    // The + menu, open while choosing the other worktree.
    menu: t >= 2500 && t < 3300 ? (t >= 2900 ? 2 : 1) : undefined,
    panes,
    key: keyAt(events, t),
  };
}

export const SUMMARY = "feat(search): add fuzzy matching to quick open";
const DESCRIPTION = "Scores consecutive hits and word starts higher, so the file you meant comes first.";

// The pointer glides to the sparkle and clicks it while the view zooms in on the commit box; the
// view zooms back out once the message is written, for the commit itself.
const CLICK_AT = 1000;
const SUMMARY_FROM = CLICK_AT + 600;
const DESCRIPTION_FROM = SUMMARY_FROM + SUMMARY.length * 28 + 200;
const WRITTEN = DESCRIPTION_FROM + DESCRIPTION.length * 16;
const ZOOM_OUT = WRITTEN + 500;
const COMMIT_AT = ZOOM_OUT + 1000;

/** Commit: a click on the sparkle asks the agent, it writes the message, ⌘↵ commits and the list empties. */
export function commitAt(t: number): { box: CommitState; committed: boolean; zoom: boolean; key?: string; files: ChangeFile[] } {
  const committed = t >= COMMIT_AT + 250;
  return {
    box: {
      summary: committed ? "" : typed(SUMMARY, t, SUMMARY_FROM, 28),
      description: committed ? "" : typed(DESCRIPTION, t, DESCRIPTION_FROM, 16),
      caret: committed || t < SUMMARY_FROM ? undefined : t < DESCRIPTION_FROM ? "summary" : t < WRITTEN ? "description" : undefined,
      asking: t >= CLICK_AT && t < WRITTEN,
      pressed: t >= COMMIT_AT && t < COMMIT_AT + 250,
      pointer: t < CLICK_AT ? "move" : t < CLICK_AT + 700 ? "click" : undefined,
    },
    committed,
    zoom: t < ZOOM_OUT,
    key: keyAt([[COMMIT_AT, "⌘↵"]], t),
    files: committed ? [] : FUZZY.files.map((f) => ({ ...f, viewed: true })),
  };
}

/** The repo as the explorer lists it, folders first. A worktree lacks the files other agents added. */
const TREE = [
  "public/",
  "public/favicon.svg",
  "src/",
  "src/api/",
  "src/api/client.ts",
  "src/auth/",
  "src/auth/login.ts",
  "src/auth/session.test.ts",
  "src/auth/session.ts",
  "src/features/",
  "src/features/orders.ts",
  "src/features/users.ts",
  "src/palette/",
  "src/palette/Palette.tsx",
  "src/palette/QuickOpen.tsx",
  "src/search/",
  "src/search/byName.ts",
  "src/search/fuzzy.test.ts",
  "src/search/fuzzy.ts",
  "src/App.tsx",
  "src/main.tsx",
  "index.html",
  "package.json",
  "README.md",
  "tsconfig.json",
  "vite.config.ts",
];

export function treeOf(scene: Scene) {
  const theirs = SCENES.filter((s) => s !== scene).flatMap((s) => s.files.filter((f) => f.status === "A").map((f) => f.path));
  return TREE.filter((p) => !theirs.includes(p));
}

const dirOf = (path: string) => path.slice(0, path.lastIndexOf("/") + 1);
/** The explorer as the app leaves it: the open file's folder expanded and the file selected. */
export const revealed = (path: string) => ({ open: ["src/", dirOf(path)], active: path });

/** A diff's new side as the file view shows it, with a change bar on each line the diff touched. */
export function fileLines(lines: DiffLine[]): FileLine[] {
  const out: FileLine[] = [];
  let removed = 0;
  for (const line of lines) {
    if (line.kind === "-") removed++;
    else if (line.kind === " ") {
      removed = 0;
      out.push({ text: line.text });
    } else {
      out.push({ text: line.text, bar: removed > 0 ? "mod" : "add" });
      removed = Math.max(0, removed - 1);
    }
  }
  return out;
}

function hitsIn(text: string, query: string, offset: number) {
  const hits: number[] = [];
  for (let i = 0; i < text.length && hits.length < query.length; i++) {
    if (text[i].toLowerCase() === query[hits.length].toLowerCase()) hits.push(offset + i);
  }
  return hits.length === query.length ? hits : undefined;
}

const RECENT = ["src/palette/QuickOpen.tsx", "src/search/fuzzy.ts", "src/search/fuzzy.test.ts", "src/palette/Palette.tsx", "src/App.tsx", "src/main.tsx"];

/** Quick open's list: recent files for no query, else the files whose name (then path) holds it in order. */
export function quickOpenItems(paths: string[], query: string) {
  if (!query) return RECENT.map((path) => ({ path, hits: [] }));
  return paths
    .filter((p) => !p.endsWith("/"))
    .flatMap((path) => {
      const cut = path.lastIndexOf("/") + 1;
      const inName = hitsIn(path.slice(cut), query, cut);
      const hits = inName ?? hitsIn(path, query, 0);
      return hits ? [{ path, hits, rank: inName ? 0 : 1 }] : [];
    })
    .sort((a, b) => a.rank - b.rank || a.path.length - b.path.length)
    .slice(0, 6);
}

const QUICK_OPEN = "src/palette/QuickOpen.tsx";

// The tour: one part of the window at a time, each doing its job.
export const TOUR_SPOTS = ["changes", "diff", "terminal", "explorer"] as const;
const SPOT_MS = 2400;

/**
 * One window, not four: the changes list walks its files, the diff lands, the tests run in the
 * terminal and the explorer opens a folder. Not playing (Infinity), it's the whole window at rest.
 */
export function tourAt(t: number) {
  if (!Number.isFinite(t)) return { spot: undefined, cursor: 0, shown: Infinity, terminal: undefined, ...revealed(FUZZY.file) };
  const i = Math.min(TOUR_SPOTS.length - 1, Math.floor(t / SPOT_MS));
  const local = t - i * SPOT_MS;
  const walk =
    i < 3 || local < 500
      ? revealed(FUZZY.file)
      : local < 1100
        ? revealed("src/search/fuzzy.test.ts")
        : { open: ["src/", "src/search/", "src/palette/"], active: local < 1700 ? "src/palette/" : QUICK_OPEN };
  return {
    spot: TOUR_SPOTS[i],
    cursor: i === 0 ? Math.min(2, Math.floor(local / 700)) : 0,
    shown: i === 1 ? Math.floor(local / 55) : Infinity,
    terminal: i === 2 ? terminalAt(local) : i > 2 ? terminalAt(SPOT_MS) : undefined,
    ...walk,
  };
}

const EDIT = " Case never matters.";
const E = { folder: 700, pick: 1300, palette: 2800, type: 3100, enter: 4200, edit: 4900, save: 6600 };

/**
 * Explorer: a click opens a folder, another opens a file with change bars in its gutter; ⌘P finds
 * fuzzy.ts, a line gets edited in place and ⌘S saves it.
 */
export function explorerAt(t: number) {
  const tree = treeOf(FUZZY);
  const query = typed("fuzzy", t, E.type, 120);
  const path = t >= E.pick && t < E.enter ? QUICK_OPEN : FUZZY.file;
  const lines = fileLines(path === QUICK_OPEN ? REVIEW_DIFFS[QUICK_OPEN].lines : FUZZY.lines);
  const editing = path === FUZZY.file && t >= E.edit;
  if (editing) lines[9] = { ...lines[9], text: lines[9].text + typed(EDIT, t, E.edit, 45) };
  return {
    tree,
    open: ["src/", "src/search/", ...(t >= E.folder ? ["src/palette/"] : [])],
    active: t >= E.folder && t < E.pick ? "src/palette/" : path,
    view: t < E.pick ? ("diff" as const) : ("file" as const),
    path,
    lines,
    caret: editing ? 9 : undefined,
    unsaved: editing && t >= E.edit + 45 && t < E.save,
    preview: !editing,
    palette: t >= E.palette && t < E.enter ? { query, items: quickOpenItems(tree, query) } : undefined,
    key: keyAt(
      [
        [E.palette, "⌘P"],
        [E.enter, "↵"],
        [E.save, "⌘S"],
      ],
      t,
    ),
  };
}

export const HOLD_MS = 2000;

/** How long each chapter's clip plays before it holds its last frame, then starts over. */
export function clipLength(chapter: number) {
  if (chapter < 0) return SPOT_MS * TOUR_SPOTS.length;
  if (chapter === 0) return 4400;
  if (chapter <= 3) return SCENES[chapter - 1].lines.length * TYPE_MS;
  if (chapter === 4) return 5900;
  if (chapter === 5) return 7000;
  if (chapter === 6) return 7600;
  return COMMIT_AT + 1800;
}
