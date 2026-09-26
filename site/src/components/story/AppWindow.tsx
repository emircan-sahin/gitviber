import { Check } from "lucide-react";
import type { ReactNode } from "react";
import { DiffRows, FileRows, numbered, type FileLine } from "../app/Diff.tsx";
import {
  CommitBox,
  EditorTab,
  ExplorerPanel,
  FileHeader,
  GitPanel,
  QuickOpen,
  StatusBar,
  TerminalBody,
  TerminalTabs,
  TopBar,
  Window,
  WorktreeMenu,
  type ChangeFile,
  type WorktreeRow,
} from "../app/parts.tsx";
import { SCENES } from "../demo-data.ts";
import { cx } from "../ui.tsx";
import { AgentRun, CommitRows, CommitToast, KeyHud, ReviewRows, TerminalPanel } from "./panels.tsx";
import {
  commitAt,
  explorerAt,
  landedAt,
  revealed,
  REVIEW_DIFFS,
  reviewAt,
  SUMMARY,
  terminalAt,
  TOUR_SPOTS,
  tourAt,
  treeOf,
  worktreesAt,
} from "./timeline.ts";

// The app at 90% interface scale: its default panel widths (320 and 260) still leave the diff
// room for its longest lines.
export const WIN_W = 1340;
export const WIN_H = 780;

export const WORKTREES: WorktreeRow[] = [
  { branch: "main", path: "~/code/acme-web", main: true, time: "2h ago" },
  ...SCENES.map((s) => ({ branch: s.branch, path: `~/code/${s.folder}`, working: true, time: "now", changes: s.files.length })),
];

/** The app's empty Changes list (src/features/changes/ChangeRows.tsx AllCaughtUp). */
export function AllCaughtUp({ className = "pt-20" }: { className?: string }) {
  return (
    <div className={cx("flex flex-col items-center gap-1.5 px-6 text-center", className)}>
      <Check className="mb-1 size-6 text-border-strong" />
      <div className="text-[12.5px] text-muted">Working tree clean</div>
      <div className="text-[11.5px] leading-relaxed text-subtle">Anything your agent writes shows up here instantly.</div>
    </div>
  );
}

/**
 * The one window the whole story plays in. `chapter` is the active chapter (-1 before the first:
 * the tour of the window), `t` the time since it became active; everything shown is derived from
 * those two, so the same moment always looks the same.
 */
