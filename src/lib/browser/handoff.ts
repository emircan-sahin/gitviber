import { failed, toast } from "../app/toast";
import { forTerminal } from "../review/notes";
import { pasteToWorktree } from "../terminal/terminals";

/**
 * Text from a browser tab for `root`'s agent: pasted into its terminal (the agent's pane, else
 * the one last used), not sent, as review notes go. With no terminal there, copied instead.
 */
export function toAgent(root: string, text: string) {
  const clean = forTerminal(text);
  if (pasteToWorktree(root, clean)) return;
  void navigator.clipboard
    .writeText(clean)
    .then(() => toast("info", "No terminal in this worktree", "Copied instead: paste it where the agent runs."), failed("Could not copy"));
}
