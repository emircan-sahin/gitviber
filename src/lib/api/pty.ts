import { Channel, invoke } from "@tauri-apps/api/core";
import type { PaneAgent } from "../terminal/agentState";

/** The shells behind the terminal's panes (pty.rs), by the id `spawn` gives. */
export const pty = {
  /**
   * `onOutput` gets what the shell prints, `onExit` how it ended (null if unknown). `integration`
   * loads the shell integration (zsh, bash 4.4+); `integrated` says whether it was. `folder`
   * starts it there instead of `cwd`, if that folder is still there.
   */
  spawn: (cwd: string, folder: string | null, cols: number, rows: number, integration: boolean, onOutput: (bytes: ArrayBuffer) => void, onExit: (exit: PtyExit | null) => void) => {
    const output = new Channel<ArrayBuffer>(onOutput);
    const exit = new Channel<PtyExit | null>(onExit);
    return invoke<{ id: number; integrated: boolean }>("pty_spawn", { cwd, folder, cols, rows, integration, output, exit });
  },
  /** The folder a shell is in now, asked of its process; null if that can't be read. */
  cwd: (id: number) => invoke<string | null>("pty_cwd", { id }),
  /** `binary`: `data` is bytes, a char each (xterm's onBinary), not text. */
  write: (id: number, data: string, binary = false) => invoke<void>("pty_write", { id, data, binary }),
  /** `bytes` of the output that xterm.js has parsed: pty.rs stops reading past a high water of unparsed output. */
  ack: (id: number, bytes: number) => invoke<void>("pty_ack", { id, bytes }),
  resize: (id: number, cols: number, rows: number) => invoke<void>("pty_resize", { id, cols, rows }),
  kill: (id: number) => invoke<void>("pty_kill", { id }),
  /** How many (of `ids`, or all) are running a command rather than sitting at the prompt. */
  busy: (ids?: number[]) => invoke<number>("pty_busy", { ids: ids ?? null }),
  /** The coding agents (agents.rs) the shells `ids` run, by id; watched from then on for "agent-state". */
  agents: (ids: number[]) => invoke<Partial<Record<number, PaneAgent>>>("pty_agents", { ids }),
  /** The conversations agents had in `cwd` (conversations.rs), newest first, each with the command that resumes it. */
  conversations: (cwd: string) => invoke<Conversation[]>("agent_conversations", { cwd }),
  /** Which of `paths` are still folders. */
  foldersLeft: (paths: string[]) => invoke<boolean[]>("folders_left", { paths }),
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

/** A past conversation of an agent, for Resume a conversation. */
export interface Conversation {
  agent: string;
  id: string;
  /** The agent's own title for it, else its first prompt. */
  title: string;
  /** The branch it started on. */
  branch: string | null;
  /** Unix seconds of its last write. */
  modified: number;
  command: string;
}