export function AppWindow({ chapter, t }: { chapter: number; t: number }) {
  const scene = SCENES[chapter >= 1 && chapter <= 3 ? chapter - 1 : 0];
  let files: ChangeFile[] = scene.files;
  let active = 0;
  let rows: ReactNode | undefined;
  let diff = { path: scene.file, start: scene.start, lines: scene.lines, shown: scene.lines.length };
  let file: { lines: FileLine[]; caret?: number; unsaved?: boolean; preview?: boolean } | undefined;
  let terminal: ReactNode = null;
  let terminalHeight = 170;
  let footer: ReactNode = <CommitBox />;
  let key: string | undefined;
  let overlay: ReactNode = null;
  let dimDiff = false;
  // Close in on the commit box, the way a screencast zooms to where the click happens.
  let zoom = false;
  let tree = { paths: treeOf(scene), ...revealed(scene.file) };
  let spot: (typeof TOUR_SPOTS)[number] | undefined;

  if (chapter <= 3) {
    const tour = chapter < 0 ? tourAt(t) : undefined;
    if (tour) {
      spot = tour.spot;
      active = tour.cursor;
      diff = { ...diff, shown: Math.min(diff.lines.length, tour.shown) };
      tree = { ...tree, open: tour.open, active: tour.active };
    }
    if (chapter >= 1) diff = { ...diff, shown: landedAt(scene.lines.length, t) };
    const typing = chapter >= 1 && diff.shown < scene.lines.length;
    // While the agent writes, the open file's counts follow the lines that have landed.
    const landed = numbered(diff.lines, diff.start).slice(0, chapter >= 1 ? diff.shown : undefined);
    files = [
      { ...scene.files[0], add: landed.filter((l) => l.kind === "+").length, del: landed.filter((l) => l.kind === "-").length },
      ...scene.files.slice(1),
    ];
    terminal = tour?.terminal ? (
      <TerminalPanel state={tour.terminal} />
    ) : (
      <>
        <TerminalTabs tabs={[{ folder: scene.folder, branch: scene.branch }]} />
        <TerminalBody className="h-full">
          <AgentRun scene={scene} typing={typing} />
        </TerminalBody>
      </>
    );
    key = tour?.terminal?.key;
    if (chapter === 0) {
      const state = worktreesAt(t);
      key = state.key;
      overlay = (
        <div className="menu-in absolute top-1 left-[486px] z-20">
          <WorktreeMenu rows={WORKTREES} current={1} highlighted={state.highlighted} />
        </div>
      );
    }
  } else if (chapter === 4) {
    const state = reviewAt(t);
    files = state.files;
    active = state.cursor;
    rows = <ReviewRows state={state} />;
    const path = files[state.cursor].path;
    diff = { path, ...REVIEW_DIFFS[path], shown: REVIEW_DIFFS[path].lines.length };
    terminalHeight = 0;
    key = state.key;
    tree = { ...tree, ...revealed(path) };
  } else if (chapter === 5) {
    const state = terminalAt(t);
    terminal = <TerminalPanel state={state} />;
    terminalHeight = 250;
    key = state.key;
  } else if (chapter === 6) {
    const state = explorerAt(t);
    terminalHeight = 0;
    key = state.key;
    tree = { paths: state.tree, open: state.open, active: state.active };
    diff = { ...diff, path: state.path };
    if (state.view === "file") file = state;
    if (state.palette) overlay = <QuickOpen {...state.palette} />;
  } else {
    const state = commitAt(t);
    files = state.files;
    rows = files.length ? <CommitRows files={files} /> : <AllCaughtUp />;
    footer = <CommitBox {...state.box} />;
    terminalHeight = 0;
    zoom = state.zoom;
    dimDiff = state.committed;
    key = state.key;
    overlay = <CommitToast show={state.committed} summary={SUMMARY} />;
  }

  const lines = numbered(diff.lines, diff.start);
  const visible = lines.slice(0, diff.shown);
  const open = files.find((f) => f.path === diff.path) ?? scene.files[0];
  const add = files.reduce((s, f) => s + f.add, 0);
  const del = files.reduce((s, f) => s + f.del, 0);
  const status = Object.fromEntries(files.map((f) => [f.path, f.status]));
  // The tour dims everything but the part it's showing.
  const lit = (part: (typeof TOUR_SPOTS)[number]) => cx("transition-opacity duration-500", spot && spot !== part && "opacity-30");

  return (
    <div
      className="h-full origin-[0%_96%] transition-transform duration-900 ease-[cubic-bezier(0.4,0,0.2,1)]"
      style={{ transform: zoom ? "scale(1.9)" : undefined }}
    >
      <Window>
        <TopBar
          repo="acme-web"
          branch={scene.branch}
          worktree={scene.folder}
          worktrees={WORKTREES.length}
          added={add}
          removed={del}
          menuOpen={chapter === 0}
        />
        <div className="relative flex min-h-0 flex-1">
          <div className={lit("changes")}>
            <GitPanel files={files} active={active} width={320} footer={footer}>
              {rows}
            </GitPanel>
          </div>
          <div className="flex min-w-0 flex-1 flex-col bg-bg">
            <div className={cx("flex min-h-0 flex-1 flex-col", lit("diff"))}>
              <EditorTab name={diff.path.split("/").pop()!} kind={file ? undefined : "diff"} preview={file?.preview ?? true} unsaved={file?.unsaved} />
              <div className={cx("flex min-h-0 flex-1 flex-col transition-opacity duration-500", dimDiff && "opacity-0")}>
                <FileHeader path={diff.path} add={open.add} del={open.del} status={open.status} file={!!file} />
                <div className="min-h-0 flex-1 overflow-hidden">
                  {file ? (
                    <FileRows lines={file.lines} caret={file.caret} />
                  ) : (
                    <DiffRows lines={visible} fresh={diff.shown < lines.length ? diff.shown - 1 : undefined} />
                  )}
                </div>
              </div>
            </div>
            <div
              className={cx(
                "flex shrink-0 flex-col overflow-hidden transition-[height,opacity] duration-500 ease-[cubic-bezier(0.3,0.7,0.1,1)]",
                terminalHeight > 0 && "border-t border-border",
                lit("terminal"),
              )}
              style={{ height: terminalHeight }}
            >
              {terminal}
            </div>
          </div>
          <div className={lit("explorer")}>
            <ExplorerPanel paths={tree.paths} open={tree.open} active={tree.active} status={status} />
          </div>
          {overlay}
          <KeyHud label={key} className="right-5 bottom-5" />
        </div>
        <StatusBar files={files.length} add={add} del={del} reviewed={files.filter((f) => f.viewed).length} />
      </Window>
    </div>
  );
}
