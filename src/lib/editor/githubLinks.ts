// Copy GitHub Link and Open on GitHub in the code view's context menu: a permalink to the file,
// or to the selected lines, at a commit GitHub has (lib/github/permalink).
import type { GitHubSide } from "../github/permalink";
import { gitHubLink } from "../github/url";
import { toast } from "../app/toast";
import { isEdited } from "./edits";
import type { monaco } from "./monaco";

export type Side = "original" | "modified";

/** The selected lines, none for an empty selection. One that ends at the start of a line doesn't take that line. */
function selectedLines(code: monaco.editor.ICodeEditor): [number, number] | null {
  const sel = code.getSelection();
  if (!sel || sel.isEmpty()) return null;
  const to = sel.endColumn === 1 && sel.endLineNumber > sel.startLineNumber ? sel.endLineNumber - 1 : sel.endLineNumber;
  return [sel.startLineNumber, to];
}

/** Copies (or opens) the link to what `code`, showing `side`, has selected. */
export function linkSelection(side: GitHubSide, code: monaco.editor.ICodeEditor, open: boolean) {
  const lines = selectedLines(code);
  // The saved file's lines are what's matched against GitHub's.
  if (lines && !side.sha && isEdited(side.path)) return toast("info", "Save the file first", "The link goes by the saved file's line numbers.");
  void gitHubLink(side, lines, open);
}

export interface GitHubLinks extends monaco.IDisposable {
  /** Shows or hides the menu items, after what `github` returns has changed. */
  update(): void;
}

/** The menu items in each of `editors`, for what `github` says its side is on GitHub (null: not there). */
export function followGitHubLinks(editors: readonly (readonly [monaco.editor.IStandaloneCodeEditor, Side])[], github: (side: Side) => GitHubSide | null): GitHubLinks {
  const subs: monaco.IDisposable[] = [];
  const keys = editors.map(([code, side]) => {
    const key = code.createContextKey<boolean>("gitviberGitHub", false);
    for (const [id, label, open] of [
      ["gitviber.copyGitHubLink", "Copy GitHub Link", false],
      ["gitviber.openOnGitHub", "Open on GitHub", true],
    ] as const) {
      const run = () => {
        const s = github(side);
        if (s) linkSelection(s, code, open);
      };
      subs.push(code.addAction({ id, label, contextMenuGroupId: "9_cutcopypaste", contextMenuOrder: 10, precondition: "gitviberGitHub", run }));
    }
    return () => key.set(!!github(side));
  });
  keys.forEach((set) => set());
  return { update: () => keys.forEach((set) => set()), dispose: () => subs.forEach((s) => s.dispose()) };
}
