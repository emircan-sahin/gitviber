import { CheckCircle2, FolderGit2, FolderOpen } from "lucide-react";
import { Fragment } from "react";
import { ansi, ChangeRow, Cursor, Prompt, TerminalBody, TerminalPaneHeader, TerminalTabs, type ChangeFile } from "../app/parts.tsx";
import { SCENES, type Scene } from "../demo-data.ts";
import { cx, Keys } from "../ui.tsx";
import { OUTPUT_LINES, type Pane, type reviewAt, type terminalAt } from "./timeline.ts";

/** The key just "pressed", popping up in the corner the way a screencast shows keystrokes. */
export function KeyHud({ label, className }: { label?: string; className?: string }) {
  return (
    <div aria-hidden className={cx("pointer-events-none absolute z-30", className)}>
      {label && (
        <div key={label} className="key-pop rounded-lg border border-border-strong bg-elevated/95 p-2 shadow-lg shadow-black/60 text-[15px]">
          <Keys keys={label.startsWith("⌘") ? ["⌘", label.slice(1)] : [label]} />
        </div>
      )}
    </div>
  );
}

const OUTPUT = {
  test: [
    <>
      <span className={ansi.green}>✔</span> score ranks word starts first <span className={ansi.dim}>(0.412ms)</span>
    </>,
    <>
      <span className={ansi.green}>✔</span> score returns -1 on a miss <span className={ansi.dim}>(0.061ms)</span>
    </>,
    <>
      <span className={ansi.green}>✔</span> quick open sorts by score <span className={ansi.dim}>(1.208ms)</span>
    </>,
    <>
      <span className={ansi.blue}>ℹ</span> tests 3
    </>,
    <>
      <span className={ansi.blue}>ℹ</span> pass 3
    </>,
    <>
      <span className={ansi.blue}>ℹ</span> fail 0
    </>,
    <>
      <span className={ansi.blue}>ℹ</span> duration_ms 88.41
    </>,
  ],
  auth: [
    <>
      <span className={ansi.green}>✔</span> sends an expired session to login <span className={ansi.dim}>(0.914ms)</span>
    </>,
    <>
      <span className={ansi.green}>✔</span> keeps the page to come back to <span className={ansi.dim}>(0.337ms)</span>
    </>,
    <>
      <span className={ansi.blue}>ℹ</span> pass 2
    </>,
  ],
  dev: [
    <> </>,
    <>
      {"  "}
      <span className={cx(ansi.green, "font-bold")}>VITE</span> <span className={ansi.green}>v8.3.0</span>
      {"  "}
      <span className={ansi.dim}>ready in</span> <b>312</b> ms
    </>,
    <> </>,
    <>
      {"  "}
      <span className={ansi.green}>➜</span>
      {"  "}
      <b>Local</b>:{"   "}
      <span className={ansi.cyan}>http://localhost:5173/</span>
    </>,
    <>
      {"  "}
      <span className={ansi.green}>➜</span>
      {"  "}
      <span className={ansi.dim}>Network: use </span>
      <b>--host</b>
      <span className={ansi.dim}> to expose</span>
    </>,
    <>
      <span className={ansi.dim}>4:12:08 PM</span> <span className={ansi.cyan}>[vite]</span>{" "}
      <span className={ansi.green}>hmr update</span> <span className={ansi.dim}>/src/auth/session.ts</span>
    </>,
  ],
} satisfies Record<Pane["output"], unknown[]>;

/** One terminal pane: the prompt, the command as it's typed, then its output line by line. */
export function PaneText({ pane }: { pane: Pane }) {
  const typing = pane.typed < pane.command.length;
  const done = pane.printed === OUTPUT_LINES[pane.output];
  return (
    <>
      <Prompt folder={pane.tab.folder} branch={pane.tab.branch} />
      {pane.command.slice(0, pane.typed)}
      {(typing || pane.printed === 0) && <Cursor />}
      {"\n"}
      {OUTPUT[pane.output].slice(0, pane.printed).map((line, i) => (
        <Fragment key={i}>
          {line}
          {"\n"}
        </Fragment>
      ))}
      {done && pane.output !== "dev" && (
        <>
          <Prompt folder={pane.tab.folder} branch={pane.tab.branch} />
          <Cursor />
        </>
      )}
    </>
  );
}

// The + menu's worktrees: every one but the terminal's own (src/features/terminal/TerminalPanel.tsx).
const OTHER_WORKTREES = [{ folder: "acme-web", branch: "main" }, ...SCENES.slice(1)];

/** A pane's title: the command while it runs (the shell names it), else the folder. */
const paneTitle = (pane: Pane) =>
  pane.typed === pane.command.length && pane.printed < OUTPUT_LINES[pane.output] ? pane.command : pane.tab.folder;

/**
 * The terminal panel: its tabs, the open tab's panes side by side (each with its title bar once
 * there are two, the last one focused), and the + menu when open.
 */
