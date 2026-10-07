import { folderName } from "../path.ts";

/**
 * The main window's title: the title bar hides it, but Mission Control, the Window menu and
 * VoiceOver tell windows apart by it. A detached HEAD shows its commit.
 */
export function windowTitle(root: string | null, branch: string | null, head: string | null) {
  if (!root) return "GitViber";
  const at = branch ?? head?.slice(0, 7);
  return at ? `${folderName(root)} — ${at}` : folderName(root);
}
