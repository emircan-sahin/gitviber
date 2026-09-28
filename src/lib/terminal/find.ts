import { compileFind, type FindOptions } from "../ui/findQuery";
import { findColors } from "./theme";
import { activeGroup, focusActive, panes, type Pane } from "./terminals";

// Find in a pane's text, its scrollback included.
// Imported through terminals.ts only: the two import each other.

/** The pane find searches, and where its count goes (index 0: past the addon's 1000 marked matches). */
let searching: { pane: Pane; onResults: (at: { index: number; total: number }) => void } | null = null;

/** Reports a pane's matches to the find box, while it's the pane searched. */
export function watchFind(p: Pane) {
  p.search.onDidChangeResults(({ resultIndex, resultCount }) => searching?.pane === p && searching.onResults({ index: resultIndex + 1, total: resultCount }));
}

/** A closed pane is searched no more. */
export function forgetFind(p: Pane) {
  if (searching?.pane === p) searching = null;
}

/**
 * Find in the active tab's focused pane: `step` 0 as the query is typed (staying on the match
 * on show while it still matches), 1 or -1 for the next or previous. An empty query clears it;
 * a regex that doesn't parse is returned as the error, nothing searched.
 */
export function findInTerminal(query: string, find: FindOptions, step: 0 | 1 | -1, onResults: (at: { index: number; total: number }) => void): string | null {
  const g = activeGroup();
  const p = g && panes.get(g.focused);
  if (searching && searching.pane !== p) searching.pane.search.clearDecorations();
  searching = p ? { pane: p, onResults } : null;
  if (!p) return null;
  const re = compileFind(query, find);
  if (!query || re instanceof Error) {
    p.search.clearDecorations();
    onResults({ index: 0, total: 0 });
    return re instanceof Error ? re.message : null;
  }
  const options = { caseSensitive: find.matchCase, wholeWord: find.wholeWord, regex: find.regex, decorations: findColors(), incremental: step === 0 };
  if (step === -1) p.search.findPrevious(query, options);
  else p.search.findNext(query, options);
  return null;
}

/** Find's marks go, and its count stops being reported (the box went, or searches elsewhere now). */
export function clearFind() {
  searching?.pane.search.clearDecorations();
  searching = null;
}

/** Closes find: its marks go, and the pane gets the keys back. */
export function endFind() {
  clearFind();
  focusActive();
}