export function TerminalPanel({ state, wrap }: { state: ReturnType<typeof terminalAt>; wrap?: boolean }) {
  const split = state.panes.length > 1;
  return (
    <div className="relative flex h-full min-h-0 flex-col bg-bg">
      <TerminalTabs tabs={state.tabs} active={state.active} fresh={state.fresh} compact={wrap} />
      <div className={cx("grid min-h-0 flex-1", split ? "grid-cols-2" : "grid-cols-1")}>
        {state.panes.map((pane, i) => (
          <div key={pane.output} className={cx("flex min-h-0 flex-col", i > 0 && "pane-in border-l border-fg/20")}>
            {split && <TerminalPaneHeader title={paneTitle(pane)} focused={i === state.panes.length - 1} />}
            <TerminalBody wrap={wrap} className="min-h-0 flex-1 overflow-hidden">
              <PaneText pane={pane} />
            </TerminalBody>
          </div>
        ))}
      </div>
      {state.menu !== undefined && (
        <div className="menu-in absolute top-9 right-[170px] z-20 w-72 rounded-md border border-border-strong bg-elevated p-1 text-[12px] shadow-lg shadow-black/50">
          <p className="px-2 pt-2 pb-1 text-[11px] font-medium tracking-wide text-subtle uppercase">New terminal in worktree</p>
          {OTHER_WORKTREES.map((w, i) => (
            <div
              key={w.folder}
              className={cx("flex h-7 items-center gap-2 rounded-sm px-2", i === state.menu && "bg-primary text-white")}
            >
              <FolderGit2 className="size-3.5 opacity-70" />
              <span className="truncate">{w.folder}</span>
              <span className={cx("ml-auto truncate font-mono text-[11px]", i === state.menu ? "text-white/80" : "text-subtle")}>{w.branch}</span>
            </div>
          ))}
          <div className="-mx-1 my-1 h-px bg-border" />
          <div className="flex h-7 items-center gap-2 rounded-sm px-2">
            <FolderOpen className="size-3.5 opacity-70" />
            Choose Folder…
          </div>
        </div>
      )}
    </div>
  );
}

/** The Changes rows while reviewing: the cursor row, and a flash when the agent saves one again. */
export function ReviewRows({ state }: { state: ReturnType<typeof reviewAt> }) {
  return state.files.map((file, i) => (
    <div key={file.path} className="relative">
      <ChangeRow file={file} active={i === state.cursor} className={cx(state.touched && i === 1 && "touched-row")} />
      {state.touched && i === 1 && (
        <span className="pointer-events-none absolute top-1/2 right-16 -translate-y-1/2 rounded bg-modified/20 px-1.5 text-[10.5px] text-modified">
          saved
        </span>
      )}
    </div>
  ));
}

export function CommitRows({ files }: { files: ChangeFile[] }) {
  return files.map((file) => <ChangeRow key={file.path} file={file} />);
}

/** The app's toast after a commit (src/components/Toaster.tsx): title, the summary, Undo. */
export function CommitToast({ show, summary }: { show: boolean; summary: string }) {
  return (
    <div
      aria-hidden
      className={cx(
        "absolute right-4 bottom-10 z-30 flex w-96 gap-3 rounded-md border border-border-strong bg-elevated p-3 shadow-lg shadow-black/50 transition-all duration-300",
        show ? "translate-y-0 opacity-100" : "translate-y-2 opacity-0",
      )}
    >
      <CheckCircle2 className="mt-0.5 size-4 shrink-0 text-added" />
      <div className="min-w-0 flex-1">
        <div className="text-[12px] font-medium">Committed</div>
        <div className="mt-1 truncate text-[11.5px] leading-relaxed text-muted">{summary}</div>
      </div>
      <span className="h-fit rounded-sm px-1.5 py-0.5 text-[12px] font-medium text-primary">Undo</span>
    </div>
  );
}

/** An agent's run in its worktree's terminal: the command, what it printed, and whether it's done. */
export function AgentRun({ scene, typing }: { scene: Scene; typing: boolean }) {
  return (
    <>
      <Prompt folder={scene.folder} branch={scene.branch} />
      {scene.agent} <span className="text-muted">"{scene.prompt}"</span>
      {"\n\n"}
      {scene.output.map((line) => (
        <span key={line}>
          <span className={line.startsWith("⏺") || line.startsWith("•") ? "text-fg" : ansi.dim}>{line}</span>
          {"\n"}
        </span>
      ))}
      {typing ? (
        <span className={ansi.yellow}>{scene.agent === "claude" ? "✻ Writing…" : "• Working"}</span>
      ) : (
        <>
          <span className={ansi.green}>{scene.agent === "claude" ? "⏺ Done." : "• Done."}</span>
          {"\n"}
          <span className="text-subtle">{"> "}</span>
          <Cursor />
        </>
      )}
    </>
  );
}
