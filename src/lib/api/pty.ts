import { Channel, invoke } from "@tauri-apps/api/core";

/** The shells behind the terminal's panes (pty.rs), by the id `spawn` gives. */
export const pty = {
  /** `onOutput` gets what the shell prints, `onExit` runs when it ends. */
  spawn: (cwd: string, cols: number, rows: number, onOutput: (bytes: ArrayBuffer) => void, onExit: () => void) => {
    const output = new Channel<ArrayBuffer>(onOutput);
    const exit = new Channel<number | null>(onExit);
    return invoke<number>("pty_spawn", { cwd, cols, rows, output, exit });
  },
  write: (id: number, data: string) => invoke<void>("pty_write", { id, data }),
  resize: (id: number, cols: number, rows: number) => invoke<void>("pty_resize", { id, cols, rows }),
  kill: (id: number) => invoke<void>("pty_kill", { id }),
  /** How many are running a command rather than sitting at the prompt. */
  busy: () => invoke<number>("pty_busy"),
  /** What ⌘V pastes into a terminal (clipboard.rs): copied files, text, or an image saved as a PNG. */
  paste: () => invoke<TerminalPaste>("terminal_paste"),
  /** Dropped files, the ones macOS takes back after the drag copied somewhere that lasts. */
  keepDropped: (paths: string[]) => invoke<string[]>("keep_dropped", { paths }),
};

export type TerminalPaste = { kind: "files"; paths: string[] } | { kind: "text"; text: string } | { kind: "image"; path: string } | { kind: "empty" };
