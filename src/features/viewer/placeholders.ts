// What a file's view says instead of, or above, its diff: a binary or too large file, a change git's
// text diff can't show (a mode, a submodule's own changes), a newline at the end coming or going.
import type { DiffPair, FileChange } from "@/lib/api";
import { changedInside } from "@/features/changes/changeList";

const KINDS: Record<string, string> = { "100644": "file", "100755": "file", "120000": "symlink", "160000": "submodule" };

/** What a text diff can't show about a change: its mode or type, or a submodule's own changes. */
function fileNote(file: FileChange | null) {
  if (file?.mode) {
    const [from, to] = file.mode.split(" → ").map((m) => KINDS[m]);
    return from && to && from !== to ? `Changed from a ${from} to a ${to} (${file.mode})` : `File mode changed: ${file.mode}`;
  }
  return changedInside(file) ? "This submodule has changes inside it; commit them in the submodule" : null;
}

export function placeholderFor(pair: DiffPair, isFile: boolean, file: FileChange | null) {
  const { original: a, modified: b } = pair;
  if (isFile && !b.exists) return "This file no longer exists";
  if (b.lfsMissing || a.lfsMissing) return b.lfsMissing ?? a.lfsMissing;
  if (a.binary || b.binary) return "Binary file";
  if (a.tooLarge || b.tooLarge) return "File is too large to display";
  if (!isFile && !pair.rows.some((r) => r.k !== 0)) {
    // First: an added or renamed file whose mode changed isn't only that.
    const note = fileNote(file);
    if (note) return note;
    if (pair.whitespaceHidden) return "Only whitespace changed (hidden)";
    // Added or deleted with no line changed: there were none.
    if (a.exists !== b.exists) return "Empty file";
    if (file?.status === "R") return "Renamed without changes";
    if (file?.status === "C") return "Copied without changes";
    return "No textual changes";
  }
  return null;
}

/** Why a last line shows removed and added unchanged: the newline after it came or went. */
function newlineNote({ original: a, modified: b, rows }: DiffPair) {
  if (!a.text || !b.text || a.text.endsWith("\n") === b.text.endsWith("\n")) return null;
  // Rows hold every line, so the last one naming the new side is its last line; ignoring
  // whitespace can leave it unchanged.
  let last = rows.length - 1;
  while (last >= 0 && !rows[last].n) last--;
  if (last < 0 || rows[last].k === 0) return null;
  return b.text.endsWith("\n") ? "Newline added at end of file" : "No newline at end of file";
}

/** What the diff's header says about a change shown as text: a mode or submodule change, line endings, a newline at the end, hidden whitespace. */
export function diffNote(pair: DiffPair, file: FileChange | null) {
  const text = pair.eolOnly ? "Only line endings changed" : (newlineNote(pair) ?? (pair.whitespaceHidden ? "Whitespace changes hidden" : null));
  return [fileNote(file), text].filter(Boolean).join(" · ") || null;
}
