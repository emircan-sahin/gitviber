import type { monaco } from "@/lib/editor/monaco";

export type Editor = monaco.editor.IStandaloneDiffEditor | monaco.editor.IStandaloneCodeEditor;
export const isDiff = (e: Editor): e is monaco.editor.IStandaloneDiffEditor => "getLineChanges" in e;
export const codeEditor = (e: Editor) => (isDiff(e) ? e.getModifiedEditor() : e);

type Shown = { path: string; line: () => number };

// The code view's editor, while one is shown.
let live: Editor | null = null;
// The file on show and where it's read, for "Open in" an editor at that line.
let onShow: Shown | null = null;

export function showEditor(e: Editor) {
  live = e;
}
export function hideEditor(e: Editor) {
  if (live === e) live = null;
}
export function showFile(f: Shown) {
  onShow = f;
}
export function hideFile(f: Shown) {
  if (onShow === f) onShow = null;
}

/** The text selected in the code view, if it's on one line: what Search in Files starts from. */
export function selectedText(): string {
  const code = live && (isDiff(live) && live.getOriginalEditor().hasWidgetFocus() ? live.getOriginalEditor() : codeEditor(live));
  const sel = code?.getSelection();
  if (!code || !sel || sel.isEmpty() || sel.startLineNumber !== sel.endLineNumber) return "";
  return code.getModel()?.getValueInRange(sel) ?? "";
}

/** The line being read in `path` if the code view shows it: the cursor's if it's on screen, else the top one. */
export function lineInView(path: string): number | undefined {
  return onShow?.path === path ? onShow.line() : undefined;
}
