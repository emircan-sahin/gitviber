import { getCurrentWebview } from "@tauri-apps/api/webview";
import { errorMessage, pty } from "../api";
import { ask } from "../app/ask";
import { failed, toast } from "../app/toast";
import { IS_WINDOWS } from "../platform";
import { getSettings } from "../settings";
import { pastedLines, pathPastes } from "./paste";
import { panes, type Pane } from "./terminals";

// What goes into a pane from outside its keys: pastes, dropped files, and a program's OSC 52 copy.
// Imported through terminals.ts only: the two import each other.

let copiedFromProgram = false;
/** OSC 52. Said once a run: a program over SSH, or a file being `cat`, can write the clipboard too. */
export function copyFromProgram(text: string) {
  const first = !copiedFromProgram;
  copiedFromProgram = true;
  pty.copy(text).then(() => first && toast("info", "Copied from the terminal", "A program in the terminal put text on the clipboard."), failed("Could not copy"));
}

/** `fallback`: the webview's own text, pasted if the native read fails or finds nothing. */
export async function pasteInto(p: Pane, fallback = "") {
  const got = await pty.paste().catch((e) => {
    if (!fallback) toast("error", "Could not paste", errorMessage(e));
    return null;
  });
  if (!got || got.kind === "empty") {
    if (fallback) await pasteText(p, fallback);
    return;
  }
  if (got.kind === "text") await pasteText(p, got.text);
  else if (got.kind === "files") pastePaths(p, got.paths);
  else if (got.kind === "image") pastePaths(p, [got.path]);
}

/** Several lines into a program without bracketed paste run one by one as they arrive: asked first, as in VS Code. */
async function pasteText(p: Pane, text: string) {
  // Its shell is gone (exited).
  if (p.term.options.disableStdin) return;
  const lines = pastedLines(text);
  if (lines > 1 && !p.term.modes.bracketedPasteMode) {
    const ok = await ask(`The program in this terminal takes a paste as typed keys, so each of the ${lines} lines runs as it arrives.`, { title: `Paste ${lines} lines`, kind: "warning", okLabel: "Paste" });
    if (!ok || !panes.has(p.id)) return;
  }
  p.term.paste(text);
}

/** Paths as the AI CLIs take them: each its own (bracketed) paste, never typed as keys. */
function pastePaths(p: Pane, paths: string[]) {
  for (const text of pathPastes(paths)) p.term.paste(text);
}

// Files dropped on a pane paste their paths into it (Tauri hands over the paths, the page only
// their names). The pane under the pointer is outlined while they're dragged.
let dropTarget: Pane | null = null;
function paneAt(pos: { x: number; y: number }) {
  // Typed physical, but on macOS and Linux wry hands over window points unscaled (drag_drop.rs):
  // halved on Retina, the point landed in the sidebar. Page zoom (the UI scale) makes a CSS pixel
  // bigger than a point. Windows' pixels are physical, and Chromium's ratio includes the zoom.
  const scale = IS_WINDOWS ? devicePixelRatio : getSettings().uiScale;
  const el = document.elementFromPoint(pos.x / scale, pos.y / scale);
  return el ? ([...panes.values()].find((p) => p.host.contains(el)) ?? null) : null;
}
function markDropTarget(p: Pane | null) {
  if (p === dropTarget) return;
  dropTarget?.host.classList.remove("gv-drop-target");
  p?.host.classList.add("gv-drop-target");
  dropTarget = p;
}
const dropListener = getCurrentWebview().onDragDropEvent(async ({ payload }) => {
  if (payload.type === "leave") return markDropTarget(null);
  const p = paneAt(payload.position);
  if (payload.type !== "drop") return markDropTarget(p);
  markDropTarget(null);
  if (!p) return;
  pastePaths(p, await pty.keepDropped(payload.paths).catch(() => payload.paths));
  p.term.focus();
});
dropListener.catch(() => {});
// A hot reload re-runs this module: the old listener goes, or each drop would paste twice.
import.meta.hot?.dispose(() => void dropListener.then((stop) => stop()).catch(() => {}));
