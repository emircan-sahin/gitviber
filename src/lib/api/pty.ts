import { Channel, invoke } from "@tauri-apps/api/core";

/** The shells behind the terminal's panes (pty.rs), by the id `spawn` gives. */
export const pty = {
  /**
   * `onOutput` gets what the shell prints, `onExit` how it ended (null if unknown). `integration`
   * loads the shell integration (zsh and bash only); `integrated` says whether it was.
   */
  spawn: (cwd: string, cols: number, rows: number, integration: boolean, onOutput: (bytes: ArrayBuffer) => void, onExit: (exit: PtyExit | null) => void) => {
    const output = new Channel<ArrayBuffer>(onOutput);
    const exit = new Channel<PtyExit | null>(onExit);
    return invoke<{ id: number; integrated: boolean }>("pty_spawn", { cwd, cols, rows, integration, output, exit });
  },
  write: (id: number, data: string) => invoke<void>("pty_write", { id, data }),
  resize: (id: number, cols: number, rows: number) => invoke<void>("pty_resize", { id, cols, rows }),
  kill: (id: number) => invoke<void>("pty_kill", { id }),
  /** How many (of `ids`, or all) are running a command rather than sitting at the prompt. */
  busy: (ids?: number[]) => invoke<number>("pty_busy", { ids: ids ?? null }),
  /** What ⌘V pastes into a terminal (clipboard.rs): copied files, text, or an image saved as a PNG. */
  paste: () => invoke<TerminalPaste>("terminal_paste"),
  /** Text a program in the terminal copies (OSC 52) onto the clipboard, natively (clipboard.rs). */
  copy: (text: string) => invoke<void>("terminal_copy", { text }),
  /** Dropped files, the ones macOS takes back after the drag copied somewhere that lasts. */
  keepDropped: (paths: string[]) => invoke<string[]>("keep_dropped", { paths }),
};

export type TerminalPaste = { kind: "files"; paths: string[] } | { kind: "text"; text: string } | { kind: "image"; path: string } | { kind: "empty" };

/** A shell's exit code, or the signal that ended it ("Segmentation fault: 11"). */
export type PtyExit = { code: number | null; signal: string | null };
